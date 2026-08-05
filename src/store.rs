use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    time::Duration as StdDuration,
};

use anyhow::{Context, Result, anyhow};
#[cfg(test)]
use chrono::{DateTime, Timelike};
use chrono::{Duration, Local, NaiveDate};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct DayStats {
    pub hours: [u64; 24],
}

impl DayStats {
    pub fn total(&self) -> u64 {
        self.hours.iter().sum()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Stats {
    pub days: BTreeMap<NaiveDate, DayStats>,
}

impl Stats {
    #[cfg(test)]
    pub fn record(&mut self, timestamp: DateTime<Local>) {
        let day = self.days.entry(timestamp.date_naive()).or_default();
        day.hours[timestamp.hour() as usize] += 1;
    }

    pub fn total_on(&self, date: NaiveDate) -> u64 {
        self.days.get(&date).map_or(0, DayStats::total)
    }

    pub fn hours_on(&self, date: NaiveDate) -> [u64; 24] {
        self.days.get(&date).map_or([0; 24], |day| day.hours)
    }

    pub fn last_days(&self, end: NaiveDate, count: usize) -> Vec<(NaiveDate, u64)> {
        (0..count)
            .rev()
            .map(|days_ago| {
                let date = end - Duration::days(days_ago as i64);
                (date, self.total_on(date))
            })
            .collect()
    }

    pub fn average_for_days(&self, end: NaiveDate, count: usize) -> u64 {
        if count == 0 {
            return 0;
        }
        let total: u64 = self
            .last_days(end, count)
            .iter()
            .map(|(_, value)| value)
            .sum();
        total / count as u64
    }

    pub fn best_day(&self) -> Option<(NaiveDate, u64)> {
        self.days
            .iter()
            .map(|(date, day)| (*date, day.total()))
            .max_by_key(|(_, total)| *total)
    }

    pub fn aggregate_hours(&self, end: NaiveDate, days: usize) -> [u64; 24] {
        let mut result = [0; 24];
        for (date, _) in self.last_days(end, days) {
            let hours = self.hours_on(date);
            for (target, value) in result.iter_mut().zip(hours) {
                *target += value;
            }
        }
        result
    }
}

pub struct Database {
    connection: Connection,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RecorderStatus {
    pub active: bool,
    pub device_count: usize,
    pub keys_per_minute: usize,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("database path has no parent: {}", path.display()))?;
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;

        let connection = Connection::open(path)
            .with_context(|| format!("failed to open database at {}", path.display()))?;
        let mut database = Self { connection };
        database.initialize()?;
        Ok(database)
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory().context("failed to open test database")?;
        let mut database = Self { connection };
        database.initialize()?;
        Ok(database)
    }

    pub fn load_stats(&self) -> Result<Stats> {
        let mut statement = self
            .connection
            .prepare("SELECT date, hour, count FROM hourly_counts ORDER BY date, hour")
            .context("failed to prepare stats query")?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .context("failed to query stats")?;

        let mut stats = Stats::default();
        for row in rows {
            let (date, hour, count) = row.context("failed to read stats row")?;
            let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                .with_context(|| format!("database contains an invalid date: {date}"))?;
            let hour = usize::try_from(hour).context("database contains a negative hour")?;
            if hour >= 24 {
                return Err(anyhow!("database contains an invalid hour: {hour}"));
            }
            let count = u64::try_from(count).context("database contains a negative count")?;
            stats.days.entry(date).or_default().hours[hour] = count;
        }
        Ok(stats)
    }

    pub fn add_counts(&mut self, counts: &BTreeMap<(NaiveDate, usize), u64>) -> Result<()> {
        if counts.is_empty() {
            return Ok(());
        }

        let transaction = self
            .connection
            .transaction()
            .context("failed to start stats transaction")?;
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO hourly_counts (date, hour, count) VALUES (?1, ?2, ?3)
                     ON CONFLICT(date, hour) DO UPDATE SET count = count + excluded.count",
                )
                .context("failed to prepare stats update")?;
            for ((date, hour), count) in counts {
                let count = i64::try_from(*count).context("key count is too large to persist")?;
                statement
                    .execute(params![date.format("%Y-%m-%d").to_string(), hour, count])
                    .context("failed to update hourly key count")?;
            }
        }
        transaction
            .commit()
            .context("failed to commit stats transaction")
    }

    pub fn update_recorder_status(
        &self,
        device_count: usize,
        keys_per_minute: usize,
    ) -> Result<()> {
        let device_count = i64::try_from(device_count).context("device count is too large")?;
        let keys_per_minute =
            i64::try_from(keys_per_minute).context("keys-per-minute count is too large")?;
        self.connection
            .execute(
                "INSERT INTO recorder_status (id, heartbeat, device_count, keys_per_minute)
                 VALUES (1, ?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET
                     heartbeat = excluded.heartbeat,
                     device_count = excluded.device_count,
                     keys_per_minute = excluded.keys_per_minute",
                params![Local::now().timestamp(), device_count, keys_per_minute],
            )
            .context("failed to update recorder status")?;
        Ok(())
    }

    pub fn load_recorder_status(&self) -> Result<RecorderStatus> {
        let status = self
            .connection
            .query_row(
                "SELECT heartbeat, device_count, keys_per_minute
                 FROM recorder_status WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .context("failed to load recorder status")?;
        let Some((heartbeat, device_count, keys_per_minute)) = status else {
            return Ok(RecorderStatus::default());
        };
        let active = Local::now().timestamp().saturating_sub(heartbeat) <= 3;
        if !active {
            return Ok(RecorderStatus::default());
        }

        Ok(RecorderStatus {
            active,
            device_count: usize::try_from(device_count)
                .context("database contains a negative device count")?,
            keys_per_minute: usize::try_from(keys_per_minute)
                .context("database contains a negative keys-per-minute count")?,
        })
    }

    pub fn clear_recorder_status(&self) -> Result<()> {
        self.connection
            .execute("DELETE FROM recorder_status WHERE id = 1", [])
            .context("failed to clear recorder status")?;
        Ok(())
    }

    pub fn migrate_json(&mut self, path: &Path) -> Result<bool> {
        if !path.exists() {
            return Ok(false);
        }

        let data = fs::read(path)
            .with_context(|| format!("failed to read legacy stats from {}", path.display()))?;
        let legacy: Stats = serde_json::from_slice(&data)
            .with_context(|| format!("legacy stats are not valid JSON: {}", path.display()))?;
        let mut counts = BTreeMap::new();
        for (date, day) in legacy.days {
            for (hour, count) in day.hours.into_iter().enumerate() {
                if count > 0 {
                    counts.insert((date, hour), count);
                }
            }
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("failed to start legacy migration transaction")?;
        let existing: i64 = transaction
            .query_row("SELECT COUNT(*) FROM hourly_counts", [], |row| row.get(0))
            .context("failed to inspect the database before migration")?;
        if existing > 0 {
            return Ok(false);
        }
        {
            let mut statement = transaction
                .prepare("INSERT INTO hourly_counts (date, hour, count) VALUES (?1, ?2, ?3)")
                .context("failed to prepare legacy stats migration")?;
            for ((date, hour), count) in counts {
                let count = i64::try_from(count).context("legacy key count is too large")?;
                statement
                    .execute(params![date.format("%Y-%m-%d").to_string(), hour, count])
                    .context("failed to migrate legacy key count")?;
            }
        }
        transaction
            .commit()
            .context("failed to commit legacy stats migration")?;
        Ok(true)
    }

    fn initialize(&mut self) -> Result<()> {
        self.connection
            .busy_timeout(StdDuration::from_secs(2))
            .context("failed to set database timeout")?;
        self.connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                  CREATE TABLE IF NOT EXISTS hourly_counts (
                     date TEXT NOT NULL,
                     hour INTEGER NOT NULL CHECK (hour BETWEEN 0 AND 23),
                     count INTEGER NOT NULL CHECK (count >= 0),
                      PRIMARY KEY (date, hour)
                  ) WITHOUT ROWID;
                  CREATE TABLE IF NOT EXISTS recorder_status (
                      id INTEGER PRIMARY KEY CHECK (id = 1),
                      heartbeat INTEGER NOT NULL,
                      device_count INTEGER NOT NULL CHECK (device_count >= 0),
                      keys_per_minute INTEGER NOT NULL CHECK (keys_per_minute >= 0)
                  );",
            )
            .context("failed to initialize database")
    }
}

pub fn default_database_path() -> Result<PathBuf> {
    Ok(data_directory()?.join("keypulse.db"))
}

pub fn legacy_json_path() -> Result<PathBuf> {
    Ok(data_directory()?.join("stats.json"))
}

pub fn recorder_lock_path() -> Result<PathBuf> {
    Ok(data_directory()?.join("recorder.lock"))
}

pub fn recorder_control_lock_path() -> Result<PathBuf> {
    Ok(data_directory()?.join("recorder-control.lock"))
}

pub fn recorder_log_path() -> Result<PathBuf> {
    Ok(data_directory()?.join("recorder.log"))
}

fn data_directory() -> Result<PathBuf> {
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(data_home).join("keypulse"));
    }

    let home = env::var_os("HOME")
        .ok_or_else(|| anyhow!("cannot determine data directory; set HOME or XDG_DATA_HOME"))?;
    Ok(PathBuf::from(home).join(".local/share/keypulse"))
}

#[cfg(test)]
mod tests {
    use chrono::{Local, NaiveDate, TimeZone};

    use std::{
        collections::BTreeMap,
        fs,
        sync::{Arc, Barrier},
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{Database, Stats};

    #[test]
    fn records_press_in_the_correct_hour() {
        let mut stats = Stats::default();
        let time = Local.with_ymd_and_hms(2026, 7, 22, 14, 30, 0).unwrap();

        stats.record(time);
        stats.record(time);

        assert_eq!(stats.total_on(time.date_naive()), 2);
        assert_eq!(stats.hours_on(time.date_naive())[14], 2);
    }

    #[test]
    fn missing_days_are_included_in_ranges() {
        let mut stats = Stats::default();
        let time = Local.with_ymd_and_hms(2026, 7, 22, 9, 0, 0).unwrap();
        stats.record(time);

        let days = stats.last_days(time.date_naive(), 3);

        assert_eq!(days.len(), 3);
        assert_eq!(days[0].1, 0);
        assert_eq!(days[2].1, 1);
    }

    #[test]
    fn database_persists_batched_counts() {
        let mut database = Database::in_memory().unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 7, 23).unwrap();
        let counts = BTreeMap::from([((date, 9), 12), ((date, 14), 8)]);

        database.add_counts(&counts).unwrap();
        database
            .add_counts(&BTreeMap::from([((date, 9), 3)]))
            .unwrap();
        let stats = database.load_stats().unwrap();

        assert_eq!(stats.hours_on(date)[9], 15);
        assert_eq!(stats.total_on(date), 23);
    }

    #[test]
    fn database_reports_live_recorder_status() {
        let database = Database::in_memory().unwrap();

        database.update_recorder_status(2, 37).unwrap();
        let status = database.load_recorder_status().unwrap();

        assert!(status.active);
        assert_eq!(status.device_count, 2);
        assert_eq!(status.keys_per_minute, 37);
    }

    #[test]
    fn concurrent_legacy_migration_is_applied_once() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "keypulse-migration-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let database_path = directory.join("keypulse.db");
        let legacy_path = directory.join("stats.json");
        let date = NaiveDate::from_ymd_opt(2026, 7, 23).unwrap();
        let mut stats = Stats::default();
        stats.days.entry(date).or_default().hours[9] = 12;
        fs::write(&legacy_path, serde_json::to_vec(&stats).unwrap()).unwrap();

        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let database_path = database_path.clone();
                let legacy_path = legacy_path.clone();
                thread::spawn(move || {
                    let mut database = Database::open(&database_path).unwrap();
                    barrier.wait();
                    database.migrate_json(&legacy_path).unwrap()
                })
            })
            .collect();

        let migrated = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|migrated| *migrated)
            .count();
        let database = Database::open(&database_path).unwrap();

        assert_eq!(migrated, 1);
        assert_eq!(database.load_stats().unwrap().total_on(date), 12);
        drop(database);
        fs::remove_dir_all(directory).unwrap();
    }
}
