use crate::{
    config::{Config, Window},
    model::{Busy, Snapshot, digest},
};
use anyhow::{Context, Result, ensure};
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

pub fn plan(api: &mut impl CalendarApi, config: &Config, window: &Window) -> Result<Plan> {
    let namespace = digest(&[&config.namespace]);
    let mut sources = Vec::new();
    // Read every source completely before considering any mutation.
    for account in &config.accounts {
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
                events.insert(busy.key.clone(), busy);
            }
        }
        sources.push(events);
    }
    // Read every target as well. Do not apply a partial plan if any read fails.
    let mut targets = Vec::new();
    for account in &config.accounts {
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
    let mut actions = Vec::new();
    let mut counts = Counts::default();
    for (index, account) in config.accounts.iter().enumerate() {
        let mut desired = BTreeMap::new();
        for (source_index, events) in sources.iter().enumerate() {
            if index != source_index {
                desired.extend(events.clone());
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
            // Expired clones are removed as the rolling horizon advances. All reads
            // have succeeded; events outside the selected horizon are not protected.
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
    for (index, action) in plan.actions.iter().enumerate() {
        let result = match action {
            Action::Create { target, busy } => api.create(target, busy, &plan.namespace),
            Action::Update { target, id, busy } => api.update(target, id, busy, &plan.namespace),
            Action::Delete { target, id } => api.delete(target, id, &plan.namespace),
        };
        result.with_context(|| format!("Stopped at operation {} of {}; earlier verified changes remain, rerun sync to reconcile", index + 1, plan.actions.len()))?;
    }
    Ok(())
}
