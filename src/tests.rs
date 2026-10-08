use super::*;
use chrono::Duration;
use model::{Event, EventTime};
use serde_json::json;

fn event(value: Value) -> Event {
    serde_json::from_value(value).unwrap()
}

#[test]
fn demo_exercises_four_sources_move_delete_privacy_and_failed_reads() {
    let result = demo::run().unwrap();
    assert_eq!(result["initial"]["create"], 12);
    assert_eq!(result["repeat"]["unchanged"], 12);
    assert_eq!(result["reschedule"]["update"], 3);
    assert_eq!(result["deletion"]["delete"], 3);
}

#[test]
fn excludes_free_declined_and_clones_but_not_other_guests_declines() {
    let (config, memory) = demo::fixture();
    let account = &config.accounts[0];
    let original = memory.calendars[&account.source][0].clone();
    let mut free = original.clone();
    free.transparency = "transparent".into();
    assert!(
        free.busy(account, "Europe/Warsaw", config.all_day_policy)
            .unwrap()
            .is_none()
    );
    let mut declined = original.clone();
    declined.attendees.push(model::Attendee {
        email: account.owner_email.clone(),
        response_status: "declined".into(),
        ..Default::default()
    });
    assert!(
        declined
            .busy(account, "Europe/Warsaw", config.all_day_policy)
            .unwrap()
            .is_none()
    );
    declined.attendees[0].email = "someone-else@example.invalid".into();
    assert!(
        declined
            .busy(account, "Europe/Warsaw", config.all_day_policy)
            .unwrap()
            .is_some()
    );
    declined.status = "tentative".into();
    declined.attendees[0].email = account.owner_email.clone();
    declined.attendees[0].response_status = "tentative".into();
    assert!(
        declined
            .busy(account, "Europe/Warsaw", config.all_day_policy)
            .unwrap()
            .is_some()
    );
    let busy = original
        .busy(account, "Europe/Warsaw", config.all_day_policy)
        .unwrap()
        .unwrap();
    let clone = event(busy.payload("clone", &model::digest(&[&config.namespace])));
    assert!(
        clone
            .busy(account, "Europe/Warsaw", config.all_day_policy)
            .unwrap()
            .is_none()
    );
}

#[test]
fn all_day_uses_source_timezone_across_dst_and_explicit_policy() {
    let (config, _) = demo::fixture();
    let account = &config.accounts[0];
    let mut all_day = event(json!({ "id": "vacation", "eventType": "outOfOffice",
        "start": { "date": "2026-03-29" }, "end": { "date": "2026-03-30" } }));
    let busy = all_day
        .busy(
            account,
            "Europe/Warsaw",
            config::AllDayPolicy::OutOfOfficeOnly,
        )
        .unwrap()
        .unwrap();
    let start = DateTime::parse_from_rfc3339(busy.start.date_time.as_deref().unwrap()).unwrap();
    let end = DateTime::parse_from_rfc3339(busy.end.date_time.as_deref().unwrap()).unwrap();
    assert_eq!(end - start, Duration::hours(23));
    assert_eq!(start.to_rfc3339(), "2026-03-28T23:00:00+00:00");
    all_day.event_type = "default".into();
    assert!(
        all_day
            .busy(
                account,
                "Europe/Warsaw",
                config::AllDayPolicy::OutOfOfficeOnly
            )
            .unwrap()
            .is_none()
    );
    assert!(
        all_day
            .busy(account, "Europe/Warsaw", config::AllDayPolicy::Busy)
            .unwrap()
            .is_some()
    );
    all_day.event_type = "birthday".into();
    assert!(
        all_day
            .busy(account, "Europe/Warsaw", config::AllDayPolicy::Busy)
            .unwrap()
            .is_none()
    );
}

#[test]
fn moved_recurring_occurrence_keeps_identity_and_sibling_is_unchanged() {
    let (config, mut api) = demo::fixture();
    let mut occurrence = api.calendars["source-0"][0].clone();
    occurrence.recurring_event_id = Some("series".into());
    occurrence.original_start_time = Some(occurrence.start.clone());
    let mut sibling = occurrence.clone();
    sibling.id = "sibling".into();
    sibling.original_start_time.as_mut().unwrap().date_time =
        Some((Utc::now() + Duration::days(3)).to_rfc3339());
    api.calendars
        .insert("source-0".into(), vec![occurrence, sibling]);
    let window = config.window(Utc::now());
    let initial = engine::plan(&mut api, &config, &window).unwrap();
    engine::apply(&mut api, &initial).unwrap();
    let occurrence = &mut api.calendars.get_mut("source-0").unwrap()[0];
    occurrence.id = "new-api-instance-id".into();
    occurrence.start.date_time = Some((Utc::now() + Duration::days(2)).to_rfc3339());
    occurrence.end.date_time =
        Some((Utc::now() + Duration::days(2) + Duration::hours(1)).to_rfc3339());
    let moved = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(moved.counts.update, 3);
    assert_eq!(moved.counts.create, 0);
    assert_eq!(moved.counts.delete, 0);
    engine::apply(&mut api, &moved).unwrap();
    api.calendars.get_mut("source-0").unwrap()[0].status = "cancelled".into();
    let cancelled = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(cancelled.counts.delete, 3);
    assert_eq!(cancelled.counts.unchanged, 12);
}

#[test]
fn failed_or_invalid_source_and_failed_target_never_write() {
    let (config, mut api) = demo::fixture();
    let window = config.window(Utc::now());
    let plan = engine::plan(&mut api, &config, &window).unwrap();
    engine::apply(&mut api, &plan).unwrap();
    let before = api.writes;
    api.failing_calendar = Some("source-2".into());
    assert!(engine::plan(&mut api, &config, &window).is_err());
    api.failing_calendar = Some("clone-3".into());
    assert!(engine::plan(&mut api, &config, &window).is_err());
    api.failing_calendar = None;
    api.calendars.get_mut("source-0").unwrap()[0].end = EventTime::default();
    assert!(engine::plan(&mut api, &config, &window).is_err());
    assert_eq!(api.writes, before);
}

#[test]
fn preserves_foreign_events_and_repairs_polluted_owned_copies() {
    let (config, mut api) = demo::fixture();
    let window = config.window(Utc::now());
    let initial = engine::plan(&mut api, &config, &window).unwrap();
    engine::apply(&mut api, &initial).unwrap();
    let mut foreign = api.calendars["clone-0"][0].clone();
    foreign.id = "foreign".into();
    foreign
        .extended_properties
        .private
        .insert(model::NAMESPACE.into(), "another-instance".into());
    api.calendars.get_mut("clone-0").unwrap().push(foreign);
    api.calendars.get_mut("clone-0").unwrap()[0]
        .extra
        .insert("description".into(), json!("sensitive accidental edit"));
    let mut duplicate = api.calendars["clone-0"][1].clone();
    duplicate.id = "duplicate".into();
    api.calendars.get_mut("clone-0").unwrap().push(duplicate);
    let repair = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(repair.counts.update, 1);
    assert_eq!(repair.counts.delete, 1);
    engine::apply(&mut api, &repair).unwrap();
    assert!(api.calendars["clone-0"].iter().any(|e| e.id == "foreign"));
    assert!(
        engine::plan(&mut api, &config, &window)
            .unwrap()
            .actions
            .is_empty()
    );
}

#[test]
fn config_rejects_feedback_paths_and_missing_operator_choices() {
    let (mut config, _) = demo::fixture();
    config.validate().unwrap();
    config.accounts[0].target = config.accounts[1].source.clone();
    assert!(config.validate().is_err());
    let incomplete = config::EXAMPLE.replace(r#"all_day_policy = "out_of_office_only""#, "");
    assert!(toml::from_str::<Config>(&incomplete).is_err());
}

#[test]
fn payload_contains_no_source_identifiers_or_private_content() {
    let (config, api) = demo::fixture();
    let original = &api.calendars["source-0"][0];
    let busy = original
        .busy(&config.accounts[0], "Europe/Warsaw", config.all_day_policy)
        .unwrap()
        .unwrap();
    let payload = busy.payload("new-id", &model::digest(&[&config.namespace]));
    let body = payload.to_string();
    for forbidden in [
        "Confidential",
        "Private",
        "private-link",
        "source-0",
        "event-0",
        "owner-0",
    ] {
        assert!(!body.contains(forbidden), "Leaked {forbidden}");
    }
    assert_eq!(
        payload["extendedProperties"]["private"][model::MANAGED],
        "v1"
    );
}

#[test]
fn uncertain_create_outcome_recovers_without_duplicates_or_early_deletes() {
    struct UnknownOutcome {
        inner: demo::Memory,
    }
    impl engine::CalendarApi for UnknownOutcome {
        fn source(&mut self, calendar: &str, window: &config::Window) -> Result<model::Snapshot> {
            self.inner.source(calendar, window)
        }
        fn clones(&mut self, calendar: &str, namespace: &str) -> Result<model::Snapshot> {
            self.inner.clones(calendar, namespace)
        }
        fn create(&mut self, calendar: &str, busy: &model::Busy, namespace: &str) -> Result<()> {
            self.inner.create(calendar, busy, namespace)?;
            bail!("Response lost after Google saved the clone")
        }
        fn update(
            &mut self,
            calendar: &str,
            id: &str,
            busy: &model::Busy,
            namespace: &str,
        ) -> Result<()> {
            self.inner.update(calendar, id, busy, namespace)
        }
        fn delete(&mut self, calendar: &str, id: &str, namespace: &str) -> Result<()> {
            self.inner.delete(calendar, id, namespace)
        }
    }
    let (config, mut memory) = demo::fixture();
    let window = config.window(Utc::now());
    let initial = engine::plan(&mut memory, &config, &window).unwrap();
    engine::apply(&mut memory, &initial).unwrap();
    memory.calendars.get_mut("source-0").unwrap()[0].id = "replacement-event".into();
    let replacement = engine::plan(&mut memory, &config, &window).unwrap();
    assert_eq!(replacement.counts.create, 3);
    assert_eq!(replacement.counts.delete, 3);
    let mut fault = UnknownOutcome { inner: memory };
    assert!(engine::apply(&mut fault, &replacement).is_err());
    let managed_after_failure: usize = config
        .accounts
        .iter()
        .map(|a| fault.inner.calendars[&a.target].len())
        .sum();
    assert_eq!(managed_after_failure, 13); // Original 12 are retained plus the saved new copy.
    let recovered = engine::plan(&mut fault.inner, &config, &window).unwrap();
    assert_eq!(recovered.counts.create, 2);
    engine::apply(&mut fault.inner, &recovered).unwrap();
    assert!(
        engine::plan(&mut fault.inner, &config, &window)
            .unwrap()
            .actions
            .is_empty()
    );
    assert_eq!(
        config
            .accounts
            .iter()
            .map(|a| fault.inner.calendars[&a.target].len())
            .sum::<usize>(),
        12
    );
}
