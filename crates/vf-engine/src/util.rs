//! Small helpers (timestamps without chrono — not on the crate whitelist).

use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::SystemInformation::GetLocalTime;

/// Local ISO-8601-ish timestamp (`YYYY-MM-DDTHH:MM:SS`) for history rows.
pub fn local_iso8601() -> String {
    let st: SYSTEMTIME = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    )
}

/// Count whitespace-separated words in `text`.
pub fn word_count(text: &str) -> u32 {
    text.split_whitespace().filter(|w| !w.is_empty()).count() as u32
}

/// Last `max` characters of `text` (char-boundary safe).
pub fn tail_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    text.chars().skip(count - max).collect()
}
