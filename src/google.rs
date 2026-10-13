use crate::{
    config::{AuthMode, Config, Window},
    engine::CalendarApi,
    model::{Busy, Event, Snapshot},
};
use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::{
    Method, Url,
    blocking::{Client, Response},
    header::IF_MATCH,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const API_URL: &str = "https://www.googleapis.com/calendar/v3/";
const SOURCE_FIELDS: &str = "nextPageToken,timeZone,accessRole,items(id,summary,status,eventType,transparency,start,end,recurringEventId,originalStartTime,attendees(email,responseStatus),extendedProperties)";

#[derive(Deserialize)]
struct ServiceKey {
    #[serde(rename = "type")]
    kind: String,
    client_email: String,
    private_key: String,
    private_key_id: Option<String>,
}

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    #[serde(default)]
    items: Vec<Event>,
    next_page_token: Option<String>,
    time_zone: Option<String>,
    access_role: Option<String>,
}

pub struct Google {
    client: Client,
    token: String,
    base: String,
    tokens: HashMap<String, String>,
}

impl Google {
    pub fn connect(key_path: &Path) -> Result<Self> {
        let bytes = std::fs::read(key_path)
            .context("Cannot read service-account key; configure service_account_key")?;
        let key: ServiceKey = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Invalid service-account JSON (contents omitted)"))?;
        ensure!(
            key.kind == "service_account",
            "Expected a Google service-account key"
        );
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let now = Utc::now().timestamp();
        let claims = Claims {
            iss: &key.client_email,
            scope: "https://www.googleapis.com/auth/calendar.events https://www.googleapis.com/auth/calendar.events.freebusy",
            aud: TOKEN_URL,
            iat: now,
            exp: now + 3600,
        };
        let mut header = Header::new(Algorithm::RS256);
        header.kid = key.private_key_id;
        let signing_key = EncodingKey::from_rsa_pem(key.private_key.as_bytes())
            .map_err(|_| anyhow::anyhow!("Invalid RSA private key (contents omitted)"))?;
        let assertion = jsonwebtoken::encode(&header, &claims, &signing_key)?;
        let response = client
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .map_err(|_| {
                anyhow::anyhow!("Google authentication request failed (network or timeout)")
            })?;
        ensure!(
            response.status().is_success(),
            "Google authentication rejected (HTTP {}); check key, clock and enabled Calendar API",
            response.status().as_u16()
        );
        let token: Token = response
            .json()
            .map_err(|_| anyhow::anyhow!("Invalid Google token response"))?;
        ensure!(
            !token.access_token.is_empty(),
            "Google returned an empty token"
        );
        Ok(Self {
            client,
            token: token.access_token,
            base: API_URL.into(),
            tokens: HashMap::new(),
        })
    }

    pub fn connect_config(config: &Config) -> Result<Self> {
        let mut api = Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            token: String::new(),
            base: API_URL.into(),
            tokens: HashMap::new(),
        };
        for account in &config.accounts {
            if !config
                .routes()
                .iter()
                .any(|r| r.from == account.name || r.to == account.name)
            {
                continue;
            }
            let token = match account.auth.unwrap_or(config.auth) {
                AuthMode::ServiceAccount => {
                    if api.token.is_empty() {
                        api.token = Self::connect(&config.service_account_key)?.token;
                    }
                    api.token.clone()
                }
                AuthMode::Oauth => crate::oauth::access_token(config, account)
                    .with_context(|| format!("OAuth account {}", account.name))?,
            };
            for calendar in [&account.source, &account.target]
                .into_iter()
                .filter(|id| !id.is_empty())
            {
                api.tokens.insert(calendar.clone(), token.clone());
            }
        }
        Ok(api)
    }

    fn events_url(&self, calendar: &str, id: Option<&str>) -> Result<Url> {
        let mut url = Url::parse(&self.base)?;
        let mut path = url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Invalid API base URL"))?;
        path.pop_if_empty()
            .extend(["calendars", calendar, "events"]);
        if let Some(id) = id {
            path.push(id);
        }
        drop(path);
        Ok(url)
    }

    fn request(
        &self,
        calendar: &str,
        method: Method,
        url: Url,
        payload: Option<&Value>,
        etag: Option<&str>,
    ) -> Result<Response> {
        let token = self.tokens.get(calendar).unwrap_or(&self.token);
        ensure!(
            !token.is_empty(),
            "No credentials configured for this calendar"
        );
        let mut request = self.client.request(method, url).bearer_auth(token);
        if let Some(payload) = payload {
            request = request.json(payload);
        }
        if let Some(etag) = etag {
            request = request.header(IF_MATCH, etag);
        }
        request.send().map_err(|_| {
            anyhow::anyhow!("Google request failed (network or timeout); rerun to reconcile")
        })
    }

    fn list(
        &self,
        calendar: &str,
        window: Option<&Window>,
        namespace: Option<&str>,
    ) -> Result<Snapshot> {
        let mut events = Vec::new();
        let mut page_token: Option<String> = None;
        let mut page_tokens = HashSet::new();
        let mut timezone = None;
        let mut access_role = None;
        loop {
            let mut url = self.events_url(calendar, None)?;
            {
                let mut query = url.query_pairs_mut();
                query
                    .append_pair("maxResults", "2500")
                    .append_pair("showDeleted", "false");
                if let Some(window) = window {
                    query
                        .append_pair("singleEvents", "true")
                        .append_pair("timeMin", &window.start.to_rfc3339())
                        .append_pair("timeMax", &window.end.to_rfc3339())
                        .append_pair("fields", SOURCE_FIELDS);
                } else {
                    query.append_pair("singleEvents", "false");
                }
                if let Some(namespace) = namespace {
                    query.append_pair(
                        "privateExtendedProperty",
                        &format!("gallendarNamespace={namespace}"),
                    );
                }
                if let Some(token) = &page_token {
                    query.append_pair("pageToken", token);
                }
            }
            let response = self.request(calendar, Method::GET, url, None, None)?;
            ensure!(
                response.status().is_success(),
                "Cannot list Google events (HTTP {}); check sharing and API permissions",
                response.status().as_u16()
            );
            let page: Page = response.json().map_err(|_| {
                anyhow::anyhow!("Invalid Google events response; refusing incomplete snapshot")
            })?;
            if let Some(zone) = page.time_zone {
                timezone = Some(zone);
            }
            if let Some(role) = page.access_role {
                access_role = Some(role);
            }
            events.extend(page.items);
            match page.next_page_token {
                Some(token) if !token.is_empty() => {
                    ensure!(
                        page_tokens.insert(token.clone()),
                        "Google repeated a pagination token; refusing incomplete snapshot"
                    );
                    page_token = Some(token);
                }
                _ => break,
            }
        }
        Ok(Snapshot {
            events,
            timezone: timezone.context("Google did not return a calendar time zone")?,
            access_role: access_role.context("Google did not return the calendar access role")?,
        })
    }

    fn get(&self, calendar: &str, id: &str) -> Result<Option<Event>> {
        let response = self.request(
            calendar,
            Method::GET,
            self.events_url(calendar, Some(id))?,
            None,
            None,
        )?;
        if matches!(response.status().as_u16(), 404 | 410) {
            return Ok(None);
        }
        ensure!(
            response.status().is_success(),
            "Cannot read Google clone (HTTP {})",
            response.status().as_u16()
        );
        Ok(Some(response.json().map_err(|_| {
            anyhow::anyhow!("Invalid Google event response")
        })?))
    }

    fn verify(&self, calendar: &str, id: &str, busy: &Busy, namespace: &str) -> Result<()> {
        let saved = self
            .get(calendar, id)?
            .context("Saved clone is missing on read-back")?;
        ensure!(
            saved.matches(busy, namespace),
            "Clone read-back differs from requested Busy state; stopping"
        );
        Ok(())
    }

    pub fn availability(
        &self,
        calendar: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Value> {
        let payload = json!({ "timeMin": start.to_rfc3339(), "timeMax": end.to_rfc3339(), "items": [{ "id": calendar }] });
        let response = self.request(
            calendar,
            Method::POST,
            Url::parse(&format!("{}freeBusy", self.base))?,
            Some(&payload),
            None,
        )?;
        ensure!(
            response.status().is_success(),
            "Free/busy query failed (HTTP {})",
            response.status().as_u16()
        );
        let result: Value = response
            .json()
            .map_err(|_| anyhow::anyhow!("Invalid free/busy response"))?;
        let calendar_result = result
            .get("calendars")
            .and_then(|v| v.get(calendar))
            .context("Calendar missing from free/busy response")?;
        ensure!(
            calendar_result
                .get("errors")
                .is_none_or(|v| v.as_array().is_some_and(Vec::is_empty)),
            "Google returned per-calendar free/busy errors; availability is unknown"
        );
        let busy = calendar_result
            .get("busy")
            .and_then(Value::as_array)
            .context("Missing free/busy intervals")?;
        Ok(json!(busy))
    }
}

impl CalendarApi for Google {
    fn source(&mut self, calendar: &str, window: &Window) -> Result<Snapshot> {
        self.list(calendar, Some(window), None)
    }
    fn clones(&mut self, calendar: &str, namespace: &str) -> Result<Snapshot> {
        self.list(calendar, None, Some(namespace))
    }

    fn create(&mut self, calendar: &str, busy: &Busy, namespace: &str) -> Result<()> {
        // Remote logical keys survive crashes and restarts. Fresh Google IDs allow
        // recreation after a deletion, whose original ID remains tombstoned.
        let id = format!("gld{}", uuid::Uuid::new_v4().simple());
        let mut url = self.events_url(calendar, None)?;
        url.query_pairs_mut().append_pair(
            "sendUpdates",
            if busy.attendees.is_empty() {
                "none"
            } else {
                "all"
            },
        );
        let response = self.request(
            calendar,
            Method::POST,
            url,
            Some(&busy.payload(&id, namespace)),
            None,
        )?;
        ensure!(
            response.status().is_success(),
            "Create clone failed (HTTP {}); rerun to reconcile",
            response.status().as_u16()
        );
        self.verify(calendar, &id, busy, namespace)
    }

    fn update(&mut self, calendar: &str, id: &str, busy: &Busy, namespace: &str) -> Result<()> {
        let current = self
            .get(calendar, id)?
            .context("Clone disappeared during planning; rerun sync")?;
        ensure!(
            current.owned(namespace) && current.key() == Some(busy.key.as_str()),
            "Clone ownership changed; refusing to overwrite it"
        );
        let mut url = self.events_url(calendar, Some(id))?;
        url.query_pairs_mut().append_pair(
            "sendUpdates",
            if busy.attendees.is_empty() && current.attendees.is_empty() {
                "none"
            } else {
                "all"
            },
        );
        // Full replacement also removes accidentally introduced sensitive fields.
        let response = self.request(
            calendar,
            Method::PUT,
            url,
            Some(&busy.update_payload(id, namespace, &current)),
            current.extra.get("etag").and_then(Value::as_str),
        )?;
        ensure!(
            response.status().is_success(),
            "Update clone failed (HTTP {}); rerun to reconcile",
            response.status().as_u16()
        );
        self.verify(calendar, id, busy, namespace)
    }

    fn delete(&mut self, calendar: &str, id: &str, namespace: &str) -> Result<()> {
        let Some(current) = self.get(calendar, id)? else {
            return Ok(());
        };
        if current.status == "cancelled" {
            return Ok(());
        }
        ensure!(
            current.owned(namespace),
            "Clone ownership changed; refusing to delete it"
        );
        let mut url = self.events_url(calendar, Some(id))?;
        url.query_pairs_mut().append_pair(
            "sendUpdates",
            if current.attendees.is_empty() {
                "none"
            } else {
                "all"
            },
        );
        let response = self.request(
            calendar,
            Method::DELETE,
            url,
            None,
            current.extra.get("etag").and_then(Value::as_str),
        )?;
        ensure!(
            response.status().is_success() || matches!(response.status().as_u16(), 404 | 410),
            "Delete clone failed (HTTP {})",
            response.status().as_u16()
        );
        match self.get(calendar, id)? {
            None => Ok(()),
            Some(event) if event.status == "cancelled" => Ok(()),
            _ => bail!("Deleted clone still exists on read-back; stopping"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
    };

    fn server(
        responses: Vec<(u16, Value)>,
    ) -> (Google, std::thread::JoinHandle<()>, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/calendar/v3/", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let handle = std::thread::spawn(move || {
            for (status, body) in responses {
                let (mut connection, _) = listener.accept().unwrap();
                connection
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let count = connection.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                    if let Some(end) = request.windows(4).position(|v| v == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                recorded
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(request).unwrap());
                let body = body.to_string();
                write!(connection, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (
            Google {
                client: Client::builder()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .unwrap(),
                token: "test-only-token".into(),
                base,
                tokens: HashMap::new(),
            },
            handle,
            requests,
        )
    }

    #[test]
    fn reads_all_pages_and_expands_recurring_instances() {
        let first = json!({ "timeZone": "Europe/Warsaw", "accessRole": "reader", "nextPageToken": "next-page", "items": [{"id": "first"}] });
        let last = json!({ "timeZone": "Europe/Warsaw", "accessRole": "reader", "items": [{"id": "second"}] });
        let (mut api, thread, requests) = server(vec![(200, first), (200, last)]);
        let now = Utc::now();
        let snapshot = api
            .source(
                "owner@example.invalid",
                &Window {
                    start: now,
                    end: now + chrono::Duration::days(30),
                },
            )
            .unwrap();
        thread.join().unwrap();
        assert_eq!(snapshot.events.len(), 2);
        let requests = requests.lock().unwrap();
        assert!(requests[0].contains("singleEvents=true"));
        assert!(requests[1].contains("pageToken=next-page"));
        assert!(requests[0].contains("timeMin="));
        assert!(requests[0].contains("summary"));
    }

    #[test]
    fn pagination_failure_is_not_an_empty_successful_snapshot() {
        let first = json!({ "timeZone": "Europe/Warsaw", "accessRole": "reader", "nextPageToken": "next", "items": [] });
        let (mut api, thread, _) = server(vec![
            (200, first),
            (
                403,
                json!({ "error": { "message": "private server response" } }),
            ),
        ]);
        let now = Utc::now();
        let error = api
            .source(
                "source",
                &Window {
                    start: now,
                    end: now + chrono::Duration::days(1),
                },
            )
            .err()
            .unwrap();
        thread.join().unwrap();
        assert!(error.to_string().contains("403"));
        assert!(!error.to_string().contains("private server response"));
    }

    #[test]
    fn put_replaces_sensitive_fields_and_verifies_saved_clone() {
        let (config, memory) = crate::demo::fixture();
        let busy = memory.calendars["source-0"][0]
            .busy(&config.accounts[0], "Europe/Warsaw", config.all_day_policy)
            .unwrap()
            .unwrap();
        let namespace = crate::model::digest(&[&config.namespace]);
        let saved = busy.payload("clone-id", &namespace);
        let mut polluted = saved.clone();
        polluted["description"] = json!("sensitive accidental field");
        polluted["etag"] = json!("etag-value");
        let (mut api, thread, requests) =
            server(vec![(200, polluted), (200, saved.clone()), (200, saved)]);
        api.update("clone-calendar", "clone-id", &busy, &namespace)
            .unwrap();
        thread.join().unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests[1].starts_with("PUT "));
        assert!(requests[1].to_lowercase().contains("if-match: etag-value"));
        assert!(!requests[1].contains("sensitive accidental field"));
        assert!(requests[1].contains("sendUpdates=none"));
        assert!(requests[2].starts_with("GET "));
    }

    #[test]
    fn invitations_notify_guests_preserve_rsvp_and_notify_removals_and_cancellation() {
        let (config, memory) = crate::demo::fixture();
        let mut busy = memory.calendars["source-0"][0]
            .busy(&config.accounts[0], "Europe/Warsaw", config.all_day_policy)
            .unwrap()
            .unwrap();
        busy.attendees = vec!["guest@example.com".into()];
        let namespace = crate::model::digest(&[&config.namespace]);
        let saved = busy.payload("clone-id", &namespace);
        let (mut api, thread, requests) = server(vec![(200, saved.clone()), (200, saved.clone())]);
        api.create("target", &busy, &namespace).unwrap();
        thread.join().unwrap();
        let request = &requests.lock().unwrap()[0];
        assert!(request.contains("sendUpdates=all"));
        assert!(request.contains("guest@example.com"));
        assert!(!request.contains("responseStatus"));
        let mut accepted = saved.clone();
        accepted["attendees"][0]["responseStatus"] = json!("accepted");
        let (mut api, thread, requests) = server(vec![
            (200, accepted.clone()),
            (200, accepted.clone()),
            (200, accepted.clone()),
        ]);
        api.update("target", "clone-id", &busy, &namespace).unwrap();
        thread.join().unwrap();
        assert!(requests.lock().unwrap()[1].contains("sendUpdates=all"));
        assert!(requests.lock().unwrap()[1].contains("accepted"));
        busy.attendees.clear();
        let empty = busy.payload("clone-id", &namespace);
        let (mut api, thread, requests) = server(vec![
            (200, accepted.clone()),
            (200, empty.clone()),
            (200, empty),
        ]);
        api.update("target", "clone-id", &busy, &namespace).unwrap();
        thread.join().unwrap();
        assert!(requests.lock().unwrap()[1].contains("sendUpdates=all"));
        assert!(!requests.lock().unwrap()[1].contains("guest@example.com"));
        let (mut api, thread, requests) = server(vec![
            (200, accepted),
            (204, json!(null)),
            (404, json!(null)),
        ]);
        api.delete("target", "clone-id", &namespace).unwrap();
        thread.join().unwrap();
        assert!(requests.lock().unwrap()[1].starts_with("DELETE "));
        assert!(requests.lock().unwrap()[1].contains("sendUpdates=all"));
    }

    #[test]
    fn refuses_mutation_after_ownership_is_changed() {
        let (config, memory) = crate::demo::fixture();
        let busy = memory.calendars["source-0"][0]
            .busy(&config.accounts[0], "Europe/Warsaw", config.all_day_policy)
            .unwrap()
            .unwrap();
        let namespace = crate::model::digest(&[&config.namespace]);
        let (mut api, thread, requests) = server(vec![(200, json!({"id": "foreign"}))]);
        assert!(api.update("target", "foreign", &busy, &namespace).is_err());
        thread.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn freebusy_per_calendar_error_is_not_free_time() {
        let (api, thread, _) = server(vec![(
            200,
            json!({ "calendars": { "source": { "errors": [{ "reason": "notFound" }], "busy": [] } } }),
        )]);
        let now = Utc::now();
        assert!(
            api.availability("source", now, now + chrono::Duration::hours(1))
                .is_err()
        );
        thread.join().unwrap();
    }
    #[test]
    fn calendar_requests_use_their_own_identity_including_freebusy() {
        let page = json!({"timeZone":"Europe/Warsaw", "accessRole":"reader", "items":[]});
        let busy = json!({"calendars":{"target":{"busy":[]}}});
        let (mut api, worker, requests) =
            server(vec![(200, page.clone()), (200, page), (200, busy)]);
        api.tokens.insert("source".into(), "source-token".into());
        api.tokens.insert("target".into(), "target-token".into());
        let start = Utc::now();
        api.source(
            "source",
            &Window {
                start,
                end: start + chrono::Duration::days(1),
            },
        )
        .unwrap();
        api.clones("target", "namespace").unwrap();
        api.availability("target", start, start + chrono::Duration::hours(1))
            .unwrap();
        worker.join().unwrap();
        let requests = requests.lock().unwrap();
        assert!(
            requests[0]
                .to_lowercase()
                .contains("authorization: bearer source-token")
        );
        assert!(
            requests[1]
                .to_lowercase()
                .contains("authorization: bearer target-token")
        );
        assert!(
            requests[2]
                .to_lowercase()
                .contains("authorization: bearer target-token")
        );
    }
}
