//! ElevenLabs realtime STT client — CONTRACTS §6.
//!
//! Wire schema confirmed 2026-07-13 from
//! https://elevenlabs.io/docs/api-reference/speech-to-text/v-1-speech-to-text-realtime
//!
//! Client → server `input_audio_chunk`:
//!   `{ "message_type": "input_audio_chunk", "audio_base_64": "...", "commit": bool, "sample_rate": 16000 }`
//!
//! Server → client (discriminated by `message_type`):
//!   `session_started`, `partial_transcript` { text }, `committed_transcript` { text },
//!   plus error types: `auth_error`, `quota_exceeded`, `rate_limited`, `resource_exhausted`, etc.

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, http::StatusCode, Message},
};
use vf_core::SttSettings;

use crate::error::{CloudError, CloudResult};

const SAMPLE_RATE: i32 = 16_000;
const PATH: &str = "/v1/speech-to-text/realtime";
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const COMMIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);
const SESSION_STARTED_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

// ---------------------------------------------------------------------------
// Key rotation state machine (pure — unit-tested with no network)
// ---------------------------------------------------------------------------

/// Error kinds that trigger key rotation per §6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotatableError {
    Auth,
    Quota,
    RateLimited,
    ResourceExhausted,
    /// HTTP 401 / 403 / 429 at WebSocket handshake.
    HandshakeAuthOrRate,
}

impl RotatableError {
    pub fn from_message_type(message_type: &str) -> Option<Self> {
        match message_type {
            "auth_error" => Some(Self::Auth),
            "quota_exceeded" => Some(Self::Quota),
            "rate_limited" => Some(Self::RateLimited),
            "resource_exhausted" => Some(Self::ResourceExhausted),
            _ => None,
        }
    }

    pub fn from_http_status(status: u16) -> Option<Self> {
        match status {
            401 | 403 | 429 => Some(Self::HandshakeAuthOrRate),
            _ => None,
        }
    }
}

/// Ordered API-key rotator: at most one full cycle per utterance.
#[derive(Debug, Clone)]
pub struct KeyRotator {
    keys: Vec<String>,
    /// Index of the key currently in use.
    current: usize,
    /// Number of keys already tried (including the current one once it fails).
    tried: usize,
    last_error: Option<String>,
}

impl KeyRotator {
    pub fn new(keys: Vec<String>) -> CloudResult<Self> {
        let keys: Vec<String> = keys
            .into_iter()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .collect();
        if keys.is_empty() {
            return Err(CloudError::NoApiKeys);
        }
        Ok(Self {
            keys,
            current: 0,
            tried: 0,
            last_error: None,
        })
    }

    pub fn current_key(&self) -> &str {
        &self.keys[self.current]
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Record a failure on the current key. Returns the next key if another
    /// attempt is allowed (strictly less than one full cycle), else `None`.
    pub fn fail_and_advance(&mut self, error: impl Into<String>) -> Option<&str> {
        self.last_error = Some(error.into());
        self.tried += 1;
        if self.tried >= self.keys.len() {
            return None;
        }
        self.current = (self.current + 1) % self.keys.len();
        Some(self.current_key())
    }

    pub fn aggregate_error(&self) -> CloudError {
        CloudError::AllKeysFailed(
            self.last_error
                .clone()
                .unwrap_or_else(|| "unknown error".into()),
        )
    }
}

// ---------------------------------------------------------------------------
// Wire messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ServerMessage {
    pub message_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

impl ServerMessage {
    pub fn parse(raw: &str) -> CloudResult<Self> {
        Ok(serde_json::from_str(raw)?)
    }
}

/// Build the `input_audio_chunk` JSON client message.
pub fn encode_audio_chunk(pcm_s16le: &[u8], commit: bool) -> String {
    let audio_base_64 = base64::engine::general_purpose::STANDARD.encode(pcm_s16le);
    serde_json::json!({
        "message_type": "input_audio_chunk",
        "audio_base_64": audio_base_64,
        "commit": commit,
        "sample_rate": SAMPLE_RATE,
    })
    .to_string()
}

/// Minimal percent-encoding for query parameter values (no extra crates).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}

/// Build the WebSocket URL from settings + keyterms.
pub fn build_ws_url(settings: &SttSettings, keyterms: &[String]) -> String {
    let base = settings.endpoint.trim_end_matches('/');
    let mut url = format!(
        "{PATH}?model_id={}&audio_format=pcm_16000&language_code={}&commit_strategy=manual",
        percent_encode(&settings.model_id),
        percent_encode(&settings.language_code),
    );
    // Prepend host from endpoint.
    url = format!("{base}{url}");
    for term in keyterms {
        url.push_str("&keyterms=");
        url.push_str(&percent_encode(term));
    }
    url
}

// ---------------------------------------------------------------------------
// Session API (caller-friendly for vf-engine)
// ---------------------------------------------------------------------------

enum SessionCmd {
    Audio(Vec<u8>),
    Commit {
        reply: oneshot::Sender<CloudResult<String>>,
    },
}

/// One utterance of realtime STT with key rotation and audio re-send on reconnect.
pub struct SttSession {
    cmd_tx: mpsc::Sender<SessionCmd>,
    partial_tx: broadcast::Sender<String>,
    join: tokio::task::JoinHandle<()>,
}

impl SttSession {
    /// Open a session for one utterance.
    ///
    /// Awaits a successful WebSocket handshake (with key rotation) before
    /// returning. Audio is buffered for the lifetime of the utterance so
    /// mid-utterance reconnect can re-send it.
    pub async fn open(settings: SttSettings, keyterms: Vec<String>) -> CloudResult<Self> {
        let rotator = KeyRotator::new(settings.api_keys.clone())?;
        let (cmd_tx, cmd_rx) = mpsc::channel::<SessionCmd>(64);
        let (partial_tx, _) = broadcast::channel::<String>(32);
        let (ready_tx, ready_rx) = oneshot::channel::<CloudResult<()>>();

        let partial_for_task = partial_tx.clone();
        let join = tokio::spawn(async move {
            session_loop(settings, keyterms, rotator, cmd_rx, partial_for_task, ready_tx).await;
        });

        match tokio::time::timeout(CONNECT_TIMEOUT, ready_rx).await {
            Ok(Ok(Ok(()))) => Ok(Self {
                cmd_tx,
                partial_tx,
                join,
            }),
            Ok(Ok(Err(e))) => {
                let _ = join.await;
                Err(e)
            }
            Ok(Err(_)) => {
                let _ = join.await;
                Err(CloudError::SessionClosed)
            }
            Err(_) => {
                // Task may still be running; drop cmd_tx by abandoning Self — abort join.
                join.abort();
                Err(CloudError::Timeout("STT connect timed out".into()))
            }
        }
    }

    /// Subscribe to partial transcripts (optional overlay preview).
    pub fn subscribe_partials(&self) -> broadcast::Receiver<String> {
        self.partial_tx.subscribe()
    }

    /// Feed a PCM 16 kHz mono s16le chunk. Chunks are buffered and streamed.
    pub async fn feed_pcm(&self, pcm_s16le: &[u8]) -> CloudResult<()> {
        self.cmd_tx
            .send(SessionCmd::Audio(pcm_s16le.to_vec()))
            .await
            .map_err(|_| CloudError::SessionClosed)
    }

    /// Send commit on the final (possibly empty) chunk and wait for the
    /// committed transcript. Consumes the session.
    pub async fn commit(self) -> CloudResult<String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCmd::Commit { reply: reply_tx })
            .await
            .map_err(|_| CloudError::SessionClosed)?;
        let result = match tokio::time::timeout(COMMIT_TIMEOUT, reply_rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(CloudError::SessionClosed),
            Err(_) => Err(CloudError::Timeout("STT commit timed out".into())),
        };
        // Best-effort join; ignore if already finished / aborted.
        let _ = self.join.await;
        result
    }
}

async fn session_loop(
    settings: SttSettings,
    keyterms: Vec<String>,
    mut rotator: KeyRotator,
    mut cmd_rx: mpsc::Receiver<SessionCmd>,
    partial_tx: broadcast::Sender<String>,
    ready_tx: oneshot::Sender<CloudResult<()>>,
) {
    let mut audio_buffer: Vec<Vec<u8>> = Vec::new();
    let mut commit_reply: Option<oneshot::Sender<CloudResult<String>>> = None;
    let mut last_error: Option<CloudError> = None;
    let mut held_transcript: Option<String> = None;
    let mut session_started = false;
    let mut dead = false;

    // Establish initial connection (with rotation on handshake failures).
    let mut conn = match connect_with_rotation(&settings, &keyterms, &mut rotator).await {
        Ok(c) => {
            let _ = ready_tx.send(Ok(()));
            c
        }
        Err(e) => {
            // Surface the structured error to open(); keep the same error for a late Commit.
            let _ = ready_tx.send(Err(CloudError::msg(e.to_string())));
            while let Some(cmd) = cmd_rx.recv().await {
                if let SessionCmd::Commit { reply } = cmd {
                    let _ = reply.send(Err(e));
                    return;
                }
            }
            return;
        }
    };

    // Best-effort wait for session_started before first audio (buffered until then).
    let started_deadline = tokio::time::Instant::now() + SESSION_STARTED_TIMEOUT;

    loop {
        if dead {
            // Only accept Commit so the caller gets `last_error`.
            match cmd_rx.recv().await {
                Some(SessionCmd::Commit { reply }) => {
                    let err = last_error
                        .take()
                        .unwrap_or(CloudError::SessionClosed);
                    let _ = reply.send(Err(err));
                    return;
                }
                Some(SessionCmd::Audio(_)) => continue,
                None => return,
            }
        }

        tokio::select! {
            biased;

            cmd = cmd_rx.recv() => {
                match cmd {
                    None => break,
                    Some(SessionCmd::Audio(chunk)) => {
                        audio_buffer.push(chunk.clone());
                        if !session_started && tokio::time::Instant::now() < started_deadline {
                            // Keep buffering; flush once session_started (or deadline via WS path).
                            continue;
                        }
                        if let Err(err) = send_audio_or_reconnect(
                            &mut conn,
                            &chunk,
                            false,
                            &settings,
                            &keyterms,
                            &mut rotator,
                            &audio_buffer,
                        ).await {
                            last_error = Some(err);
                            dead = true;
                        }
                    }
                    Some(SessionCmd::Commit { reply }) => {
                        // Flush any buffered-only audio if we never got session_started.
                        if !session_started {
                            for chunk in &audio_buffer {
                                if let Err(err) = send_audio_or_reconnect(
                                    &mut conn,
                                    chunk,
                                    false,
                                    &settings,
                                    &keyterms,
                                    &mut rotator,
                                    &audio_buffer,
                                ).await {
                                    let _ = reply.send(Err(err));
                                    return;
                                }
                            }
                            session_started = true;
                        }

                        if let Some(t) = held_transcript.take() {
                            let _ = reply.send(Ok(t));
                            let _ = conn.ws.close(None).await;
                            return;
                        }
                        if let Some(err) = last_error.take() {
                            let _ = reply.send(Err(err));
                            return;
                        }

                        commit_reply = Some(reply);
                        if let Err(err) = send_audio_or_reconnect(
                            &mut conn,
                            &[],
                            true,
                            &settings,
                            &keyterms,
                            &mut rotator,
                            &audio_buffer,
                        ).await {
                            if let Some(r) = commit_reply.take() {
                                let _ = r.send(Err(err));
                            }
                            return;
                        }
                    }
                }
            }

            msg = conn.ws.next() => {
                match msg {
                    None => {
                        let err = CloudError::WebSocket("WebSocket closed unexpectedly".into());
                        if let Some(r) = commit_reply.take() {
                            let _ = r.send(Err(err));
                            return;
                        }
                        last_error = Some(err);
                        dead = true;
                    }
                    Some(Err(e)) => {
                        let err = CloudError::WebSocket(e.to_string());
                        if let Some(r) = commit_reply.take() {
                            let _ = r.send(Err(err));
                            return;
                        }
                        last_error = Some(err);
                        dead = true;
                    }
                    Some(Ok(Message::Text(text))) => {
                        let parsed = match ServerMessage::parse(&text) {
                            Ok(m) => m,
                            Err(_) => continue,
                        };
                        match parsed.message_type.as_str() {
                            "session_started" => {
                                log::debug!(
                                    "STT session started id={:?}",
                                    parsed.session_id
                                );
                                if !session_started {
                                    session_started = true;
                                    // Flush buffered audio that waited for session_started.
                                    for chunk in audio_buffer.clone() {
                                        if let Err(err) = send_audio_or_reconnect(
                                            &mut conn,
                                            &chunk,
                                            false,
                                            &settings,
                                            &keyterms,
                                            &mut rotator,
                                            &audio_buffer,
                                        ).await {
                                            last_error = Some(err);
                                            dead = true;
                                            break;
                                        }
                                    }
                                }
                            }
                            "partial_transcript" => {
                                if let Some(t) = parsed.text {
                                    let _ = partial_tx.send(t);
                                }
                            }
                            "committed_transcript"
                            | "committed_transcript_with_timestamps" => {
                                let transcript = parsed.text.unwrap_or_default();
                                if let Some(r) = commit_reply.take() {
                                    let _ = r.send(Ok(transcript));
                                    let _ = conn.ws.close(None).await;
                                    return;
                                }
                                // Premature commit — hold until caller commits.
                                held_transcript = Some(transcript);
                            }
                            other => {
                                if let Some(kind) = RotatableError::from_message_type(other) {
                                    let err_text = parsed
                                        .error
                                        .unwrap_or_else(|| other.to_string());
                                    match reconnect_and_resend(
                                        &settings,
                                        &keyterms,
                                        &mut rotator,
                                        &audio_buffer,
                                        kind,
                                        &err_text,
                                    ).await {
                                        Ok(mut c) => {
                                            if commit_reply.is_some() {
                                                if let Err(e) = send_audio(&mut c, &[], true).await {
                                                    if let Some(r) = commit_reply.take() {
                                                        let _ = r.send(Err(CloudError::WebSocket(e.message)));
                                                    }
                                                    return;
                                                }
                                            }
                                            conn = c;
                                            session_started = true;
                                        }
                                        Err(err) => {
                                            if let Some(r) = commit_reply.take() {
                                                let _ = r.send(Err(err));
                                                return;
                                            }
                                            last_error = Some(err);
                                            dead = true;
                                        }
                                    }
                                } else if other == "error"
                                    || other.ends_with("_error")
                                    || matches!(
                                        other,
                                        "input_error"
                                            | "chunk_size_exceeded"
                                            | "insufficient_audio_activity"
                                            | "session_time_limit_exceeded"
                                            | "queue_overflow"
                                            | "commit_throttled"
                                            | "unaccepted_terms"
                                            | "transcriber_error"
                                    )
                                {
                                    let err_text = parsed
                                        .error
                                        .unwrap_or_else(|| other.to_string());
                                    let err = CloudError::msg(err_text);
                                    if let Some(r) = commit_reply.take() {
                                        let _ = r.send(Err(err));
                                        return;
                                    }
                                    last_error = Some(err);
                                    dead = true;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = conn.ws.send(Message::Pong(p)).await;
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
    }

    if let Some(r) = commit_reply.take() {
        let err = last_error.unwrap_or(CloudError::SessionClosed);
        let _ = r.send(Err(err));
    }
}

async fn send_audio_or_reconnect(
    conn: &mut Connection,
    pcm: &[u8],
    commit: bool,
    settings: &SttSettings,
    keyterms: &[String],
    rotator: &mut KeyRotator,
    audio_buffer: &[Vec<u8>],
) -> CloudResult<()> {
    match send_audio(conn, pcm, commit).await {
        Ok(()) => Ok(()),
        Err(e) => {
            // Any send failure means the socket is dead — try one reconnect + resend.
            log::warn!("STT send failed, attempting reconnect: {}", e.message);
            match reconnect_and_resend(
                settings,
                keyterms,
                rotator,
                audio_buffer,
                RotatableError::ResourceExhausted,
                &e.message,
            )
            .await
            {
                Ok(mut c) => {
                    if commit {
                        send_audio(&mut c, &[], true)
                            .await
                            .map_err(|e2| CloudError::WebSocket(e2.message))?;
                    }
                    *conn = c;
                    Ok(())
                }
                Err(err) => Err(err),
            }
        }
    }
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Connection {
    ws: WsStream,
}

struct SendErr {
    message: String,
}

async fn send_audio(conn: &mut Connection, pcm: &[u8], commit: bool) -> Result<(), SendErr> {
    let payload = encode_audio_chunk(pcm, commit);
    conn.ws
        .send(Message::Text(payload))
        .await
        .map_err(|e| SendErr {
            message: e.to_string(),
        })
}

async fn connect_with_rotation(
    settings: &SttSettings,
    keyterms: &[String],
    rotator: &mut KeyRotator,
) -> CloudResult<Connection> {
    loop {
        let key = rotator.current_key().to_string();
        match try_connect(settings, keyterms, &key).await {
            Ok(c) => return Ok(c),
            Err(ConnectErr::Rotatable { status, message }) => {
                let detail = format!("handshake HTTP {status}: {message}");
                if rotator.fail_and_advance(detail).is_none() {
                    return Err(rotator.aggregate_error());
                }
                // try next key
            }
            Err(ConnectErr::Other(e)) => {
                // Transient network/protocol failures: advance and try remaining keys
                // so a flaky first key does not abort the whole cycle.
                if rotator.fail_and_advance(format!("connect: {e}")).is_none() {
                    return Err(rotator.aggregate_error());
                }
            }
        }
    }
}

enum ConnectErr {
    Rotatable { status: u16, message: String },
    Other(String),
}

/// Extract HTTP status codes that trigger key rotation from a tungstenite error string.
/// Uses word-boundary matching so substrings like "1401" do not false-positive as 401.
pub fn parse_rotatable_http_status(msg: &str) -> Option<u16> {
    for code in [401u16, 403, 429] {
        let needle = code.to_string();
        if let Some(idx) = msg.find(&needle) {
            let before_ok = idx == 0
                || !msg.as_bytes()[idx - 1].is_ascii_digit();
            let after = idx + needle.len();
            let after_ok = after >= msg.len() || !msg.as_bytes()[after].is_ascii_digit();
            if before_ok && after_ok {
                return Some(code);
            }
        }
    }
    None
}

async fn try_connect(
    settings: &SttSettings,
    keyterms: &[String],
    api_key: &str,
) -> Result<Connection, ConnectErr> {
    let url = build_ws_url(settings, keyterms);
    let mut request = url
        .into_client_request()
        .map_err(|e| ConnectErr::Other(e.to_string()))?;

    request.headers_mut().insert(
        "xi-api-key",
        api_key
            .parse()
            .map_err(|e: tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue| {
                ConnectErr::Other(e.to_string())
            })?,
    );

    let connect_result = tokio::time::timeout(CONNECT_TIMEOUT, connect_async(request)).await;
    match connect_result {
        Err(_) => Err(ConnectErr::Other("WebSocket connect timed out".into())),
        Ok(Ok((ws, response))) => {
            let status = response.status();
            if status == StatusCode::SWITCHING_PROTOCOLS || status.is_success() {
                Ok(Connection { ws })
            } else {
                let code = status.as_u16();
                if RotatableError::from_http_status(code).is_some() {
                    Err(ConnectErr::Rotatable {
                        status: code,
                        message: status.to_string(),
                    })
                } else {
                    Err(ConnectErr::Other(format!("handshake HTTP {code}")))
                }
            }
        }
        Ok(Err(e)) => {
            let msg = e.to_string();
            if let Some(code) = parse_rotatable_http_status(&msg) {
                return Err(ConnectErr::Rotatable {
                    status: code,
                    message: msg,
                });
            }
            Err(ConnectErr::Other(msg))
        }
    }
}

async fn reconnect_and_resend(
    settings: &SttSettings,
    keyterms: &[String],
    rotator: &mut KeyRotator,
    audio_buffer: &[Vec<u8>],
    _kind: RotatableError,
    err_text: &str,
) -> CloudResult<Connection> {
    if rotator.fail_and_advance(err_text).is_none() {
        return Err(rotator.aggregate_error());
    }
    let mut conn = connect_with_rotation(settings, keyterms, rotator).await?;
    // Resend all buffered audio (uncommitted).
    for chunk in audio_buffer {
        send_audio(&mut conn, chunk, false)
            .await
            .map_err(|e| CloudError::WebSocket(e.message))?;
    }
    Ok(conn)
}

// ---------------------------------------------------------------------------
// Tests (no network)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_rotator_rejects_empty() {
        assert!(matches!(
            KeyRotator::new(vec![]),
            Err(CloudError::NoApiKeys)
        ));
        assert!(matches!(
            KeyRotator::new(vec!["".into(), "  ".into()]),
            Err(CloudError::NoApiKeys)
        ));
    }

    #[test]
    fn key_rotator_one_full_cycle() {
        let mut r = KeyRotator::new(vec!["k1".into(), "k2".into(), "k3".into()]).unwrap();
        assert_eq!(r.current_key(), "k1");

        assert_eq!(r.fail_and_advance("e1"), Some("k2"));
        assert_eq!(r.fail_and_advance("e2"), Some("k3"));
        // Third failure exhausts the cycle (3 keys tried).
        assert_eq!(r.fail_and_advance("e3"), None);
        assert!(matches!(
            r.aggregate_error(),
            CloudError::AllKeysFailed(msg) if msg == "e3"
        ));
    }

    #[test]
    fn key_rotator_single_key_fails_once() {
        let mut r = KeyRotator::new(vec!["only".into()]).unwrap();
        assert_eq!(r.fail_and_advance("boom"), None);
        assert!(matches!(
            r.aggregate_error(),
            CloudError::AllKeysFailed(msg) if msg == "boom"
        ));
    }

    #[test]
    fn rotatable_from_message_types() {
        assert_eq!(
            RotatableError::from_message_type("auth_error"),
            Some(RotatableError::Auth)
        );
        assert_eq!(
            RotatableError::from_message_type("quota_exceeded"),
            Some(RotatableError::Quota)
        );
        assert_eq!(
            RotatableError::from_message_type("rate_limited"),
            Some(RotatableError::RateLimited)
        );
        assert_eq!(
            RotatableError::from_message_type("resource_exhausted"),
            Some(RotatableError::ResourceExhausted)
        );
        assert_eq!(RotatableError::from_message_type("input_error"), None);
        assert_eq!(RotatableError::from_message_type("partial_transcript"), None);
    }

    #[test]
    fn rotatable_from_http_status() {
        assert!(RotatableError::from_http_status(401).is_some());
        assert!(RotatableError::from_http_status(403).is_some());
        assert!(RotatableError::from_http_status(429).is_some());
        assert!(RotatableError::from_http_status(500).is_none());
        assert!(RotatableError::from_http_status(200).is_none());
    }

    #[test]
    fn encode_audio_chunk_fields() {
        let pcm = [0u8, 1, 2, 3];
        let json = encode_audio_chunk(&pcm, true);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["message_type"], "input_audio_chunk");
        assert_eq!(v["commit"], true);
        assert_eq!(v["sample_rate"], 16000);
        assert!(!v["audio_base_64"].as_str().unwrap().is_empty());
    }

    #[test]
    fn parse_server_messages() {
        let partial = ServerMessage::parse(
            r#"{"message_type":"partial_transcript","text":"hel"}"#,
        )
        .unwrap();
        assert_eq!(partial.message_type, "partial_transcript");
        assert_eq!(partial.text.as_deref(), Some("hel"));

        let committed = ServerMessage::parse(
            r#"{"message_type":"committed_transcript","text":"hello"}"#,
        )
        .unwrap();
        assert_eq!(committed.text.as_deref(), Some("hello"));

        let err = ServerMessage::parse(
            r#"{"message_type":"auth_error","error":"bad key"}"#,
        )
        .unwrap();
        assert_eq!(err.message_type, "auth_error");
        assert_eq!(err.error.as_deref(), Some("bad key"));
    }

    #[test]
    fn build_ws_url_shape() {
        let settings = SttSettings {
            api_keys: vec![],
            endpoint: "wss://api.elevenlabs.io".into(),
            model_id: "scribe_v2_realtime".into(),
            language_code: "en".into(),
        };
        let url = build_ws_url(&settings, &["VillFlow".into(), "term with space".into()]);
        assert!(url.starts_with("wss://api.elevenlabs.io/v1/speech-to-text/realtime?"));
        assert!(url.contains("model_id=scribe_v2_realtime"));
        assert!(url.contains("audio_format=pcm_16000"));
        assert!(url.contains("language_code=en"));
        assert!(url.contains("commit_strategy=manual"));
        assert!(url.contains("keyterms=VillFlow"));
        assert!(url.contains("keyterms=term%20with%20space"));
    }

    /// Mock transport simulation of the rotation decision loop.
    #[test]
    fn mock_transport_rotation_exhausts_keys() {
        // Simulate: each key fails with auth_error until cycle complete.
        let mut rotator = KeyRotator::new(vec!["a".into(), "b".into()]).unwrap();
        let errors = ["auth_error", "quota_exceeded"];
        let mut attempts = 0;
        for err in errors {
            attempts += 1;
            assert!(RotatableError::from_message_type(err).is_some());
            if rotator.fail_and_advance(err).is_none() {
                break;
            }
        }
        // Two keys → two failures exhaust.
        assert_eq!(attempts, 2);
        assert!(matches!(
            rotator.aggregate_error(),
            CloudError::AllKeysFailed(_)
        ));
    }

    #[test]
    fn mock_transport_success_on_second_key() {
        let mut rotator = KeyRotator::new(vec!["bad".into(), "good".into()]).unwrap();
        // First key fails
        let next = rotator.fail_and_advance("auth_error");
        assert_eq!(next, Some("good"));
        // Second key "succeeds" — no further advance; transcript would be returned.
        assert_eq!(rotator.current_key(), "good");
        assert_eq!(rotator.tried, 1);
    }

    #[test]
    fn parse_rotatable_status_word_boundary() {
        assert_eq!(parse_rotatable_http_status("HTTP 401 Unauthorized"), Some(401));
        assert_eq!(parse_rotatable_http_status("status: 403"), Some(403));
        assert_eq!(parse_rotatable_http_status("rate 429"), Some(429));
        // Must not false-positive on embedded digits.
        assert_eq!(parse_rotatable_http_status("request id 1401"), None);
        assert_eq!(parse_rotatable_http_status("code 4030"), None);
        assert_eq!(parse_rotatable_http_status("all good"), None);
    }

    #[test]
    fn encode_empty_commit_chunk() {
        let json = encode_audio_chunk(&[], true);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["commit"], true);
        assert_eq!(v["audio_base_64"], "");
    }
}
