//! Groq LLM client — CONTRACTS §7.

use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;

use crate::error::{CloudError, CloudResult};
use vf_core::normalize_max_completion_tokens;

const GROQ_CHAT_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const GROQ_MODELS_URL: &str = "https://api.groq.com/openai/v1/models";
const TEMPERATURE: f64 = 0.2;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// Strip wrapping quotes / markdown code fences from model output.
///
/// Applied after trimming. Handles:
/// - surrounding double or single quotes
/// - a single outer ``` / ```lang fenced block (leading *and* trailing fences only)
///
/// Internal code fences are preserved — we never search with `rfind`, which would
/// truncate multi-block model output.
pub fn clean_completion_text(raw: &str) -> String {
    let mut s = raw.trim().to_string();

    // Only strip when the *entire* response is wrapped in one fence pair.
    if s.starts_with("```") && s.ends_with("```") && s.len() >= 6 {
        // Drop trailing fence.
        let without_close = s[..s.len() - 3].trim_end();
        // Drop opening fence (+ optional language tag on the same / first line).
        if let Some(rest) = without_close.strip_prefix("```") {
            s = match rest.find('\n') {
                // ```lang\n...\n```  or  ```\n...\n```
                Some(nl) => rest[nl + 1..].trim().to_string(),
                // ```content``` on one line
                None => rest.trim().to_string(),
            };
        }
    }

    // Strip a single pair of wrapping quotes if the whole string is quoted.
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            s = s[1..s.len() - 1].to_string();
        }
    }

    s.trim().to_string()
}

/// Parse `choices[0].message.content` from a Groq/OpenAI chat completions JSON body.
pub fn parse_chat_completion_content(body: &serde_json::Value) -> CloudResult<String> {
    let content = body
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CloudError::Groq("missing choices[0].message.content".into()))?;

    let cleaned = clean_completion_text(content);
    if cleaned.is_empty() {
        return Err(CloudError::EmptyGroqResponse);
    }
    Ok(cleaned)
}

/// Parse model id list from `GET /openai/v1/models`.
pub fn parse_model_ids(body: &serde_json::Value) -> CloudResult<Vec<String>> {
    let data = body
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| CloudError::Groq("missing data array in models response".into()))?;

    let mut ids: Vec<String> = data
        .iter()
        .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(|s| s.to_string()))
        .collect();
    ids.sort();
    Ok(ids)
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

/// True for Groq GPT-OSS reasoning models where `max_completion_tokens` is shared
/// with internal reasoning and default medium effort can starve the visible answer.
fn is_gpt_oss_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.contains("gpt-oss")
}

/// Non-streaming chat completion. Returns cleaned `choices[0].message.content`.
///
/// `max_completion_tokens` should be a preset from
/// [`vf_core::MAX_COMPLETION_TOKEN_PRESETS`] (engine passes settings value).
pub async fn chat_completion(
    system: &str,
    user: &str,
    model: &str,
    api_key: &str,
    max_completion_tokens: u32,
) -> CloudResult<String> {
    if api_key.trim().is_empty() {
        return Err(CloudError::Groq("LLM API key is empty".into()));
    }

    let max_tokens = normalize_max_completion_tokens(max_completion_tokens);

    // Dictation cleanup is a simple rewrite — keep reasoning low so token budget
    // goes to the cleaned text (otherwise long transcripts get mid-sentence cuts).
    let mut body = serde_json::json!({
        "model": model,
        "temperature": TEMPERATURE,
        "max_completion_tokens": max_tokens,
        "stream": false,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    });
    if is_gpt_oss_model(model) {
        body.as_object_mut().unwrap().insert(
            "reasoning_effort".into(),
            serde_json::Value::String("low".into()),
        );
        // Don't ship reasoning payload back; content-only is enough for cleanup.
        body.as_object_mut()
            .unwrap()
            .insert("include_reasoning".into(), serde_json::Value::Bool(false));
    }

    let resp = http_client()
        .post(GROQ_CHAT_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| CloudError::Http(e.to_string()))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| CloudError::Http(e.to_string()))?;

    if !status.is_success() {
        // Never include the API key; status + body snippet only.
        let snippet: String = text.chars().take(300).collect();
        return Err(CloudError::Groq(format!(
            "HTTP {status}: {snippet}"
        )));
    }

    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| CloudError::Groq(e.to_string()))?;

    let finish_reason = value
        .pointer("/choices/0/finish_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let cleaned = parse_chat_completion_content(&value)?;
    if finish_reason == "length" {
        log::warn!(
            "Groq finish_reason=length (output may be truncated; chars={})",
            cleaned.len()
        );
        return Err(CloudError::Groq(format!(
            "completion truncated (finish_reason=length, chars={})",
            cleaned.len()
        )));
    }
    Ok(cleaned)
}

/// Live model list from Groq (`GET /openai/v1/models` → ids).
pub async fn list_models(api_key: &str) -> CloudResult<Vec<String>> {
    if api_key.trim().is_empty() {
        return Err(CloudError::Groq("LLM API key is empty".into()));
    }

    let resp = http_client()
        .get(GROQ_MODELS_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .map_err(|e| CloudError::Http(e.to_string()))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| CloudError::Http(e.to_string()))?;

    if !status.is_success() {
        let snippet: String = text.chars().take(300).collect();
        return Err(CloudError::Groq(format!(
            "HTTP {status}: {snippet}"
        )));
    }

    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| CloudError::Groq(e.to_string()))?;
    parse_model_ids(&value)
}

/// Convenience: same as [`chat_completion`] but typed around the internal response shape.
#[allow(dead_code)]
fn parse_typed_content(json: &str) -> CloudResult<String> {
    let parsed: ChatCompletionResponse =
        serde_json::from_str(json).map_err(|e| CloudError::Groq(e.to_string()))?;
    let content = parsed
        .choices
        .first()
        .and_then(|c| c.message.content.as_deref())
        .ok_or_else(|| CloudError::Groq("missing choices[0].message.content".into()))?;
    let cleaned = clean_completion_text(content);
    if cleaned.is_empty() {
        return Err(CloudError::EmptyGroqResponse);
    }
    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clean_strips_wrapping_double_quotes() {
        assert_eq!(clean_completion_text("\"hello world\""), "hello world");
    }

    #[test]
    fn clean_strips_wrapping_single_quotes() {
        assert_eq!(clean_completion_text("'hello'"), "hello");
    }

    #[test]
    fn clean_strips_code_fence() {
        assert_eq!(
            clean_completion_text("```\nhello world\n```"),
            "hello world"
        );
        assert_eq!(
            clean_completion_text("```text\nhello world\n```"),
            "hello world"
        );
        assert_eq!(clean_completion_text("```hello```"), "hello");
    }

    #[test]
    fn clean_preserves_internal_fences() {
        // Multi-block output must not be truncated at an inner ```.
        let multi = "Intro:\n\n```python\nprint(1)\n```\n\nOutro.";
        assert_eq!(clean_completion_text(multi), multi);

        // Leading fence without a matching trailing fence → leave untouched.
        let unclosed = "```\npartial only";
        assert_eq!(clean_completion_text(unclosed), unclosed);

        // Outer wrap with an inner fence still yields the full inner body.
        let wrapped = "```\nSee:\n```js\nx()\n```\ndone\n```";
        assert_eq!(clean_completion_text(wrapped), "See:\n```js\nx()\n```\ndone");
    }

    #[test]
    fn clean_preserves_internal_quotes() {
        assert_eq!(
            clean_completion_text("He said \"hi\" to me"),
            "He said \"hi\" to me"
        );
    }

    #[test]
    fn parse_chat_completion_happy_path() {
        let body = json!({
            "choices": [{
                "message": { "role": "assistant", "content": "  Cleaned text.  " }
            }]
        });
        assert_eq!(
            parse_chat_completion_content(&body).unwrap(),
            "Cleaned text."
        );
    }

    #[test]
    fn parse_chat_completion_with_fences() {
        let body = json!({
            "choices": [{
                "message": { "content": "```\nFinal answer\n```" }
            }]
        });
        assert_eq!(
            parse_chat_completion_content(&body).unwrap(),
            "Final answer"
        );
    }

    #[test]
    fn parse_chat_completion_missing_content() {
        let body = json!({ "choices": [] });
        assert!(parse_chat_completion_content(&body).is_err());
    }

    #[test]
    fn parse_models_list() {
        let body = json!({
            "data": [
                { "id": "openai/gpt-oss-120b" },
                { "id": "llama-3.3-70b-versatile" },
                { "object": "model" }
            ]
        });
        let ids = parse_model_ids(&body).unwrap();
        assert_eq!(
            ids,
            vec![
                "llama-3.3-70b-versatile".to_string(),
                "openai/gpt-oss-120b".to_string()
            ]
        );
    }

    #[test]
    fn parse_typed_content_works() {
        let json = r#"{"choices":[{"message":{"content":"\"quoted\""}}]}"#;
        assert_eq!(parse_typed_content(json).unwrap(), "quoted");
    }
}
