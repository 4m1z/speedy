use std::{
    collections::{BTreeMap, VecDeque},
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{Local, NaiveDate, Timelike};

use crate::{
    capture::{CaptureEvent, CaptureHandle},
    store::{
        Database, default_database_path, recorder_control_lock_path, recorder_lock_path,
        recorder_log_path,
    },
};

const FLUSH_INTERVAL: Duration = Duration::from_millis(500);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);

pub fn ensure_running() -> Result<()> {
    let _lifecycle = LifecycleLock::acquire()?;
    if RecorderLock::try_acquire()?.is_none() {
        return Ok(());
    }

    let database = Database::open(&default_database_path()?)?;
    database.clear_recorder_status()?;
    let log_path = recorder_log_path()?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open recorder log at {}", log_path.display()))?;
    let executable = std::env::current_exe().context("failed to locate the speedy executable")?;
    let mut command = Command::new(executable);
    command
        .arg("--recorder")
        .env("SPEEDY_RECORDER_CHILD", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log));
    // A separate session keeps recording alive after its launching terminal closes.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .context("failed to start the background recorder")?;

    let started = Instant::now();
    loop {
        if database.load_recorder_status()?.active && RecorderLock::try_acquire()?.is_none() {
            thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(());
        }
        if let Some(status) = child
            .try_wait()
            .context("failed to inspect the background recorder")?
        {
            bail!(
                "background recorder exited during startup with {status}; see {}",
                log_path.display()
            );
        }
        if started.elapsed() >= STARTUP_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "background recorder did not become ready within three seconds; see {}",
                log_path.display()
            );
        }
        thread::sleep(Duration::from_millis(25));
    }
}

pub fn run() -> Result<()> {
    let Some(mut recorder_lock) = RecorderLock::try_acquire()? else {
        return Ok(());
    };
    recorder_lock.write_pid()?;

    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop))
        .context("failed to install the recorder stop handler")?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop))
        .context("failed to install the recorder interrupt handler")?;

    let mut database = Database::open(&default_database_path()?)?;
    let (sender, receiver) = std::sync::mpsc::channel();
    let capture = CaptureHandle::start(sender);
    let mut pending = BTreeMap::<(NaiveDate, usize), u64>::new();
    let mut recent = VecDeque::<SystemTime>::new();
    let mut device_count = 0;
    let mut last_flush = Instant::now();
    let mut capture_error = None;
    database.update_recorder_status(device_count, 0)?;

    while !stop.load(Ordering::Relaxed) {
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => handle_event(event, &mut pending, &mut recent, &mut device_count),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                capture_error = Some(anyhow!("keyboard capture worker stopped"));
                break;
            }
        }
        drain_events(&receiver, &mut pending, &mut recent, &mut device_count);

        if last_flush.elapsed() >= FLUSH_INTERVAL {
            if let Err(error) = flush(&mut database, &mut pending, &mut recent, device_count) {
                eprintln!("failed to flush recorder data; will retry: {error:#}");
            }
            last_flush = Instant::now();
        }
    }

    drop(capture);
    drain_events(&receiver, &mut pending, &mut recent, &mut device_count);
    flush_before_exit(&mut database, &mut pending, &mut recent, device_count);
    database.clear_recorder_status()?;
    if let Some(error) = capture_error {
        return Err(error);
    }
    Ok(())
}

pub fn stop() -> Result<bool> {
    let _lifecycle = LifecycleLock::acquire()?;
    if RecorderLock::try_acquire()?.is_some() {
        Database::open(&default_database_path()?)?.clear_recorder_status()?;
        return Ok(false);
    }

    let path = recorder_lock_path()?;
    let mut file = File::open(&path)
        .with_context(|| format!("failed to open recorder lock at {}", path.display()))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .context("failed to read the recorder process ID")?;
    let pid: libc::pid_t = contents
        .trim()
        .parse()
        .context("recorder lock contains an invalid process ID")?;
    if unsafe { libc::kill(pid, libc::SIGTERM) } == -1 {
        return Err(std::io::Error::last_os_error()).context("failed to stop the recorder");
    }

    let started = Instant::now();
    while started.elapsed() < STOP_TIMEOUT {
        thread::sleep(Duration::from_millis(50));
        if RecorderLock::try_acquire()?.is_some() {
            Database::open(&default_database_path()?)?.clear_recorder_status()?;
            return Ok(true);
        }
    }
    bail!("recorder is still flushing data after ten seconds")
}

fn handle_event(
    event: CaptureEvent,
    pending: &mut BTreeMap<(NaiveDate, usize), u64>,
    recent: &mut VecDeque<SystemTime>,
    device_count: &mut usize,
) {
    match event {
        CaptureEvent::KeyPress => {
            let timestamp = Local::now();
            *pending
                .entry((timestamp.date_naive(), timestamp.hour() as usize))
                .or_default() += 1;
            recent.push_back(SystemTime::now());
        }
        CaptureEvent::Devices(devices) => *device_count = devices.len(),
    }
}

fn drain_events(
    receiver: &Receiver<CaptureEvent>,
    pending: &mut BTreeMap<(NaiveDate, usize), u64>,
    recent: &mut VecDeque<SystemTime>,
    device_count: &mut usize,
) {
    while let Ok(event) = receiver.try_recv() {
        handle_event(event, pending, recent, device_count);
    }
}

fn flush(
    database: &mut Database,
    pending: &mut BTreeMap<(NaiveDate, usize), u64>,
    recent: &mut VecDeque<SystemTime>,
    device_count: usize,
) -> Result<()> {
    if !pending.is_empty() {
        database.add_counts(pending)?;
        pending.clear();
    }
    let now = SystemTime::now();
    if recent
        .front()
        .is_some_and(|pressed| now.duration_since(*pressed).is_err())
    {
        recent.clear();
    }
    while recent.front().is_some_and(|pressed| {
        now.duration_since(*pressed)
            .is_ok_and(|elapsed| elapsed > Duration::from_secs(60))
    }) {
        recent.pop_front();
    }
    database.update_recorder_status(device_count, recent.len())
}

fn flush_before_exit(
    database: &mut Database,
    pending: &mut BTreeMap<(NaiveDate, usize), u64>,
    recent: &mut VecDeque<SystemTime>,
    device_count: usize,
) {
    loop {
        match flush(database, pending, recent, device_count) {
            Ok(()) => return,
            Err(error) => {
                eprintln!("failed to flush recorder data during shutdown; retrying: {error:#}");
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

struct RecorderLock {
    file: File,
}

impl RecorderLock {
    fn try_acquire() -> Result<Option<Self>> {
        let path = recorder_lock_path()?;
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("recorder lock path has no parent: {}", path.display()))?;
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("failed to open recorder lock at {}", path.display()))?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == -1 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
                return Ok(None);
            }
            return Err(error).context("failed to lock the background recorder");
        }
        Ok(Some(Self { file }))
    }

    fn write_pid(&mut self) -> Result<()> {
        self.file
            .set_len(0)
            .context("failed to clear the recorder lock")?;
        write!(self.file, "{}", std::process::id())
            .context("failed to write the recorder process ID")?;
        self.file
            .sync_data()
            .context("failed to sync the recorder process ID")
    }
}

struct LifecycleLock {
    _file: File,
}

impl LifecycleLock {
    fn acquire() -> Result<Self> {
        let path = recorder_control_lock_path()?;
        let file = open_lock_file(&path)?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == -1 {
            return Err(std::io::Error::last_os_error())
                .context("failed to lock recorder lifecycle operations");
        }
        Ok(Self { _file: file })
    }
}

fn open_lock_file(path: &std::path::Path) -> Result<File> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("lock path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("failed to open lock at {}", path.display()))
}
