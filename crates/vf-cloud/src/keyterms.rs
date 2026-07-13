//! Keyterm selection for ElevenLabs STT — CONTRACTS §6.
//!
//! Up to 50 terms, each ≤ 20 chars; starred words first, then highest `use_count`.

use vf_core::DictEntry;

const MAX_KEYTERMS: usize = 50;
const MAX_KEYTERM_CHARS: usize = 20;

/// Build the `keyterms` list from dictionary entries per §6.
pub fn build_keyterms(entries: &[DictEntry]) -> Vec<String> {
    let mut sorted: Vec<&DictEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        // Starred first (true > false), then highest use_count.
        b.starred
            .cmp(&a.starred)
            .then_with(|| b.use_count.cmp(&a.use_count))
            .then_with(|| a.word.cmp(&b.word))
    });

    let mut out = Vec::with_capacity(MAX_KEYTERMS.min(sorted.len()));
    for e in sorted {
        let word = e.word.trim();
        if word.is_empty() {
            continue;
        }
        let truncated: String = word.chars().take(MAX_KEYTERM_CHARS).collect();
        if truncated.is_empty() {
            continue;
        }
        // Dedup while preserving order.
        if out.iter().any(|w: &String| w.eq_ignore_ascii_case(&truncated)) {
            continue;
        }
        out.push(truncated);
        if out.len() >= MAX_KEYTERMS {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(word: &str, starred: bool, use_count: i64) -> DictEntry {
        DictEntry {
            id: 0,
            word: word.to_string(),
            starred,
            source: "manual".into(),
            use_count,
        }
    }

    #[test]
    fn starred_first_then_use_count() {
        let entries = vec![
            entry("low", false, 1),
            entry("high", false, 99),
            entry("star", true, 0),
            entry("star2", true, 5),
        ];
        let terms = build_keyterms(&entries);
        assert_eq!(terms, vec!["star2", "star", "high", "low"]);
    }

    #[test]
    fn truncates_length_and_count() {
        let long = "a".repeat(30);
        let mut entries: Vec<DictEntry> = (0..60)
            .map(|i| entry(&format!("w{i:02}"), false, i))
            .collect();
        entries.push(entry(&long, true, 0));
        let terms = build_keyterms(&entries);
        assert_eq!(terms.len(), 50);
        assert_eq!(terms[0].chars().count(), 20);
        assert!(terms[0].chars().all(|c| c == 'a'));
    }
}
