//! Mixed-language text tokenization utilities.
//!
//! This module turns user input into search-friendly phrases without hard splits.
//! It keeps punctuation boundaries, supports Han tokenization via Jieba, and
//! performs lightweight normalization and token merging (for example `16` + `GB`).

use jieba_rs::Jieba;
use regex::Regex;
use std::collections::HashSet;
use std::ops::Range;
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

/// Token span for cursor-by-word navigation.
///
/// Columns are Unicode scalar (char) offsets in one editor line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordTokenSpan {
    /// Inclusive start column (char offset) of a token.
    pub start: usize,
    /// Exclusive end column (char offset) of a token.
    pub end: usize,
}

impl WordTokenSpan {
    fn from_range(range: Range<usize>) -> Self {
        Self { start: range.start, end: range.end }
    }
}

/// Cache for line tokenization used by editor word-motion shortcuts.
///
/// The cache stores the last tokenized line and invalidates automatically when
/// the line text changes.
#[derive(Debug, Clone, Default)]
pub struct WordTokenizationCache {
    cached_line: String,
    cached_spans: Vec<WordTokenSpan>,
}

impl WordTokenizationCache {
    /// Return token spans for one line, recomputing only when needed.
    pub fn spans_for_line(&mut self, line: &str) -> &[WordTokenSpan] {
        if self.cached_line != line {
            self.cached_spans = word_token_spans_for_navigation(line);
            self.cached_line.clear();
            self.cached_line.push_str(line);
        }
        &self.cached_spans
    }
}

/// Tokenize a single editor line into word spans for cursor navigation.
///
/// Behavior by script run:
/// - Latin: use [`tokenize_latin`] and keep only lexical tokens.
/// - Han: each Han character is treated as one navigation token.
/// - Other: ignored for word stepping.
pub fn word_token_spans_for_navigation(line: &str) -> Vec<WordTokenSpan> {
    let mut spans: Vec<WordTokenSpan> = Vec::new();
    let mut run_script: Option<Script> = None;
    let mut run_start_byte = 0usize;
    let mut run_start_char = 0usize;
    let mut total_chars = 0usize;

    for (byte_idx, ch) in line.char_indices() {
        let sc = script_of(ch);
        match run_script {
            | None => {
                run_script = Some(sc);
                run_start_byte = byte_idx;
                run_start_char = total_chars;
            }
            | Some(existing) if existing == sc => {}
            | Some(existing) => {
                let run = &line[run_start_byte..byte_idx];
                extend_word_spans_for_run(&mut spans, existing, run, run_start_char);
                run_script = Some(sc);
                run_start_byte = byte_idx;
                run_start_char = total_chars;
            }
        }
        total_chars += 1;
    }

    if let Some(script) = run_script {
        let run = &line[run_start_byte..];
        extend_word_spans_for_run(&mut spans, script, run, run_start_char);
    }

    spans
}

fn extend_word_spans_for_run(
    spans: &mut Vec<WordTokenSpan>, script: Script, run: &str, base_char: usize,
) {
    match script {
        | Script::Han => {
            for (offset, _) in run.chars().enumerate() {
                spans
                    .push(WordTokenSpan { start: base_char + offset, end: base_char + offset + 1 });
            }
        }
        | Script::Latin => {
            for range in latin_word_char_ranges(run) {
                spans.push(WordTokenSpan::from_range(
                    (base_char + range.start)..(base_char + range.end),
                ));
            }
        }
        | Script::Other => {}
    }
}

fn latin_word_char_ranges(seg: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut search_start_byte = 0usize;
    let mut search_start_char = 0usize;

    for token in tokenize_latin(seg) {
        if !token.chars().any(|ch| ch.is_ascii_alphanumeric()) {
            continue;
        }

        let Some(relative_start) = seg[search_start_byte..].find(&token) else {
            continue;
        };
        let token_start_byte = search_start_byte + relative_start;
        let prefix = &seg[search_start_byte..token_start_byte];
        let token_start_char = search_start_char + prefix.chars().count();
        let token_end_char = token_start_char + token.chars().count();
        ranges.push(token_start_char..token_end_char);

        search_start_byte = token_start_byte + token.len();
        search_start_char = token_end_char;
    }

    ranges
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

/// Tokenize text for inline diff comparison.
///
/// Uses script-aware splitting so that CJK text is tokenized per-character
/// (since there are no word-separating spaces) while Latin/ASCII text is
/// split on whitespace boundaries with whitespace preserved as separate tokens.
///
/// This produces fine-grained tokens suitable for `similar::TextDiff` so that
/// diffs highlight the exact characters that changed, regardless of script.
pub fn tokenize_for_diff(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let runs = split_by_script(text);

    for (script, segment) in runs {
        match script {
            | Script::Han => {
                // Each Han character becomes its own diff token.
                for ch in segment.chars() {
                    tokens.push(ch.to_string());
                }
            }
            | Script::Latin => {
                // Split on whitespace boundaries, preserving whitespace as tokens.
                tokenize_latin_for_diff(&segment, &mut tokens);
            }
            | Script::Other => {
                // Whitespace and symbols: each character is its own token so
                // that newlines and spaces diff independently.
                for ch in segment.chars() {
                    tokens.push(ch.to_string());
                }
            }
        }
    }

    tokens
}

/// Split a Latin/ASCII segment on whitespace boundaries, keeping whitespace
/// characters as individual tokens.
fn tokenize_latin_for_diff(seg: &str, out: &mut Vec<String>) {
    let mut start = 0;
    for (i, ch) in seg.char_indices() {
        if ch.is_whitespace() {
            if start < i {
                out.push(seg[start..i].to_string());
            }
            out.push(seg[i..i + ch.len_utf8()].to_string());
            start = i + ch.len_utf8();
        }
    }
    if start < seg.len() {
        out.push(seg[start..].to_string());
    }
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

/// Truncate text for compact UI labels without splitting on bytes.
///
/// The returned string is limited to `max_chars` Unicode scalar values and uses
/// `...` as suffix when truncation happens. The function prefers to cut at a
/// nearby token boundary (for example whitespace or punctuation) and falls back
/// to a hard character boundary when no such boundary exists.
pub fn truncate_for_display(s: &str, max_chars: usize) -> String {
    const ELLIPSIS: &str = "...";

    if max_chars == 0 {
        return String::new();
    }

    let char_count = s.chars().count();
    if char_count <= max_chars {
        return s.to_string();
    }

    if max_chars <= ELLIPSIS.len() {
        return ELLIPSIS.chars().take(max_chars).collect();
    }

    let budget = max_chars - ELLIPSIS.len();
    let hard_cut = byte_index_at_char(s, budget);
    let token_cut = find_last_display_boundary(s, hard_cut);
    let cut = token_cut.filter(|idx| *idx > 0).unwrap_or(hard_cut);
    let head = s[..cut].trim_end();
    format!("{head}{ELLIPSIS}")
}

fn byte_index_at_char(s: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    s.char_indices().nth(char_index).map_or(s.len(), |(idx, _)| idx)
}

fn find_last_display_boundary(s: &str, limit: usize) -> Option<usize> {
    let mut last_boundary: Option<usize> = None;
    let mut iter = s.char_indices().peekable();

    while let Some((idx, ch)) = iter.next() {
        if idx >= limit {
            break;
        }

        let next_idx = idx + ch.len_utf8();
        if next_idx > limit {
            break;
        }

        if is_display_delimiter(ch) {
            last_boundary = Some(idx);
            continue;
        }

        if let Some((_, next_ch)) = iter.peek().copied()
            && script_of(ch) != script_of(next_ch)
        {
            last_boundary = Some(next_idx);
        }
    }

    last_boundary
}

fn is_display_delimiter(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '。' | '！'
                | '？'
                | '!'
                | '?'
                | ';'
                | '；'
                | '，'
                | ','
                | '、'
                | ':'
                | '：'
                | '('
                | ')'
                | '（'
                | '）'
                | '['
                | ']'
        )
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
        Script, WordTokenizationCache, extract_search_phrases, normalize_space, script_of,
        split_by_script, tokenize_for_diff, tokenize_latin, truncate_for_display,
        word_token_spans_for_navigation,
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
    fn word_navigation_spans_follow_latin_tokens() {
        let spans = word_token_spans_for_navigation("alpha,beta v1.2.3");
        let got: Vec<(usize, usize)> = spans.iter().map(|span| (span.start, span.end)).collect();
        assert_eq!(got, vec![(0, 5), (6, 10), (11, 17)]);
    }

    #[test]
    fn word_navigation_spans_split_han_by_character() {
        let spans = word_token_spans_for_navigation("中文ab");
        let got: Vec<(usize, usize)> = spans.iter().map(|span| (span.start, span.end)).collect();
        assert_eq!(got, vec![(0, 1), (1, 2), (2, 4)]);
    }

    #[test]
    fn word_tokenization_cache_reuses_previous_result_until_line_changes() {
        let mut cache = WordTokenizationCache::default();

        let first = cache.spans_for_line("alpha,beta").len();
        let second = cache.spans_for_line("alpha,beta").len();
        let third = cache.spans_for_line("alpha,beta,gamma").len();

        assert_eq!(first, 2);
        assert_eq!(second, 2);
        assert_eq!(third, 3);
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

    #[test]
    fn truncate_for_display_keeps_short_text_unchanged() {
        assert_eq!(truncate_for_display("alpha", 10), "alpha");
    }

    #[test]
    fn truncate_for_display_prefers_word_boundary() {
        assert_eq!(truncate_for_display("alpha beta gamma", 11), "alpha...");
    }

    #[test]
    fn truncate_for_display_falls_back_to_hard_character_cut() {
        assert_eq!(truncate_for_display("abcdefghijk", 7), "abcd...");
    }

    #[test]
    fn truncate_for_display_handles_tiny_limits() {
        assert_eq!(truncate_for_display("alphabet", 2), "..");
    }

    // ── tokenize_for_diff tests ──────────────────────────────────────

    #[test]
    fn diff_tokenize_latin_splits_on_whitespace() {
        let tokens = tokenize_for_diff("hello world");
        assert_eq!(tokens, vec!["hello", " ", "world"]);
    }

    #[test]
    fn diff_tokenize_han_splits_per_character() {
        let tokens = tokenize_for_diff("今天天气");
        assert_eq!(tokens, vec!["今", "天", "天", "气"]);
    }

    #[test]
    fn diff_tokenize_mixed_script() {
        let tokens = tokenize_for_diff("使用Rust编程");
        assert_eq!(tokens, vec!["使", "用", "Rust", "编", "程"]);
    }

    #[test]
    fn diff_tokenize_preserves_newlines() {
        let tokens = tokenize_for_diff("hello\nworld");
        assert_eq!(tokens, vec!["hello", "\n", "world"]);
    }

    #[test]
    fn diff_tokenize_empty() {
        let tokens = tokenize_for_diff("");
        assert!(tokens.is_empty());
    }
}
