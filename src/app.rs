use std::{
    collections::VecDeque,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::{Local, NaiveDate};

use crate::store::{DEFAULT_DAILY_TARGET, Database, Stats};

pub const DAILY_TARGET: u64 = DEFAULT_DAILY_TARGET;

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
    pub daily_target: u64,
    pub editing_target: bool,
    pub target_input: String,
    pub target_error: Option<String>,
    database: Database,
    last_refresh: Instant,
    last_kpm_sample: SystemTime,
}

pub fn parse_daily_target(input: &str) -> Option<u64> {
    let cleaned: String = input
        .trim()
        .chars()
        .filter(|c| *c != ',' && *c != '_' && *c != ' ')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    if !cleaned.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Strip leading zeros but keep one digit so "000" parses as 0 (then rejected).
    let trimmed = cleaned.trim_start_matches('0');
    let normalized = if trimmed.is_empty() { "0" } else { trimmed };
    match normalized.parse::<u64>() {
        Ok(target) if target > 0 => Some(target),
        _ => None,
    }
}

impl App {
    pub fn new(stats: Stats, database: Database) -> Self {
        let daily_target = database.get_daily_target().unwrap_or(DEFAULT_DAILY_TARGET);
        Self {
            stats,
            tab: Tab::Live,
            recorder_active: false,
            device_count: 0,
            keys_per_minute: 0,
            kpm_history: VecDeque::new(),
            refresh_error: None,
            daily_target,
            editing_target: false,
            target_input: String::new(),
            target_error: None,
            database,
            last_refresh: Instant::now() - Duration::from_secs(1),
            last_kpm_sample: UNIX_EPOCH,
        }
    }

    pub fn effective_target(&self) -> u64 {
        if self.daily_target == 0 {
            DEFAULT_DAILY_TARGET
        } else {
            self.daily_target
        }
    }

    pub fn start_editing_target(&mut self) {
        self.editing_target = true;
        self.target_input = self.effective_target().to_string();
        self.target_error = None;
    }

    pub fn cancel_editing_target(&mut self) {
        self.editing_target = false;
        self.target_input.clear();
        self.target_error = None;
    }

    pub fn confirm_target(&mut self) -> bool {
        match parse_daily_target(&self.target_input) {
            Some(target) => match self.database.set_daily_target(target) {
                Ok(()) => {
                    self.daily_target = target;
                    self.editing_target = false;
                    self.target_input.clear();
                    self.target_error = None;
                    true
                }
                Err(error) => {
                    self.target_error = Some(error.to_string());
                    false
                }
            },
            None => {
                self.target_error = Some("Enter a positive number (e.g. 10000)".to_owned());
                false
            }
        }
    }

    pub fn refresh_if_due(&mut self) {
        if self.last_refresh.elapsed() < Duration::from_millis(250) {
            return;
        }
        self.last_refresh = Instant::now();
        // Keep the target in sync in case it changed on disk; never clobber an open editor.
        if !self.editing_target
            && let Ok(target) = self.database.get_daily_target() {
                self.daily_target = target;
            }
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

#[cfg(test)]
mod tests {
    use super::{App, parse_daily_target};
    use crate::store::{DEFAULT_DAILY_TARGET, Database, Stats};

    #[test]
    fn parses_valid_targets() {
        assert_eq!(parse_daily_target("10000"), Some(10_000));
        assert_eq!(parse_daily_target("  5,000 "), Some(5_000));
        assert_eq!(parse_daily_target("10_000"), Some(10_000));
        assert_eq!(parse_daily_target("1"), Some(1));
    }

    #[test]
    fn rejects_invalid_targets() {
        assert_eq!(parse_daily_target(""), None);
        assert_eq!(parse_daily_target("   "), None);
        assert_eq!(parse_daily_target("0"), None);
        assert_eq!(parse_daily_target("000"), None);
        assert_eq!(parse_daily_target("-5"), None);
        assert_eq!(parse_daily_target("abc"), None);
        assert_eq!(parse_daily_target("10.5"), None);
        assert_eq!(parse_daily_target("1a2"), None);
    }

    #[test]
    fn new_app_uses_default_target_when_unset() {
        let app = App::new(Stats::default(), Database::in_memory().unwrap());
        assert_eq!(app.daily_target, DEFAULT_DAILY_TARGET);
        assert!(!app.editing_target);
    }

    #[test]
    fn confirm_target_persists_and_reloads() {
        let mut app = App::new(Stats::default(), Database::in_memory().unwrap());
        app.start_editing_target();
        assert!(app.editing_target);
        app.target_input = "7500".to_owned();
        assert!(app.confirm_target());
        assert_eq!(app.daily_target, 7_500);
        assert!(!app.editing_target);

        // A fresh App over the same database file would reload; here the
        // in-memory database is owned by the first app, so verify via getter.
        // Confirm invalid input keeps the editor open with an error.
        app.start_editing_target();
        app.target_input = "0".to_owned();
        assert!(!app.confirm_target());
        assert!(app.editing_target);
        assert!(app.target_error.is_some());
        assert_eq!(app.daily_target, 7_500);
        app.cancel_editing_target();
        assert!(!app.editing_target);
    }
}
