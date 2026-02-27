//! LLM context types: block context, lineage, friend blocks, and result types.

/// Immutable snapshot of a block's LLM-relevant context: ancestor lineage,
/// existing child point texts, and user-selected friend blocks.
///
/// The target block point is represented by the final lineage item.
/// Therefore one `BlockContext` captures the full readable context envelope:
/// target point, parent chain, direct children, and friend blocks.
///
/// Constructed by the store layer; consumed by [`LlmClient`] methods.
/// The `existing_children` field carries only point texts (no `BlockId`s)
/// so this module stays decoupled from store identity types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockContext {
    pub(crate) lineage: Lineage,
    pub(crate) existing_children: Vec<String>,
    pub(crate) friend_blocks: Vec<FriendContext>,
}

/// One friend context item supplied alongside lineage and existing children.
///
/// `point` is the friend block text itself.
/// `perspective` is optional target-authored framing describing how the
/// current block views that friend block.
/// `parent_lineage_telescope` controls whether the friend block's parent lineage is included.
/// `children_telescope` controls whether the friend block's children are included.
/// `friend_lineage` contains the friend block's parent lineage (when visible).
/// `friend_children` contains the friend block's children point texts (when visible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendContext {
    pub(crate) point: String,
    pub(crate) perspective: Option<String>,
    pub(crate) parent_lineage_telescope: bool,
    pub(crate) children_telescope: bool,
    pub(crate) friend_lineage: Option<Lineage>,
    pub(crate) friend_children: Option<Vec<String>>,
}

impl BlockContext {
    /// Create a new block context with the given lineage, existing children, and friend blocks.
    ///
    /// # Requires
    /// - `lineage` should represent the path from root to the target block.
    pub fn new(
        lineage: Lineage, existing_children: Vec<String>, friend_blocks: Vec<FriendContext>,
    ) -> Self {
        Self { lineage, existing_children, friend_blocks }
    }

    /// Get a reference to the lineage (ancestor chain).
    pub fn lineage(&self) -> &Lineage {
        &self.lineage
    }

    pub fn existing_children(&self) -> &[String] {
        &self.existing_children
    }

    pub fn friend_blocks(&self) -> &[FriendContext] {
        &self.friend_blocks
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.lineage.is_empty()
    }
}

impl FriendContext {
    /// Create a new friend context with the given point text and optional perspective.
    ///
    /// # Arguments
    /// * `point` - The friend block text.
    /// * `perspective` - Optional framing text describing how the target views this friend.
    /// * `parent_lineage_telescope` - Whether to include the friend block's parent lineage.
    /// * `children_telescope` - Whether to include the friend block's children.
    pub fn new(
        point: String, perspective: Option<String>, parent_lineage_telescope: bool,
        children_telescope: bool,
    ) -> Self {
        Self {
            point,
            perspective,
            parent_lineage_telescope,
            children_telescope,
            friend_lineage: None,
            friend_children: None,
        }
    }

    /// Create a new friend context with full context including lineage and children.
    pub fn with_context(
        point: String, perspective: Option<String>, parent_lineage_telescope: bool,
        children_telescope: bool, friend_lineage: Option<Lineage>,
        friend_children: Option<Vec<String>>,
    ) -> Self {
        Self {
            point,
            perspective,
            parent_lineage_telescope,
            children_telescope,
            friend_lineage,
            friend_children,
        }
    }

    pub fn point(&self) -> &str {
        &self.point
    }

    pub fn perspective(&self) -> Option<&str> {
        self.perspective.as_deref()
    }

    pub fn parent_lineage_telescope(&self) -> bool {
        self.parent_lineage_telescope
    }

    pub fn children_telescope(&self) -> bool {
        self.children_telescope
    }

    pub fn friend_lineage(&self) -> Option<&Lineage> {
        self.friend_lineage.as_ref()
    }

    pub fn friend_children(&self) -> Option<&[String]> {
        self.friend_children.as_deref()
    }
}

/// Ordered ancestor chain from root to a target block.
///
/// Used to give the LLM context about where in the document tree the
/// target point lives. The last item is always the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lineage {
    pub(crate) items: Vec<LineageItem>,
}

impl Lineage {
    /// Create a new lineage from a list of lineage items.
    pub fn new(items: Vec<LineageItem>) -> Self {
        Self { items }
    }

    /// Create a lineage from a list of point texts.
    ///
    /// Each point is wrapped in a `LineageItem`.
    pub fn from_points(points: Vec<String>) -> Self {
        Self::new(points.into_iter().map(LineageItem::new).collect())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &LineageItem> {
        self.items.iter()
    }

    /// Get an iterator over the lineage items.
    pub fn points(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(LineageItem::point)
    }
}

/// One element in a [`Lineage`] chain: wraps a block's point text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageItem {
    point: String,
}

impl LineageItem {
    /// Create a new lineage item wrapping the given point text.
    pub fn new(point: String) -> Self {
        Self { point }
    }

    pub(crate) fn point(&self) -> &str {
        &self.point
    }
}

/// One candidate child point returned from an expand request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandSuggestion {
    point: String,
}

impl ExpandSuggestion {
    /// Construct one suggestion with raw point text.
    pub fn new(point: String) -> Self {
        Self { point }
    }

    /// Consume and return the suggestion text.
    pub fn into_point(self) -> String {
        self.point
    }
}

/// Structured result returned by one expand request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandResult {
    rewrite: Option<String>,
    children: Vec<ExpandSuggestion>,
}

impl ExpandResult {
    /// Build an expand result from optional rewrite and children.
    pub fn new(rewrite: Option<String>, children: Vec<ExpandSuggestion>) -> Self {
        Self { rewrite, children }
    }

    /// Consume the result and return owned parts.
    pub fn into_parts(self) -> (Option<String>, Vec<ExpandSuggestion>) {
        (self.rewrite, self.children)
    }
}

/// Structured result returned by one reduce request.
///
/// Contains the condensed text plus 0-based indices of existing children
/// the LLM considers redundant (their content is captured by the reduction).
/// The caller maps these indices to `BlockId`s using the children snapshot
/// that was active at request time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReduceResult {
    reduction: String,
    /// 0-based indices into the `existing_children` that were sent in the prompt.
    redundant_children: Vec<usize>,
}

impl ReduceResult {
    pub fn new(reduction: String, redundant_children: Vec<usize>) -> Self {
        Self { reduction, redundant_children }
    }

    /// Consume and return owned parts.
    pub fn into_parts(self) -> (String, Vec<usize>) {
        (self.reduction, self.redundant_children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_suggestion_into_point() {
        let suggestion = ExpandSuggestion::new("text".into());
        assert_eq!(suggestion.into_point(), "text");
    }

    #[test]
    fn expand_result_into_parts_with_both() {
        let suggestion = ExpandSuggestion::new("child".into());
        let result = ExpandResult::new(Some("rewrite".into()), vec![suggestion]);
        let (rewrite, children) = result.into_parts();
        assert_eq!(rewrite, Some("rewrite".to_string()));
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], ExpandSuggestion::new("child".into()));
    }

    #[test]
    fn expand_result_into_parts_rewrite_only() {
        let result = ExpandResult::new(Some("rewrite".into()), vec![]);
        let (rewrite, children) = result.into_parts();
        assert_eq!(rewrite, Some("rewrite".to_string()));
        assert!(children.is_empty());
    }

    #[test]
    fn expand_result_into_parts_children_only() {
        let suggestion1 = ExpandSuggestion::new("child1".into());
        let suggestion2 = ExpandSuggestion::new("child2".into());
        let result = ExpandResult::new(None, vec![suggestion1, suggestion2]);
        let (rewrite, children) = result.into_parts();
        assert_eq!(rewrite, None);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn reduce_result_into_parts() {
        let result = ReduceResult::new("condensed".into(), vec![0, 2]);
        let (reduction, redundant) = result.into_parts();
        assert_eq!(reduction, "condensed");
        assert_eq!(redundant, vec![0, 2]);
    }

    #[test]
    fn reduce_result_empty_redundant() {
        let result = ReduceResult::new("text".into(), vec![]);
        let (_, redundant) = result.into_parts();
        assert!(redundant.is_empty());
    }

    #[test]
    fn lineage_from_points_creates_items() {
        let lineage = Lineage::from_points(vec!["a".into(), "b".into()]);
        let expected =
            Lineage::new(vec![LineageItem::new("a".into()), LineageItem::new("b".into())]);
        assert_eq!(lineage, expected);
    }

    #[test]
    fn lineage_empty() {
        let lineage = Lineage::from_points(vec![]);
        let expected = Lineage::new(vec![]);
        assert_eq!(lineage, expected);
    }

    #[test]
    fn lineage_from_points_roundtrip() {
        let lineage = Lineage::from_points(vec!["a".into()]);
        let expected = Lineage::new(vec![LineageItem::new("a".into())]);
        assert_eq!(lineage, expected);
    }

    #[test]
    fn block_context_empty_lineage_is_empty() {
        let ctx = BlockContext::new(Lineage::from_points(vec![]), vec![], vec![]);
        assert!(ctx.is_empty());
    }

    #[test]
    fn block_context_with_lineage_is_not_empty() {
        let ctx = BlockContext::new(Lineage::from_points(vec!["root".into()]), vec![], vec![]);
        assert!(!ctx.is_empty());
    }

    #[test]
    fn block_context_accessors() {
        let lineage = Lineage::from_points(vec!["root".into()]);
        let children = vec!["child_a".to_string(), "child_b".to_string()];
        let friends =
            vec![FriendContext::new("friend".to_string(), Some("ally".to_string()), true, true)];
        let ctx = BlockContext::new(lineage.clone(), children.clone(), friends.clone());
        assert_eq!(ctx.lineage(), &lineage);
        assert_eq!(ctx.existing_children(), &children[..]);
        assert_eq!(ctx.friend_blocks(), &friends[..]);
    }
}
