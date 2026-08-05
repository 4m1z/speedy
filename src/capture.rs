use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use evdev::{Device, EventType, KeyCode};

#[derive(Clone, Debug)]
pub enum CaptureEvent {
    KeyPress,
    Devices(Vec<String>),
}

pub struct CaptureHandle {
    stop: Arc<AtomicBool>,
    supervisor: Option<JoinHandle<()>>,
}

impl CaptureHandle {
    pub fn start(sender: Sender<CaptureEvent>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let supervisor_stop = Arc::clone(&stop);
        let supervisor = thread::spawn(move || supervise_devices(sender, supervisor_stop));
        Self {
            stop,
            supervisor: Some(supervisor),
        }
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(supervisor) = self.supervisor.take() {
            let _ = supervisor.join();
        }
    }
}

fn supervise_devices(sender: Sender<CaptureEvent>, stop: Arc<AtomicBool>) {
    let (done_sender, done_receiver) = std::sync::mpsc::channel::<PathBuf>();
    let mut active = HashMap::<PathBuf, (String, JoinHandle<()>)>::new();
    let mut last_report = Vec::new();

    while !stop.load(Ordering::Relaxed) {
        while let Ok(path) = done_receiver.try_recv() {
            if let Some((_, worker)) = active.remove(&path) {
                let _ = worker.join();
            }
        }

        for (path, device) in evdev::enumerate() {
            if active.contains_key(&path) || !is_keyboard(&device) {
                continue;
            }

            let name = device.name().unwrap_or("Unnamed keyboard").to_owned();
            let worker_sender = sender.clone();
            let worker_done = done_sender.clone();
            let worker_stop = Arc::clone(&stop);
            let worker_path = path.clone();
            let worker = thread::spawn(move || {
                watch_device(device, &worker_sender, &worker_stop);
                let _ = worker_done.send(worker_path);
            });
            active.insert(path.clone(), (name, worker));
        }

        let mut report: Vec<String> = active.values().map(|(name, _)| name.clone()).collect();
        report.sort();
        if report != last_report {
            last_report.clone_from(&report);
            if sender.send(CaptureEvent::Devices(report)).is_err() {
                break;
            }
        }

        sleep_interruptibly(&stop, Duration::from_secs(2));
    }

    stop.store(true, Ordering::Relaxed);
    for (_, (_, worker)) in active {
        let _ = worker.join();
    }
}

fn watch_device(mut device: Device, sender: &Sender<CaptureEvent>, stop: &AtomicBool) {
    if device.set_nonblocking(true).is_err() {
        return;
    }

    while !stop.load(Ordering::Relaxed) {
        match device.fetch_events() {
            Ok(events) => {
                for event in events {
                    // Value 1 is a physical key-down. Values 0 and 2 are release and repeat.
                    if event.event_type() == EventType::KEY
                        && event.value() == 1
                        && sender.send(CaptureEvent::KeyPress).is_err()
                    {
                        return;
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(8));
            }
            Err(_) => return,
        }
    }
}

fn is_keyboard(device: &Device) -> bool {
    device.supported_keys().is_some_and(|keys| {
        keys.contains(KeyCode::KEY_A)
            && keys.contains(KeyCode::KEY_Z)
            && keys.contains(KeyCode::KEY_ENTER)
            && keys.contains(KeyCode::KEY_SPACE)
    })
}

fn sleep_interruptibly(stop: &AtomicBool, duration: Duration) {
    let slices = duration.as_millis() / 100;
    for _ in 0..slices {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
}
