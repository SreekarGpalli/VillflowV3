use thiserror::Error;

/// Errors produced by vf-cloud (STT, Groq, prompt building).
#[derive(Debug, Error)]
pub enum CloudError {
    #[error("{0}")]
    Message(String),

    #[error("no ElevenLabs API keys configured")]
    NoApiKeys,

    #[error("All ElevenLabs keys failed: {0}")]
    AllKeysFailed(String),

    #[error("STT session closed before commit")]
    SessionClosed,

    #[error("STT WebSocket error: {0}")]
    WebSocket(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Groq API error: {0}")]
    Groq(String),

    #[error("empty response from Groq")]
    EmptyGroqResponse,
}

impl CloudError {
    pub fn msg(s: impl Into<String>) -> Self {
        CloudError::Message(s.into())
    }
}

pub type CloudResult<T> = Result<T, CloudError>;
