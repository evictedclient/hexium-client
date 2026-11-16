//! A minimal SignalR client for hexium.wtf's chat hub at `/chat`, doing what the
//! site's bundled SignalR 6 client does: negotiate, open a WebSocket with the
//! session cookie, send the JSON-protocol handshake, then invoke
//! `ListenForMessages` every 5 seconds. Server events arrive as invocations whose
//! single argument is itself a JSON string.
//!
//! It runs on the tokio runtime, reconnects with backoff, and reports through a
//! callback. It never sends anything except the handshake, `ListenForMessages`
//! and pings: messages are sent over REST, behind the confirmation dialog.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message as Frame;

use super::{ApiClient, USER_AGENT};

/// SignalR's record separator: every JSON message ends with it.
const SEPARATOR: char = '\u{1e}';

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubStatus {
    Connecting,
    Live,
    /// Lost or refused; the text says why. A retry is scheduled.
    Offline(String),
}

#[derive(Debug, Clone)]
pub enum HubEvent {
    Status(HubStatus),
    /// `ChatMessageReceived`: `{ conversationId, id, message, sent, userId }`.
    Message {
        conversation_id: u64,
        id: String,
        content: String,
        sent: String,
        user_id: u64,
    },
    /// `ChatTyping`: `{ conversationId, userId, endsAt }` (epoch milliseconds).
    Typing {
        conversation_id: u64,
        user_id: u64,
        ends_at_ms: u64,
    },
    /// `ChatConversationAdded`: a new conversation; the list should reload.
    ConversationAdded,
}

/// Stops the background connection when dropped or when `stop` is called.
pub struct HubHandle {
    stop: Arc<AtomicBool>,
}

impl HubHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for HubHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts the connection loop. `emit` is called from the runtime for every event.
pub fn spawn<F>(rt: &tokio::runtime::Handle, api: Arc<ApiClient>, emit: F) -> HubHandle
where
    F: Fn(HubEvent) + Send + Sync + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    rt.spawn(async move {
        let mut backoff = Duration::from_secs(2);
        while !flag.load(Ordering::Relaxed) {
            emit(HubEvent::Status(HubStatus::Connecting));
            let result = run_once(&api, &flag, &emit).await;
            if flag.load(Ordering::Relaxed) {
                break;
            }
            let reason = match result {
                Ok(()) => "the connection closed".to_string(),
                Err(error) => error,
            };
            emit(HubEvent::Status(HubStatus::Offline(reason)));
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(60));
        }
    });
    HubHandle { stop }
}

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Negotiates, opens the socket and completes the handshake.
async fn open(api: &ApiClient) -> Result<Socket, String> {
    // 1. Negotiate (same origin, cookie auth).
    let negotiated = api
        .post("/chat/negotiate?negotiateVersion=1", None)
        .await
        .map_err(|error| format!("negotiate failed: {error}"))?;
    let token = negotiated
        .get("connectionToken")
        .or_else(|| negotiated.get("connectionId"))
        .and_then(Value::as_str)
        .ok_or("negotiate returned no connection token")?
        .to_string();

    // 2. Open the WebSocket with the session cookie.
    let origin = api.origin().to_string();
    let ws_origin = origin.replacen("https://", "wss://", 1).replacen("http://", "ws://", 1);
    let url = format!("{ws_origin}/chat?id={}", encode(&token));
    let mut request = url
        .into_client_request()
        .map_err(|error| format!("bad hub address: {error}"))?;
    let headers = request.headers_mut();
    if let Some(cookie) = api.cookie_header_value() {
        headers.insert(
            "Cookie",
            HeaderValue::from_str(&cookie).map_err(|_| "cookie is not a valid header")?,
        );
    }
    headers.insert("User-Agent", HeaderValue::from_static(USER_AGENT));
    if let Ok(value) = HeaderValue::from_str(&origin) {
        headers.insert("Origin", value);
    }
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|error| format!("could not open the hub socket: {error}"))?;

    // 3. Handshake: `{"protocol":"json","version":1}` answered by `{}`.
    socket.send(Frame::Text(format!("{}{SEPARATOR}", json!({ "protocol": "json", "version": 1 }))))
        .await
        .map_err(|error| format!("handshake failed: {error}"))?;
    let first = tokio::time::timeout(Duration::from_secs(10), socket.next())
        .await
        .map_err(|_| "no handshake reply")?
        .ok_or("closed during handshake")?
        .map_err(|error| format!("handshake failed: {error}"))?;
    if let Frame::Text(text) = &first {
        for record in text.split(SEPARATOR).filter(|record| !record.trim().is_empty()) {
            let value: Value = serde_json::from_str(record).unwrap_or(Value::Null);
            if let Some(error) = value.get("error").and_then(Value::as_str) {
                return Err(format!("hub refused the handshake: {error}"));
            }
        }
    }
    Ok(socket)
}

/// For `--check`: connect, subscribe once, wait for the server's answer, close.
pub async fn probe(api: &ApiClient) -> Result<String, String> {
    let mut socket = open(api).await?;
    let frame = json!({ "type": 1, "invocationId": "1", "target": "ListenForMessages", "arguments": [] });
    socket
        .send(Frame::Text(format!("{frame}{SEPARATOR}")))
        .await
        .map_err(|error| format!("send failed: {error}"))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let next = tokio::time::timeout_at(deadline, socket.next())
            .await
            .map_err(|_| "handshake ok, but ListenForMessages got no answer".to_string())?;
        let Some(Ok(Frame::Text(text))) = next else {
            return Err("the hub closed before answering ListenForMessages".to_string());
        };
        for record in text.split(SEPARATOR).filter(|record| !record.trim().is_empty()) {
            let value: Value = serde_json::from_str(record).unwrap_or(Value::Null);
            if value.get("type").and_then(Value::as_u64) == Some(3) {
                let _ = socket.close(None).await;
                return match value.get("error").and_then(Value::as_str) {
                    Some(error) => Err(format!("ListenForMessages failed: {error}")),
                    None => Ok("handshake ok · ListenForMessages completed".to_string()),
                };
            }
        }
    }
}

async fn run_once<F>(api: &ApiClient, stop: &AtomicBool, emit: &F) -> Result<(), String>
where
    F: Fn(HubEvent),
{
    let socket = open(api).await?;
    let (mut sink, mut stream) = socket.split();
    emit(HubEvent::Status(HubStatus::Live));

    // 4. Subscribe now and every 5 seconds, as the site does; ping every 15.
    let mut invocation: u64 = 0;
    let mut listen = tokio::time::interval(Duration::from_secs(5));
    let mut ping = tokio::time::interval(Duration::from_secs(15));
    let mut watchdog = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {
            _ = listen.tick() => {
                invocation += 1;
                let frame = json!({
                    "type": 1,
                    "invocationId": invocation.to_string(),
                    "target": "ListenForMessages",
                    "arguments": [],
                });
                sink.send(Frame::Text(format!("{frame}{SEPARATOR}")))
                    .await
                    .map_err(|error| format!("lost the hub: {error}"))?;
            }
            _ = ping.tick() => {
                sink.send(Frame::Text(format!("{}{SEPARATOR}", json!({ "type": 6 }))))
                    .await
                    .map_err(|error| format!("lost the hub: {error}"))?;
            }
            _ = watchdog.tick() => {
                if stop.load(Ordering::Relaxed) {
                    let _ = sink.send(Frame::Close(None)).await;
                    return Ok(());
                }
            }
            incoming = stream.next() => {
                let frame = match incoming {
                    Some(Ok(frame)) => frame,
                    Some(Err(error)) => return Err(format!("lost the hub: {error}")),
                    None => return Ok(()),
                };
                match frame {
                    Frame::Text(text) => {
                        for record in text.split(SEPARATOR).filter(|record| !record.trim().is_empty()) {
                            if let Some(closed) = handle_record(record, emit) {
                                return Err(closed);
                            }
                        }
                    }
                    Frame::Close(_) => return Ok(()),
                    _ => {}
                }
            }
        }
    }
}

/// Handles one hub message. Returns `Some(reason)` when the server closes the
/// connection.
fn handle_record<F>(record: &str, emit: &F) -> Option<String>
where
    F: Fn(HubEvent),
{
    let value: Value = serde_json::from_str(record).ok()?;
    match value.get("type").and_then(Value::as_u64) {
        // Invocation from the server.
        Some(1) => {
            let target = value.get("target").and_then(Value::as_str).unwrap_or_default();
            let argument = value
                .get("arguments")
                .and_then(Value::as_array)
                .and_then(|arguments| arguments.first())
                .cloned()
                .unwrap_or(Value::Null);
            // The site does `JSON.parse(payload)`: the argument is a JSON string.
            let payload = match &argument {
                Value::String(text) => serde_json::from_str(text).unwrap_or(Value::Null),
                other => other.clone(),
            };
            if let Some(event) = to_event(target, &payload) {
                emit(event);
            }
            None
        }
        // Close.
        Some(7) => Some(
            value
                .get("error")
                .and_then(Value::as_str)
                .map(|error| format!("hub closed: {error}"))
                .unwrap_or_else(|| "hub closed the connection".to_string()),
        ),
        // Completions (type 3) and pings (type 6) need nothing.
        _ => None,
    }
}

fn to_event(target: &str, payload: &Value) -> Option<HubEvent> {
    let number = |key: &str| match payload.get(key) {
        Some(Value::Number(number)) => number.as_u64().unwrap_or(0),
        Some(Value::String(text)) => text.parse().unwrap_or(0),
        _ => 0,
    };
    let text = |key: &str| match payload.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    };
    match target {
        "ChatMessageReceived" => Some(HubEvent::Message {
            conversation_id: number("conversationId"),
            id: text("id"),
            content: text("message"),
            sent: text("sent"),
            user_id: number("userId"),
        }),
        "ChatTyping" => Some(HubEvent::Typing {
            conversation_id: number("conversationId"),
            user_id: number("userId"),
            ends_at_ms: number("endsAt"),
        }),
        "ChatConversationAdded" => Some(HubEvent::ConversationAdded),
        _ => None,
    }
}

/// Percent-encodes a connection token for the query string.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn message_event_parses_the_json_string_argument() {
        let seen = Mutex::new(Vec::new());
        let record = r#"{"type":1,"target":"ChatMessageReceived","arguments":["{\"conversationId\":42,\"id\":\"abc\",\"message\":\"hi\",\"sent\":\"2026-11-16T18:00:00Z\",\"userId\":7}"]}"#;
        assert!(handle_record(record, &|event| seen.lock().unwrap().push(event)).is_none());
        let seen = seen.into_inner().unwrap();
        match seen.as_slice() {
            [HubEvent::Message { conversation_id, id, content, user_id, .. }] => {
                assert_eq!((*conversation_id, id.as_str(), content.as_str(), *user_id), (42, "abc", "hi", 7));
            }
            other => panic!("unexpected events: {other:?}"),
        }
    }

    #[test]
    fn close_and_completion_records() {
        assert_eq!(
            handle_record(r#"{"type":7,"error":"bye"}"#, &|_| {}),
            Some("hub closed: bye".to_string())
        );
        assert!(handle_record(r#"{"type":3,"invocationId":"1","result":"x"}"#, &|_| {}).is_none());
        assert_eq!(encode("a b+c"), "a%20b%2Bc");
    }
}
