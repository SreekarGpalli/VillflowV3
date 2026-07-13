//! Prompt builder — CONTRACTS §8–§9.
//!
//! Resolves placeholders and produces exact (system, user) message pairs.

use vf_core::{CleanupLevel, PromptsSettings};

/// Context used when resolving dictation / command prompts.
#[derive(Debug, Clone, Default)]
pub struct PromptContext {
    /// Process / app name (e.g. exe name). Empty → `(none)`.
    pub app_name: String,
    /// Nearby field text for continuity. Empty → `(none)`.
    pub field_context: String,
    /// Dictionary words, starred first (caller orders them). Joined with ", ".
    pub dictionary: Vec<String>,
}

/// Resolved chat messages for a Groq call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessages {
    pub system: String,
    pub user: String,
}

/// Substitute a placeholder value: empty → `(none)`.
pub fn resolve_placeholder(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "(none)".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Format dictionary words as a comma-separated list; empty → `(none)`.
pub fn format_dictionary(words: &[String]) -> String {
    let joined: Vec<&str> = words
        .iter()
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .collect();
    if joined.is_empty() {
        "(none)".to_string()
    } else {
        joined.join(", ")
    }
}

fn apply_dictation_placeholders(template: &str, ctx: &PromptContext) -> String {
    template
        .replace("{dictionary}", &format_dictionary(&ctx.dictionary))
        .replace("{app_name}", &resolve_placeholder(&ctx.app_name))
        .replace("{field_context}", &resolve_placeholder(&ctx.field_context))
}

fn apply_command_system_placeholders(template: &str, ctx: &PromptContext) -> String {
    template.replace("{app_name}", &resolve_placeholder(&ctx.app_name))
}

/// Build dictation (system, user) for a cleanup level.
///
/// Returns `None` when `level == CleanupLevel::None` (caller skips the LLM).
/// `user` is always the raw transcript, nothing else.
pub fn build_dictation(
    level: CleanupLevel,
    prompts: &PromptsSettings,
    ctx: &PromptContext,
    raw_transcript: &str,
) -> Option<ChatMessages> {
    let template = match level {
        CleanupLevel::None => return None,
        CleanupLevel::Light => &prompts.light,
        CleanupLevel::Medium => &prompts.medium,
        CleanupLevel::High => &prompts.high,
    };
    Some(ChatMessages {
        system: apply_dictation_placeholders(template, ctx),
        user: raw_transcript.to_string(),
    })
}

/// Build command-mode (system, user) messages.
///
/// System = resolved PROMPT_COMMAND (`{app_name}` only).
/// User = `INSTRUCTION:\n{instruction}\n\nTEXT:\n{selection}` with empty → `(none)`.
pub fn build_command(
    prompts: &PromptsSettings,
    ctx: &PromptContext,
    instruction: &str,
    selection: &str,
) -> ChatMessages {
    let system = apply_command_system_placeholders(&prompts.command, ctx);
    let user = format!(
        "INSTRUCTION:\n{}\n\nTEXT:\n{}",
        resolve_placeholder(instruction),
        resolve_placeholder(selection)
    );
    ChatMessages { system, user }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vf_core::{CleanupLevel, PromptsSettings, PROMPT_COMMAND, PROMPT_LIGHT};

    fn default_prompts() -> PromptsSettings {
        PromptsSettings::default()
    }

    #[test]
    fn empty_placeholder_becomes_none() {
        assert_eq!(resolve_placeholder(""), "(none)");
        assert_eq!(resolve_placeholder("   "), "(none)");
        assert_eq!(resolve_placeholder("hello"), "hello");
    }

    #[test]
    fn format_dictionary_empty_and_nonempty() {
        assert_eq!(format_dictionary(&[]), "(none)");
        assert_eq!(
            format_dictionary(&["alpha".into(), "beta".into()]),
            "alpha, beta"
        );
        assert_eq!(
            format_dictionary(&["  ".into(), "keep".into(), "".into()]),
            "keep"
        );
    }

    #[test]
    fn dictation_none_skips_llm() {
        let ctx = PromptContext::default();
        let msgs = build_dictation(
            CleanupLevel::None,
            &default_prompts(),
            &ctx,
            "raw transcript",
        );
        assert!(msgs.is_none());
    }

    #[test]
    fn dictation_light_resolves_placeholders_and_user_is_transcript() {
        let prompts = default_prompts();
        let ctx = PromptContext {
            app_name: "notepad.exe".into(),
            field_context: "Hello world".into(),
            dictionary: vec!["VillFlow".into(), "Groq".into()],
        };
        let msgs = build_dictation(CleanupLevel::Light, &prompts, &ctx, "um hello there")
            .expect("light should produce messages");

        assert_eq!(msgs.user, "um hello there");
        assert!(msgs.system.contains("VillFlow, Groq"));
        assert!(msgs.system.contains("notepad.exe"));
        assert!(msgs.system.contains("Hello world"));
        assert!(!msgs.system.contains("{dictionary}"));
        assert!(!msgs.system.contains("{app_name}"));
        assert!(!msgs.system.contains("{field_context}"));
    }

    #[test]
    fn dictation_empty_context_uses_none() {
        let prompts = PromptsSettings {
            light: PROMPT_LIGHT.to_string(),
            ..default_prompts()
        };
        let ctx = PromptContext {
            app_name: String::new(),
            field_context: String::new(),
            dictionary: vec![],
        };
        let msgs = build_dictation(CleanupLevel::Light, &prompts, &ctx, "hi").unwrap();
        // All three placeholders should be (none)
        assert!(msgs.system.contains("(none)"));
        assert!(!msgs.system.contains("{dictionary}"));
        assert!(!msgs.system.contains("{app_name}"));
        assert!(!msgs.system.contains("{field_context}"));
    }

    #[test]
    fn command_message_shape() {
        let prompts = PromptsSettings {
            command: PROMPT_COMMAND.to_string(),
            ..default_prompts()
        };
        let ctx = PromptContext {
            app_name: "Code.exe".into(),
            field_context: String::new(),
            dictionary: vec![],
        };
        let msgs = build_command(&prompts, &ctx, "make formal", "hey there");
        assert!(msgs.system.contains("Code.exe"));
        assert!(!msgs.system.contains("{app_name}"));
        assert_eq!(
            msgs.user,
            "INSTRUCTION:\nmake formal\n\nTEXT:\nhey there"
        );
    }

    #[test]
    fn command_empty_instruction_and_selection() {
        let prompts = default_prompts();
        let ctx = PromptContext {
            app_name: String::new(),
            ..PromptContext::default()
        };
        let msgs = build_command(&prompts, &ctx, "", "");
        assert!(msgs.system.contains("(none)")); // app_name
        assert_eq!(
            msgs.user,
            "INSTRUCTION:\n(none)\n\nTEXT:\n(none)"
        );
    }

    #[test]
    fn medium_and_high_use_respective_templates() {
        let mut prompts = default_prompts();
        prompts.medium = "MED {app_name}".into();
        prompts.high = "HIGH {app_name}".into();
        let ctx = PromptContext {
            app_name: "x".into(),
            ..PromptContext::default()
        };
        let med = build_dictation(CleanupLevel::Medium, &prompts, &ctx, "t").unwrap();
        let high = build_dictation(CleanupLevel::High, &prompts, &ctx, "t").unwrap();
        assert_eq!(med.system, "MED x");
        assert_eq!(high.system, "HIGH x");
    }
}
