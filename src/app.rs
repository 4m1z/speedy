use std::{
    collections::VecDeque,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::{Local, NaiveDate};

use crate::store::{Database, Stats};

pub const DAILY_TARGET: u64 = 10_000;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Tab {
    #[default]
    Live,
    Daily,
    Hourly,
    Records,
}

impl Tab {
    pub fn index(self) -> usize {
        match self {
            Self::Live => 0,
            Self::Daily => 1,
            Self::Hourly => 2,
            Self::Records => 3,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Live => Self::Daily,
            Self::Daily => Self::Hourly,
            Self::Hourly => Self::Records,
            Self::Records => Self::Live,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Live => Self::Records,
            Self::Daily => Self::Live,
            Self::Hourly => Self::Daily,
            Self::Records => Self::Hourly,
        }
    }
}

pub struct App {
    pub stats: Stats,
    pub tab: Tab,
    pub recorder_active: bool,
    pub device_count: usize,
    pub keys_per_minute: usize,
    pub kpm_history: VecDeque<u64>,
    pub refresh_error: Option<String>,
    database: Database,
    last_refresh: Instant,
    last_kpm_sample: SystemTime,
}

impl App {
    pub fn new(stats: Stats, database: Database) -> Self {
        Self {
            stats,
            tab: Tab::Live,
            recorder_active: false,
            device_count: 0,
            keys_per_minute: 0,
            kpm_history: VecDeque::new(),
            refresh_error: None,
            database,
            last_refresh: Instant::now() - Duration::from_secs(1),
            last_kpm_sample: UNIX_EPOCH,
        }
    }

    pub fn refresh_if_due(&mut self) {
        if self.last_refresh.elapsed() < Duration::from_millis(250) {
            return;
        }
        self.last_refresh = Instant::now();
        match (
            self.database.load_stats(),
            self.database.load_recorder_status(),
        ) {
            (Ok(stats), Ok(status)) => {
                self.stats = stats;
                self.recorder_active = status.active;
                self.device_count = status.device_count;
                self.keys_per_minute = status.keys_per_minute;
                let sample_gap = self.last_kpm_sample.elapsed().unwrap_or(Duration::MAX);
                if self.kpm_history.is_empty() || sample_gap >= Duration::from_secs(3) {
                    if !self.kpm_history.is_empty() && sample_gap >= Duration::from_secs(6) {
                        self.kpm_history.push_back(0);
                    }
                    self.kpm_history.push_back(status.keys_per_minute as u64);
                    while self.kpm_history.len() > 24 {
                        self.kpm_history.pop_front();
                    }
                    self.last_kpm_sample = SystemTime::now();
                }
                self.refresh_error = None;
            }
            (stats, status) => {
                let error = stats
                    .err()
                    .or_else(|| status.err())
                    .expect("one refresh operation failed");
                self.refresh_error = Some(error.to_string());
            }
        }
    }

    pub fn refresh_now(&mut self) {
        self.last_refresh = Instant::now() - Duration::from_secs(1);
        self.refresh_if_due();
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
}
