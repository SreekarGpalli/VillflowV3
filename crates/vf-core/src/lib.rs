use serde::{Deserialize, Serialize};

// --- DEFAULT PROMPT CONSTANTS (§9) ---

pub const PROMPT_LIGHT: &str = "\
You clean up raw speech-to-text dictation from an Indian English speaker. Output ONLY the cleaned text — no preamble, no quotes, no explanation. Remove filler words (uh, um, like, you know, actually when meaningless). Fix punctuation and capitalization. Do not change, add, or reorder any other words. Prefer these spellings when the words occur: {dictionary}. The text will be inserted into {app_name}. Existing text before the cursor is shown for continuity — continue from it naturally and never repeat it: {field_context}\
";

pub const PROMPT_MEDIUM: &str = "\
You clean up raw speech-to-text dictation from an Indian English speaker. Output ONLY the cleaned text — no preamble, no quotes, no explanation. Remove filler words (uh, um, like, you know). Fix grammar, punctuation, and capitalization. Split run-on sentences. If the speaker dictates a list, format it as a bulleted or numbered list. Keep the speaker's meaning and vocabulary — do not add content or embellish. Prefer these spellings when the words occur: {dictionary}. The text will be inserted into {app_name} — match a tone appropriate to that app. Existing text before the cursor is shown for continuity — continue from it naturally and never repeat it: {field_context}\
";

pub const PROMPT_HIGH: &str = "\
You clean up raw speech-to-text dictation from an Indian English speaker. Output ONLY the cleaned text — no preamble, no quotes, no explanation. Remove filler words. Fix grammar, punctuation, and capitalization. Split run-on sentences. Format spoken lists as bulleted or numbered lists. Tighten the wording for clarity and concision, but preserve the speaker's meaning and intent exactly — never add new information. Prefer these spellings when the words occur: {dictionary}. The text will be inserted into {app_name} — match a tone appropriate to that app. Existing text before the cursor is shown for continuity — continue from it naturally and never repeat it: {field_context}\
";

pub const PROMPT_COMMAND: &str = "\
You apply a spoken editing instruction to a piece of text. Output ONLY the transformed text — no preamble, no quotes, no explanation. Preserve the original formatting style (line breaks, lists) unless the instruction says otherwise. The text lives in {app_name}. Apply the INSTRUCTION to the TEXT that follows.\
";

// --- ENUMS & TYPES (§12) ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupLevel {
    None,
    Light,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectionMethod {
    ClipboardPaste,
    #[serde(rename = "sendinput_typing")]
    SendInputTyping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineState {
    Idle,
    Recording,
    Processing,
    Injecting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EngineEvent {
    State(EngineState),
    Error(String),
    Injected { words: u32, total_ms: u64 },
    ToggleScratchpad,
    DictionaryLearned(String),
}

#[derive(Debug, Clone)]
pub enum EngineCmd {
    ApplySettings(Box<Settings>),
    Shutdown,
}

// --- SETTINGS DEFAULT HELPERS ---

fn default_version() -> u32 { 1 }
fn default_true() -> bool { true }
fn default_false() -> bool { false }
fn default_dictation_hotkey() -> String { "Ctrl+Shift+Z".to_string() }
fn default_command_hotkey() -> String { "Ctrl+Shift+X".to_string() }
fn default_scratchpad_hotkey() -> String { "Ctrl+Shift+C".to_string() }
fn default_system_default() -> String { "system_default".to_string() }
fn default_stt_endpoint() -> String { "wss://api.elevenlabs.io".to_string() }
fn default_stt_model() -> String { "scribe_v2_realtime".to_string() }
fn default_stt_language() -> String { "en".to_string() }
fn default_llm_model() -> String { "openai/gpt-oss-120b".to_string() }
fn default_cleanup_level() -> CleanupLevel { CleanupLevel::Medium }
fn default_prompt_light() -> String { PROMPT_LIGHT.to_string() }
fn default_prompt_medium() -> String { PROMPT_MEDIUM.to_string() }
fn default_prompt_high() -> String { PROMPT_HIGH.to_string() }
fn default_prompt_command() -> String { PROMPT_COMMAND.to_string() }
fn default_injection_method() -> InjectionMethod { InjectionMethod::ClipboardPaste }

// --- SETTINGS SUB-STRUCTS ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneralSettings {
    #[serde(default = "default_false")]
    pub launch_at_startup: bool,
    #[serde(default = "default_false")]
    pub start_minimized: bool,
    #[serde(default = "default_true")]
    pub show_error_notifications: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            launch_at_startup: false,
            start_minimized: false,
            show_error_notifications: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HotkeysSettings {
    #[serde(default = "default_dictation_hotkey")]
    pub dictation: String,
    #[serde(default = "default_command_hotkey")]
    pub command_mode: String,
    #[serde(default = "default_scratchpad_hotkey")]
    pub scratchpad: String,
}

impl Default for HotkeysSettings {
    fn default() -> Self {
        Self {
            dictation: default_dictation_hotkey(),
            command_mode: default_command_hotkey(),
            scratchpad: default_scratchpad_hotkey(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioSettings {
    #[serde(default = "default_system_default")]
    pub input_device: String,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            input_device: default_system_default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SttSettings {
    #[serde(default)]
    pub api_keys: Vec<String>,
    #[serde(default = "default_stt_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_stt_model")]
    pub model_id: String,
    #[serde(default = "default_stt_language")]
    pub language_code: String,
}

impl Default for SttSettings {
    fn default() -> Self {
        Self {
            api_keys: Vec::new(),
            endpoint: default_stt_endpoint(),
            model_id: default_stt_model(),
            language_code: default_stt_language(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmSettings {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_llm_model")]
    pub model: String,
    #[serde(default = "default_cleanup_level")]
    pub cleanup_level: CleanupLevel,
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: default_llm_model(),
            cleanup_level: default_cleanup_level(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptsSettings {
    #[serde(default = "default_prompt_light")]
    pub light: String,
    #[serde(default = "default_prompt_medium")]
    pub medium: String,
    #[serde(default = "default_prompt_high")]
    pub high: String,
    #[serde(default = "default_prompt_command")]
    pub command: String,
}

impl Default for PromptsSettings {
    fn default() -> Self {
        Self {
            light: default_prompt_light(),
            medium: default_prompt_medium(),
            high: default_prompt_high(),
            command: default_prompt_command(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputSettings {
    #[serde(default = "default_injection_method")]
    pub injection_method: InjectionMethod,
    #[serde(default = "default_true")]
    pub restore_clipboard: bool,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            injection_method: default_injection_method(),
            restore_clipboard: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DictionarySettings {
    #[serde(default = "default_true")]
    pub auto_learn: bool,
}

impl Default for DictionarySettings {
    fn default() -> Self {
        Self {
            auto_learn: true,
        }
    }
}

// --- SETTINGS ROOT STRUCT ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Settings {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub general: GeneralSettings,
    #[serde(default)]
    pub hotkeys: HotkeysSettings,
    #[serde(default)]
    pub audio: AudioSettings,
    #[serde(default)]
    pub stt: SttSettings,
    #[serde(default)]
    pub llm: LlmSettings,
    #[serde(default)]
    pub prompts: PromptsSettings,
    #[serde(default)]
    pub output: OutputSettings,
    #[serde(default)]
    pub dictionary: DictionarySettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: default_version(),
            general: GeneralSettings::default(),
            hotkeys: HotkeysSettings::default(),
            audio: AudioSettings::default(),
            stt: SttSettings::default(),
            llm: LlmSettings::default(),
            prompts: PromptsSettings::default(),
            output: OutputSettings::default(),
            dictionary: DictionarySettings::default(),
        }
    }
}

pub fn default_settings() -> Settings {
    Settings::default()
}

// --- DATABASE STRUCTS (§12) ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DictEntry {
    pub id: i64,
    pub word: String,
    pub starred: bool,
    pub source: String,
    pub use_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub ts: String,
    pub app_name: String,
    pub window_title: String,
    pub mode: String,
    pub raw_transcript: String,
    pub final_text: String,
    pub duration_ms: i64,
    pub word_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InsightsSummary {
    pub total_words: i64,
    pub avg_wpm: f64,
    pub top_apps: Vec<(String, i64)>,
    pub daily_words: Vec<(String, i64)>,
}

// --- STORE TRAIT (§12) ---

pub trait Store: Send + Sync {
    // Dictionary CRUD + star toggle + bump_use_count(words: &[String])
    fn dictionary_list(&self) -> anyhow::Result<Vec<DictEntry>>;
    fn dictionary_add(&self, word: &str, source: &str) -> anyhow::Result<DictEntry>;
    fn dictionary_delete(&self, id: i64) -> anyhow::Result<()>;
    fn dictionary_update(&self, id: i64, word: &str) -> anyhow::Result<()>;
    fn dictionary_toggle_star(&self, id: i64) -> anyhow::Result<()>;
    fn dictionary_bump_use_count(&self, words: &[String]) -> anyhow::Result<()>;

    // History
    fn history_append(&self, entry: &HistoryEntry) -> anyhow::Result<()>;
    fn history_list(&self, limit: u32, offset: u32) -> anyhow::Result<Vec<HistoryEntry>>;

    // Scratchpad
    fn scratchpad_get(&self) -> anyhow::Result<String>;
    fn scratchpad_set(&self, content: &str) -> anyhow::Result<()>;

    // Insights
    fn insights_summary(&self) -> anyhow::Result<InsightsSummary>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injection_method_wire_names() {
        let paste = serde_json::to_string(&InjectionMethod::ClipboardPaste).unwrap();
        assert_eq!(paste, "\"clipboard_paste\"");
        let typing = serde_json::to_string(&InjectionMethod::SendInputTyping).unwrap();
        assert_eq!(typing, "\"sendinput_typing\"");
        let back: InjectionMethod = serde_json::from_str("\"sendinput_typing\"").unwrap();
        assert_eq!(back, InjectionMethod::SendInputTyping);
    }

    #[test]
    fn cleanup_level_wire_names() {
        for (level, name) in [
            (CleanupLevel::None, "none"),
            (CleanupLevel::Light, "light"),
            (CleanupLevel::Medium, "medium"),
            (CleanupLevel::High, "high"),
        ] {
            let s = serde_json::to_string(&level).unwrap();
            assert_eq!(s, format!("\"{name}\""));
            let back: CleanupLevel = serde_json::from_str(&s).unwrap();
            assert_eq!(back, level);
        }
    }

    #[test]
    fn default_settings_match_contract() {
        let s = default_settings();
        assert_eq!(s.version, 1);
        assert_eq!(s.hotkeys.dictation, "Ctrl+Shift+Z");
        assert_eq!(s.audio.input_device, "system_default");
        assert_eq!(s.llm.model, "openai/gpt-oss-120b");
        assert_eq!(s.llm.cleanup_level, CleanupLevel::Medium);
        assert_eq!(s.prompts.light, PROMPT_LIGHT);
        assert_eq!(s.prompts.medium, PROMPT_MEDIUM);
        assert_eq!(s.prompts.high, PROMPT_HIGH);
        assert_eq!(s.prompts.command, PROMPT_COMMAND);
    }
}
