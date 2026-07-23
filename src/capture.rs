use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
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
}

impl CaptureHandle {
    pub fn start(sender: Sender<CaptureEvent>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let supervisor_stop = Arc::clone(&stop);
        thread::spawn(move || supervise_devices(sender, supervisor_stop));
        Self { stop }
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn supervise_devices(sender: Sender<CaptureEvent>, stop: Arc<AtomicBool>) {
    let (done_sender, done_receiver) = std::sync::mpsc::channel::<PathBuf>();
    let mut active = HashMap::<PathBuf, String>::new();
    let mut last_report = Vec::new();

    while !stop.load(Ordering::Relaxed) {
        while let Ok(path) = done_receiver.try_recv() {
            active.remove(&path);
        }

        for (path, device) in evdev::enumerate() {
            if active.contains_key(&path) || !is_keyboard(&device) {
                continue;
            }

            let name = device.name().unwrap_or("Unnamed keyboard").to_owned();
            active.insert(path.clone(), name);

            let worker_sender = sender.clone();
            let worker_done = done_sender.clone();
            let worker_stop = Arc::clone(&stop);
            thread::spawn(move || {
                watch_device(device, &worker_sender, &worker_stop);
                let _ = worker_done.send(path);
            });
        }

        let mut report: Vec<String> = active.values().cloned().collect();
        report.sort();
        if report != last_report {
            last_report.clone_from(&report);
            if sender.send(CaptureEvent::Devices(report)).is_err() {
                break;
            }
        }

        sleep_interruptibly(&stop, Duration::from_secs(2));
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
