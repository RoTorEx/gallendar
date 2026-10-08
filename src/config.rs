use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub const EXAMPLE: &str = include_str!("../gallendar.example.toml");

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub service_account_key: PathBuf,
    pub namespace: String,
    pub horizon_days: u32,
    pub poll_seconds: u64,
    pub all_day_policy: AllDayPolicy,
    pub accounts: Vec<Account>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllDayPolicy {
    Busy,
    OutOfOfficeOnly,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub name: String,
    pub owner_email: String,
    pub source: String,
    pub target: String,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .context("Cannot read config; run `gallendar init` first")?;
        let mut config: Self = toml::from_str(&contents).map_err(|_| {
            anyhow::anyhow!(
                "Invalid config; compare with gallendar.example.toml (values omitted for privacy)"
            )
        })?;
        config.validate()?;
        if config.service_account_key.is_relative() {
            config.service_account_key = path
                .parent()
                .unwrap_or(Path::new("."))
                .join(&config.service_account_key);
        }
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            (2..=4).contains(&self.accounts.len()),
            "Configure 2 accounts for the experiment or 4 for the MVP (3 is also supported)"
        );
        ensure!(
            !self.namespace.trim().is_empty(),
            "namespace must not be empty"
        );
        ensure!(
            (1..=3650).contains(&self.horizon_days),
            "horizon_days must be between 1 and 3650"
        );
        ensure!(
            (1..=120).contains(&self.poll_seconds),
            "poll_seconds must be between 1 and 120"
        );
        let mut sources = HashSet::new();
        let mut targets = HashSet::new();
        let mut names = HashSet::new();
        let mut owners = HashSet::new();
        for account in &self.accounts {
            ensure!(
                !account.name.is_empty()
                    && account
                        .name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "Account names must contain only ASCII letters, numbers, '-' or '_'"
            );
            ensure!(names.insert(&account.name), "Account names must be unique");
            ensure!(
                account.owner_email.contains('@')
                    && owners.insert(account.owner_email.to_lowercase()),
                "Each account needs a unique owner_email"
            );
            for id in [&account.source, &account.target] {
                ensure!(
                    !id.trim().is_empty() && id != "primary" && !id.contains("REPLACE"),
                    "Use explicit calendar IDs, not 'primary' or example placeholders"
                );
            }
            ensure!(
                sources.insert(&account.source),
                "Source calendars must be unique"
            );
            ensure!(
                targets.insert(&account.target),
                "Target calendars must be unique"
            );
        }
        ensure!(
            sources.is_disjoint(&targets),
            "Clone calendars must be separate from every source calendar"
        );
        Ok(())
    }

    pub fn window(&self, now: DateTime<Utc>) -> Window {
        Window {
            start: now,
            end: now + Duration::days(i64::from(self.horizon_days)),
        }
    }
}

#[derive(Clone)]
pub struct Window {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}
