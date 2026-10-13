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
    let mut audit = Vec::new();
    assert!(
        engine::apply_with_reporter(&mut fault, &replacement, |_, stage, error| {
            audit.push((stage.to_owned(), error.map(|e| e.to_string())));
            Ok(())
        })
        .is_err()
    );
    assert_eq!(audit.len(), 2);
    assert_eq!(audit[0].0, "operation_started");
    assert_eq!(audit[1].0, "operation_failed");
    assert!(audit[1].1.as_ref().unwrap().contains("Response lost"));
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

fn mirror_fixture() -> (Config, demo::Memory) {
    let (mut config, api) = demo::fixture();
    config.accounts.truncate(3);
    config.accounts[0].target.clear();
    config.accounts[1].target.clear();
    config.accounts[2].source.clear();
    config.routes = Some(vec![
        config::Route {
            from: "account-0".into(),
            to: "account-2".into(),
            mode: config::MirrorMode::Masked,
            title: "Aveni Busy".into(),
            prefix: String::new(),
            attendees: Vec::new(),
        },
        config::Route {
            from: "account-1".into(),
            to: "account-2".into(),
            mode: config::MirrorMode::Original,
            title: "Busy".into(),
            prefix: String::new(),
            attendees: Vec::new(),
        },
    ]);
    config.validate().unwrap();
    (config, api)
}

#[test]
fn directed_mirror_titles_moves_deletes_and_mode_changes() {
    let (mut config, mut api) = mirror_fixture();
    // Same IDs and overlapping intervals in distinct sources remain independent.
    let second = api.calendars["source-0"][0].clone();
    api.calendars.insert("source-1".into(), vec![second]);
    api.calendars.get_mut("source-1").unwrap()[0].summary = "Thira meeting".into();
    // A read of Innowise as a source would fail: it must only be a destination.
    api.failing_calendar = Some("source-2".into());
    let window = config.window(Utc::now());
    let initial = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(initial.counts.create, 2);
    engine::apply(&mut api, &initial).unwrap();
    assert!(!api.calendars.contains_key("clone-0"));
    assert!(!api.calendars.contains_key("clone-1"));
    let titles: Vec<_> = api.calendars["clone-2"]
        .iter()
        .map(|e| e.summary.as_str())
        .collect();
    assert!(titles.contains(&"Aveni Busy"));
    assert!(titles.contains(&"Thira meeting"));
    assert!(
        api.calendars["clone-2"]
            .iter()
            .all(|e| e.attendees.is_empty()
                && !e.extra.contains_key("description")
                && !e.extra.contains_key("conferenceData"))
    );
    assert!(
        engine::plan(&mut api, &config, &window)
            .unwrap()
            .actions
            .is_empty()
    );
    api.calendars.get_mut("source-1").unwrap()[0].summary = "Renamed Thira".into();
    let renamed = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(renamed.counts.update, 1);
    engine::apply(&mut api, &renamed).unwrap();
    config.routes.as_mut().unwrap()[1].mode = config::MirrorMode::Masked;
    config.routes.as_mut().unwrap()[1].title = "Unavailable".into();
    let masked = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(masked.counts.update, 1);
    engine::apply(&mut api, &masked).unwrap();
    assert!(
        !api.calendars["clone-2"]
            .iter()
            .any(|e| e.summary.contains("Thira"))
    );
    api.calendars.get_mut("source-0").unwrap()[0]
        .start
        .date_time = Some((Utc::now() + Duration::days(2)).to_rfc3339());
    api.calendars.get_mut("source-0").unwrap()[0].end.date_time =
        Some((Utc::now() + Duration::days(2) + Duration::hours(1)).to_rfc3339());
    let moved = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(moved.counts.update, 1);
    engine::apply(&mut api, &moved).unwrap();
    api.calendars.get_mut("source-0").unwrap().clear();
    let deleted = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(deleted.counts.delete, 1);
    assert_eq!(deleted.counts.unchanged, 1);
    engine::apply(&mut api, &deleted).unwrap();
    assert_eq!(api.calendars["clone-2"].len(), 1);
    api.failing_calendar = Some("source-1".into());
    let writes = api.writes;
    assert!(engine::plan(&mut api, &config, &window).is_err());
    assert_eq!(api.writes, writes);
}

#[test]
fn route_validation_and_new_toml_template() {
    let filled = config::EXAMPLE.replace("REPLACE-", "test-");
    let template: Config = toml::from_str(&filled).unwrap();
    template.validate().unwrap();
    assert_eq!(template.sources().len(), 2);
    assert_eq!(template.targets()[0].name, "innowise");
    let (config, _) = mirror_fixture();
    for case in 0..6 {
        let mut invalid = config.clone();
        let routes = invalid.routes.as_mut().unwrap();
        match case {
            0 => routes[0].from = "unknown".into(),
            1 => routes[0].to = routes[0].from.clone(),
            2 => routes.push(routes[0].clone()),
            3 => routes[0].to = "account-1".into(), // no target calendar
            4 => routes[0].title = "  ".into(),
            _ => routes.clear(),
        }
        assert!(invalid.validate().is_err());
    }
    let bad_mode = filled.replace("mode = \"masked\"", "mode = \"typo\"");
    assert!(toml::from_str::<Config>(&bad_mode).is_err());
    // Removing one route cleans up only that source's copies in the retained target.
    let (mut config, mut api) = mirror_fixture();
    let window = config.window(Utc::now());
    let initial = engine::plan(&mut api, &config, &window).unwrap();
    engine::apply(&mut api, &initial).unwrap();
    config.routes.as_mut().unwrap().remove(0);
    api.failing_calendar = Some("source-0".into());
    let removed = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(removed.counts.delete, 1);
    assert_eq!(removed.counts.unchanged, 1);
}

#[test]
fn basic_commands_parse_without_calendar_credentials() {
    for args in [
        vec!["gallendar", "--help"],
        vec!["gallendar", "--version"],
        vec!["gallendar", "version"],
        vec!["gallendar", "home"],
        vec!["gallendar", "config"],
        vec!["gallendar", "config", "--edit"],
        vec!["gallendar", "setup", "--no-edit"],
        vec!["gallendar", "init", "--no-edit"],
        vec!["gallendar", "setup-config", "--no-edit"],
        vec!["gallendar", "update"],
        vec!["gallendar", "update", "--automatic"],
    ] {
        if args[1].starts_with("--") {
            let error = Cli::try_parse_from(args).err().unwrap();
            assert!(matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ));
        } else {
            assert!(Cli::try_parse_from(args).is_ok());
        }
    }
}

#[test]
fn oauth_configuration_keeps_legacy_and_mixed_modes() {
    let legacy: Config = toml::from_str(config::EXAMPLE).unwrap();
    assert!(legacy.auth == config::AuthMode::ServiceAccount);
    let text = config::EXAMPLE
        .replace("auth = \"service_account\"", "auth = \"oauth\"")
        .replace("# [oauth]", "[oauth]")
        .replace("# client =", "client =")
        .replace("# tokens_dir =", "tokens_dir =")
        .replace(
            "name = \"innowise\"",
            "name = \"innowise\"\nauth = \"service_account\"",
        );
    let oauth: Config = toml::from_str(&text).unwrap();
    assert!(oauth.auth == config::AuthMode::Oauth);
    assert!(oauth.accounts[2].auth == Some(config::AuthMode::ServiceAccount));
    assert!(oauth.oauth.is_some());
    assert!(
        Cli::try_parse_from([
            "gallendar",
            "auth",
            "login",
            "--account",
            "aveni",
            "--no-browser"
        ])
        .is_ok()
    );
    assert!(Cli::try_parse_from(["gallendar", "auth", "status", "--json"]).is_ok());
}

#[test]
fn route_prefixes_update_existing_copies_without_changing_sources_or_guest_responses() {
    let (mut config, mut api) = mirror_fixture();
    config.auth = config::AuthMode::Oauth;
    let source_before = serde_json::to_value(&api.calendars["source-0"]).unwrap();
    config.routes.as_mut().unwrap()[0].attendees = vec!["guest@example.com".into()];
    config.routes.as_mut().unwrap()[0].prefix = "[Aveni] ".into();
    config.routes.as_mut().unwrap()[1].prefix = "[Thira] ".into();
    let window = config.window(Utc::now());
    let plan = engine::plan(&mut api, &config, &window).unwrap();
    engine::apply(&mut api, &plan).unwrap();
    let copies = api.calendars.get_mut("clone-2").unwrap();
    let invited = copies.iter_mut().find(|e| !e.attendees.is_empty()).unwrap();
    assert_eq!(invited.summary, "[Aveni] Aveni Busy");
    invited.attendees[0].response_status = "accepted".into();
    let ids: Vec<_> = copies.iter().map(|e| e.id.clone()).collect();
    assert!(copies.iter().any(|e| e.summary.starts_with("[Thira] ")));
    assert_eq!(
        engine::plan(&mut api, &config, &window)
            .unwrap()
            .counts
            .unchanged,
        2
    );
    config.routes.as_mut().unwrap()[0].mode = config::MirrorMode::Original;
    config.routes.as_mut().unwrap()[0].prefix = "Project / ".into();
    config.routes.as_mut().unwrap()[1].prefix.clear();
    let changed = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(changed.counts.update, 2);
    assert_eq!(changed.counts.create, 0);
    assert_eq!(changed.counts.delete, 0);
    engine::apply(&mut api, &changed).unwrap();
    let copies = &api.calendars["clone-2"];
    assert_eq!(copies.iter().map(|e| e.id.clone()).collect::<Vec<_>>(), ids);
    let invited = copies.iter().find(|e| !e.attendees.is_empty()).unwrap();
    assert_eq!(
        invited.summary,
        format!("Project / {}", api.calendars["source-0"][0].summary)
    );
    assert_eq!(invited.attendees[0].response_status, "accepted");
    assert_eq!(
        serde_json::to_value(&api.calendars["source-0"]).unwrap(),
        source_before
    );
    assert!(
        engine::plan(&mut api, &config, &window)
            .unwrap()
            .actions
            .is_empty()
    );
    api.calendars.get_mut("source-0").unwrap()[0]
        .summary
        .clear();
    let empty = engine::plan(&mut api, &config, &window).unwrap();
    engine::apply(&mut api, &empty).unwrap();
    assert!(
        api.calendars["clone-2"]
            .iter()
            .any(|e| e.summary == "Project / ")
    );
}

#[test]
fn polling_accepts_ten_minutes_and_rejects_a_zero_interval() {
    let (mut config, _) = demo::fixture();
    for seconds in [1, 60, 120, 600, 3600] {
        config.poll_seconds = seconds;
        config.validate().unwrap();
    }
    config.poll_seconds = 0;
    assert!(
        config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("poll_seconds")
    );
}

#[test]
fn horizon_accepts_one_to_thirty_days_and_starts_at_now() {
    let (mut config, _) = demo::fixture();
    let now = Utc::now();
    for days in [1, 7, 30] {
        config.horizon_days = days;
        config.validate().unwrap();
        let window = config.window(now);
        assert_eq!(window.start, now);
        assert_eq!(window.end, now + Duration::days(i64::from(days)));
    }
    for days in [0, 31, 3650, u32::MAX] {
        config.horizon_days = days;
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("horizon_days")
        );
    }
}

#[test]
fn route_attendees_are_independent_and_changes_reconcile_existing_copies() {
    let (mut config, mut api) = mirror_fixture();
    config.auth = config::AuthMode::Oauth;
    config.routes.as_mut().unwrap()[0].attendees = vec!["Colleague@example.com".into()];
    config.validate().unwrap();
    api.calendars.get_mut("source-0").unwrap()[0]
        .attendees
        .push(model::Attendee {
            email: "source-guest@example.com".into(),
            response_status: "accepted".into(),
            ..Default::default()
        });
    let window = config.window(Utc::now());
    let initial = engine::plan(&mut api, &config, &window).unwrap();
    engine::apply(&mut api, &initial).unwrap();
    let events = api.calendars.get_mut("clone-2").unwrap();
    let invited = events
        .iter_mut()
        .find(|e| e.summary == "Aveni Busy")
        .unwrap();
    let id = invited.id.clone();
    assert_eq!(invited.attendees.len(), 1);
    assert_eq!(invited.attendees[0].email, "colleague@example.com");
    invited.attendees[0].response_status = "accepted".into();
    // Google can omit guestsCanModify=false, its documented default.
    invited.extra.remove("guestsCanModify");
    assert!(
        events
            .iter()
            .find(|e| e.summary != "Aveni Busy")
            .unwrap()
            .attendees
            .is_empty()
    );
    assert_eq!(
        engine::plan(&mut api, &config, &window)
            .unwrap()
            .counts
            .unchanged,
        2
    );
    // Reordering/case and RSVP changes do not trigger invitations every poll.
    config.routes.as_mut().unwrap()[0].attendees[0] = "COLLEAGUE@example.com".into();
    assert!(
        engine::plan(&mut api, &config, &window)
            .unwrap()
            .actions
            .is_empty()
    );
    config.routes.as_mut().unwrap()[0]
        .attendees
        .push("new@example.com".into());
    let change = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(change.counts.update, 1);
    assert_eq!(change.counts.create, 0);
    engine::apply(&mut api, &change).unwrap();
    let invited = api.calendars["clone-2"]
        .iter()
        .find(|e| e.id == id)
        .unwrap();
    assert_eq!(invited.attendees[0].response_status, "accepted");
    assert_eq!(invited.attendees.len(), 2);
    assert!(
        engine::plan(&mut api, &config, &window)
            .unwrap()
            .actions
            .is_empty()
    );
    config.routes.as_mut().unwrap()[0].attendees.clear();
    let removal = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(removal.counts.update, 1);
    engine::apply(&mut api, &removal).unwrap();
    assert!(
        api.calendars["clone-2"]
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .attendees
            .is_empty()
    );
}

#[test]
fn route_attendee_validation_rejects_invalid_duplicates_feedback_and_service_auth() {
    let (mut config, _) = mirror_fixture();
    config.routes.as_mut().unwrap()[0].attendees = vec!["guest@example.com".into()];
    assert!(config.validate().unwrap_err().to_string().contains("OAuth"));
    config.auth = config::AuthMode::Oauth;
    config.validate().unwrap();
    for emails in [
        vec!["invalid"],
        vec!["guest@@example.com"],
        vec!["guest@example.com\n"],
        vec!["Name <guest@example.com>"],
        vec!["guest@example.com", "GUEST@example.com"],
        vec!["owner-0@example.invalid"],
        vec!["owner-2@example.invalid"],
    ] {
        config.routes.as_mut().unwrap()[0].attendees =
            emails.into_iter().map(str::to_owned).collect();
        assert!(config.validate().is_err());
    }
}

#[test]
fn named_calendar_config_accepts_empty_target_but_requires_destination_oauth() {
    let (mut config, _) = mirror_fixture();
    config.mirror_calendar_name = Some("Gallendar".into());
    config.accounts[2].target.clear();
    assert!(config.validate().is_err());
    config.accounts[2].auth = Some(config::AuthMode::Oauth);
    config.validate().unwrap();
    config.mirror_calendar_name = Some(" ".into());
    assert!(config.validate().is_err());
    assert!(
        Cli::try_parse_from(["gallendar", "calendar", "ensure", "--account", "innowise"]).is_ok()
    );
    assert!(Cli::try_parse_from(["gallendar", "sync", "--dry-run", "--details"]).is_ok());
}

#[test]
fn audit_failure_stops_before_mutation_or_before_the_next_operation() {
    let (config, mut api) = mirror_fixture();
    let plan = engine::plan(&mut api, &config, &config.window(Utc::now())).unwrap();
    assert_eq!(plan.counts.create, 2);
    assert!(
        engine::apply_with_reporter(&mut api, &plan, |_, _, _| bail!("Log unavailable")).is_err()
    );
    assert!(!api.calendars.contains_key("clone-2"));
    let mut stages = Vec::new();
    assert!(
        engine::apply_with_reporter(&mut api, &plan, |_, stage, _| {
            stages.push(stage.to_owned());
            if stage == "operation_verified" {
                bail!("Disk full after read-back");
            }
            Ok(())
        })
        .is_err()
    );
    assert_eq!(api.calendars["clone-2"].len(), 1);
    assert_eq!(stages, vec!["operation_started", "operation_verified"]);
    let remaining = engine::plan(&mut api, &config, &config.window(Utc::now())).unwrap();
    assert_eq!(remaining.counts.create, 1);
    assert_eq!(remaining.counts.unchanged, 1);
}

#[test]
fn simple_run_applies_and_explicit_dry_run_preserves_preview_mode() {
    for (arguments, periodic, writes) in [
        (vec!["gallendar", "run"], true, true),
        (vec!["gallendar", "run", "--dry-run"], true, false),
        (vec!["gallendar", "run", "--apply"], true, true),
        (vec!["gallendar", "sync"], false, false),
        (vec!["gallendar", "sync", "--apply"], false, true),
    ] {
        let parsed = Cli::try_parse_from(arguments).unwrap();
        let args = match parsed.command.unwrap() {
            Command::Run(args) | Command::Sync(args) => args,
            _ => panic!("sync/run command"),
        };
        assert_eq!(args.writes(periodic), writes);
    }
    assert!(Cli::try_parse_from(["gallendar", "run", "--apply", "--dry-run"]).is_err());
    assert!(Cli::try_parse_from(["gallendar", "run", "--availability-confirmed"]).is_err());
    assert!(Cli::try_parse_from(["gallendar", "run", "--live-trial"]).is_err());
}

#[test]
fn google_omitted_default_transparency_and_timezone_normalization_match_busy() {
    let (config, api) = demo::fixture();
    let busy = api.calendars["source-0"][0]
        .busy(&config.accounts[0], "Europe/Warsaw", config.all_day_policy)
        .unwrap()
        .unwrap();
    let namespace = model::digest(&[&config.namespace]);
    let mut saved = busy.payload("clone-id", &namespace);
    saved.as_object_mut().unwrap().remove("transparency");
    for field in ["start", "end"] {
        let utc = DateTime::parse_from_rfc3339(saved[field]["dateTime"].as_str().unwrap()).unwrap();
        saved[field]["dateTime"] =
            json!(utc.with_timezone(&chrono_tz::Europe::Warsaw).to_rfc3339());
        saved[field]["timeZone"] = json!("Europe/Warsaw");
    }
    let mut clone = event(saved);
    assert!(clone.matches(&busy, &namespace));
    clone.transparency = "transparent".into();
    assert!(!clone.matches(&busy, &namespace));
}

#[test]
fn named_calendar_rejects_reverse_routes_into_source_accounts() {
    let (mut config, _) = mirror_fixture();
    config.auth = config::AuthMode::Oauth;
    config.mirror_calendar_name = Some("Gallendar".into());
    config.routes.as_mut().unwrap().push(config::Route {
        from: "account-2".into(),
        to: "account-0".into(),
        mode: config::MirrorMode::Original,
        title: "Busy".into(),
        prefix: String::new(),
        attendees: vec![],
    });
    config.accounts[2].source = "source-2".into();
    config.accounts[0].target = "clone-0".into();
    assert!(
        config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("one-way")
    );
}

#[test]
fn every_reconciliation_write_targets_only_the_destination_and_preserves_original_guests() {
    use engine::CalendarApi;
    struct DestinationOnly(demo::Memory);
    impl CalendarApi for DestinationOnly {
        fn source(&mut self, c: &str, w: &config::Window) -> Result<model::Snapshot> {
            self.0.source(c, w)
        }
        fn clones(&mut self, c: &str, n: &str) -> Result<model::Snapshot> {
            self.0.clones(c, n)
        }
        fn create(&mut self, c: &str, b: &model::Busy, n: &str) -> Result<()> {
            assert_eq!(c, "clone-2");
            self.0.create(c, b, n)
        }
        fn update(&mut self, c: &str, id: &str, b: &model::Busy, n: &str) -> Result<()> {
            assert_eq!(c, "clone-2");
            self.0.update(c, id, b, n)
        }
        fn delete(&mut self, c: &str, id: &str, n: &str) -> Result<()> {
            assert_eq!(c, "clone-2");
            self.0.delete(c, id, n)
        }
    }
    let (mut config, mut memory) = mirror_fixture();
    config.auth = config::AuthMode::Oauth;
    config.routes.as_mut().unwrap()[0].mode = config::MirrorMode::Original;
    config.routes.as_mut().unwrap()[0].attendees = vec!["colleague@example.invalid".into()];
    config.validate().unwrap();
    memory.calendars.get_mut("source-0").unwrap()[0]
        .attendees
        .push(model::Attendee {
            email: "original-guest@example.invalid".into(),
            response_status: "accepted".into(),
            ..Default::default()
        });
    let mut api = DestinationOnly(memory);
    let w = config.window(Utc::now());
    let sources = |api: &DestinationOnly| {
        serde_json::to_value([&api.0.calendars["source-0"], &api.0.calendars["source-1"]]).unwrap()
    };
    let before = sources(&api);
    let plan = engine::plan(&mut api, &config, &w).unwrap();
    engine::apply(&mut api, &plan).unwrap();
    assert_eq!(sources(&api), before);
    assert!(
        engine::plan(&mut api, &config, &w)
            .unwrap()
            .actions
            .is_empty()
    );
    assert!(
        api.0.calendars["clone-2"]
            .iter()
            .flat_map(|e| &e.attendees)
            .all(|a| a.email == "colleague@example.invalid")
    );
    let e = &mut api.0.calendars.get_mut("source-0").unwrap()[0];
    e.summary = "Renamed original".into();
    e.end.date_time = Some(
        (DateTime::parse_from_rfc3339(e.end.date_time.as_deref().unwrap()).unwrap()
            + Duration::minutes(10))
        .to_rfc3339(),
    );
    let before = sources(&api);
    let plan = engine::plan(&mut api, &config, &w).unwrap();
    assert_eq!(plan.counts.update, 1);
    engine::apply(&mut api, &plan).unwrap();
    assert_eq!(sources(&api), before);
    api.0.calendars.get_mut("source-0").unwrap()[0].status = "cancelled".into();
    let before = sources(&api);
    let plan = engine::plan(&mut api, &config, &w).unwrap();
    assert_eq!(plan.counts.delete, 1);
    engine::apply(&mut api, &plan).unwrap();
    assert_eq!(sources(&api), before);
    assert_eq!(
        api.0.calendars["source-0"][0].attendees[0].email,
        "original-guest@example.invalid"
    );
    assert!(!api.0.calendars.contains_key("clone-0"));
    assert!(!api.0.calendars.contains_key("clone-1"));
}

#[test]
fn retarget_migration_fills_primary_before_cleanup_and_preserves_foreign_events() {
    let (mut config, mut api) = mirror_fixture();
    let window = config.window(Utc::now());
    let initial = engine::plan(&mut api, &config, &window).unwrap();
    engine::apply(&mut api, &initial).unwrap();
    let old = config.accounts[2].target.clone();
    let originals = api.calendars[&config.accounts[0].source].clone();
    let foreign = api.calendars[&config.accounts[0].source][0].clone();
    api.calendars.get_mut(&old).unwrap().push(foreign.clone());
    config.accounts[2].target = config.accounts[2].owner_email.clone();
    config.accounts[2].retired_targets = vec![old.clone()];
    config.validate().unwrap();
    let primary = config.accounts[2].target.clone();
    api.calendars.insert(primary.clone(), vec![foreign.clone()]);
    let migration = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(migration.counts.create, initial.counts.create);
    assert_eq!(migration.counts.delete, initial.counts.create);
    let first_delete = migration
        .actions
        .iter()
        .position(|a| matches!(a, engine::Action::Delete { .. }))
        .unwrap();
    assert_eq!(first_delete, migration.counts.create);
    // A verified creation followed by an interrupted pass leaves every old block intact.
    let mut verified = 0;
    assert!(
        engine::apply_with_reporter(&mut api, &migration, |_, stage, _| {
            if stage == "operation_started" && verified == 1 {
                anyhow::bail!("Simulated interruption");
            }
            if stage == "operation_verified" {
                verified += 1;
            }
            Ok(())
        })
        .is_err()
    );
    assert_eq!(api.calendars[&old].len(), initial.counts.create + 1);
    let recovery = engine::plan(&mut api, &config, &window).unwrap();
    assert_eq!(recovery.counts.unchanged, 1);
    engine::apply(&mut api, &recovery).unwrap();
    assert_eq!(api.calendars[&old].len(), 1);
    assert_eq!(api.calendars[&old][0].id, foreign.id);
    assert_eq!(api.calendars[&primary].len(), initial.counts.create + 1);
    assert_eq!(
        api.calendars[&config.accounts[0].source][0].id,
        originals[0].id
    );
    let repeated = engine::plan(&mut api, &config, &window).unwrap();
    assert!(repeated.actions.is_empty());
}

#[test]
fn failed_retired_calendar_read_prevents_all_mutations() {
    let (mut config, mut api) = mirror_fixture();
    config.accounts[2].retired_targets = vec!["old-secondary".into()];
    api.failing_calendar = Some("old-secondary".into());
    assert!(engine::plan(&mut api, &config, &config.window(Utc::now())).is_err());
    assert_eq!(api.writes, 0);
}

#[test]
fn migration_rejects_source_primary_active_and_duplicate_cleanup_ids() {
    let (mut config, _) = mirror_fixture();
    for ids in [
        vec![config.accounts[0].source.clone()],
        vec![config.accounts[2].target.clone()],
        vec![config.accounts[0].owner_email.clone()],
        vec!["primary".into()],
        vec!["old-secondary".into(), "old-secondary".into()],
    ] {
        config.accounts[2].retired_targets = ids;
        assert!(config.validate().is_err());
    }
}
