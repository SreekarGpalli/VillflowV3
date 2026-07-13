//! Dictionary auto-learn — CONTRACTS §15.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use vf_core::Store;

use crate::context;

/// Common English stopwords to ignore for auto-learn candidates.
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "as", "by",
    "with", "from", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might", "must", "shall",
    "can", "this", "that", "these", "those", "i", "you", "he", "she", "it", "we", "they",
    "me", "him", "her", "us", "them", "my", "your", "his", "its", "our", "their", "not",
    "no", "yes", "so", "if", "then", "than", "too", "very", "just", "about", "into", "over",
    "after", "before", "between", "under", "again", "further", "once", "here", "there",
    "when", "where", "why", "how", "all", "each", "few", "more", "most", "other", "some",
    "such", "only", "own", "same", "than", "too", "very", "s", "t", "don", "now",
];

/// Levenshtein distance (edit distance) for short tokens.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut cur = vec![0usize; m + 1];
    for i in 1..=n {
        cur[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1)
                .min(cur[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}

fn tokenize(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-'))
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

fn is_stopword(w: &str) -> bool {
    let lower = w.to_ascii_lowercase();
    STOPWORDS.iter().any(|s| *s == lower)
}

/// Word-align injected vs current; return up to 3 auto-learn candidates.
///
/// Rules: single-token substitutions after a greedy align that can skip one
/// insert/delete, edit distance 1–3, token length ≥ 4, not stopword.
pub fn find_learn_candidates(injected: &str, current: &str) -> Vec<String> {
    let inj = tokenize(injected);
    let cur = tokenize(current);
    if inj.is_empty() || cur.is_empty() {
        return Vec::new();
    }

    let n = inj.len();
    let m = cur.len();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut i = 0usize;
    let mut j = 0usize;

    while i < n && j < m {
        if inj[i].eq_ignore_ascii_case(&cur[j]) {
            i += 1;
            j += 1;
            continue;
        }
        // Prefer skipping a pure insert/delete when the next token realigns.
        if i + 1 < n && inj[i + 1].eq_ignore_ascii_case(&cur[j]) {
            i += 1;
            continue;
        }
        if j + 1 < m && inj[i].eq_ignore_ascii_case(&cur[j + 1]) {
            j += 1;
            continue;
        }
        // Single-token substitution.
        let a = &inj[i];
        let b = &cur[j];
        let blen = b.chars().count();
        if blen >= 4 && !is_stopword(b) {
            let d = edit_distance(&a.to_ascii_lowercase(), &b.to_ascii_lowercase());
            if (1..=3).contains(&d) && seen.insert(b.to_ascii_lowercase()) {
                out.push(b.clone());
                if out.len() >= 3 {
                    break;
                }
            }
        }
        i += 1;
        j += 1;
    }
    out
}

/// Words from final text that should bump dictionary use_count (case-insensitive match left to store).
pub fn words_for_bump(final_text: &str) -> Vec<String> {
    tokenize(final_text)
}

/// Schedule auto-learn: wait ~8s, re-read the same window's focused text, add candidates (max 3).
/// Also bumps use_count for dictionary words that appeared in final text.
pub fn spawn_auto_learn(
    store: Arc<dyn Store>,
    injected_text: String,
    target_hwnd: isize,
    auto_learn: bool,
    event_tx: tokio::sync::broadcast::Sender<vf_core::EngineEvent>,
) {
    // Immediate bump (does not need the 8s wait).
    let bump_words = words_for_bump(&injected_text);
    if !bump_words.is_empty() {
        let _ = store.dictionary_bump_use_count(&bump_words);
    }

    if !auto_learn {
        return;
    }

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(8)).await;
        let hwnd = target_hwnd;
        let Some(current) = tokio::task::spawn_blocking(move || {
            context::reread_text_for_hwnd(if hwnd != 0 { Some(hwnd) } else { None })
        })
        .await
        .ok()
        .flatten()
        else {
            // Silent no-op if the element can't be re-read.
            return;
        };
        let candidates = find_learn_candidates(&injected_text, &current);
        for word in candidates {
            match store.dictionary_add(&word, "auto") {
                Ok(_) => {
                    let _ = event_tx.send(vf_core::EngineEvent::DictionaryLearned(word));
                }
                Err(e) => {
                    // Likely UNIQUE conflict — ignore silently.
                    log::debug!("auto-learn skip '{word}': {e}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_basic() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("color", "colour"), 1);
        assert_eq!(edit_distance("same", "same"), 0);
    }

    #[test]
    fn learn_candidates_detects_typo_fix() {
        let injected = "please check the colour of the widget";
        let current = "please check the color of the widget";
        let c = find_learn_candidates(injected, current);
        // "color" is a replacement of "colour" with distance 1, len >= 4
        assert!(c.iter().any(|w| w.eq_ignore_ascii_case("color")));
        assert!(c.len() <= 3);
    }

    #[test]
    fn ignores_short_and_stopwords() {
        let injected = "I am here";
        let current = "I be here";
        let c = find_learn_candidates(injected, current);
        assert!(c.is_empty());
    }
}
