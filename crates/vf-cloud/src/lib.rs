//! VillFlow cloud clients: ElevenLabs realtime STT, Groq LLM, prompt builder.
//!
//! Owned by GrokBuild (CONTRACTS §4). Wire schemas: §6–§9.

mod error;
mod groq;
mod keyterms;
mod prompt;
mod stt;

pub use error::{CloudError, CloudResult};
pub use groq::{
    chat_completion, clean_completion_text, list_models, parse_chat_completion_content,
    parse_model_ids,
};
pub use keyterms::build_keyterms;
pub use prompt::{
    build_command, build_dictation, format_dictionary, resolve_placeholder, ChatMessages,
    PromptContext,
};
pub use stt::{
    build_ws_url, encode_audio_chunk, parse_rotatable_http_status, KeyRotator, RotatableError,
    ServerMessage, SttSession,
};
