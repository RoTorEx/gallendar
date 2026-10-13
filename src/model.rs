use crate::config::{Account, AllDayPolicy};
use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MANAGED: &str = "gallendar";
pub const NAMESPACE: &str = "gallendarNamespace";
pub const KEY: &str = "gallendarKey";

#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventTime {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attendee {
    #[serde(default)]
    pub email: String,
    #[serde(default, rename = "self")]
    pub is_self: bool,
    #[serde(default)]
    pub response_status: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct ExtendedProperties {
    #[serde(default)]
    pub private: BTreeMap<String, String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub event_type: String,
    #[serde(default)]
    pub transparency: String,
    #[serde(default)]
    pub visibility: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub start: EventTime,
    #[serde(default)]
    pub end: EventTime,
    #[serde(default)]
    pub recurring_event_id: Option<String>,
    #[serde(default)]
    pub original_start_time: Option<EventTime>,
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    #[serde(default)]
    pub extended_properties: ExtendedProperties,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone)]
pub struct Snapshot {
    pub events: Vec<Event>,
    pub timezone: String,
    pub access_role: String,
}

#[derive(Clone)]
pub struct Busy {
    pub attendees: Vec<String>,
    pub summary: String,
    pub key: String,
    pub start: EventTime,
    pub end: EventTime,
}

pub fn digest(parts: &[&str]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

impl Event {
    pub fn managed(&self) -> bool {
        self.extended_properties
            .private
            .get(MANAGED)
            .is_some_and(|v| v == "v1")
    }
    pub fn owned(&self, namespace: &str) -> bool {
        self.managed()
            && self
                .extended_properties
                .private
                .get(NAMESPACE)
                .is_some_and(|v| v == namespace)
    }
    pub fn key(&self) -> Option<&str> {
        self.extended_properties
            .private
            .get(KEY)
            .map(String::as_str)
    }

    pub fn busy(
        &self,
        account: &Account,
        timezone: &str,
        policy: AllDayPolicy,
    ) -> Result<Option<Busy>> {
        if self.managed()
            || self.status == "cancelled"
            || self.transparency == "transparent"
            || self.event_type == "birthday"
            || self.event_type == "workingLocation"
            || self.attendees.iter().any(|a| {
                a.email.eq_ignore_ascii_case(&account.owner_email)
                    && a.response_status == "declined"
            })
        {
            return Ok(None);
        }
        if self.start.date.is_some()
            && matches!(policy, AllDayPolicy::OutOfOfficeOnly)
            && self.event_type != "outOfOffice"
        {
            return Ok(None);
        }
        ensure!(
            !self.id.is_empty(),
            "Source event has no ID; refusing an incomplete snapshot"
        );
        let start = normalize_time(&self.start, timezone)?;
        let end = normalize_time(&self.end, timezone)?;
        let s = DateTime::parse_from_rfc3339(start.date_time.as_deref().unwrap())?;
        let e = DateTime::parse_from_rfc3339(end.date_time.as_deref().unwrap())?;
        ensure!(
            e > s,
            "Source event has an invalid interval; refusing an incomplete snapshot"
        );
        // Original occurrence time stays stable when one occurrence is rescheduled.
        let identity = match (&self.recurring_event_id, &self.original_start_time) {
            (Some(series), Some(original)) => digest(&[series, &serde_json::to_string(original)?]),
            _ => self.id.clone(),
        };
        Ok(Some(Busy {
            attendees: Vec::new(),
            summary: "Busy".into(),
            key: digest(&[&account.source, &identity]),
            start,
            end,
        }))
    }

    pub fn matches(&self, desired: &Busy, namespace: &str) -> bool {
        let times_match = |left: &EventTime, right: &EventTime| match (
            left.date_time.as_deref(),
            right.date_time.as_deref(),
        ) {
            (Some(a), Some(b)) => match (
                DateTime::parse_from_rfc3339(a),
                DateTime::parse_from_rfc3339(b),
            ) {
                (Ok(a), Ok(b)) => a == b,
                _ => false,
            },
            _ => false,
        };
        self.owned(namespace)
            && self.key() == Some(desired.key.as_str())
            && self.status != "cancelled"
            && self.summary == desired.summary
            && self.transparency == "opaque"
            && self.visibility == "private"
            && {
                let mut actual: Vec<_> = self
                    .attendees
                    .iter()
                    .map(|a| a.email.to_ascii_lowercase())
                    .collect();
                actual.sort();
                actual == desired.attendees
            }
            && (desired.attendees.is_empty()
                || (self
                    .extra
                    .get("guestsCanModify")
                    .is_none_or(|v| v == &Value::Bool(false))
                    && self.extra.get("guestsCanInviteOthers") == Some(&Value::Bool(false))))
            && (self.event_type.is_empty() || self.event_type == "default")
            && self.recurring_event_id.is_none()
            && [
                "description",
                "location",
                "conferenceData",
                "attachments",
                "recurrence",
            ]
            .iter()
            .all(|field| self.extra.get(*field).is_none_or(|value| value.is_null()))
            && self
                .extra
                .get("reminders")
                .is_some_and(|r| r.get("useDefault") == Some(&Value::Bool(false)))
            && times_match(&self.start, &desired.start)
            && times_match(&self.end, &desired.end)
    }
}

impl Busy {
    pub fn payload(&self, id: &str, namespace: &str) -> Value {
        // Construct from an allowlist. Never copy the original event object.
        let mut payload = json!({ "id": id, "summary": self.summary, "status": "confirmed", "eventType": "default",
            "visibility": "private", "transparency": "opaque", "start": self.start, "end": self.end,
            "reminders": { "useDefault": false },
            "extendedProperties": { "private": { MANAGED: "v1", NAMESPACE: namespace, KEY: self.key } }
        });
        if !self.attendees.is_empty() {
            payload["attendees"] = json!(
                self.attendees
                    .iter()
                    .map(|email| json!({"email":email}))
                    .collect::<Vec<_>>()
            );
            payload["guestsCanModify"] = json!(false);
            payload["guestsCanInviteOthers"] = json!(false);
        }
        payload
    }

    pub fn update_payload(&self, id: &str, namespace: &str, current: &Event) -> Value {
        let mut payload = self.payload(id, namespace);
        if self.attendees.is_empty() && !current.attendees.is_empty() {
            payload["attendees"] = json!([]);
        }
        if let Some(guests) = payload.get_mut("attendees").and_then(Value::as_array_mut) {
            for guest in guests {
                if let Some(previous) = current.attendees.iter().find(|a| {
                    guest["email"]
                        .as_str()
                        .is_some_and(|email| a.email.eq_ignore_ascii_case(email))
                }) && matches!(
                    previous.response_status.as_str(),
                    "accepted" | "declined" | "tentative" | "needsAction"
                ) {
                    guest["responseStatus"] = json!(previous.response_status);
                }
            }
        }
        payload
    }
}

fn normalize_time(time: &EventTime, calendar_timezone: &str) -> Result<EventTime> {
    let instant: DateTime<Utc> = match (&time.date, &time.date_time) {
        (None, Some(datetime)) => DateTime::parse_from_rfc3339(datetime)
            .context("Event dateTime must include an explicit UTC offset")?
            .with_timezone(&Utc),
        (Some(date), None) => {
            let tz: Tz = time
                .time_zone
                .as_deref()
                .unwrap_or(calendar_timezone)
                .parse()
                .context("Calendar has an invalid IANA time zone")?;
            let midnight = NaiveDate::parse_from_str(date, "%Y-%m-%d")?
                .and_hms_opt(0, 0, 0)
                .unwrap();
            tz.from_local_datetime(&midnight)
                .single()
                .context("Ambiguous or nonexistent all-day boundary; refusing to guess")?
                .with_timezone(&Utc)
        }
        _ => bail!("Event must have exactly one of date or dateTime"),
    };
    Ok(EventTime {
        date_time: Some(instant.to_rfc3339()),
        ..Default::default()
    })
}
