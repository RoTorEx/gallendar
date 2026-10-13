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
    write_targets: HashSet<String>,
    cleanup_targets: HashSet<String>,
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
            write_targets: HashSet::new(),
            cleanup_targets: HashSet::new(),
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
            write_targets: HashSet::new(),
            cleanup_targets: HashSet::new(),
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
            api.tokens
                .insert(format!("account:{}", account.name), token.clone());
            api.tokens
                .insert(account.owner_email.clone(), token.clone());
            for calendar in [&account.source, &account.target]
                .into_iter()
                .chain(account.retired_targets.iter())
                .filter(|id| !id.is_empty())
            {
                api.tokens.insert(calendar.clone(), token.clone());
            }
        }
        api.cleanup_targets.extend(
            config
                .targets()
                .iter()
                .flat_map(|a| a.retired_targets.clone()),
        );
        if config.mirror_calendar_name.is_none() {
            api.write_targets
                .extend(config.targets().iter().map(|a| a.target.clone()));
        }
        Ok(api)
    }

    /// Resolve names before planning. This path never creates a calendar.
    pub fn connect_resolved(config: &mut Config) -> Result<Self> {
        let mut api = Self::connect_config(config)?;
        if let Some(name) = config.mirror_calendar_name.clone() {
            let targets: Vec<_> = config.targets().iter().map(|a| a.name.clone()).collect();
            for account in targets {
                let (id, _) = api.named_calendar(&account, &name, false)?;
                config
                    .accounts
                    .iter_mut()
                    .find(|a| a.name == account)
                    .unwrap()
                    .target = id;
            }
        }
        config.validate()?;
        Ok(api)
    }

    pub fn named_calendar(
        &mut self,
        account: &str,
        name: &str,
        create: bool,
    ) -> Result<(String, bool)> {
        let token_key = format!("account:{account}");
        let mut matches = Vec::new();
        let mut pages = HashSet::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = Url::parse(&format!("{}users/me/calendarList", self.base))?;
            url.query_pairs_mut()
                .append_pair("showHidden", "true")
                .append_pair(
                    "fields",
                    "nextPageToken,items(id,summary,accessRole,primary)",
                );
            if let Some(token) = &page_token {
                url.query_pairs_mut().append_pair("pageToken", token);
            }
            let response = self.request(&token_key, Method::GET, url, None, None)?;
            ensure!(
                response.status().is_success(),
                "Cannot list calendars (HTTP {}); run auth login --account {} for named-calendar permissions",
                response.status().as_u16(),
                account
            );
            let page: Value = response.json().context("Invalid calendar list response")?;
            let items = page["items"]
                .as_array()
                .context("Incomplete calendar list response")?;
            matches.extend(
                items
                    .iter()
                    .filter(|c| c["summary"].as_str() == Some(name))
                    .cloned(),
            );
            match page["nextPageToken"].as_str() {
                Some(token) if !token.is_empty() => {
                    ensure!(
                        pages.insert(token.to_owned()),
                        "Repeated calendar list page; refusing incomplete results"
                    );
                    page_token = Some(token.to_owned());
                }
                _ => break,
            }
        }
        ensure!(
            matches.len() <= 1,
            "Multiple calendars named '{}' in account {}; rename duplicates before syncing",
            name,
            account
        );
        let (calendar, created) = if let Some(calendar) = matches.pop() {
            ensure!(
                calendar["primary"] != Value::Bool(true),
                "Named mirror calendar must be separate from the primary calendar"
            );
            ensure!(
                calendar["accessRole"].as_str() == Some("owner"),
                "Named mirror calendar must be owned by the destination account"
            );
            (calendar, false)
        } else {
            ensure!(
                create,
                "Calendar '{}' is missing in account {}; run calendar ensure --account {} first",
                name,
                account,
                account
            );
            let url = Url::parse(&format!("{}calendars", self.base))?;
            let response = self.request(
                &token_key,
                Method::POST,
                url,
                Some(&json!({"summary":name,"timeZone":"Europe/Warsaw"})),
                None,
            )?;
            ensure!(
                response.status().is_success(),
                "Create mirror calendar failed (HTTP {}); inspect calendars before retrying",
                response.status().as_u16()
            );
            let calendar: Value = response
                .json()
                .context("Invalid created calendar response")?;
            let id = calendar["id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .context("Created calendar has no ID")?;
            let mut url = Url::parse(&self.base)?;
            url.path_segments_mut()
                .map_err(|_| anyhow::anyhow!("Invalid API base"))?
                .pop_if_empty()
                .extend(["calendars", id]);
            let response = self.request(&token_key, Method::GET, url, None, None)?;
            ensure!(
                response.status().is_success(),
                "Created calendar read-back failed; inspect calendars before retrying"
            );
            let verified: Value = response.json().context("Invalid calendar read-back")?;
            ensure!(
                verified["id"] == calendar["id"]
                    && verified["summary"].as_str() == Some(name)
                    && verified["timeZone"] == "Europe/Warsaw",
                "Created calendar differs on read-back"
            );
            (verified, true)
        };
        let id = calendar["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("Named calendar has no ID")?
            .to_owned();
        ensure!(
            id != "primary",
            "Named mirror calendar must have an explicit ID"
        );
        let token = self.tokens.get(&token_key).unwrap_or(&self.token).clone();
        self.tokens.insert(id.clone(), token);
        self.write_targets.insert(id.clone());
        Ok((id, created))
    }

    fn require_delete_target(&self, calendar: &str) -> Result<()> {
        ensure!(
            self.write_targets.contains(calendar) || self.cleanup_targets.contains(calendar),
            "Calendar is not an authorized destination or retired target"
        );
        Ok(())
    }

    fn require_write_target(&self, calendar: &str) -> Result<()> {
        ensure!(
            self.write_targets.contains(calendar),
            "Refusing calendar write outside configured resolved destinations"
        );
        Ok(())
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
        if method != Method::GET
            && url
                .path_segments()
                .is_some_and(|mut segments| segments.any(|segment| segment == "events"))
        {
            if method == Method::DELETE {
                self.require_delete_target(calendar)?;
            } else {
                self.require_write_target(calendar)?;
            }
        }
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
        self.require_write_target(calendar)?;
        // Remote logical keys survive crashes and restarts. Fresh Google IDs allow
        // recreation after a deletion, whose original ID remains tombstoned.
        let id = format!("gld{}", uuid::Uuid::new_v4().simple());
        crate::logging::event(
            "copy_identity",
            json!({"target":calendar,"id":id,"key":busy.key}),
        )?;
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
        self.require_write_target(calendar)?;
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
        self.require_delete_target(calendar)?;
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
                write_targets: HashSet::from(["target".into(), "clone-calendar".into()]),
                cleanup_targets: HashSet::new(),
            },
            handle,
            requests,
        )
    }

    #[test]
    fn named_calendar_lookup_reads_all_pages_and_uses_destination_credentials() {
        let (mut api, thread, requests) = server(vec![
            (
                200,
                json!({"items":[{"id":"primary-id","summary":"Owner","primary":true}],"nextPageToken":"next"}),
            ),
            (
                200,
                json!({"items":[{"id":"secondary-id","summary":"Gallendar","accessRole":"owner"}]}),
            ),
        ]);
        api.tokens
            .insert("account:innowise".into(), "innowise-token".into());
        assert_eq!(
            api.named_calendar("innowise", "Gallendar", false).unwrap(),
            ("secondary-id".into(), false)
        );
        assert_eq!(api.tokens["secondary-id"], "innowise-token");
        thread.join().unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests.iter().all(|r| {
            r.starts_with("GET ")
                && r.to_lowercase()
                    .contains("authorization: bearer innowise-token")
        }));
        assert!(requests[1].contains("pageToken=next"));
    }

    #[test]
    fn named_calendar_rejects_duplicates_missing_primary_and_wrong_owner_without_writes() {
        for items in [
            json!([]),
            json!([{"id":"one","summary":"Gallendar","accessRole":"owner"},{"id":"two","summary":"Gallendar","accessRole":"owner"}]),
            json!([{"id":"one","summary":"Gallendar","accessRole":"owner","primary":true}]),
            json!([{"id":"one","summary":"Gallendar","accessRole":"writer"}]),
        ] {
            let (mut api, thread, requests) = server(vec![(200, json!({"items":items}))]);
            assert!(api.named_calendar("innowise", "Gallendar", false).is_err());
            thread.join().unwrap();
            assert_eq!(requests.lock().unwrap().len(), 1);
            assert!(requests.lock().unwrap()[0].starts_with("GET "));
        }
        let (mut api, thread, requests) = server(vec![
            (200, json!({"items":[],"nextPageToken":"next"})),
            (403, json!({})),
        ]);
        assert!(api.named_calendar("innowise", "Gallendar", true).is_err());
        thread.join().unwrap();
        assert!(
            requests
                .lock()
                .unwrap()
                .iter()
                .all(|r| r.starts_with("GET "))
        );
    }

    #[test]
    fn explicit_named_calendar_creation_is_verified_and_idempotent() {
        let calendar = json!({"id":"new-secondary","summary":"Gallendar","timeZone":"Europe/Warsaw","accessRole":"owner"});
        let (mut api, thread, requests) = server(vec![
            (200, json!({"items":[]})),
            (200, calendar.clone()),
            (200, calendar.clone()),
            (200, json!({"items":[calendar]})),
        ]);
        assert_eq!(
            api.named_calendar("innowise", "Gallendar", true).unwrap(),
            ("new-secondary".into(), true)
        );
        assert_eq!(
            api.named_calendar("innowise", "Gallendar", true).unwrap(),
            ("new-secondary".into(), false)
        );
        thread.join().unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(
            requests.iter().filter(|r| r.starts_with("POST ")).count(),
            1
        );
        assert!(requests[1].contains("Europe/Warsaw"));
        assert!(!requests[1].contains("/events"));
        assert!(requests[2].starts_with("GET /calendar/v3/calendars/new-secondary "));
    }

    #[test]
    fn source_and_primary_event_mutations_are_rejected_before_any_http_request() {
        let (config, memory) = crate::demo::fixture();
        let busy = memory.calendars["source-0"][0]
            .busy(&config.accounts[0], "Europe/Warsaw", config.all_day_policy)
            .unwrap()
            .unwrap();
        let (mut api, thread, requests) = server(vec![]);
        thread.join().unwrap();
        for calendar in ["source-0", "source-1", "primary", "innowise-primary"] {
            assert!(
                api.create(calendar, &busy, "namespace")
                    .unwrap_err()
                    .to_string()
                    .contains("outside configured")
            );
            assert!(api.update(calendar, "id", &busy, "namespace").is_err());
            assert!(api.delete(calendar, "id", "namespace").is_err());
            assert!(
                api.request(
                    calendar,
                    Method::POST,
                    api.events_url(calendar, None).unwrap(),
                    Some(&json!({})),
                    None
                )
                .is_err()
            );
        }
        assert!(requests.lock().unwrap().is_empty());
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
    #[test]
    fn retired_targets_allow_only_owned_deletion_not_create_or_update() {
        let (config, memory) = crate::demo::fixture();
        let busy = memory.calendars["source-0"][0]
            .busy(&config.accounts[0], "Europe/Warsaw", config.all_day_policy)
            .unwrap()
            .unwrap();
        let (mut api, thread, requests) = server(vec![]);
        thread.join().unwrap();
        api.cleanup_targets.insert("old-secondary".into());
        assert!(api.create("old-secondary", &busy, "namespace").is_err());
        assert!(
            api.update("old-secondary", "id", &busy, "namespace")
                .is_err()
        );
        assert!(api.require_delete_target("old-secondary").is_ok());
        assert!(requests.lock().unwrap().is_empty());
    }
    #[test]
    fn cleanup_deletes_owned_copy_with_readback_and_refuses_foreign_event() {
        let (config, memory) = crate::demo::fixture();
        let busy = memory.calendars["source-0"][0]
            .busy(&config.accounts[0], "Europe/Warsaw", config.all_day_policy)
            .unwrap()
            .unwrap();
        let namespace = crate::model::digest(&[&config.namespace]);
        let saved = busy.payload("old-id", &namespace);
        let (mut api, worker, requests) =
            server(vec![(200, saved), (204, json!(null)), (404, json!(null))]);
        api.cleanup_targets.insert("old-secondary".into());
        api.delete("old-secondary", "old-id", &namespace).unwrap();
        worker.join().unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(
            requests[1].starts_with("DELETE /calendar/v3/calendars/old-secondary/events/old-id")
        );
        assert!(requests[2].starts_with("GET "));
        let (mut api, worker, requests) = server(vec![(200, json!({"id":"foreign"}))]);
        api.cleanup_targets.insert("old-secondary".into());
        assert!(api.delete("old-secondary", "foreign", &namespace).is_err());
        worker.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 1);
    }
}
