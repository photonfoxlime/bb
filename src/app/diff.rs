//! Word-wise diff computation for expansion draft rendering.
//!
//! Uses the script-aware tokenizer from [`crate::text`] so that CJK text is
//! diffed per-character while Latin text is diffed per-word. The result is a
//! flat sequence of [`WordChange`] values that the renderer can map to styled
//! `rich_text` spans.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.

use similar::{DiffTag, TextDiff};

use crate::text::tokenize_for_diff;

/// A word-level change in a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WordChange {
    /// Unchanged text
    Unchanged(String),
    /// Deleted text (from old version)
    Deleted(String),
    /// Added text (in new version)
    Added(String),
}

/// Compute word-wise diff between old and new text.
///
/// Returns a sequence of word-level changes that can be rendered with
/// appropriate styling (deletions in red, additions in green).
///
/// Note: uses [`tokenize_for_diff`] which is script-aware — Han characters
/// are individual tokens, Latin words are whitespace-delimited tokens.
pub(crate) fn word_diff(old: &str, new: &str) -> Vec<WordChange> {
    if old.is_empty() && new.is_empty() {
        return vec![];
    }

    let old_tokens = tokenize_for_diff(old);
    let new_tokens = tokenize_for_diff(new);

    let old_refs: Vec<&str> = old_tokens.iter().map(|s| s.as_str()).collect();
    let new_refs: Vec<&str> = new_tokens.iter().map(|s| s.as_str()).collect();
    let diff = TextDiff::from_slices(&old_refs, &new_refs);

    let mut changes = Vec::new();
    for op in diff.ops() {
        match op.tag() {
            | DiffTag::Equal => {
                let range = op.old_range();
                for i in range {
                    changes.push(WordChange::Unchanged(old_tokens[i].clone()));
                }
            }
            | DiffTag::Delete => {
                let range = op.old_range();
                for i in range {
                    changes.push(WordChange::Deleted(old_tokens[i].clone()));
                }
            }
            | DiffTag::Insert => {
                let range = op.new_range();
                for i in range {
                    changes.push(WordChange::Added(new_tokens[i].clone()));
                }
            }
            | DiffTag::Replace => {
                let old_range = op.old_range();
                for i in old_range {
                    changes.push(WordChange::Deleted(old_tokens[i].clone()));
                }
                let new_range = op.new_range();
                for i in new_range {
                    changes.push(WordChange::Added(new_tokens[i].clone()));
                }
            }
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_diff_unchanged() {
        let changes = word_diff("hello world", "hello world");
        assert!(changes.iter().all(|c| matches!(c, WordChange::Unchanged(_))));
    }

    #[test]
    fn word_diff_simple_addition() {
        let changes = word_diff("hello", "hello world");
        let added: Vec<_> = changes
            .iter()
            .filter_map(|c| match c {
                | WordChange::Added(s) => Some(s.clone()),
                | _ => None,
            })
            .collect();
        assert!(added.contains(&"world".to_string()));
    }

    #[test]
    fn word_diff_simple_deletion() {
        let changes = word_diff("hello world", "hello");
        let deleted: Vec<_> = changes
            .iter()
            .filter_map(|c| match c {
                | WordChange::Deleted(s) => Some(s.clone()),
                | _ => None,
            })
            .collect();
        assert!(deleted.contains(&"world".to_string()));
    }

    #[test]
    fn word_diff_replacement() {
        let changes = word_diff("hello world", "hello there");
        let deleted: Vec<_> = changes
            .iter()
            .filter_map(|c| match c {
                | WordChange::Deleted(s) => Some(s.clone()),
                | _ => None,
            })
            .collect();
        let added: Vec<_> = changes
            .iter()
            .filter_map(|c| match c {
                | WordChange::Added(s) => Some(s.clone()),
                | _ => None,
            })
            .collect();
        assert!(deleted.contains(&"world".to_string()));
        assert!(added.contains(&"there".to_string()));
    }

    #[test]
    fn word_diff_han_per_character() {
        // Changing one character in a Chinese sentence should only mark
        // that character as deleted/added, not the whole run.
        let changes = word_diff("今天天气很好", "今天天气不好");
        let deleted: Vec<_> = changes
            .iter()
            .filter_map(|c| match c {
                | WordChange::Deleted(s) => Some(s.clone()),
                | _ => None,
            })
            .collect();
        let added: Vec<_> = changes
            .iter()
            .filter_map(|c| match c {
                | WordChange::Added(s) => Some(s.clone()),
                | _ => None,
            })
            .collect();
        assert_eq!(deleted, vec!["很"]);
        assert_eq!(added, vec!["不"]);
    }

    #[test]
    fn word_diff_mixed_script() {
        let changes = word_diff("使用Rust编程", "使用Go编程");
        let deleted: Vec<_> = changes
            .iter()
            .filter_map(|c| match c {
                | WordChange::Deleted(s) => Some(s.clone()),
                | _ => None,
            })
            .collect();
        let added: Vec<_> = changes
            .iter()
            .filter_map(|c| match c {
                | WordChange::Added(s) => Some(s.clone()),
                | _ => None,
            })
            .collect();
        assert_eq!(deleted, vec!["Rust"]);
        assert_eq!(added, vec!["Go"]);
    }
}
