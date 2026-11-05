//! X OAuth PKCE authorization and automatic token refresh; no credential logging.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::error::is_shown;
use crate::shown;

const PREFIX: &str = "ENOUGH_INTERNET_FOR_TODAY_X_COM_";
const REDIRECT: &str = "http://127.0.0.1:8765/callback";
const TOKEN_URL: &str = "https://api.x.com/2/oauth2/token";

/// Where the tokens live by default.
pub fn default_store() -> PathBuf {
    crate::files::home().join(".config/herdr-digest/x-oauth.json")
}

/// The secrets file by default.
pub fn default_secrets() -> PathBuf {
    crate::files::home().join(".secrets")
}

/// The app's client ID and, unless it is a public client, its secret, from a shell-style
/// `NAME=value` file, read without running any of its shell code.
pub fn credentials(path: &Path, public: bool) -> Result<(String, Option<String>)> {
    let text = std::fs::read_to_string(path)?;
    let mut client_id = None;
    let mut secret = None;
    for line in text.lines() {
        let Some(mut parts) = shlex::split(line) else {
            return Err(shown!("Could not parse a line of the secrets file"));
        };
        if parts.first().is_some_and(|p| p == "export") {
            parts.remove(0);
        }
        if let [part] = parts.as_slice()
            && let Some((key, value)) = part.split_once('=')
        {
            match key.strip_prefix(PREFIX) {
                Some("CLIENT_ID") => client_id = Some(value.to_owned()),
                Some("SECRET") => secret = Some(value.to_owned()),
                _ => {}
            }
        }
    }
    match client_id {
        Some(id) if !id.is_empty() => Ok((id, if public { None } else { secret })),
        _ => Err(shown!(
            "X Client ID not found in the configured secrets file"
        )),
    }
}

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// Ask X's token endpoint for tokens; the result carries `expires_at`, in Unix seconds.
pub fn exchange(
    params: &[(&str, &str)],
    client_id: &str,
    client_secret: Option<&str>,
) -> Result<Map<String, Value>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(25)))
        .build()
        .into();
    let mut request = agent.post(TOKEN_URL);
    let mut form = params.to_vec();
    match client_secret.filter(|s| !s.is_empty()) {
        Some(secret) => {
            let encoded = STANDARD.encode(format!("{client_id}:{secret}"));
            request = request.header("Authorization", &format!("Basic {encoded}"));
        }
        None => form.push(("client_id", client_id)),
    }
    let mut response = match request.send_form(form) {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(code)) => {
            return Err(shown!(
                "X token exchange returned HTTP {code}. Check app type and OAuth credentials."
            ));
        }
        Err(e) => return Err(e.into()),
    };
    let result: Value = serde_json::from_str(&response.body_mut().read_to_string()?)?;
    issued(result, unix_now())
}

/// The tokens of an exchange's answer, which must carry both, with their expiry.
fn issued(result: Value, now: f64) -> Result<Map<String, Value>> {
    let Value::Object(mut result) = result else {
        return Err(shown!("X returned an invalid token response"));
    };
    let has = |key: &str| crate::files::truthy(result.get(key));
    if !has("access_token") || !has("refresh_token") {
        return Err(shown!(
            "X did not issue both access and refresh tokens; authorize with offline.access"
        ));
    }
    let expires_in = match result.get("expires_in") {
        None => 7200.0,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(7200.0),
        Some(Value::String(s)) => s
            .trim()
            .parse()
            .map_err(|_| shown!("X returned an invalid token lifetime"))?,
        Some(_) => return Err(shown!("X returned an invalid token lifetime")),
    };
    result.insert("expires_at".into(), json!(now + expires_in));
    Ok(result)
}

/// What exchanges a grant for tokens, [`exchange`] outside tests.
pub type Exchange<'a> =
    dyn FnMut(&[(&str, &str)], &str, Option<&str>) -> Result<Map<String, Value>> + 'a;

/// A valid access token from the store at `path`, refreshed and saved first when it expires
/// within two minutes. Holds the store's lock meanwhile, so two refreshes never race.
pub fn access_token(path: &Path) -> Result<String> {
    access_token_with(path, &mut exchange)
}

pub fn access_token_with(path: &Path, exchange: &mut Exchange<'_>) -> Result<String> {
    let lock = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path.with_extension("lock"))?;
    lock.lock()?;
    let mut stored: Map<String, Value> =
        match serde_json::from_str(&std::fs::read_to_string(path)?)? {
            Value::Object(map) => map,
            _ => return Err(shown!("Invalid X token store")),
        };
    let tokens = stored
        .get("tokens")
        .and_then(Value::as_object)
        .ok_or_else(|| shown!("Invalid X token store"))?
        .clone();
    let expires_at = tokens
        .get("expires_at")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let tokens = if expires_at <= unix_now() + 120.0 {
        let secrets = stored
            .get("secrets_file")
            .and_then(Value::as_str)
            .ok_or_else(|| shown!("Invalid X token store"))?;
        let public = crate::files::truthy(stored.get("public_client"));
        let (client_id, client_secret) = credentials(Path::new(secrets), public)?;
        let refresh = tokens
            .get("refresh_token")
            .and_then(Value::as_str)
            .ok_or_else(|| shown!("Invalid X token store"))?;
        let fresh = exchange(
            &[("grant_type", "refresh_token"), ("refresh_token", refresh)],
            &client_id,
            client_secret.as_deref(),
        )?;
        stored.insert("tokens".into(), Value::Object(fresh.clone()));
        crate::files::save(path, &Value::Object(stored))?;
        fresh
    } else {
        tokens
    };
    let token = tokens
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| shown!("Invalid X token store"))?;
    Ok(token.to_owned())
}

/// The authorization code of X's redirect to `target`, once its path and state check out.
pub fn callback_values(target: &str, expected_state: &str) -> Result<String> {
    let target = target.split_once('#').map_or(target, |(t, _)| t);
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut args: Map<String, Value> = Map::new();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if !value.is_empty() && !args.contains_key(key.as_ref()) {
            args.insert(key.into_owned(), Value::String(value.into_owned()));
        }
    }
    let arg = |key: &str| args.get(key).and_then(Value::as_str).unwrap_or("");
    if path != "/callback" {
        return Err(shown!("Unexpected callback path"));
    }
    if !constant_time_eq(arg("state").as_bytes(), expected_state.as_bytes()) {
        return Err(shown!("Invalid OAuth state"));
    }
    if !arg("error").is_empty() {
        return Err(shown!("X authorization was declined"));
    }
    if arg("code").is_empty() {
        return Err(shown!("Missing authorization code"));
    }
    Ok(arg("code").to_owned())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `bytes` random bytes, URL-safe base64 without padding, as Python's `token_urlsafe`.
fn token_urlsafe(bytes: usize) -> Result<String> {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(buffer))
}

/// How a callback request went.
enum Outcome {
    Connected,
    Failed(String),
}

/// Authorize the app in the user's browser, through a callback on 127.0.0.1:8765, then save
/// the tokens to `store` with mode 0600.
pub fn authorize(store: &Path, secrets_file: &Path, public: bool) -> Result<()> {
    let (client_id, client_secret) = credentials(secrets_file, public)?;
    let state = token_urlsafe(32)?;
    let verifier = token_urlsafe(64)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("response_type", "code"),
            ("client_id", &client_id),
            ("redirect_uri", REDIRECT),
            ("scope", "tweet.read users.read offline.access"),
            ("state", &state),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ])
        .finish();
    let listener = TcpListener::bind("127.0.0.1:8765")?;
    listener.set_nonblocking(true)?;
    let url = format!("https://x.com/i/oauth2/authorize?{query}");
    println!("Opening X authorization. Approve read access in your browser.");
    println!("{url}");
    let _ = Command::new("xdg-open")
        .arg(&url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut outcome = None;
    while outcome.is_none() && Instant::now() < deadline {
        match listener.accept() {
            Ok((stream, _)) => {
                outcome = callback(stream, |target| {
                    let code = callback_values(target, &state)?;
                    let tokens = exchange(
                        &[
                            ("grant_type", "authorization_code"),
                            ("code", &code),
                            ("redirect_uri", REDIRECT),
                            ("code_verifier", &verifier),
                        ],
                        &client_id,
                        client_secret.as_deref(),
                    )?;
                    crate::files::save(
                        store,
                        &json!({"tokens": tokens, "secrets_file": secrets_file.to_string_lossy(), "public_client": public}),
                    )
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e.into()),
        }
    }
    match outcome {
        Some(Outcome::Connected) => {
            println!("X connected. Tokens saved locally with mode 0600.");
            Ok(())
        }
        Some(Outcome::Failed(message)) => Err(shown!("{message}")),
        None => Err(shown!("Authorization timed out; run it again when ready")),
    }
}

/// Answer one request to the callback server with `complete`'s outcome; `None` when it does not
/// end the authorization, such as a stray request for another path.
fn callback(mut stream: TcpStream, complete: impl FnOnce(&str) -> Result<()>) -> Option<Outcome> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut request = Vec::new();
    let mut buffer = [0u8; 2048];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < 16 * 1024 {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(n) => request.extend_from_slice(&buffer[..n]),
        }
    }
    let request = String::from_utf8_lossy(&request);
    let mut words = request
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let (method, target) = (
        words.next().unwrap_or_default(),
        words.next().unwrap_or_default(),
    );
    let (status, body, outcome) = if method == "GET" {
        match complete(target) {
            Ok(()) => (
                "200 OK",
                "Enough Internet for Today is connected. You can close this tab.".to_owned(),
                Some(Outcome::Connected),
            ),
            Err(e) if is_shown(&e) => {
                let message = e.to_string();
                let ends = message.contains("declined") || message.contains("exchange");
                (
                    "400 Bad Request",
                    message.clone(),
                    ends.then_some(Outcome::Failed(message)),
                )
            }
            Err(_) => {
                let message = "Could not complete authorization; credentials were not logged";
                (
                    "500 Internal Server Error",
                    message.to_owned(),
                    Some(Outcome::Failed(message.to_owned())),
                )
            }
        }
    } else {
        ("501 Not Implemented", "Unsupported method".to_owned(), None)
    };
    let _ = write!(
        stream,
        "HTTP/1.0 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn rejects_forged_callback_and_declined_consent() {
        for target in [
            "/callback?code=secret&state=wrong",
            "/other?code=secret&state=ok",
            "/callback?error=access_denied&state=ok",
        ] {
            assert!(callback_values(target, "ok").is_err(), "{target}");
        }
        assert_eq!(
            callback_values("/callback?code=abc&state=ok", "ok").unwrap(),
            "abc"
        );
    }

    #[test]
    fn refreshes_expired_token_and_persists_rotated_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = directory.path().join("secrets");
        std::fs::write(
            &secrets,
            "ENOUGH_INTERNET_FOR_TODAY_X_COM_CLIENT_ID=client\n",
        )
        .unwrap();
        let path = directory.path().join("oauth.json");
        crate::files::save(
            &path,
            &json!({"tokens": {"access_token": "old", "refresh_token": "old-refresh", "expires_at": 0},
                    "secrets_file": secrets, "public_client": true}),
        )
        .unwrap();
        let mut calls = Vec::new();
        let mut endpoint = |params: &[(&str, &str)], client: &str, secret: Option<&str>| {
            calls.push((
                params
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect::<Vec<_>>(),
                client.to_owned(),
                secret.map(str::to_owned),
            ));
            issued(
                json!({"access_token": "new", "refresh_token": "new-refresh"}),
                unix_now(),
            )
        };
        assert_eq!(access_token_with(&path, &mut endpoint).unwrap(), "new");
        assert_eq!(access_token_with(&path, &mut endpoint).unwrap(), "new");
        assert_eq!(calls.len(), 1);
        assert!(
            calls[0]
                .0
                .contains(&("refresh_token".into(), "old-refresh".into()))
        );
        assert_eq!(
            (calls[0].1.as_str(), calls[0].2.as_deref()),
            ("client", None)
        );
        let stored: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(stored["tokens"]["refresh_token"], "new-refresh");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn secret_file_is_parsed_without_executing_shell() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("secrets");
        std::fs::write(
            &path,
            "export ENOUGH_INTERNET_FOR_TODAY_X_COM_CLIENT_ID='client'\nENOUGH_INTERNET_FOR_TODAY_X_COM_SECRET='secret'\nOTHER=$(bad-command)\n# comment\n",
        )
        .unwrap();
        assert_eq!(
            credentials(&path, false).unwrap(),
            ("client".into(), Some("secret".into()))
        );
        assert_eq!(credentials(&path, true).unwrap(), ("client".into(), None));
    }

    #[test]
    fn token_lifetime_sets_expiry() {
        let tokens = issued(
            json!({"access_token": "a", "refresh_token": "r", "expires_in": 60}),
            1000.0,
        )
        .unwrap();
        assert_eq!(tokens["expires_at"], 1060.0);
        assert!(issued(json!({"access_token": "a"}), 0.0).is_err());
    }
}
