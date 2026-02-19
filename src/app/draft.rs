use crate::llm;

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
}
