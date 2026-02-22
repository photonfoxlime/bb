//! Transient draft staging for LLM results (expansion and reduction).

use crate::llm;
use crate::store::{ExpansionDraftRecord, ReductionDraftRecord};

/// Staging area for one block's LLM expand results before user acceptance.
///
/// Invariant: removed from `AppState.expansion_drafts` when [`is_empty`](Self::is_empty)
/// returns true (both `rewrite` consumed and all `children` accepted/rejected).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpansionDraft {
    pub(crate) rewrite: Option<String>,
    pub(crate) children: Vec<String>,
}

impl ExpansionDraft {
    pub(crate) fn new(rewrite: Option<String>, children: Vec<String>) -> Self {
        Self { rewrite, children }
    }

    pub(crate) fn from_expand_result(result: llm::ExpandResult) -> Self {
        let (rewrite, children) = result.into_parts();
        let children =
            children.into_iter().map(llm::ExpandSuggestion::into_point).collect::<Vec<_>>();
        Self::new(rewrite, children)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rewrite.is_none() && self.children.is_empty()
    }

    pub(crate) fn from_record(record: ExpansionDraftRecord) -> Self {
        Self::new(record.rewrite, record.children)
    }

    pub(crate) fn to_record(&self) -> ExpansionDraftRecord {
        ExpansionDraftRecord { rewrite: self.rewrite.clone(), children: self.children.clone() }
    }
}

/// Staging area for one block's LLM reduction result before user acceptance.
///
/// Invariant: removed from `AppState.reduction_drafts` when accepted or rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReductionDraft {
    pub(crate) reduction: String,
}

impl ReductionDraft {
    pub(crate) fn new(reduction: String) -> Self {
        Self { reduction }
    }

    pub(crate) fn from_record(record: ReductionDraftRecord) -> Self {
        Self::new(record.reduction)
    }

    pub(crate) fn to_record(&self) -> ReductionDraftRecord {
        ReductionDraftRecord { reduction: self.reduction.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_with_rewrite_and_children() {
        let draft =
            ExpansionDraft::new(Some("text".to_string()), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(draft.rewrite, Some("text".to_string()));
        assert_eq!(draft.children, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn new_empty() {
        let draft = ExpansionDraft::new(None, vec![]);
        assert!(draft.is_empty());
    }

    #[test]
    fn not_empty_with_rewrite_only() {
        let draft = ExpansionDraft::new(Some("text".to_string()), vec![]);
        assert!(!draft.is_empty());
    }

    #[test]
    fn not_empty_with_children_only() {
        let draft = ExpansionDraft::new(None, vec!["child".to_string()]);
        assert!(!draft.is_empty());
    }

    #[test]
    fn from_expand_result() {
        let result = llm::ExpandResult::new(
            Some("rewritten".to_string()),
            vec![llm::ExpandSuggestion::new("c1".to_string())],
        );
        let draft = ExpansionDraft::from_expand_result(result);
        assert_eq!(draft.rewrite, Some("rewritten".to_string()));
        assert_eq!(draft.children, vec!["c1".to_string()]);
    }

    #[test]
    fn reduction_draft_new() {
        let draft = ReductionDraft::new("reduction text".to_string());
        assert_eq!(draft.reduction, "reduction text");
    }
}
