use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

#[derive(Deserialize, Serialize)]
pub struct Health {
    pub schema_version: u8,
    pub version: String,
    pub started_at: DateTime<Utc>,
    pub poll_seconds: u64,
    pub last_pass_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub error_kind: Option<String>,
    pub account: Option<String>,
}

impl Health {
    pub fn start(directory: &Path, poll_seconds: u64) -> Result<Self> {
        let previous = fs::read(directory.join("health.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|state| state.schema_version == 1);
        let state = Self {
            schema_version: 1,
            version: env!("CARGO_PKG_VERSION").into(),
            // Keep the initial grace deadline through crash loops before any success.
            started_at: previous.as_ref().map_or_else(Utc::now, |s| s.started_at),
            poll_seconds,
            last_pass_at: previous.as_ref().and_then(|s| s.last_pass_at),
            last_success_at: previous.as_ref().and_then(|s| s.last_success_at),
            error_kind: previous.as_ref().and_then(|s| s.error_kind.clone()),
            account: previous.and_then(|s| s.account),
        };
        state.publish(directory)?;
        Ok(state)
    }

    pub fn completed(&mut self, result: &Result<()>, poll_seconds: u64) {
        let now = Utc::now();
        self.poll_seconds = poll_seconds;
        self.last_pass_at = Some(now);
        self.account = None;
        self.error_kind = result.as_ref().err().map(|error| {
            if let Some(auth) = error.downcast_ref::<crate::oauth::ReauthorizationRequired>() {
                self.account = Some(auth.account.clone());
                "reauth_required".into()
            } else {
                "sync_failed".into()
            }
        });
        if result.is_ok() {
            self.last_success_at = Some(now);
        }
    }

    pub fn publish(&self, directory: &Path) -> Result<()> {
        fs::create_dir_all(directory).context("Cannot create health directory")?;
        let temporary = directory.join(format!(".health-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o644);
            }
            let mut file = options.open(&temporary)?;
            serde_json::to_writer(&mut file, self)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, directory.join("health.json"))?;
            Ok(())
        })();
        let _ = fs::remove_file(temporary);
        result.context("Cannot publish synchronization health")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_preserves_last_success_and_failed_pass_does_not_advance_it() {
        let root = std::env::temp_dir().join(format!("gallendar-health-{}", uuid::Uuid::new_v4()));
        let mut health = Health::start(&root, 1800).unwrap();
        assert!(health.last_success_at.is_none());
        let restarted = Health::start(&root, 1800).unwrap();
        assert_eq!(restarted.started_at, health.started_at);
        health.completed(&Ok(()), 1800);
        let success = health.last_success_at;
        health.completed(&Err(anyhow::anyhow!("secret diagnostic")), 900);
        health.publish(&root).unwrap();
        let restored = Health::start(&root, 900).unwrap();
        assert_eq!(restored.last_success_at, success);
        assert_eq!(restored.error_kind.as_deref(), Some("sync_failed"));
        assert!(
            !fs::read_to_string(root.join("health.json"))
                .unwrap()
                .contains("secret")
        );
        let error = anyhow::Error::new(crate::oauth::ReauthorizationRequired {
            account: "aveni".into(),
        })
        .context("Google connection failed");
        health.completed(&Err(error), 1800);
        assert_eq!(health.error_kind.as_deref(), Some("reauth_required"));
        assert_eq!(health.account.as_deref(), Some("aveni"));
        health.completed(&Ok(()), 1800);
        assert!(health.error_kind.is_none());
        assert!(health.account.is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
