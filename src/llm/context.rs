//! LLM context types and formatting for prompt construction.
//!
//! Includes block context, lineage, friend blocks, result types, and
//! [`ContextFormatter`] for converting context into prompt-ready strings.

/// Immutable snapshot of a block's LLM-relevant context: ancestor lineage,
/// existing child point texts, and user-selected friend blocks.
///
/// The target block point is represented by the final lineage item.
/// Therefore one `BlockContext` captures the full readable context envelope:
/// target point, parent chain, direct children, and friend blocks.
///
/// Constructed by the store layer; consumed by [`LlmClient`] methods.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BlockContext {
    pub lineage: LineageContext,
    pub existing_children: ChildrenContext,
    pub friend_blocks: Vec<FriendContext>,
}

impl BlockContext {
    /// Create a new block context with the given lineage, existing children, and friend blocks.
    ///
    /// # Requires
    /// - `lineage` should represent the path from root to the target block.
    pub fn new(
        lineage: LineageContext, children: impl Into<ChildrenContext>,
        friend_blocks: Vec<FriendContext>,
    ) -> Self {
        Self { lineage, existing_children: children.into(), friend_blocks }
    }

    /// Get a reference to the lineage (ancestor chain).
    pub fn lineage(&self) -> &LineageContext {
        &self.lineage
    }

    /// Get the existing child contexts.
    pub fn existing_children(&self) -> &ChildrenContext {
        &self.existing_children
    }

    pub fn friend_blocks(&self) -> &[FriendContext] {
        &self.friend_blocks
    }

    pub fn is_empty(&self) -> bool {
        self.lineage.is_empty()
    }
}

/// Ordered ancestor chain from root to a target block.
///
/// Used to give the LLM context about where in the document tree the
/// target point lives. The last item is always the target.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LineageContext {
    pub items: Vec<LineageItem>,
}

impl LineageContext {
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

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &LineageItem> {
        self.items.iter()
    }

    /// Get an iterator over the lineage items.
    pub fn points(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(LineageItem::point)
    }
}

/// One element in a [`Lineage`] chain: wraps a block's point text.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LineageItem {
    point: String,
}

impl LineageItem {
    /// Create a new lineage item wrapping the given point text.
    pub fn new(point: String) -> Self {
        Self { point }
    }

    pub fn point(&self) -> &str {
        &self.point
    }
}

/// One child block's point text in a [`ChildrenContext`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildContext {
    point: String,
}

impl ChildContext {
    /// Create from a child point text.
    pub fn new(point: String) -> Self {
        Self { point }
    }

    /// The child's point text.
    pub fn point(&self) -> &str {
        &self.point
    }
}

impl From<String> for ChildContext {
    fn from(point: String) -> Self {
        Self::new(point)
    }
}

/// Direct child point texts of the target block.
///
/// Carries only point texts (no `BlockId`s) so this module stays decoupled
/// from store identity types.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChildrenContext(Vec<ChildContext>);

impl ChildrenContext {
    /// Create from a list of child point texts.
    pub fn from_points(points: Vec<String>) -> Self {
        Self(points.into_iter().map(ChildContext::new).collect())
    }

    /// Whether there are no child points.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Number of child points.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Iterate over child point texts.
    pub fn point_strs(&self) -> impl Iterator<Item = &str> + '_ {
        self.0.iter().map(ChildContext::point)
    }

    /// Consume and return the inner point texts.
    pub fn into_points(self) -> Vec<String> {
        self.0.into_iter().map(|c| c.point().to_string()).collect()
    }
}

impl From<Vec<String>> for ChildrenContext {
    fn from(points: Vec<String>) -> Self {
        Self::from_points(points)
    }
}

impl From<ChildrenContext> for Vec<String> {
    fn from(cc: ChildrenContext) -> Self {
        cc.into_points()
    }
}

impl serde::Serialize for ChildrenContext {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for point in self.point_strs() {
            seq.serialize_element(point)?;
        }
        seq.end()
    }
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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct FriendContext {
    pub point: String,
    pub perspective: Option<String>,
    pub parent_lineage_telescope: bool,
    pub children_telescope: bool,
    pub friend_lineage: Option<LineageContext>,
    pub friend_children: Option<ChildrenContext>,
}

impl FriendContext {
    /// Create a new friend context with full context including lineage and children.
    pub fn with_context(
        point: String, perspective: Option<String>, parent_lineage_telescope: bool,
        children_telescope: bool, friend_lineage: Option<LineageContext>,
        friend_children: Option<ChildrenContext>,
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

    pub fn friend_lineage(&self) -> Option<&LineageContext> {
        self.friend_lineage.as_ref()
    }

    pub fn friend_children(&self) -> Option<&ChildrenContext> {
        self.friend_children.as_ref()
    }
}

/// One candidate child point returned from an expand request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmplifySuggestion {
    point: String,
}

impl AmplifySuggestion {
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
pub struct AmplifyResult {
    rewrite: Option<String>,
    children: Vec<AmplifySuggestion>,
}

impl AmplifyResult {
    /// Build an amplify result from optional rewrite and children.
    pub fn new(rewrite: Option<String>, children: Vec<AmplifySuggestion>) -> Self {
        Self { rewrite, children }
    }

    /// Consume the result and return owned parts.
    pub fn into_parts(self) -> (Option<String>, Vec<AmplifySuggestion>) {
        (self.rewrite, self.children)
    }
}

/// Structured result returned by one atomize request.
///
/// Contains an optional rewrite of the original text plus distinct information
/// points. The rewrite may summarize or restructure the source; points are the
/// decomposed facts/ideas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtomizeResult {
    rewrite: Option<String>,
    points: Vec<String>,
}

impl AtomizeResult {
    /// Build from parsed rewrite and points.
    pub fn new(rewrite: Option<String>, points: Vec<String>) -> Self {
        Self { rewrite, points }
    }

    /// Consume and return owned rewrite and points.
    pub fn into_parts(self) -> (Option<String>, Vec<String>) {
        (self.rewrite, self.points)
    }
}

/// Structured result returned by one reduce request.
///
/// Contains the condensed text plus 0-based indices of existing children
/// the LLM considers redundant (their content is captured by the reduction).
/// The caller maps these indices to `BlockId`s using the children snapshot
/// that was active at request time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistillResult {
    reduction: String,
    /// 0-based indices into the `existing_children` that were sent in the prompt.
    redundant_children: Vec<usize>,
}

impl DistillResult {
    pub fn new(reduction: String, redundant_children: Vec<usize>) -> Self {
        Self { reduction, redundant_children }
    }

    /// Consume and return owned parts.
    pub fn into_parts(self) -> (String, Vec<usize>) {
        (self.reduction, self.redundant_children)
    }
}

/// Indicates which optional context sections are present when building prompts.
#[derive(Clone, Copy, Debug)]
pub struct ContextPresence {
    pub has_children: bool,
    pub has_friends: bool,
}

impl From<&BlockContext> for ContextPresence {
    fn from(ctx: &BlockContext) -> Self {
        Self {
            has_children: !ctx.existing_children.is_empty(),
            has_friends: !ctx.friend_blocks.is_empty(),
        }
    }
}

/// Context holder that formats raw lineage, BlockContext children, and friends for prompt construction.
///
/// Built via [`ContextFormatter::from_block_context`] or [`ContextFormatterBuilder`].
/// Holds raw structural data and formats on demand.
#[derive(Debug)]
pub struct ContextFormatter {
    lineage: LineageContext,
    children: ChildrenContext,
    friends: Vec<FriendContext>,
}

impl ContextFormatter {
    /// Create from a block context (primary entry point).
    pub fn from_block_context(ctx: &BlockContext) -> Self {
        Self::new(ctx.lineage.clone())
            .with_children(ctx.existing_children.clone())
            .with_friends(ctx.friend_blocks.clone())
            .build()
    }

    /// Start building with lineage. Use `with_children` and `with_friends` to add optional sections.
    pub fn new(lineage: LineageContext) -> ContextFormatterBuilder {
        ContextFormatterBuilder::new(lineage)
    }

    /// Which optional sections are present.
    pub fn presence(&self) -> ContextPresence {
        ContextPresence {
            has_children: !self.children.is_empty(),
            has_friends: !self.friends.is_empty(),
        }
    }

    /// Formatted lineage lines (Parent / Target labels).
    pub fn lineage_lines(&self) -> String {
        self.fmt_lineage()
    }

    /// Full block context: lineage, existing children, and friend blocks.
    ///
    /// Same structure for all LLM tasks. Use when building user prompts so
    /// reduce, expand, and inquire all receive identical context.
    pub fn format_context_block(&self) -> String {
        let mut s = self.fmt_lineage();
        if self.presence().has_children {
            s.push_str(&format!("\nExisting children:\n{}", self.fmt_children()));
        }
        if self.presence().has_friends {
            s.push_str(&format!("\nFriend blocks:\n{}", self.fmt_friends()));
        }
        s
    }

    /// Iterate over lineage point texts.
    pub fn lineage_points(&self) -> impl Iterator<Item = &str> + '_ {
        self.lineage.points()
    }

    /// Child contexts.
    pub fn children(&self) -> &ChildrenContext {
        &self.children
    }

    /// Number of friend blocks.
    pub fn friends_count(&self) -> usize {
        self.friends.len()
    }

    /// Build the full user prompt body for a task (e.g. reduce or expand).
    pub fn build_user_body(&self, task_intro: &str) -> String {
        format!("{task_intro}\n{}", self.format_context_block())
    }

    /// Full human-readable format for CLI display.
    ///
    /// Multi-line output with lineage (Parent/Target labels), numbered children,
    /// and friend blocks with optional lineage, children, and perspective.
    pub fn format_for_display(&self) -> String {
        let mut s = self.fmt_lineage().trim_end().to_string();
        if self.presence().has_children {
            s.push_str("\n\nChildren\n");
            s.push_str(&self.fmt_children());
        }
        if self.presence().has_friends {
            s.push_str("\n\nFriends\n");
            s.push_str(&self.fmt_friends());
        }
        s
    }

    fn fmt_lineage(&self) -> String {
        let mut lines = String::new();
        let total = self.lineage.items.len();
        for (index, item) in self.lineage.iter().enumerate() {
            let label = if index + 1 == total { "Target" } else { "Parent" };
            lines.push_str(&format!("{label}: {}\n", item.point()));
        }
        lines
    }

    fn fmt_children(&self) -> String {
        let mut lines = String::new();
        for (index, point) in self.children.point_strs().enumerate() {
            lines.push_str(&format!("[{index}] {point}\n"));
        }
        lines
    }

    fn fmt_friends(&self) -> String {
        let mut lines = String::new();
        for (index, friend_block) in self.friends.iter().enumerate() {
            let mut line = format!("[{}] {}", index, friend_block.point());

            if friend_block.parent_lineage_telescope {
                if let Some(lineage) = friend_block.friend_lineage() {
                    let lineage_str = lineage.points().collect::<Vec<_>>().join(" > ");
                    line.push_str(&format!(" (lineage: {})", lineage_str));
                }
            }

            if friend_block.children_telescope {
                if let Some(children) = friend_block.friend_children() {
                    if !children.is_empty() {
                        let children_str = children.point_strs().collect::<Vec<_>>().join("; ");
                        line.push_str(&format!(" (children: {})", children_str));
                    }
                }
            }

            if let Some(perspective) = friend_block.perspective() {
                line.push_str(&format!(" (perspective: {})", perspective));
            }

            lines.push_str(&line);
            lines.push('\n');
        }
        lines
    }
}

/// Builder for [`ContextFormatter`].
pub struct ContextFormatterBuilder {
    lineage: LineageContext,
    children: ChildrenContext,
    friends: Vec<FriendContext>,
}

impl ContextFormatterBuilder {
    fn new(lineage: LineageContext) -> Self {
        Self { lineage, children: ChildrenContext::default(), friends: Vec::new() }
    }

    /// Add existing children (child contexts).
    pub fn with_children(mut self, children: impl Into<ChildrenContext>) -> Self {
        self.children = children.into();
        self
    }

    /// Add friend blocks (user-selected related context).
    pub fn with_friends(mut self, friends: Vec<FriendContext>) -> Self {
        self.friends = friends;
        self
    }

    /// Produce the context formatter with raw structural data.
    pub fn build(self) -> ContextFormatter {
        ContextFormatter { lineage: self.lineage, children: self.children, friends: self.friends }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amplify_suggestion_into_point() {
        let suggestion = AmplifySuggestion::new("text".into());
        assert_eq!(suggestion.into_point(), "text");
    }

    #[test]
    fn amplify_result_into_parts_with_both() {
        let suggestion = AmplifySuggestion::new("child".into());
        let result = AmplifyResult::new(Some("rewrite".into()), vec![suggestion]);
        let (rewrite, children) = result.into_parts();
        assert_eq!(rewrite, Some("rewrite".to_string()));
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], AmplifySuggestion::new("child".into()));
    }

    #[test]
    fn amplify_result_into_parts_rewrite_only() {
        let result = AmplifyResult::new(Some("rewrite".into()), vec![]);
        let (rewrite, children) = result.into_parts();
        assert_eq!(rewrite, Some("rewrite".to_string()));
        assert!(children.is_empty());
    }

    #[test]
    fn amplify_result_into_parts_children_only() {
        let suggestion1 = AmplifySuggestion::new("child1".into());
        let suggestion2 = AmplifySuggestion::new("child2".into());
        let result = AmplifyResult::new(None, vec![suggestion1, suggestion2]);
        let (rewrite, children) = result.into_parts();
        assert_eq!(rewrite, None);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn atomize_result_into_parts_with_both() {
        let result = AtomizeResult::new(Some("heading".into()), vec!["a".into(), "b".into()]);
        let (rewrite, points) = result.into_parts();
        assert_eq!(rewrite, Some("heading".to_string()));
        assert_eq!(points, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn atomize_result_into_parts_rewrite_only() {
        let result = AtomizeResult::new(Some("restated".into()), vec![]);
        let (rewrite, points) = result.into_parts();
        assert_eq!(rewrite, Some("restated".to_string()));
        assert!(points.is_empty());
    }

    #[test]
    fn atomize_result_into_parts_points_only() {
        let result = AtomizeResult::new(None, vec!["p1".into()]);
        let (rewrite, points) = result.into_parts();
        assert_eq!(rewrite, None);
        assert_eq!(points, vec!["p1".to_string()]);
    }

    #[test]
    fn distill_result_into_parts() {
        let result = DistillResult::new("condensed".into(), vec![0, 2]);
        let (reduction, redundant) = result.into_parts();
        assert_eq!(reduction, "condensed");
        assert_eq!(redundant, vec![0, 2]);
    }

    #[test]
    fn distill_result_empty_redundant() {
        let result = DistillResult::new("text".into(), vec![]);
        let (_, redundant) = result.into_parts();
        assert!(redundant.is_empty());
    }

    #[test]
    fn lineage_from_points_creates_items() {
        let lineage = LineageContext::from_points(vec!["a".into(), "b".into()]);
        let expected =
            LineageContext::new(vec![LineageItem::new("a".into()), LineageItem::new("b".into())]);
        assert_eq!(lineage, expected);
    }

    #[test]
    fn lineage_empty() {
        let lineage = LineageContext::from_points(vec![]);
        let expected = LineageContext::new(vec![]);
        assert_eq!(lineage, expected);
    }

    #[test]
    fn lineage_from_points_roundtrip() {
        let lineage = LineageContext::from_points(vec!["a".into()]);
        let expected = LineageContext::new(vec![LineageItem::new("a".into())]);
        assert_eq!(lineage, expected);
    }

    #[test]
    fn block_context_empty_lineage_is_empty() {
        let ctx = BlockContext::new(LineageContext::from_points(vec![]), vec![], vec![]);
        assert!(ctx.is_empty());
    }

    #[test]
    fn block_context_with_lineage_is_not_empty() {
        let ctx =
            BlockContext::new(LineageContext::from_points(vec!["root".into()]), vec![], vec![]);
        assert!(!ctx.is_empty());
    }

    #[test]
    fn block_context_accessors() {
        let lineage = LineageContext::from_points(vec!["root".into()]);
        let children = vec!["child_a".to_string(), "child_b".to_string()];
        let friends = vec![FriendContext::with_context(
            "friend".to_string(),
            Some("ally".to_string()),
            true,
            true,
            None,
            None,
        )];
        let ctx = BlockContext::new(lineage.clone(), children.clone(), friends.clone());
        assert_eq!(ctx.lineage(), &lineage);
        assert!(ctx.existing_children().point_strs().eq(children.iter().map(String::as_str)));
        assert_eq!(ctx.friend_blocks(), &friends[..]);
    }

    #[test]
    fn context_formatter_builder_produces_same_output_as_from_block_context() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let children = vec!["child".to_string()];
        let friends = vec![FriendContext::with_context(
            "friend".to_string(),
            Some("perspective".to_string()),
            true,
            true,
            None,
            None,
        )];

        let ctx = BlockContext::new(lineage.clone(), children.clone(), friends.clone());
        let from_ctx = ContextFormatter::from_block_context(&ctx);
        let from_builder =
            ContextFormatter::new(lineage).with_children(children).with_friends(friends).build();

        assert_eq!(from_ctx.lineage_lines(), from_builder.lineage_lines());
        assert_eq!(from_ctx.presence().has_children, from_builder.presence().has_children);
        assert_eq!(from_ctx.presence().has_friends, from_builder.presence().has_friends);
        assert_eq!(from_ctx.build_user_body("Task:"), from_builder.build_user_body("Task:"));
    }
}
