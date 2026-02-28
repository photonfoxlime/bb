//! Mixed-language text tokenization utilities.
//!
//! This module turns user input into search-friendly phrases without hard splits.
//! It keeps punctuation boundaries, supports Han tokenization via Jieba, and
//! performs lightweight normalization and token merging (for example `16` + `GB`).

use jieba_rs::Jieba;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// Coarse script bucket used by tokenizer run splitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    /// CJK Unified Ideographs basic range (Han).
    Han,
    /// ASCII alnum and punctuation run handled by Latin tokenizer.
    Latin,
    /// Whitespace and remaining symbols.
    Other,
}

/// Classify a char into a coarse script bucket.
/// This is intentionally simple and optimized for "mixed input search phrases".
pub fn script_of(ch: char) -> Script {
    if ('\u{4E00}'..='\u{9FFF}').contains(&ch) {
        Script::Han
    } else if ch.is_ascii_alphanumeric() || (ch.is_ascii() && ch.is_ascii_punctuation()) {
        // Treat ASCII punctuation as part of the Latin run so our Latin tokenizer can handle it.
        Script::Latin
    } else if ch.is_whitespace() {
        Script::Other
    } else {
        // Emojis and other Unicode symbols.
        Script::Other
    }
}

/// Split input into contiguous runs of the same Script.
pub fn split_by_script(s: &str) -> Vec<(Script, String)> {
    let mut out: Vec<(Script, String)> = Vec::new();
    let mut cur_script: Option<Script> = None;
    let mut buf = String::new();

    for ch in s.chars() {
        let sc = script_of(ch);
        match cur_script {
            | None => {
                cur_script = Some(sc);
                buf.push(ch);
            }
            | Some(cs) if cs == sc => buf.push(ch),
            | Some(cs) => {
                out.push((cs, buf));
                buf = String::new();
                buf.push(ch);
                cur_script = Some(sc);
            }
        }
    }

    if let Some(cs) = cur_script {
        if !buf.is_empty() {
            out.push((cs, buf));
        }
    }
    out
}

/// Tokenize Latin/number-ish segments and keep boundaries in lexical order.
///
/// For example, `alpha,beta` becomes `alpha`, `,`, `beta`.
pub fn tokenize_latin(seg: &str) -> Vec<String> {
    static TOKEN_OR_BOUNDARY_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"[A-Za-z0-9]+(?:[._+\-/][A-Za-z0-9]+)*|[。！？!?\n;；，,、:：（）()\[\]]")
            .expect("valid regex")
    });

    TOKEN_OR_BOUNDARY_RE.find_iter(seg).map(|m| m.as_str().to_string()).collect()
}

/// Tokenize Han segments using Jieba's search mode, which tends to produce
/// tokens closer to search keywords.
fn tokenize_han(jieba: &Jieba, seg: &str) -> Vec<String> {
    jieba.cut_for_search(seg, true).into_iter().map(|t| t.to_string()).collect()
}

/// Strong boundaries: always flush the current phrase.
fn is_strong_boundary_token(tok: &str) -> bool {
    matches!(tok, "。" | "！" | "？" | "!" | "?" | ";" | "；" | "\n")
}

/// Weak boundaries: for search phrases, it's often beneficial to flush too.
/// You can change this behavior if you want longer phrases.
fn is_weak_boundary_token(tok: &str) -> bool {
    matches!(tok, "，" | "," | "、" | ":" | "：" | "(" | ")" | "（" | "）" | "[" | "]")
}

/// Minimal unit list (extend for your domain).
fn is_unit(tok: &str) -> bool {
    matches!(
        tok.to_ascii_lowercase().as_str(),
        "kb" | "mb" | "gb" | "tb" | "hz" | "khz" | "mhz" | "ghz" | "w" | "kw" | "v" | "mah"
    ) || matches!(tok, "个" | "条" | "张" | "杯" | "瓶" | "元" | "块" | "秒" | "分" | "小时" | "天")
}

fn is_number(tok: &str) -> bool {
    tok.chars().all(|c| c.is_ascii_digit())
}

/// Collapse horizontal whitespace and trim while preserving newlines.
///
/// Newlines are retained because they are strong phrase boundaries.
pub fn normalize_space(s: &str) -> String {
    static HSPACE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[^\S\n]+").expect("valid regex"));
    static NEWLINE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r" *\n+ *").expect("valid regex"));

    let normalized_newline = s.replace("\r\n", "\n").replace('\r', "\n");
    let collapsed = HSPACE_RE.replace_all(&normalized_newline, " ");
    NEWLINE_RE.replace_all(&collapsed, "\n").trim().to_string()
}

/// Extract "search phrases" from mixed-language user input (pure Rust).
///
/// - Splits by script runs (Han vs Latin-ish)
/// - Han: Jieba search-mode cut
/// - Latin: regex tokenizer that preserves common connectors
/// - Builds phrases by flushing on punctuation boundaries
/// - Merges number+unit (e.g., "16" + "GB" => "16GB")
pub fn extract_search_phrases(input: &str, user_words: &[&str]) -> Vec<String> {
    let input = normalize_space(input);

    // Boundary punctuation we keep as standalone tokens for phrase splitting.
    static BOUNDARY_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[。！？!?\n;；，,、:：（）()\[\]]").expect("valid regex"));

    let mut jieba = Jieba::new();
    // User dictionary: brand names, product names, model numbers, library names, etc.
    for w in user_words {
        jieba.add_word(w, None, None);
    }

    // 1) Script runs
    let runs = split_by_script(&input);

    // 2) Tokenize each run and also extract boundary punctuation into tokens.
    let mut tokens: Vec<String> = Vec::new();
    for (sc, seg) in runs {
        match sc {
            | Script::Other => {
                for m in BOUNDARY_RE.find_iter(&seg) {
                    tokens.push(m.as_str().to_string());
                }
            }
            | Script::Han => {
                tokens.extend(tokenize_han(&jieba, &seg));
            }
            | Script::Latin => {
                tokens.extend(tokenize_latin(&seg));
            }
        }
    }

    // 3) Phrase builder
    let mut phrases: Vec<String> = Vec::new();
    let mut cur: Vec<String> = Vec::new();

    let flush = |cur: &mut Vec<String>, phrases: &mut Vec<String>| {
        if cur.is_empty() {
            return;
        }
        // Join by spaces; for search phrases, this is acceptable for both Han and Latin.
        let phrase = cur.join(" ");
        if phrase.len() >= 2 {
            phrases.push(phrase);
        }
        cur.clear();
    };

    let mut i = 0usize;
    while i < tokens.len() {
        let tok = tokens[i].as_str();

        if is_strong_boundary_token(tok) || is_weak_boundary_token(tok) {
            flush(&mut cur, &mut phrases);
            i += 1;
            continue;
        }

        // Merge: number + unit => single token
        if is_number(tok) && i + 1 < tokens.len() && is_unit(&tokens[i + 1]) {
            cur.push(format!("{}{}", tok, tokens[i + 1]));
            i += 2;
            continue;
        }

        cur.push(tokens[i].clone());
        i += 1;
    }
    flush(&mut cur, &mut phrases);

    // 4) Stable de-duplication while preserving order
    let mut seen: HashSet<String> = HashSet::new();
    phrases.into_iter().filter(|p| seen.insert(p.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        Script, extract_search_phrases, normalize_space, script_of, split_by_script, tokenize_latin,
    };

    fn assert_any_contains<'a>(phrases: &'a [String], needle: &str) -> &'a String {
        phrases.iter().find(|p| p.contains(needle)).unwrap_or_else(|| {
            panic!("Expected some phrase to contain `{}`. Got: {:?}", needle, phrases)
        })
    }

    fn assert_eq_vec(got: Vec<String>, expected: &[&str]) {
        let got_refs: Vec<&str> = got.iter().map(|s| s.as_str()).collect();
        assert_eq!(got_refs, expected);
    }

    #[test]
    fn merges_number_and_unit_basic() {
        // "16 GB" should become "16GB" inside a phrase.
        let input = "RTX-4090 16 GB";
        let user_words = ["RTX-4090"];
        let phrases = extract_search_phrases(input, &user_words);

        // We don't enforce exact phrasing order beyond containing the merged token.
        assert_any_contains(&phrases, "16GB");
        assert_any_contains(&phrases, "RTX-4090");
    }

    #[test]
    fn splits_on_strong_and_weak_boundaries() {
        // Weak boundary (comma/，) currently flushes a phrase in our implementation.
        // Strong boundary (。) also flushes.
        let input = "alpha,beta。gamma，delta";
        let phrases = extract_search_phrases(input, &[]);

        // Because we flush on comma and Chinese comma, each token becomes its own phrase here.
        // Also note: Latin tokenizer extracts "alpha" "beta" "gamma" "delta".
        assert_eq_vec(phrases, &["alpha", "beta", "gamma", "delta"]);
    }

    #[test]
    fn preserves_searchy_latin_tokens_with_connectors() {
        let input = "node.js foo-bar x86_64 v1.2.3 path/to";
        let phrases = extract_search_phrases(input, &[]);

        // With no boundaries, they should all end up in one phrase (space-joined).
        // (Our phrase builder joins tokens with spaces.)
        let p = phrases.get(0).expect("should have at least one phrase");
        assert!(p.contains("node.js"));
        assert!(p.contains("foo-bar"));
        assert!(p.contains("x86_64"));
        assert!(p.contains("v1.2.3"));
        assert!(p.contains("path/to"));
    }

    #[test]
    fn mixed_han_and_latin_are_both_present() {
        // We avoid asserting exact Jieba output.
        // Instead, we inject stable user words to ensure those show up as tokens.
        let input = "想买 显卡 RTX-4090 on macOS";
        let user_words = ["RTX-4090", "macOS", "显卡"];
        let phrases = extract_search_phrases(input, &user_words);

        assert_any_contains(&phrases, "RTX-4090");
        assert_any_contains(&phrases, "macOS");
        assert_any_contains(&phrases, "显卡");
    }

    #[test]
    fn de_duplicates_phrases_stably() {
        // Same phrase repeated should appear once.
        // We craft input that would generate identical phrases with our current logic.
        let input = "alpha alpha, alpha";
        let _phrases = extract_search_phrases(input, &[]);

        // Likely tokens => phrases ["alpha alpha", "alpha"]? Actually:
        // - "alpha alpha" (no boundary yet)
        // - comma flush -> phrase "alpha alpha"
        // - then "alpha" -> phrase "alpha"
        // No duplicates here. Let's do something more direct:
        let input2 = "alpha,alpha";
        let phrases2 = extract_search_phrases(input2, &[]);
        // This generates ["alpha", "alpha"] before de-dup => after de-dup => ["alpha"]
        assert_eq_vec(phrases2, &["alpha"]);
    }

    #[test]
    fn whitespace_is_normalized() {
        let input = "alpha   beta\t\tgamma\n\ndelta";
        let phrases = extract_search_phrases(input, &[]);

        // Newlines are strong boundaries => should split before/after '\n'.
        // But note: boundary regex also captures '\n' as boundary token;
        // So we should get phrases like "alpha beta gamma" then "delta".
        // Depending on how TOKEN_RE sees the segment, it should still be stable.
        assert_eq_vec(phrases, &["alpha beta gamma", "delta"]);
    }

    #[test]
    fn number_unit_merge_with_chinese_unit() {
        let input = "买 3 个 苹果";
        let user_words = ["苹果"]; // stabilize the noun token
        let phrases = extract_search_phrases(input, &user_words);

        // Expect "3个" merged somewhere.
        assert_any_contains(&phrases, "3个");
        assert_any_contains(&phrases, "苹果");
    }

    #[test]
    fn bracket_like_boundaries_flush() {
        let input = "alpha(beta)gamma";
        let phrases = extract_search_phrases(input, &[]);

        // '(' and ')' are weak boundaries => flush around them.
        // Latin tokenizer extracts alpha beta gamma.
        assert_eq_vec(phrases, &["alpha", "beta", "gamma"]);
    }

    #[test]
    fn empty_input_returns_no_phrases() {
        let phrases = extract_search_phrases("", &[]);
        assert!(phrases.is_empty());
    }

    #[test]
    fn one_character_tokens_are_filtered_out() {
        let phrases = extract_search_phrases("a,b", &[]);
        assert!(phrases.is_empty());
    }

    #[test]
    fn merges_number_and_latin_unit_case_insensitive() {
        let input = "power 500 w 1 kHz";
        let phrases = extract_search_phrases(input, &[]);

        assert_any_contains(&phrases, "500w");
        assert_any_contains(&phrases, "1kHz");
    }

    #[test]
    fn unknown_unit_is_not_merged() {
        let input = "alpha 16 xb";
        let phrases = extract_search_phrases(input, &[]);
        let phrase = phrases.first().expect("at least one phrase");

        assert!(phrase.contains("16 xb"));
        assert!(!phrase.contains("16xb"));
    }

    #[test]
    fn crlf_is_normalized_to_newline_boundary() {
        let phrases = extract_search_phrases("alpha\r\n\r\nbeta", &[]);
        assert_eq_vec(phrases, &["alpha", "beta"]);
    }

    #[test]
    fn semicolon_boundaries_flush_phrases() {
        let phrases = extract_search_phrases("alpha;beta；gamma", &[]);
        assert_eq_vec(phrases, &["alpha", "beta", "gamma"]);
    }

    #[test]
    fn symbols_do_not_force_boundary_without_punctuation_match() {
        let phrases = extract_search_phrases("alpha😀beta", &[]);
        assert_eq_vec(phrases, &["alpha beta"]);
    }

    #[test]
    fn tokenize_latin_keeps_connectors_and_boundaries_in_order() {
        let tokens = tokenize_latin("foo-bar,baz/qux");
        assert_eq!(tokens, vec!["foo-bar", ",", "baz/qux"]);
    }

    #[test]
    fn exported_helpers_classify_and_split_script_runs() {
        assert_eq!(script_of('你'), Script::Han);
        assert_eq!(script_of('-'), Script::Latin);
        assert_eq!(script_of(' '), Script::Other);

        let runs = split_by_script("abc中文 123");
        assert_eq!(runs.len(), 4);
        assert_eq!(runs[0], (Script::Latin, "abc".to_string()));
        assert_eq!(runs[1], (Script::Han, "中文".to_string()));
        assert_eq!(runs[2], (Script::Other, " ".to_string()));
        assert_eq!(runs[3], (Script::Latin, "123".to_string()));
    }

    #[test]
    fn normalize_space_preserves_newline_and_trims_edges() {
        let normalized = normalize_space("  alpha\t beta \n  gamma  ");
        assert_eq!(normalized, "alpha beta\ngamma");
    }
}
