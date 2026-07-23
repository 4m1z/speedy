use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    time::Duration as StdDuration,
};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Duration, Local, NaiveDate, Timelike};
use rusqlite::{Connection, params};
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

    pub fn migrate_json(&mut self, path: &Path) -> Result<bool> {
        if !path.exists() || !self.is_empty()? {
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
        self.add_counts(&counts)?;
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
                 ) WITHOUT ROWID;",
            )
            .context("failed to initialize database")
    }

    fn is_empty(&self) -> Result<bool> {
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM hourly_counts", [], |row| row.get(0))
            .context("failed to inspect database")?;
        Ok(count == 0)
    }
}

pub fn default_database_path() -> Result<PathBuf> {
    Ok(data_directory()?.join("keypulse.db"))
}

pub fn legacy_json_path() -> Result<PathBuf> {
    Ok(data_directory()?.join("stats.json"))
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

    use std::collections::BTreeMap;

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
}
