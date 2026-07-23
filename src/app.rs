use std::{
    collections::{BTreeMap, VecDeque},
    time::{Duration, Instant},
};

use anyhow::Result;
use chrono::{Local, NaiveDate, Timelike};

use crate::{
    capture::CaptureEvent,
    store::{Database, Stats},
};

pub const DAILY_TARGET: u64 = 10_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Tab {
    #[default]
    Today,
    Week,
    Report,
}

impl Tab {
    pub fn index(self) -> usize {
        match self {
            Self::Today => 0,
            Self::Week => 1,
            Self::Report => 2,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Today => Self::Week,
            Self::Week => Self::Report,
            Self::Report => Self::Today,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Today => Self::Report,
            Self::Week => Self::Today,
            Self::Report => Self::Week,
        }
    }
}

pub struct App {
    pub stats: Stats,
    pub tab: Tab,
    pub devices: Vec<String>,
    pub save_error: Option<String>,
    database: Database,
    pending_counts: BTreeMap<(NaiveDate, usize), u64>,
    recent_presses: VecDeque<Instant>,
    dirty: bool,
    last_save_attempt: Instant,
}

impl App {
    pub fn new(stats: Stats, database: Database) -> Self {
        Self {
            stats,
            tab: Tab::Today,
            devices: Vec::new(),
            save_error: None,
            database,
            pending_counts: BTreeMap::new(),
            recent_presses: VecDeque::new(),
            dirty: false,
            last_save_attempt: Instant::now(),
        }
    }

    pub fn handle_capture(&mut self, event: CaptureEvent) {
        match event {
            CaptureEvent::KeyPress => self.record_press(),
            CaptureEvent::Devices(devices) => self.devices = devices,
        }
    }

    pub fn record_press(&mut self) {
        let timestamp = Local::now();
        self.stats.record(timestamp);
        *self
            .pending_counts
            .entry((timestamp.date_naive(), timestamp.hour() as usize))
            .or_default() += 1;
        let now = Instant::now();
        self.recent_presses.push_back(now);
        self.trim_recent(now);
        self.dirty = true;
    }

    pub fn keys_per_minute(&mut self) -> usize {
        self.trim_recent(Instant::now());
        self.recent_presses.len()
    }

    pub fn save_if_due(&mut self) {
        if self.dirty && self.last_save_attempt.elapsed() >= Duration::from_secs(1) {
            self.last_save_attempt = Instant::now();
            if let Err(error) = self.save() {
                self.save_error = Some(error.to_string());
            }
        }
    }

    pub fn save(&mut self) -> Result<()> {
        self.database.add_counts(&self.pending_counts)?;
        self.pending_counts.clear();
        self.dirty = false;
        self.save_error = None;
        Ok(())
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn today(&self) -> NaiveDate {
        Local::now().date_naive()
    }

    pub fn current_streak(&self) -> usize {
        let today = self.today();
        let mut cursor = if self.stats.total_on(today) == 0 {
            today.pred_opt().unwrap_or(today)
        } else {
            today
        };
        let mut streak = 0;

        while self.stats.total_on(cursor) > 0 {
            streak += 1;
            let Some(previous) = cursor.pred_opt() else {
                break;
            };
            cursor = previous;
        }
        streak
    }

    fn trim_recent(&mut self, now: Instant) {
        while self
            .recent_presses
            .front()
            .is_some_and(|pressed| now.duration_since(*pressed) > Duration::from_secs(60))
        {
            self.recent_presses.pop_front();
        }
    }
}
