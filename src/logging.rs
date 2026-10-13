use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::OnceLock,
};

static LOGGER: OnceLock<Logger> = OnceLock::new();

pub struct Logger {
    directory: PathBuf,
    session: String,
}

impl Logger {
    pub fn new(directory: &Path) -> Result<Self> {
        fs::create_dir_all(directory).context("Cannot create CLI log directory")?;
        ensure!(
            !fs::symlink_metadata(directory)?.file_type().is_symlink(),
            "Log directory must not be a symlink"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            directory: directory.into(),
            session: uuid::Uuid::new_v4().to_string(),
        })
    }

    pub fn write(&self, at: DateTime<Utc>, event: &str, data: Value) -> Result<()> {
        let path = self
            .directory
            .join(format!("{}.jsonl", at.format("%Y-%m-%d")));
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Log file must be a regular file"
            );
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).context("Cannot open CLI log file")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.lock_exclusive()?;
        let line = json!({"timestamp":at.to_rfc3339(), "pid":std::process::id(), "session":self.session, "event":event, "data":data});
        serde_json::to_writer(&mut file, &line)?;
        file.write_all(b"\n")?;
        file.sync_data().context("Cannot persist CLI log record")?;
        Ok(())
    }
}

pub fn init(home: &Path) -> Result<()> {
    fs::create_dir_all(home)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(home, fs::Permissions::from_mode(0o700))?;
    }
    let logger = Logger::new(&home.join("logs"))?;
    LOGGER
        .set(logger)
        .map_err(|_| anyhow::anyhow!("CLI logger already initialized"))
}

pub fn event(name: &str, data: Value) -> Result<()> {
    if let Some(logger) = LOGGER.get() {
        logger.write(Utc::now(), name, data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_sessions_append_without_interleaving_records() {
        let root = std::env::temp_dir().join(format!("gallendar-log-{}", uuid::Uuid::new_v4()));
        let at: DateTime<Utc> = "2026-10-13T12:00:00Z".parse().unwrap();
        let threads: Vec<_> = (0..2)
            .map(|writer| {
                let logger = Logger::new(&root).unwrap();
                std::thread::spawn(move || {
                    for item in 0..5 {
                        logger
                            .write(at, "concurrent", json!({"writer":writer,"item":item}))
                            .unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let records: Vec<Value> = fs::read_to_string(root.join("2026-10-13.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), 10);
        for writer in 0..2 {
            for item in 0..5 {
                assert_eq!(
                    records
                        .iter()
                        .filter(|r| r["data"]["writer"] == writer && r["data"]["item"] == item)
                        .count(),
                    1
                );
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn logs_append_valid_records_rotate_daily_and_stay_private() {
        let root = std::env::temp_dir().join(format!("gallendar-log-{}", uuid::Uuid::new_v4()));
        let logger = Logger::new(&root).unwrap();
        let day: DateTime<Utc> = "2026-10-13T23:59:59Z".parse().unwrap();
        logger
            .write(day, "operation_started", json!({"key":"copy-key"}))
            .unwrap();
        logger
            .write(
                day,
                "operation_failed",
                json!({"error":"read-back mismatch"}),
            )
            .unwrap();
        logger
            .write(day + chrono::Duration::seconds(1), "retry", json!({}))
            .unwrap();
        let records: Vec<Value> = fs::read_to_string(root.join("2026-10-13.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["session"], records[1]["session"]);
        assert_eq!(records[1]["event"], "operation_failed");
        assert!(root.join("2026-10-14.jsonl").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&root).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(root.join("2026-10-13.jsonl"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            std::os::unix::fs::symlink(
                root.join("2026-10-13.jsonl"),
                root.join("2026-10-15.jsonl"),
            )
            .unwrap();
            assert!(
                logger
                    .write(day + chrono::Duration::days(2), "blocked", json!({}))
                    .is_err()
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
