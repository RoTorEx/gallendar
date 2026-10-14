use crate::{
    config::{Config, Window},
    model::{Busy, Event, Snapshot, digest},
};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

pub trait CalendarApi {
    fn source(&mut self, calendar: &str, window: &Window) -> Result<Snapshot>;
    fn clones(&mut self, calendar: &str, namespace: &str) -> Result<Snapshot>;
    fn create(&mut self, calendar: &str, busy: &Busy, namespace: &str) -> Result<()>;
    fn update(&mut self, calendar: &str, id: &str, busy: &Busy, namespace: &str) -> Result<()>;
    fn delete(&mut self, calendar: &str, id: &str, namespace: &str) -> Result<()>;
}

#[derive(Clone)]
pub enum Action {
    Create {
        target: String,
        busy: Busy,
    },
    Update {
        target: String,
        id: String,
        busy: Busy,
    },
    Delete {
        target: String,
        id: String,
    },
}

#[derive(Default, Serialize, PartialEq, Eq)]
pub struct Counts {
    pub create: usize,
    pub update: usize,
    pub delete: usize,
    pub unchanged: usize,
}

pub struct Plan {
    pub actions: Vec<Action>,
    pub counts: Counts,
    pub namespace: String,
}

fn is_past(event: &Event, window: &Window) -> Result<bool> {
    let end = event
        .end
        .date_time
        .as_deref()
        .context("Managed event has no end time; no changes applied")?;
    let end = DateTime::parse_from_rfc3339(end)
        .context("Managed event has an invalid end time; no changes applied")?
        .with_timezone(&Utc);
    Ok(end <= window.start)
}

pub fn plan(api: &mut impl CalendarApi, config: &Config, window: &Window) -> Result<Plan> {
    let namespace = digest(&[&config.namespace]);
    let source_accounts = config.sources();
    let target_accounts = config.targets();
    let routes = config.routes();
    let mut sources = Vec::new();
    // Read every source completely before considering any mutation.
    for account in &source_accounts {
        let snapshot = api.source(&account.source, window).with_context(|| {
            format!("Cannot read source '{}'; no changes applied", account.name)
        })?;
        ensure!(
            matches!(
                snapshot.access_role.as_str(),
                "reader" | "writer" | "writerWithoutPrivateAccess" | "owner"
            ),
            "Source '{}' needs event read access, not just free/busy; no changes applied",
            account.name
        );
        let mut events = BTreeMap::new();
        for event in snapshot.events {
            if let Some(busy) = event
                .busy(account, &snapshot.timezone, config.all_day_policy)
                .with_context(|| {
                    format!(
                        "Invalid source data for '{}'; no changes applied",
                        account.name
                    )
                })?
            {
                events.insert(busy.key.clone(), (busy, event.summary.clone()));
            }
        }
        sources.push(events);
    }
    // Read every target as well. Do not apply a partial plan if any read fails.
    let mut targets = Vec::new();
    for account in &target_accounts {
        let snapshot = api.clones(&account.target, &namespace).with_context(|| {
            format!("Cannot read target '{}'; no changes applied", account.name)
        })?;
        ensure!(
            matches!(
                snapshot.access_role.as_str(),
                "writer" | "writerWithoutPrivateAccess" | "owner"
            ),
            "Target '{}' needs write access; no changes applied",
            account.name
        );
        targets.push(snapshot);
    }
    // Migration reads finish before any write. Only managed events are eligible.
    let mut actions = Vec::new();
    let mut counts = Counts::default();
    for account in &target_accounts {
        for calendar in &account.retired_targets {
            let snapshot = api
                .clones(calendar, &namespace)
                .context("Cannot read retired target; no changes applied")?;
            ensure!(
                snapshot.access_role == "owner",
                "Retired target must be owned; no changes applied"
            );
            for event in snapshot.events {
                if event.owned(&namespace) && event.status != "cancelled" {
                    if is_past(&event, window)? {
                        continue;
                    }
                    ensure!(
                        !event.id.is_empty(),
                        "Retired target has an incomplete managed event; no changes applied"
                    );
                    counts.delete += 1;
                    actions.push(Action::Delete {
                        target: calendar.clone(),
                        id: event.id,
                    });
                }
            }
        }
    }
    for (index, account) in target_accounts.iter().enumerate() {
        let mut desired = BTreeMap::new();
        for (source_index, events) in sources.iter().enumerate() {
            if let Some(route) = routes
                .iter()
                .find(|r| r.from == source_accounts[source_index].name && r.to == account.name)
            {
                for (key, (busy, original_title)) in events {
                    let mut copy = busy.clone();
                    let title = match route.mode {
                        crate::config::MirrorMode::Hidden => route.hidden_title.clone(),
                        crate::config::MirrorMode::Visible
                        | crate::config::MirrorMode::LegacyOriginal => original_title.clone(),
                    };
                    copy.summary = format!("{}{title}", route.prefix);
                    copy.visibility = match route.mode {
                        crate::config::MirrorMode::Visible => "public",
                        crate::config::MirrorMode::Hidden => "private",
                        crate::config::MirrorMode::LegacyOriginal => "default",
                    }
                    .into();
                    copy.attendees = route
                        .attendees
                        .iter()
                        .map(|email| email.to_ascii_lowercase())
                        .collect();
                    copy.attendees.sort();
                    desired.insert(key.clone(), copy);
                }
            }
        }
        let mut seen = HashSet::new();
        let mut existing = targets[index].events.clone();
        existing.sort_by(|a, b| a.id.cmp(&b.id));
        for event in existing {
            if !event.owned(&namespace) || event.status == "cancelled" {
                continue;
            }
            ensure!(
                !event.id.is_empty(),
                "Target has an incomplete managed event; no changes applied"
            );
            // Past copies are historical records and are never mutated by a pass.
            if is_past(&event, window)? {
                continue;
            }
            // Future copies outside the configured horizon are cleaned up.
            if let Some(busy) = event.key().and_then(|key| desired.get(key))
                && seen.insert(busy.key.clone())
            {
                if event.matches(busy, &namespace) {
                    counts.unchanged += 1;
                } else {
                    counts.update += 1;
                    actions.push(Action::Update {
                        target: account.target.clone(),
                        id: event.id,
                        busy: busy.clone(),
                    });
                }
                continue;
            }
            counts.delete += 1;
            actions.push(Action::Delete {
                target: account.target.clone(),
                id: event.id,
            });
        }
        for (key, busy) in desired {
            if !seen.contains(&key) {
                counts.create += 1;
                actions.push(Action::Create {
                    target: account.target.clone(),
                    busy,
                });
            }
        }
    }
    // Fill new occupied time before freeing old time. An error stops the pass.
    actions.sort_by_key(|action| matches!(action, Action::Delete { .. }));
    Ok(Plan {
        actions,
        counts,
        namespace,
    })
}

pub fn apply(api: &mut impl CalendarApi, plan: &Plan) -> Result<()> {
    apply_with_reporter(api, plan, |_, _, _| Ok(()))
}

pub fn apply_with_reporter(
    api: &mut impl CalendarApi,
    plan: &Plan,
    mut report: impl FnMut(&Action, &str, Option<&anyhow::Error>) -> Result<()>,
) -> Result<()> {
    for (index, action) in plan.actions.iter().enumerate() {
        report(action, "operation_started", None)?;
        let result = match action {
            Action::Create { target, busy } => api.create(target, busy, &plan.namespace),
            Action::Update { target, id, busy } => api.update(target, id, busy, &plan.namespace),
            Action::Delete { target, id } => api.delete(target, id, &plan.namespace),
        };
        match &result {
            Ok(()) => report(action, "operation_verified", None)?,
            Err(error) => report(action, "operation_failed", Some(error))?,
        }
        result.with_context(|| format!("Stopped at operation {} of {}; earlier verified changes remain, rerun sync to reconcile", index + 1, plan.actions.len()))?;
    }
    Ok(())
}
