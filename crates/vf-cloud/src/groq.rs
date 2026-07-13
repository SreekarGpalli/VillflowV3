//! Groq LLM client — CONTRACTS §7.

use serde::Deserialize;

use crate::error::{CloudError, CloudResult};

const GROQ_CHAT_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const GROQ_MODELS_URL: &str = "https://api.groq.com/openai/v1/models";
const TEMPERATURE: f64 = 0.2;
const MAX_COMPLETION_TOKENS: u32 = 2048;

/// Strip wrapping quotes / markdown code fences from model output.
///
/// Applied after trimming. Handles:
/// - surrounding double or single quotes
/// - ``` / ```lang fenced blocks
pub fn clean_completion_text(raw: &str) -> String {
    let mut s = raw.trim().to_string();

    // Strip markdown code fences (```lang\n...\n``` or ```\n...\n```)
    if s.starts_with("```") {
        if let Some(rest) = s.strip_prefix("```") {
            let rest = rest.trim_start_matches(|c: char| c != '\n').trim_start_matches('\n');
            if let Some(end) = rest.rfind("```") {
                s = rest[..end].trim().to_string();
            } else {
                s = rest.trim().to_string();
            }
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

/// Non-streaming chat completion. Returns cleaned `choices[0].message.content`.
pub async fn chat_completion(
    system: &str,
    user: &str,
    model: &str,
    api_key: &str,
) -> CloudResult<String> {
    if api_key.trim().is_empty() {
        return Err(CloudError::Groq("LLM API key is empty".into()));
    }

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": model,
        "temperature": TEMPERATURE,
        "max_completion_tokens": MAX_COMPLETION_TOKENS,
        "stream": false,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    });

    let resp = client
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
    parse_chat_completion_content(&value)
}

/// Live model list from Groq (`GET /openai/v1/models` → ids).
pub async fn list_models(api_key: &str) -> CloudResult<Vec<String>> {
    if api_key.trim().is_empty() {
        return Err(CloudError::Groq("LLM API key is empty".into()));
    }

    let client = reqwest::Client::new();
    let resp = client
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
