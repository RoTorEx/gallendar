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

/// Read applied periodic-pass outcomes; previews and other configurations are excluded.
pub fn sync_status(directory: &Path, config: &Path, pid: Option<u32>) -> Result<Value> {
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader};
    let mut files: Vec<_> = fs::read_dir(directory)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|s| s == "jsonl"))
        .collect();
    files.sort();
    let canonical = config
        .canonicalize()
        .unwrap_or_else(|_| config.to_path_buf());
    let mut sessions: BTreeMap<String, bool> = BTreeMap::new();
    let mut applied: BTreeMap<String, bool> = BTreeMap::new();
    let mut worker_version = Value::Null;
    let mut worker_started_at = Value::Null;
    let mut last_pass = Value::Null;
    let mut last_success = Value::Null;
    let mut last_failure = Value::Null;
    for file in files {
        if !fs::symlink_metadata(&file)?.is_file() {
            continue;
        }
        for line in BufReader::new(fs::File::open(file)?).lines() {
            let Ok(line) = line else {
                continue;
            };
            let Ok(record) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(session) = record["session"].as_str() else {
                continue;
            };
            let event = record["event"].as_str().unwrap_or("");
            let data = &record["data"];
            if event == "command_started" {
                let same = data["config"].as_str().is_some_and(|p| {
                    Path::new(p)
                        .canonicalize()
                        .unwrap_or_else(|_| PathBuf::from(p))
                        == canonical
                });
                let is_run = same && data["command"] == "run";
                sessions.insert(session.into(), is_run);
                if is_run && pid.is_some_and(|p| record["pid"].as_u64() == Some(u64::from(p))) {
                    worker_version = data["version"].clone();
                    worker_started_at = record["timestamp"].clone();
                }
                continue;
            }
            if sessions.get(session) != Some(&true) {
                continue;
            }
            if event == "pass_started" {
                applied.insert(session.into(), data["apply"] == true);
            }
            if applied.get(session) == Some(&true)
                && (event == "pass_completed" || event == "pass_failed")
            {
                let outcome = json!({"timestamp":record["timestamp"],"pid":record["pid"],
                    "duration_ms":data["duration_ms"],"error":data["error"]});
                last_pass = outcome.clone();
                if event == "pass_completed" {
                    last_success = outcome;
                } else {
                    last_failure = outcome;
                }
            }
        }
    }
    Ok(
        json!({"last_pass":last_pass,"last_success":last_success,"last_failure":last_failure,"worker_version":worker_version,"worker_started_at":worker_started_at}),
    )
}

#[cfg(test)]
mod status_tests {
    use super::*;
    #[test]
    fn worker_version_requires_the_running_pid_config_and_command() {
        let path = std::env::temp_dir().join(format!("gallendar-status-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        let logger = Logger::new(&path).unwrap();
        let config = path.join("config.toml");
        logger
            .write(
                Utc::now(),
                "command_started",
                json!({
                    "command":"run","config":config,"version":"0.2.2"
                }),
            )
            .unwrap();
        let pid = std::process::id();
        assert_eq!(
            sync_status(&path, &config, Some(pid)).unwrap()["worker_version"],
            "0.2.2"
        );
        assert!(sync_status(&path, &config, Some(pid + 1)).unwrap()["worker_version"].is_null());
        assert!(sync_status(&path, &config, None).unwrap()["worker_version"].is_null());
        for data in [
            json!({"command":"run","config":"/unrelated","version":"9.9.9"}),
            json!({"command":"status","config":config,"version":"9.9.9"}),
        ] {
            logger.write(Utc::now(), "command_started", data).unwrap();
        }
        assert_eq!(
            sync_status(&path, &config, Some(pid)).unwrap()["worker_version"],
            "0.2.2"
        );
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn status_excludes_previews_other_configs_and_unrelated_commands() {
        let path = std::env::temp_dir().join(format!("gallendar-status-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        let logger = Logger::new(&path).unwrap();
        let config = path.join("config.toml");
        let data = json!({"command":"run","config":config});
        logger
            .write(Utc::now(), "command_started", data.clone())
            .unwrap();
        logger
            .write(Utc::now(), "pass_started", json!({"apply":false}))
            .unwrap();
        logger
            .write(
                Utc::now(),
                "pass_completed",
                json!({"duration_ms":1,"error":null}),
            )
            .unwrap();
        assert!(sync_status(&path, &config, None).unwrap()["last_success"].is_null());
        logger
            .write(Utc::now(), "pass_started", json!({"apply":true}))
            .unwrap();
        logger
            .write(
                Utc::now(),
                "pass_completed",
                json!({"duration_ms":2,"error":null}),
            )
            .unwrap();
        logger
            .write(
                Utc::now(),
                "pass_failed",
                json!({"duration_ms":3,"error":"timeout"}),
            )
            .unwrap();
        logger
            .write(
                Utc::now(),
                "command_started",
                json!({"command":"run","config":"/unrelated"}),
            )
            .unwrap();
        logger
            .write(
                Utc::now(),
                "pass_completed",
                json!({"duration_ms":4,"error":null}),
            )
            .unwrap();
        logger
            .write(
                Utc::now(),
                "command_started",
                json!({"command":"sync","config":config}),
            )
            .unwrap();
        logger
            .write(
                Utc::now(),
                "pass_completed",
                json!({"duration_ms":5,"error":null}),
            )
            .unwrap();
        let state = sync_status(&path, &config, None).unwrap();
        assert_eq!(state["last_success"]["duration_ms"], 2);
        assert_eq!(state["last_failure"]["error"], "timeout");
        assert_eq!(state["last_pass"]["duration_ms"], 3);
        fs::remove_dir_all(path).unwrap();
    }
}
