use crate::config::{Account, AuthMode, Config};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use fs2::FileExt;
use reqwest::{Url, blocking::Client};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";

#[derive(Deserialize)]
struct ClientFile {
    installed: DesktopClient,
}
#[derive(Deserialize)]
struct DesktopClient {
    client_id: String,
    client_secret: String,
}
#[derive(Serialize, Deserialize)]
struct Credentials {
    client_id: String,
    email: String,
    refresh_token: String,
    scopes: Vec<String>,
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    scope: Option<String>,
}
#[derive(Deserialize)]
struct Identity {
    email: String,
    email_verified: bool,
}

fn client() -> Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
fn desktop_client(path: &Path) -> Result<DesktopClient> {
    let bytes = fs::read(path).context("Cannot read OAuth client JSON; configure oauth.client")?;
    let file: ClientFile = serde_json::from_slice(&bytes).map_err(|_| {
        anyhow::anyhow!("Expected Google Desktop app client JSON (contents omitted)")
    })?;
    ensure!(
        file.installed
            .client_id
            .ends_with(".apps.googleusercontent.com")
            && !file.installed.client_secret.is_empty(),
        "Invalid Google Desktop app client credentials"
    );
    Ok(file.installed)
}

pub fn token_path(config: &Config, account: &Account) -> Result<PathBuf> {
    ensure!(
        !account.name.is_empty()
            && account
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "Invalid account name"
    );
    Ok(config
        .oauth
        .as_ref()
        .context("Configure [oauth] client and tokens_dir first")?
        .tokens_dir
        .join(format!("{}.json", account.name)))
}
fn scopes(config: &Config, account: &Account) -> Result<Vec<String>> {
    let reader = config.routes().iter().any(|r| r.from == account.name);
    let writer = config.routes().iter().any(|r| r.to == account.name);
    ensure!(reader || writer, "Account is not used by any route");
    let mut required = vec![
        "openid".into(),
        "email".into(),
        if writer {
            "https://www.googleapis.com/auth/calendar.events"
        } else {
            "https://www.googleapis.com/auth/calendar.events.readonly"
        }
        .into(),
        "https://www.googleapis.com/auth/calendar.events.freebusy".into(),
    ];
    if writer && config.mirror_calendar_name.is_some() {
        required.extend([
            "https://www.googleapis.com/auth/calendar.calendarlist.readonly".into(),
            "https://www.googleapis.com/auth/calendar.calendars".into(),
        ]);
    }
    Ok(required)
}
fn scope_present(granted: &[String], requested: &str) -> bool {
    granted.iter().any(|s| {
        s == requested
            || (requested == "email" && s == "https://www.googleapis.com/auth/userinfo.email")
            || (requested == "https://www.googleapis.com/auth/calendar.events.readonly"
                && s == "https://www.googleapis.com/auth/calendar.events")
    })
}
fn check_credentials(
    saved: &Credentials,
    desktop: &DesktopClient,
    account: &Account,
    required: &[String],
) -> Result<()> {
    ensure!(
        saved.client_id == desktop.client_id,
        "OAuth client changed; log in again for this account"
    );
    ensure!(
        saved.email.eq_ignore_ascii_case(&account.owner_email),
        "OAuth token belongs to another owner_email; log in again"
    );
    ensure!(
        !saved.refresh_token.is_empty(),
        "Missing refresh token; log in again"
    );
    ensure!(
        required.iter().all(|s| scope_present(&saved.scopes, s)),
        "OAuth permissions changed or incomplete; log in again"
    );
    Ok(())
}
fn read_credentials(path: &Path) -> Result<Credentials> {
    let bytes =
        fs::read(path).context("OAuth login missing; run gallendar auth login --account NAME")?;
    serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Invalid OAuth token file (contents omitted); log in again"))
}
fn credentials_lock(path: &Path) -> Result<std::fs::File> {
    let parent = path.parent().context("Missing token directory")?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path.with_extension("lock"))?;
    file.try_lock_exclusive().context(
        "Another command is using this account's OAuth credentials; retry after it finishes",
    )?;
    Ok(file)
}

fn save_credentials(path: &Path, credentials: &Credentials) -> Result<()> {
    let parent = path.parent().context("Missing token directory")?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    let temp = parent.join(format!(".oauth-{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| -> Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&serde_json::to_vec(credentials)?)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        Ok(())
    })();
    let _ = fs::remove_file(temp);
    result
}
fn exchange(http: &Client, endpoint: &str, form: &[(&str, &str)]) -> Result<TokenResponse> {
    let response = http
        .post(endpoint)
        .form(form)
        .send()
        .map_err(|_| anyhow::anyhow!("OAuth token request failed (network or timeout)"))?;
    ensure!(
        response.status().is_success(),
        "OAuth token request rejected (HTTP {}); log in again if access expired or was revoked; check corporate policy",
        response.status().as_u16()
    );
    let token: TokenResponse = response
        .json()
        .map_err(|_| anyhow::anyhow!("Invalid OAuth token response (contents omitted)"))?;
    ensure!(
        !token.access_token.is_empty(),
        "Google returned an empty access token"
    );
    Ok(token)
}
pub fn access_token(config: &Config, account: &Account) -> Result<String> {
    let path = token_path(config, account)?;
    let _credentials_lock = credentials_lock(&path)?;
    let desktop = desktop_client(&config.oauth.as_ref().unwrap().client)?;
    refresh(config, account, &desktop, &path, &client()?, TOKEN_URL)
}
fn refresh(
    config: &Config,
    account: &Account,
    desktop: &DesktopClient,
    path: &Path,
    http: &Client,
    endpoint: &str,
) -> Result<String> {
    let mut saved = read_credentials(path)?;
    let required = scopes(config, account)?;
    check_credentials(&saved, desktop, account, &required)?;
    let token = exchange(
        http,
        endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &desktop.client_id),
            ("client_secret", &desktop.client_secret),
            ("refresh_token", &saved.refresh_token),
        ],
    )?;
    if let Some(scope) = token.scope {
        let granted = scope
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        ensure!(
            required.iter().all(|s| scope_present(&granted, s)),
            "Google reduced OAuth permissions; refusing calendar access"
        );
        saved.scopes = granted;
    }
    if let Some(refresh_token) = token.refresh_token {
        ensure!(
            !refresh_token.is_empty(),
            "Google returned an empty refresh token"
        );
        saved.refresh_token = refresh_token;
        save_credentials(path, &saved)?;
    }
    Ok(token.access_token)
}

fn callback(target: &str, state: &str) -> Result<String> {
    let url = Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|_| anyhow::anyhow!("Invalid OAuth callback"))?;
    ensure!(url.path() == "/callback", "Unexpected OAuth callback path");
    let pairs = url.query_pairs().collect::<Vec<_>>();
    let values = |key: &str| {
        pairs
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.to_string())
            .collect::<Vec<_>>()
    };
    ensure!(values("state") == [state], "OAuth callback state mismatch");
    ensure!(
        values("error").is_empty(),
        "Google sign-in was denied or blocked; previous login preserved"
    );
    let codes = values("code");
    ensure!(
        codes.len() == 1 && !codes[0].is_empty(),
        "Missing OAuth authorization code"
    );
    Ok(codes[0].clone())
}
fn wait_code(listener: &TcpListener, state: &str) -> Result<String> {
    listener.set_nonblocking(true)?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(180) {
        match listener.accept() {
            Ok((mut connection, _)) => {
                connection.set_read_timeout(Some(Duration::from_secs(2)))?;
                connection.set_write_timeout(Some(Duration::from_secs(2)))?;
                let mut request = Vec::new();
                let mut byte = [0; 1];
                while request.len() < 8192 {
                    if connection.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    request.push(byte[0]);
                    if request.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                let line = String::from_utf8_lossy(&request);
                let mut parts = line.split_whitespace();
                let method = parts.next();
                let target = parts.next().unwrap_or("");
                let result = if method == Some("GET") {
                    callback(target, state)
                } else {
                    Err(anyhow::anyhow!("Expected OAuth GET callback"))
                };
                let valid = result.is_ok();
                let status = if valid { "200 OK" } else { "400 Bad Request" };
                let message = if valid {
                    "Authorization received. Return to the terminal to check the result."
                } else {
                    "Invalid callback. Return to the terminal and retry sign-in."
                };
                let _ = write!(
                    connection,
                    "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{message}",
                    message.len()
                );
                if valid {
                    return result;
                }
                // A genuine denial should stop immediately; unrelated requests do not consume the login.
                if target.contains("error=")
                    && callback(target, state)
                        .err()
                        .is_some_and(|e| e.to_string().contains("denied"))
                {
                    return result;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(e) => return Err(e.into()),
        }
    }
    anyhow::bail!("OAuth sign-in timed out after 180 seconds; previous login preserved")
}

pub fn login(config: &Config, name: &str, no_browser: bool) -> Result<()> {
    let account = config
        .accounts
        .iter()
        .find(|a| a.name == name)
        .context("Unknown account name")?;
    ensure!(
        account.auth.unwrap_or(config.auth) == AuthMode::Oauth,
        "Select auth = \"oauth\" globally or for this account first"
    );
    ensure!(
        account.owner_email.contains('@') && !account.owner_email.contains("REPLACE"),
        "Set the real owner_email before OAuth login"
    );
    let path = token_path(config, account)?;
    let _credentials_lock = credentials_lock(&path)?;
    let desktop = desktop_client(&config.oauth.as_ref().unwrap().client)?;
    let required = scopes(config, account)?;
    let listener =
        TcpListener::bind("127.0.0.1:0").context("Cannot start local OAuth callback listener")?;
    let redirect = format!(
        "http://127.0.0.1:{}/callback",
        listener.local_addr()?.port()
    );
    let state = uuid::Uuid::new_v4().simple().to_string();
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = Url::parse("https://accounts.google.com/o/oauth2/v2/auth")?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", desktop.client_id.as_str()),
        ("redirect_uri", &redirect),
        ("response_type", "code"),
        ("scope", &required.join(" ")),
        ("access_type", "offline"),
        ("prompt", "consent select_account"),
        ("login_hint", &account.owner_email),
        ("state", &state),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
    ]);
    println!(
        "Sign in as {} for {name}. Open this URL on this computer:\n{url}",
        account.owner_email
    );
    if !no_browser {
        #[cfg(target_os = "macos")]
        let opener = "open";
        #[cfg(not(target_os = "macos"))]
        let opener = "xdg-open";
        let _ = std::process::Command::new(opener)
            .arg(url.as_str())
            .status();
    }
    let code = wait_code(&listener, &state)?;
    let http = client()?;
    let token = exchange(
        &http,
        TOKEN_URL,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", &desktop.client_id),
            ("client_secret", &desktop.client_secret),
            ("code", &code),
            ("redirect_uri", &redirect),
            ("code_verifier", &verifier),
        ],
    )?;
    complete_login(config, account, &desktop, token, &http, USERINFO_URL)?;
    println!("OAuth login saved for {name}. Other account logins are preserved.");
    Ok(())
}

fn complete_login(
    config: &Config,
    account: &Account,
    desktop: &DesktopClient,
    token: TokenResponse,
    http: &Client,
    identity_url: &str,
) -> Result<()> {
    let response = http
        .get(identity_url)
        .bearer_auth(&token.access_token)
        .send()
        .map_err(|_| anyhow::anyhow!("Cannot verify signed-in Google identity"))?;
    ensure!(
        response.status().is_success(),
        "Google identity verification rejected"
    );
    let identity: Identity = response
        .json()
        .map_err(|_| anyhow::anyhow!("Invalid Google identity response"))?;
    ensure!(
        identity.email_verified && identity.email.eq_ignore_ascii_case(&account.owner_email),
        "Signed in to the wrong Google account; existing credentials preserved"
    );
    let required = scopes(config, account)?;
    let path = token_path(config, account)?;
    let saved = Credentials {
        client_id: desktop.client_id.clone(),
        email: identity.email,
        refresh_token: token
            .refresh_token
            .context("Google did not issue offline credentials; retry login")?,
        scopes: token
            .scope
            .context("Google did not return granted permissions")?
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
    };
    check_credentials(&saved, desktop, account, &required)?;
    save_credentials(&path, &saved)?;
    Ok(())
}

pub fn status(config: &Config) -> serde_json::Value {
    serde_json::json!({ "accounts": config.accounts.iter().map(|a| {
        let mode = a.auth.unwrap_or(config.auth);
        let stored = if mode == AuthMode::ServiceAccount { config.service_account_key.is_file() }
            else { (|| -> Result<()> {
                let path = token_path(config, a)?;
                let desktop = desktop_client(&config.oauth.as_ref().unwrap().client)?;
                check_credentials(&read_credentials(&path)?, &desktop, a, &scopes(config, a)?)
            })().is_ok() };
        serde_json::json!({"account": a.name, "auth": mode, "credentials_present": stored})
    }).collect::<Vec<_>>(), "google_access_verified": false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OAuthConfig;
    use serde_json::json;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p =
                std::env::temp_dir().join(format!("gallendar-oauth-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture(temp: &Temp) -> (Config, DesktopClient) {
        let (mut config, _) = crate::demo::fixture();
        config.auth = AuthMode::Oauth;
        config.oauth = Some(OAuthConfig {
            client: temp.0.join("client.json"),
            tokens_dir: temp.0.join("tokens"),
        });
        let desktop = DesktopClient {
            client_id: "test.apps.googleusercontent.com".into(),
            client_secret: "test-secret".into(),
        };
        (config, desktop)
    }
    fn credentials(config: &Config, account: &Account, desktop: &DesktopClient) -> Credentials {
        Credentials {
            client_id: desktop.client_id.clone(),
            email: account.owner_email.clone(),
            refresh_token: format!("refresh-{}", account.name),
            scopes: scopes(config, account).unwrap(),
        }
    }

    #[test]
    fn callback_rejects_forged_state_duplicates_and_denial() {
        assert_eq!(
            callback("/callback?state=expected&code=hello%2Bworld", "expected").unwrap(),
            "hello+world"
        );
        for target in [
            "/callback?state=wrong&code=secret",
            "/callback?state=expected&state=expected&code=secret",
            "/callback?state=expected&code=a&code=b",
            "/callback?state=expected&error=access_denied",
            "/other?state=expected&code=secret",
        ] {
            let error = callback(target, "expected").unwrap_err().to_string();
            assert!(!error.contains("secret"));
        }
    }

    #[test]
    fn loopback_ignores_unrelated_request_then_accepts_callback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || wait_code(&listener, "test-state"));
        let http = client().unwrap();
        assert_eq!(
            http.get(format!("http://{address}/favicon.ico"))
                .send()
                .unwrap()
                .status(),
            400
        );
        assert_eq!(
            http.get(format!(
                "http://{address}/callback?state=test-state&code=test-code"
            ))
            .send()
            .unwrap()
            .status(),
            200
        );
        assert_eq!(worker.join().unwrap().unwrap(), "test-code");
    }

    #[test]
    fn separate_account_credentials_and_permissions_survive_relogin() {
        let temp = Temp::new();
        let (config, desktop) = fixture(&temp);
        let first = &config.accounts[0];
        let second = &config.accounts[1];
        let second_path = token_path(&config, second).unwrap();
        save_credentials(&second_path, &credentials(&config, second, &desktop)).unwrap();
        let original = fs::read(&second_path).unwrap();
        let first_path = token_path(&config, first).unwrap();
        save_credentials(&first_path, &credentials(&config, first, &desktop)).unwrap();
        assert_eq!(fs::read(&second_path).unwrap(), original);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(first_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(second_path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn named_calendar_permissions_apply_only_to_destinations_and_require_relogin() {
        let (mut config, _) = crate::demo::fixture();
        config.routes = Some(vec![crate::config::Route {
            from: "account-0".into(),
            to: "account-1".into(),
            mode: crate::config::MirrorMode::Hidden,
            hidden_title: "Busy".into(),
            prefix: String::new(),
            attendees: vec![],
        }]);
        let previous = scopes(&config, &config.accounts[1]).unwrap();
        config.mirror_calendar_name = Some("Gallendar".into());
        let source = scopes(&config, &config.accounts[0]).unwrap();
        let target = scopes(&config, &config.accounts[1]).unwrap();
        assert!(
            !source
                .iter()
                .any(|s| s.contains("calendar.calendarlist") || s.contains("calendar.calendars"))
        );
        assert!(
            target
                .iter()
                .any(|s| s.ends_with("calendar.calendarlist.readonly"))
        );
        assert!(target.iter().any(|s| s.ends_with("calendar.calendars")));
        assert!(target.iter().any(|s| !scope_present(&previous, s)));
    }

    #[test]
    fn wrong_owner_client_or_scopes_cannot_use_saved_token() {
        let temp = Temp::new();
        let (config, desktop) = fixture(&temp);
        let account = &config.accounts[0];
        let mut saved = credentials(&config, account, &desktop);
        let required = scopes(&config, account).unwrap();
        saved.email = config.accounts[1].owner_email.clone();
        assert!(check_credentials(&saved, &desktop, account, &required).is_err());
        saved.email = account.owner_email.clone();
        saved.client_id = "different-client".into();
        assert!(check_credentials(&saved, &desktop, account, &required).is_err());
        saved.client_id = desktop.client_id.clone();
        saved.scopes.retain(|s| !s.contains("calendar.events"));
        assert!(check_credentials(&saved, &desktop, account, &required).is_err());
    }

    fn token_server(
        status: u16,
        response: serde_json::Value,
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/token", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut request = Vec::new();
            loop {
                let mut bytes = [0; 4096];
                let count = conn.read(&mut bytes).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&bytes[..count]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&request[..end]);
                    let length = header
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
            let response = response.to_string();
            write!(conn, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (url, worker)
    }

    #[test]
    fn refresh_without_browser_preserves_or_rotates_offline_token() {
        let temp = Temp::new();
        let (config, desktop) = fixture(&temp);
        let account = &config.accounts[0];
        let path = token_path(&config, account).unwrap();
        save_credentials(&path, &credentials(&config, account, &desktop)).unwrap();
        let original = fs::read(&path).unwrap();
        for rotated in [None, Some("rotated-token")] {
            let mut response = json!({"access_token":"fresh-access-token"});
            if let Some(token) = rotated {
                response["refresh_token"] = json!(token);
            }
            let (url, worker) = token_server(200, response);
            assert_eq!(
                refresh(&config, account, &desktop, &path, &client().unwrap(), &url).unwrap(),
                "fresh-access-token"
            );
            let request = worker.join().unwrap();
            assert!(request.contains("grant_type=refresh_token"));
            if rotated.is_none() {
                assert_eq!(fs::read(&path).unwrap(), original);
            } else {
                assert_eq!(
                    read_credentials(&path).unwrap().refresh_token,
                    "rotated-token"
                );
            }
        }
    }

    #[test]
    fn rejected_refresh_preserves_credentials_and_hides_server_secrets() {
        let temp = Temp::new();
        let (config, desktop) = fixture(&temp);
        let account = &config.accounts[0];
        let path = token_path(&config, account).unwrap();
        save_credentials(&path, &credentials(&config, account, &desktop)).unwrap();
        let original = fs::read(&path).unwrap();
        let (url, worker) = token_server(
            400,
            json!({"error":"invalid_grant", "error_description":"secret-server-data"}),
        );
        let error = refresh(&config, account, &desktop, &path, &client().unwrap(), &url)
            .unwrap_err()
            .to_string();
        worker.join().unwrap();
        assert!(!error.contains("secret-server-data"));
        assert_eq!(fs::read(&path).unwrap(), original);
    }
    #[test]
    fn verified_login_saves_only_selected_account_and_wrong_identity_preserves_it() {
        let temp = Temp::new();
        let (config, desktop) = fixture(&temp);
        let account = &config.accounts[0];
        let other = &config.accounts[1];
        let other_path = token_path(&config, other).unwrap();
        save_credentials(&other_path, &credentials(&config, other, &desktop)).unwrap();
        let original_other = fs::read(&other_path).unwrap();
        let (url, worker) = token_server(
            200,
            json!({"email":account.owner_email, "email_verified":true}),
        );
        let response = TokenResponse {
            access_token: "login-access".into(),
            refresh_token: Some("login-refresh".into()),
            scope: Some(scopes(&config, account).unwrap().join(" ")),
        };
        complete_login(
            &config,
            account,
            &desktop,
            response,
            &client().unwrap(),
            &url,
        )
        .unwrap();
        let request = worker.join().unwrap();
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer login-access")
        );
        let path = token_path(&config, account).unwrap();
        assert_eq!(
            read_credentials(&path).unwrap().refresh_token,
            "login-refresh"
        );
        assert_eq!(fs::read(&other_path).unwrap(), original_other);
        let original = fs::read(&path).unwrap();
        for identity in [
            json!({"email":other.owner_email,"email_verified":true}),
            json!({"email":account.owner_email,"email_verified":false}),
        ] {
            let (url, worker) = token_server(200, identity);
            let response = TokenResponse {
                access_token: "wrong-login".into(),
                refresh_token: Some("wrong-refresh".into()),
                scope: Some(scopes(&config, account).unwrap().join(" ")),
            };
            assert!(
                complete_login(
                    &config,
                    account,
                    &desktop,
                    response,
                    &client().unwrap(),
                    &url
                )
                .is_err()
            );
            worker.join().unwrap();
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn authorization_code_exchange_sends_pkce_and_keeps_tokens_out_of_errors() {
        let (url, worker) = token_server(
            200,
            json!({"access_token":"test-access","refresh_token":"test-refresh", "scope":"openid email"}),
        );
        let token = exchange(
            &client().unwrap(),
            &url,
            &[
                ("grant_type", "authorization_code"),
                ("code", "test-code"),
                ("code_verifier", "test-verifier"),
                ("redirect_uri", "http://127.0.0.1:1234/callback"),
            ],
        )
        .unwrap();
        assert_eq!(token.refresh_token.as_deref(), Some("test-refresh"));
        let request = worker.join().unwrap();
        assert!(request.contains("grant_type=authorization_code"));
        assert!(request.contains("code_verifier=test-verifier"));
        assert!(request.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A1234%2Fcallback"));
    }

    #[test]
    fn per_account_lock_prevents_concurrent_token_replacement() {
        let temp = Temp::new();
        let (config, _) = fixture(&temp);
        let path = token_path(&config, &config.accounts[0]).unwrap();
        let first = credentials_lock(&path).unwrap();
        assert!(credentials_lock(&path).is_err());
        assert!(credentials_lock(&token_path(&config, &config.accounts[1]).unwrap()).is_ok());
        drop(first);
        assert!(credentials_lock(&path).is_ok());
    }
}
