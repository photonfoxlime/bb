use jieba_rs::Jieba;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Han,
    Latin,
    Other,
}

/// Classify a char into a coarse script bucket.
/// This is intentionally simple and optimized for "mixed input search phrases".
fn script_of(ch: char) -> Script {
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
fn split_by_script(s: &str) -> Vec<(Script, String)> {
    let mut out: Vec<(Script, String)> = Vec::new();
    let mut cur_script: Option<Script> = None;
    let mut buf = String::new();

    for ch in s.chars() {
        let sc = script_of(ch);
        match cur_script {
            None => {
                cur_script = Some(sc);
                buf.push(ch);
            }
            Some(cs) if cs == sc => buf.push(ch),
            Some(cs) => {
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

/// Tokenize Latin/number-ish segments in a search-friendly way.
///
/// Keeps patterns like:
/// - "node.js", "foo-bar", "x86_64", "v1.2.3", "path/to", "C++" (partially)
///
/// You can tune the allowed connectors depending on your domain.
fn tokenize_latin(seg: &str) -> Vec<String> {
    static TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
        // A-Za-z0-9 chunks joined by connector chars.
        Regex::new(r"[A-Za-z0-9]+(?:[._+\-/][A-Za-z0-9]+)*").expect("valid regex")
    });

    TOKEN_RE
        .find_iter(seg)
        .map(|m| m.as_str().to_string())
        .collect()
}

/// Tokenize Han segments using Jieba's search mode, which tends to produce
/// tokens closer to search keywords.
fn tokenize_han(jieba: &Jieba, seg: &str) -> Vec<String> {
    jieba
        .cut_for_search(seg, true)
        .into_iter()
        .map(|t| t.to_string())
        .collect()
}

/// Strong boundaries: always flush the current phrase.
fn is_strong_boundary_token(tok: &str) -> bool {
    matches!(tok, "。" | "！" | "？" | "!" | "?" | ";" | "；" | "\n")
}

/// Weak boundaries: for search phrases, it's often beneficial to flush too.
/// You can change this behavior if you want longer phrases.
fn is_weak_boundary_token(tok: &str) -> bool {
    matches!(
        tok,
        "，" | "," | "、" | ":" | "：" | "(" | ")" | "（" | "）" | "[" | "]"
    )
}

/// Minimal unit list (extend for your domain).
fn is_unit(tok: &str) -> bool {
    matches!(
        tok.to_ascii_lowercase().as_str(),
        "kb" | "mb" | "gb" | "tb" | "hz" | "khz" | "mhz" | "ghz" | "w" | "kw" | "v" | "mah"
    ) || matches!(
        tok,
        "个" | "条" | "张" | "杯" | "瓶" | "元" | "块" | "秒" | "分" | "小时" | "天"
    )
}

fn is_number(tok: &str) -> bool {
    tok.chars().all(|c| c.is_ascii_digit())
}

/// Collapse whitespace into a single space and trim.
fn normalize_space(s: &str) -> String {
    static SPACE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\s+").expect("valid regex"));
    SPACE_RE.replace_all(s, " ").trim().to_string()
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
    static BOUNDARY_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"[。！？!?\n;；，,、:：（）()\[\]]").expect("valid regex")
    });

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
            Script::Other => {
                for m in BOUNDARY_RE.find_iter(&seg) {
                    tokens.push(m.as_str().to_string());
                }
            }
            Script::Han => {
                tokens.extend(tokenize_han(&jieba, &seg));
            }
            Script::Latin => {
                tokens.extend(tokenize_latin(&seg));
                for m in BOUNDARY_RE.find_iter(&seg) {
                    tokens.push(m.as_str().to_string());
                }
            }
        }
    }

    // 3) Phrase builder
    let mut phrases: Vec<String> = Vec::new();
    let mut cur: Vec<String> = Vec::new();

    let mut flush = |cur: &mut Vec<String>, phrases: &mut Vec<String>| {
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
    phrases
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::extract_search_phrases;

    fn assert_any_contains<'a>(phrases: &'a [String], needle: &str) -> &'a String {
        phrases
            .iter()
            .find(|p| p.contains(needle))
            .unwrap_or_else(|| panic!("Expected some phrase to contain `{}`. Got: {:?}", needle, phrases))
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
        let phrases = extract_search_phrases(input, &[]);

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
}
