use crate::{
    config::{Account, AllDayPolicy, Config, Window},
    engine::{self, CalendarApi},
    model::{Busy, Event, Snapshot},
};
use anyhow::{Result, ensure};
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Default)]
pub struct Memory {
    pub calendars: BTreeMap<String, Vec<Event>>,
    pub failing_calendar: Option<String>,
    pub writes: usize,
}

impl Memory {
    fn snapshot(&self, calendar: &str) -> Result<Snapshot> {
        ensure!(
            self.failing_calendar.as_deref() != Some(calendar),
            "Simulated read failure"
        );
        Ok(Snapshot {
            events: self.calendars.get(calendar).cloned().unwrap_or_default(),
            timezone: "Europe/Warsaw".into(),
            access_role: "owner".into(),
        })
    }
}

impl CalendarApi for Memory {
    fn source(&mut self, calendar: &str, _window: &Window) -> Result<Snapshot> {
        self.snapshot(calendar)
    }
    fn clones(&mut self, calendar: &str, _namespace: &str) -> Result<Snapshot> {
        self.snapshot(calendar)
    }
    fn create(&mut self, calendar: &str, busy: &Busy, namespace: &str) -> Result<()> {
        self.writes += 1;
        let event =
            serde_json::from_value(busy.payload(&format!("demo{}", self.writes), namespace))?;
        self.calendars
            .entry(calendar.into())
            .or_default()
            .push(event);
        Ok(())
    }
    fn update(&mut self, calendar: &str, id: &str, busy: &Busy, namespace: &str) -> Result<()> {
        self.writes += 1;
        let events = self.calendars.get_mut(calendar).unwrap();
        let index = events.iter().position(|e| e.id == id).unwrap();
        events[index] = serde_json::from_value(busy.payload(id, namespace))?;
        Ok(())
    }
    fn delete(&mut self, calendar: &str, id: &str, _namespace: &str) -> Result<()> {
        self.writes += 1;
        self.calendars
            .get_mut(calendar)
            .unwrap()
            .retain(|e| e.id != id);
        Ok(())
    }
}

pub fn fixture() -> (Config, Memory) {
    let accounts: Vec<_> = (0..4)
        .map(|i| Account {
            auth: None,
            name: format!("account-{i}"),
            owner_email: format!("owner-{i}@example.invalid"),
            source: format!("source-{i}"),
            target: format!("clone-{i}"),
        })
        .collect();
    let mut memory = Memory::default();
    for (i, account) in accounts.iter().enumerate() {
        let start = Utc::now() + Duration::days(1) + Duration::hours(i as i64);
        let event = serde_json::from_value(json!({
            "id": format!("event-{i}"), "summary": "Confidential source title",
            "description": "Private context", "location": "Private address",
            "conferenceData": { "conferenceId": "private-link" },
            "status": "confirmed", "transparency": "opaque",
            "start": { "dateTime": start.to_rfc3339() },
            "end": { "dateTime": (start + Duration::minutes(30)).to_rfc3339() }
        }))
        .unwrap();
        memory.calendars.insert(account.source.clone(), vec![event]);
    }
    (
        Config {
            auth: Default::default(),
            oauth: None,
            service_account_key: PathBuf::new(),
            namespace: "demo".into(),
            horizon_days: 30,
            poll_seconds: 60,
            all_day_policy: AllDayPolicy::Busy,
            accounts,
            routes: None,
            auto_update: false,
        },
        memory,
    )
}

pub fn run() -> Result<Value> {
    let (config, mut memory) = fixture();
    let window = config.window(Utc::now());
    let created = engine::plan(&mut memory, &config, &window)?;
    ensure!(created.counts.create == 12, "Demo four-way coverage failed");
    engine::apply(&mut memory, &created)?;
    let repeated = engine::plan(&mut memory, &config, &window)?;
    ensure!(repeated.actions.is_empty(), "Demo idempotency failed");
    let event = &mut memory.calendars.get_mut("source-0").unwrap()[0];
    let moved_start = Utc::now() + Duration::days(2);
    event.start.date_time = Some(moved_start.to_rfc3339());
    event.end.date_time = Some((moved_start + Duration::minutes(30)).to_rfc3339());
    let moved = engine::plan(&mut memory, &config, &window)?;
    ensure!(moved.counts.update == 3, "Demo move failed");
    engine::apply(&mut memory, &moved)?;
    memory.calendars.get_mut("source-0").unwrap().clear();
    let deleted = engine::plan(&mut memory, &config, &window)?;
    ensure!(deleted.counts.delete == 3, "Demo deletion failed");
    engine::apply(&mut memory, &deleted)?;
    for account in &config.accounts {
        for event in memory.calendars.get(&account.target).unwrap() {
            ensure!(
                event.summary == "Busy"
                    && event.attendees.is_empty()
                    && !event.extra.contains_key("description")
                    && !event.extra.contains_key("location")
                    && !event.extra.contains_key("conferenceData"),
                "Demo privacy failed"
            );
        }
    }
    let before = memory.writes;
    memory.failing_calendar = Some("source-1".into());
    ensure!(
        engine::plan(&mut memory, &config, &window).is_err() && memory.writes == before,
        "Demo read failure safety failed"
    );
    Ok(json!({ "mode": "offline-demo", "google_validated": false,
        "initial": created.counts, "repeat": repeated.counts,
        "reschedule": moved.counts, "deletion": deleted.counts,
        "privacy": "passed", "source_read_failure": "no writes" }))
}
