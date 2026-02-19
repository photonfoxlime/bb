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
