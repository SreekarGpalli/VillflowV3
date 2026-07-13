//! Output safeguards for the dictation pipeline.
//!
//! The LLM cleanup stage receives the user's field text as continuity
//! context; models occasionally echo or rewrite that context instead of
//! returning only the cleaned transcript. These deterministic guards catch
//! that so the engine can strip the echo or retry without context.

/// Lowercased alphanumeric words.
fn words_normalized(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// Byte offset in `original` at the first non-word char after its `n`-th word.
fn offset_after_n_words(original: &str, n: usize) -> usize {
    let mut count = 0;
    let mut in_word = false;
    for (i, c) in original.char_indices() {
        let alnum = c.is_alphanumeric();
        if alnum && !in_word {
            in_word = true;
        }
        if !alnum && in_word {
            in_word = false;
            count += 1;
            if count == n {
                return i;
            }
        }
    }
    original.len()
}

/// If the output begins by repeating the tail of the field context, strip the
/// repeated prefix. Word-based and case/punctuation-insensitive; requires an
/// overlap of at least 4 words so outputs that legitimately start with common
/// words are untouched. Returns an empty string when the whole output is an
/// echo of the context.
pub fn strip_context_echo(output: &str, field_context: &str) -> String {
    const MIN_ECHO_WORDS: usize = 4;
    let out_words = words_normalized(output);
    let ctx_words = words_normalized(field_context);
    if out_words.len() < MIN_ECHO_WORDS || ctx_words.is_empty() {
        return output.to_string();
    }
    let max_k = out_words.len().min(ctx_words.len());
    let mut overlap = 0usize;
    for k in (MIN_ECHO_WORDS..=max_k).rev() {
        if ctx_words[ctx_words.len() - k..] == out_words[..k] {
            overlap = k;
            break;
        }
    }
    if overlap == 0 {
        return output.to_string();
    }
    if overlap == out_words.len() {
        return String::new();
    }
    let cut = offset_after_n_words(output, overlap);
    output[cut..]
        .trim_start_matches(|c: char| !c.is_alphanumeric() && c != '\n')
        .trim_start()
        .to_string()
}

/// Heuristic: the cleaned output should be about the transcript, not the
/// document. Flags outputs that (a) still contain a long run of the field
/// context verbatim, or (b) balloon far beyond the dictated length — both
/// symptoms of the model rewriting the document instead of cleaning the
/// transcript. The engine retries without context when this returns true.
pub fn dictation_output_suspicious(
    output: &str,
    field_context: &str,
    raw_transcript: &str,
) -> bool {
    let out_words = words_normalized(output);
    let ctx_words = words_normalized(field_context);
    let raw_len = words_normalized(raw_transcript).len().max(1);

    // (b) Length balloon: cleanup never triples a transcript.
    if out_words.len() > raw_len * 3 + 20 {
        return true;
    }

    // (a) Context containment: the last (up to) 12 context words appearing as
    // a contiguous run inside the output means the context leaked through.
    if ctx_words.len() >= 8 {
        let start = ctx_words.len().saturating_sub(12);
        let probe = &ctx_words[start..];
        if out_words.len() >= probe.len()
            && out_words.windows(probe.len()).any(|w| w == probe)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_removes_leading_context_echo() {
        let ctx = "Dear team,\nPlease find attached the quarterly report";
        let out = "find attached the quarterly report. Let me know your thoughts.";
        assert_eq!(strip_context_echo(out, ctx), "Let me know your thoughts.");
    }

    #[test]
    fn strip_ignores_short_overlaps() {
        let ctx = "I went to the store";
        let out = "the store was closed so I came home"; // 2-word overlap only
        assert_eq!(strip_context_echo(out, ctx), out);
    }

    #[test]
    fn strip_full_echo_returns_empty() {
        let ctx = "this is the existing document text right here";
        let out = "This is the existing document text right here.";
        assert_eq!(strip_context_echo(out, ctx), "");
    }

    #[test]
    fn strip_no_context_is_noop() {
        assert_eq!(strip_context_echo("hello world out there", ""), "hello world out there");
    }

    #[test]
    fn suspicious_on_length_balloon() {
        let out = "word ".repeat(60);
        assert!(dictation_output_suspicious(&out, "", "just three words"));
    }

    #[test]
    fn suspicious_on_context_containment() {
        let ctx = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
        let out = format!("Cleaned sentence first. {ctx} trailing.");
        assert!(dictation_output_suspicious(&out, ctx, &"w ".repeat(40)));
    }

    #[test]
    fn clean_output_not_suspicious() {
        let ctx = "Dear team, please find attached the quarterly report for review";
        let out = "I will send the final numbers tomorrow morning.";
        assert!(!dictation_output_suspicious(out, ctx, "um I will send the final numbers uh tomorrow morning"));
    }
}
