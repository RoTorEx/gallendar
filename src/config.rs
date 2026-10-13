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
    #[serde(default)]
    pub service_account_key: PathBuf,
    #[serde(default)]
    pub auth: AuthMode,
    pub oauth: Option<OAuthConfig>,
    pub namespace: String,
    pub horizon_days: u32,
    pub poll_seconds: u64,
    pub all_day_policy: AllDayPolicy,
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub routes: Option<Vec<Route>>,
    #[serde(default = "auto_update_enabled")]
    pub auto_update: bool,
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
    pub auth: Option<AuthMode>,
    pub name: String,
    pub owner_email: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub target: String,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    #[default]
    ServiceAccount,
    Oauth,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthConfig {
    pub client: PathBuf,
    pub tokens_dir: PathBuf,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub mode: MirrorMode,
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default)]
    pub attendees: Vec<String>,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MirrorMode {
    #[default]
    Masked,
    Original,
}

fn auto_update_enabled() -> bool {
    true
}

fn default_title() -> String {
    "Busy".into()
}

impl Config {
    pub fn routes(&self) -> Vec<Route> {
        self.routes.clone().unwrap_or_else(|| {
            self.accounts
                .iter()
                .flat_map(|from| {
                    self.accounts
                        .iter()
                        .filter(move |to| from.name != to.name)
                        .map(move |to| Route {
                            from: from.name.clone(),
                            to: to.name.clone(),
                            mode: MirrorMode::Masked,
                            title: default_title(),
                            attendees: Vec::new(),
                        })
                })
                .collect()
        })
    }

    pub fn sources(&self) -> Vec<&Account> {
        let routes = self.routes();
        self.accounts
            .iter()
            .filter(|a| routes.iter().any(|r| r.from == a.name))
            .collect()
    }

    pub fn targets(&self) -> Vec<&Account> {
        let routes = self.routes();
        self.accounts
            .iter()
            .filter(|a| routes.iter().any(|r| r.to == a.name))
            .collect()
    }

    pub fn load(path: &Path) -> Result<Self> {
        let config = Self::load_for_auth(path)?;
        config.validate()?;
        Ok(config)
    }

    pub fn load_for_auth(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .context("Cannot read config; run `gallendar init` first")?;
        let mut config: Self = toml::from_str(&contents).map_err(|_| {
            anyhow::anyhow!(
                "Invalid config; compare with gallendar.example.toml (values omitted for privacy)"
            )
        })?;
        if config.service_account_key.is_relative() {
            config.service_account_key = path
                .parent()
                .unwrap_or(Path::new("."))
                .join(&config.service_account_key);
        }
        if let Some(oauth) = &mut config.oauth {
            let parent = path.parent().unwrap_or(Path::new("."));
            if oauth.client.is_relative() {
                oauth.client = parent.join(&oauth.client);
            }
            if oauth.tokens_dir.is_relative() {
                oauth.tokens_dir = parent.join(&oauth.tokens_dir);
            }
        }
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            (2..=4).contains(&self.accounts.len()),
            "Configure between 2 and 4 accounts"
        );
        ensure!(
            !self.namespace.trim().is_empty(),
            "namespace must not be empty"
        );
        ensure!(
            (1..=30).contains(&self.horizon_days),
            "horizon_days must be between 1 and 30"
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
            for id in [&account.source, &account.target]
                .into_iter()
                .filter(|id| !id.is_empty())
            {
                ensure!(
                    !id.trim().is_empty() && id != "primary" && !id.contains("REPLACE"),
                    "Use explicit calendar IDs, not 'primary' or example placeholders"
                );
            }
            ensure!(
                account.source.is_empty() || sources.insert(&account.source),
                "Source calendars must be unique"
            );
            ensure!(
                account.target.is_empty() || targets.insert(&account.target),
                "Target calendars must be unique"
            );
        }
        let routes = self.routes();
        ensure!(!routes.is_empty(), "Configure at least one route");
        let mut pairs = HashSet::new();
        for route in &routes {
            ensure!(
                !matches!(route.mode, MirrorMode::Masked) || !route.title.trim().is_empty(),
                "Masked route title must not be empty"
            );
            let from = self
                .accounts
                .iter()
                .find(|a| a.name == route.from)
                .context("Route refers to an unknown source account")?;
            let to = self
                .accounts
                .iter()
                .find(|a| a.name == route.to)
                .context("Route refers to an unknown target account")?;
            ensure!(
                from.name != to.name,
                "A route cannot mirror an account into itself"
            );
            ensure!(
                !from.source.is_empty(),
                "Route source needs a source calendar ID"
            );
            ensure!(
                !to.target.is_empty(),
                "Route target needs a target calendar ID"
            );
            if !route.attendees.is_empty() {
                ensure!(
                    to.auth.unwrap_or(self.auth) == AuthMode::Oauth,
                    "Route attendees require OAuth on the destination account"
                );
            }
            let mut guests = HashSet::new();
            for email in &route.attendees {
                let valid = email.split_once('@').is_some_and(|(local, domain)| {
                    !local.is_empty()
                        && !domain.is_empty()
                        && !local.starts_with('.')
                        && !local.ends_with('.')
                        && !local.contains("..")
                        && local.bytes().all(|b| {
                            b.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&b)
                        })
                        && domain.split('.').all(|label| {
                            !label.is_empty()
                                && !label.starts_with('-')
                                && !label.ends_with('-')
                                && label
                                    .bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        })
                });
                ensure!(
                    valid,
                    "Route attendees must contain plain valid email addresses"
                );
                ensure!(
                    guests.insert(email.to_ascii_lowercase()),
                    "Duplicate route attendee"
                );
                ensure!(
                    !self
                        .accounts
                        .iter()
                        .any(|a| a.owner_email.eq_ignore_ascii_case(email)
                            || a.source.eq_ignore_ascii_case(email)
                            || a.target.eq_ignore_ascii_case(email)),
                    "Route attendees cannot include configured accounts; this could create invitation feedback loops"
                );
            }
            ensure!(pairs.insert((&route.from, &route.to)), "Duplicate route");
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
