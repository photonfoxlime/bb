//! Word-wise diff computation for expansion draft rendering.

use similar::{DiffTag, TextDiff};

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
pub(crate) fn word_diff(old: &str, new: &str) -> Vec<WordChange> {
    if old.is_empty() && new.is_empty() {
        return vec![];
    }

    // Tokenize into words (split on whitespace, preserving it)
    let old_words = tokenize_words(old);
    let new_words = tokenize_words(new);

    // Use similar crate's diff algorithm on word slices
    let diff = TextDiff::from_slices(&old_words, &new_words);

    let mut changes = Vec::new();
    for op in diff.ops() {
        match op.tag() {
            DiffTag::Equal => {
                // Unchanged words - iterate over old range
                let range = op.old_range();
                for i in range {
                    changes.push(WordChange::Unchanged(old_words[i].to_string()));
                }
            }
            DiffTag::Delete => {
                // Deleted words
                let range = op.old_range();
                for i in range {
                    changes.push(WordChange::Deleted(old_words[i].to_string()));
                }
            }
            DiffTag::Insert => {
                // Added words
                let range = op.new_range();
                for i in range {
                    changes.push(WordChange::Added(new_words[i].to_string()));
                }
            }
            DiffTag::Replace => {
                // Replacement: deletions followed by additions
                let old_range = op.old_range();
                for i in old_range {
                    changes.push(WordChange::Deleted(old_words[i].to_string()));
                }
                let new_range = op.new_range();
                for i in new_range {
                    changes.push(WordChange::Added(new_words[i].to_string()));
                }
            }
        }
    }

    changes
}

/// Tokenize text into words, preserving whitespace as separate tokens.
///
/// Splits on whitespace boundaries but keeps whitespace sequences as
/// separate tokens so they can be rendered correctly.
fn tokenize_words(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = 0;

    for (i, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if start < i {
                // Add non-whitespace token
                tokens.push(&text[start..i]);
            }
            // Add whitespace token
            tokens.push(&text[i..i + ch.len_utf8()]);
            start = i + ch.len_utf8();
        }
    }

    // Add remaining text
    if start < text.len() {
        tokens.push(&text[start..]);
    }

    tokens
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
                WordChange::Added(s) => Some(s.clone()),
                _ => None,
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
                WordChange::Deleted(s) => Some(s.clone()),
                _ => None,
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
                WordChange::Deleted(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        let added: Vec<_> = changes
            .iter()
            .filter_map(|c| match c {
                WordChange::Added(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        assert!(deleted.contains(&"world".to_string()));
        assert!(added.contains(&"there".to_string()));
    }
}
