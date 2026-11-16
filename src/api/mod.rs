//! Transport for hexium.wtf's own `/apisite/{namespace}{path}` proxy.
//!
//! Every call is same-origin and carries the `.ROBLOSECURITY` session cookie. The
//! site hands out a CSRF token in an `x-csrf-token` response header; requests send
//! the latest one back. A 403 that carries a new token is retried once with that
//! token, exactly as the site's own axios client does (`requestWithCsrf` in
//! `api-client.js`). Reads also get one retry after a dropped connection or a 5xx;
//! writes never do, because the first attempt may already have gone through. Any
//! 401/403 flips a flag the app watches so a dead session drops back to sign-in.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::RwLock;
use std::time::Duration;

use reqwest::header::{HeaderMap, SET_COOKIE};
use reqwest::{Client, Method, StatusCode};
use serde_json::Value;
use tokio::time::sleep;

pub mod endpoints;
pub mod models;

/// What the site's own bundle sends.
pub const USER_AGENT: &str = "Roblox2016/1.0";
/// The session cookie. HttpOnly on the site, which is why a native client is the
/// only place it can be attached without a browser doing it for you.
pub const SESSION_COOKIE: &str = ".ROBLOSECURITY";

#[derive(Debug, Clone)]
pub enum ApiError {
    Network(String),
    Auth(String),
    NotFound,
    RateLimited,
    Rejected(u16, String),
    Server(u16),
    Parse(String),
    /// A batch write that stopped partway; the text says how far it got.
    Partial(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Network(message) => write!(f, "network error: {message}"),
            ApiError::Auth(message) => write!(f, "not authorised — {message}"),
            ApiError::NotFound => write!(f, "not found (HTTP 404)"),
            ApiError::RateLimited => write!(f, "rate limited (HTTP 429)"),
            ApiError::Rejected(status, body) => {
                if body.is_empty() {
                    write!(f, "request rejected (HTTP {status})")
                } else {
                    write!(f, "request rejected (HTTP {status}): {body}")
                }
            }
            ApiError::Server(status) => write!(f, "server error (HTTP {status})"),
            ApiError::Parse(detail) => write!(f, "unexpected response: {detail}"),
            ApiError::Partial(detail) => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for ApiError {}

pub type Result<T> = std::result::Result<T, ApiError>;

pub struct ApiClient {
    http: Client,
    origin: String,
    cookies: RwLock<HashMap<String, String>>,
    csrf: RwLock<Option<String>>,
    last_auth_status: AtomicU16,
}

impl ApiClient {
    pub fn new(origin: &str, session: Option<&str>) -> Self {
        let http = match Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(client) => client,
            Err(_) => Client::new(),
        };
        let client = Self {
            http,
            origin: origin.trim_end_matches('/').to_string(),
            cookies: RwLock::new(HashMap::new()),
            csrf: RwLock::new(None),
            last_auth_status: AtomicU16::new(0),
        };
        client.set_session(session);
        client
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub fn has_session(&self) -> bool {
        self.cookies
            .read()
            .map(|cookies| cookies.contains_key(SESSION_COOKIE))
            .unwrap_or(false)
    }

    pub fn set_session(&self, session: Option<&str>) {
        if let Ok(mut cookies) = self.cookies.write() {
            match session.map(str::trim).filter(|value| !value.is_empty()) {
                Some(value) => {
                    cookies.insert(SESSION_COOKIE.to_string(), value.to_string());
                }
                None => {
                    cookies.remove(SESSION_COOKIE);
                }
            }
        }
        if let Ok(mut csrf) = self.csrf.write() {
            *csrf = None;
        }
        self.last_auth_status.store(0, Ordering::Relaxed);
    }

    /// Status of the most recent unauthorised answer (0 when there was none).
    /// 401 means the cookie is dead, 403 usually means CSRF trouble.
    pub fn take_auth_status(&self) -> u16 {
        self.last_auth_status.swap(0, Ordering::Relaxed)
    }

    fn url(&self, path: &str) -> String {
        if path.starts_with('/') {
            format!("{}{}", self.origin, path)
        } else {
            format!("{}/{path}", self.origin)
        }
    }

    fn cookie_header(&self) -> Option<String> {
        let cookies = self.cookies.read().ok()?;
        if cookies.is_empty() {
            return None;
        }
        Some(
            cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    fn csrf_token(&self) -> Option<String> {
        if let Some(token) = self.csrf.read().ok().and_then(|token| token.clone()) {
            return Some(token);
        }
        self.cookies
            .read()
            .ok()?
            .get("rbxcsrf4")
            .filter(|token| !token.is_empty())
            .cloned()
    }

    /// Keep whatever the server set: cookies by name, and its CSRF token.
    fn absorb(&self, headers: &HeaderMap) {
        if let Some(token) = headers
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
        {
            if !token.is_empty() {
                if let Ok(mut csrf) = self.csrf.write() {
                    *csrf = Some(token.to_string());
                }
            }
        }

        let mut cookies = match self.cookies.write() {
            Ok(cookies) => cookies,
            Err(poisoned) => poisoned.into_inner(),
        };
        for raw in headers.get_all(SET_COOKIE).iter() {
            let Ok(line) = raw.to_str() else { continue };
            let pair = line.split(';').next().unwrap_or_default().trim();
            if let Some((name, value)) = pair.split_once('=') {
                let name = name.trim();
                if !name.is_empty() {
                    cookies.insert(name.to_string(), value.trim().to_string());
                }
            }
        }
    }

    /// True when `/home` answers 200 instead of redirecting to `/login`.
    pub async fn probe(&self) -> Result<bool> {
        let mut request = self
            .http
            .get(self.url("/home"))
            .header("user-agent", USER_AGENT);
        if let Some(cookie) = self.cookie_header() {
            request = request.header("cookie", cookie);
        }
        let response = request
            .send()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        self.absorb(response.headers());
        Ok(response.status().is_success())
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        self.get_query(path, &[]).await
    }

    pub async fn get_query(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.send(Method::GET, path, query, None)
            .await
            .map(|(_, value)| value)
    }

    pub async fn post(&self, path: &str, body: Option<Value>) -> Result<Value> {
        self.send(Method::POST, path, &[], body)
            .await
            .map(|(_, value)| value)
    }

    pub async fn get_with_status(&self, path: &str) -> Result<(u16, Value)> {
        self.send(Method::GET, path, &[], None).await
    }

    pub async fn get_query_with_status(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<(u16, Value)> {
        self.send(Method::GET, path, query, None).await
    }

    pub async fn post_with_status(&self, path: &str, body: Option<Value>) -> Result<(u16, Value)> {
        self.send(Method::POST, path, &[], body).await
    }

    pub async fn patch(&self, path: &str, body: Option<Value>) -> Result<Value> {
        self.send(Method::PATCH, path, &[], body)
            .await
            .map(|(_, value)| value)
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        self.send(Method::DELETE, path, &[], None)
            .await
            .map(|(_, value)| value)
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<(u16, Value)> {
        // Only reads are safe to repeat blindly.
        let is_read = method == Method::GET;
        let mut csrf_retry = true;
        let mut transient_retry = is_read;

        loop {
            let (status, text, fresh_token) =
                match self.attempt(&method, path, query, body.as_ref()).await {
                    Ok(answer) => answer,
                    Err(error) => {
                        if transient_retry {
                            transient_retry = false;
                            sleep(Duration::from_millis(300)).await;
                            continue;
                        }
                        return Err(error);
                    }
                };

            // The site's rule: retry a 403 once, and only when it handed back a
            // token (which `attempt` has already stored). A 403 without one is a
            // real refusal. The rejected request did nothing, so writes may retry.
            if status == StatusCode::FORBIDDEN && csrf_retry && fresh_token {
                csrf_retry = false;
                continue;
            }
            if status.is_server_error() && transient_retry {
                transient_retry = false;
                sleep(Duration::from_millis(400)).await;
                continue;
            }

            match classify(status, &text) {
                Ok(value) => return Ok((status.as_u16(), value)),
                Err(error) => {
                    if matches!(error, ApiError::Auth(_)) {
                        self.last_auth_status.store(status.as_u16(), Ordering::Relaxed);
                    }
                    return Err(error);
                }
            }
        }
    }

    /// One request. Returns the status, the body, and whether the answer carried
    /// an `x-csrf-token` (already stored for the next request).
    async fn attempt(
        &self,
        method: &Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<(StatusCode, String, bool)> {

        let mut request = self
            .http
            .request(method.clone(), self.url(path))
            .header("user-agent", USER_AGENT)
            .header("accept", "application/json, text/plain, */*");
        if !query.is_empty() {
            request = request.query(query);
        }
        if let Some(cookie) = self.cookie_header() {
            request = request.header("cookie", cookie);
        }
        if let Some(token) = self.csrf_token() {
            request = request.header("x-csrf-token", token);
        }
        if let Some(payload) = body {
            request = request.json(payload);
        }

        let response = request
            .send()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        let status = response.status();
        let fresh_token = response
            .headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|token| !token.is_empty());
        self.absorb(response.headers());
        let text = response
            .text()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        Ok((status, text, fresh_token))
    }

    /// Raw bytes for a CDN image URL (thumbnails are public, but they still need a
    /// browser-ish user agent).
    pub async fn fetch_image(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .http
            .get(url)
            .header("user-agent", USER_AGENT)
            .send()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        if !response.status().is_success() {
            return Err(ApiError::Rejected(
                response.status().as_u16(),
                String::new(),
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        Ok(bytes.to_vec())
    }
}

fn classify(status: StatusCode, text: &str) -> Result<Value> {
    if status.is_success() {
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        // The site sometimes answers API calls with a full web page (seen after
        // bursts of requests); the HTML itself is no use to show anyone.
        if text.trim_start().starts_with('<') {
            return Err(ApiError::Parse(
                "the site sent back a web page instead of data, usually from too many \
                 requests in a row; try again in a moment"
                    .to_string(),
            ));
        }
        return serde_json::from_str(text).map_err(|_| ApiError::Parse(snippet(text)));
    }

    match status.as_u16() {
        401 => Err(ApiError::Auth(
            "the session cookie was not accepted".to_string(),
        )),
        403 => Err(ApiError::Auth(
            "forbidden (session or CSRF token rejected)".to_string(),
        )),
        404 => Err(ApiError::NotFound),
        429 => Err(ApiError::RateLimited),
        other if (400..500).contains(&other) => {
            Err(ApiError::Rejected(other, snippet(text)))
        }
        other => Err(ApiError::Server(other)),
    }
}

fn snippet(text: &str) -> String {
    let trimmed = text.trim().replace('\n', " ");
    if trimmed.is_empty() {
        return "empty body".to_string();
    }
    let cut: String = trimmed.chars().take(140).collect();
    if trimmed.chars().count() > 140 {
        format!("{cut}…")
    } else {
        cut
    }
}
