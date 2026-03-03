//! LLM prompt construction from context.
//!
//! All three tasks (reduce, expand, inquire) receive the same block context:
//! lineage (Parent/Target), existing children, and friend blocks. Custom
//! prompts support partial override; the full context block is always appended.
//!
//! Construction is unified via [`PromptTask`]; task-specific defaults apply
//! per variant.

use crate::llm::context::BlockContext;
use crate::llm::context::{ContextFormatter, ContextPresence};
#[cfg(test)]
use crate::llm::context::{FriendContext, LineageContext};

/// System + user prompt pair sent to the chat completions endpoint.
pub struct Prompt {
    pub(crate) system: String,
    pub(crate) user: String,
}

/// LLM task kind; each variant supplies its own default prompt templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromptTask {
    Reduce,
    Expand,
    Inquire,
}

impl PromptTask {
    fn context_qualifier(presence: ContextPresence) -> &'static str {
        match (presence.has_children, presence.has_friends) {
            | (true, _) => ", existing children, and optional friend blocks as context",
            | (false, false) => " as context",
            | (false, true) => ", plus friend blocks as context",
        }
    }

    fn friends_explanation(self, presence: ContextPresence) -> &'static str {
        match (presence.has_friends, presence.has_children) {
            | (false, _) => "",
            | (true, true) => {
                " Friend blocks are additional context only and must never appear in redundant_children. Friend blocks may include optional perspective text that can refine interpretation."
            }
            | (true, false) => {
                " Friend blocks are user-selected related context and are not children of the target. Each friend blocks may include an optional perspective describing how the target views that friend block; use it when relevant."
            }
        }
    }

    fn default_system(self, presence: ContextPresence) -> String {
        match self {
            Self::Reduce => {
                let context_qualifier = Self::context_qualifier(presence);
                let (json_schema, reduction_qualifier) = if presence.has_children {
                    (r#"{"reduction": string, "redundant_children": number[]}"#, " that captures the essential meaning")
                } else {
                    (r#"{"reduction": string}"#, "")
                };
                let children_explanation = if presence.has_children {
                    " redundant_children: 0-based indices of existing children whose information is fully captured by the reduction and can be safely removed. Only mark a child redundant when its content is genuinely subsumed."
                } else {
                    ""
                };
                let friends_explanation = self.friends_explanation(presence);
                format!(
                    "You reduce a bullet point using its ancestors{context_qualifier}. Return strict JSON only: {json_schema}. The reduction must be a single concise sentence{reduction_qualifier}.{children_explanation}{friends_explanation} No markdown, no extra keys."
                )
            }
            Self::Expand => {
                let context_qualifier = Self::context_qualifier(presence);
                let children_qualifier = if presence.has_children {
                    " Generate 3-6 concise NEW child points."
                } else {
                    " Generate 3-6 concise child points."
                };
                let children_constraint = if presence.has_children {
                    ", and MUST NOT overlap with the existing children listed below"
                } else {
                    ""
                };
                let friends_explanation = self.friends_explanation(presence);
                format!(
                    "You expand one target bullet point using its ancestors{context_qualifier}. Return strict JSON only with this shape: {{\"rewrite\": string|null, \"children\": string[]}}. Keep rewrite to one concise sentence.{children_qualifier} Children must be mutually non-overlapping, each focused on a distinct subtopic, and should not restate the rewrite{children_constraint}.{friends_explanation} No markdown, no extra keys."
                )
            }
            Self::Inquire => "You are a helpful writing assistant. Respond to the user's instruction based on the provided context.".to_string(),
        }
    }

    fn default_user_intro(self) -> &'static str {
        match self {
            | Self::Reduce => "Reduce the target point with context:",
            | Self::Expand => "Expand the target point with context:",
            | Self::Inquire => "Context:",
        }
    }
}

impl Prompt {
    /// Build a prompt from block context using a unified construction flow.
    ///
    /// - `task`: reduce, expand, or inquire.
    /// - `instruction`: optional for reduce/expand (prefixed to system); required
    ///   for inquire (embedded in user message). Must be `Some` when `task` is
    ///   `Inquire`.
    /// - Custom prompts override defaults when non-empty; lineage is always appended.
    pub(crate) fn from_context(
        task: PromptTask, context: &BlockContext, instruction: Option<&str>,
        custom_system_prompt: Option<&str>, custom_user_prompt: Option<&str>,
    ) -> Self {
        let fmt = ContextFormatter::from_block_context(context);
        let presence = fmt.presence();
        let instruction_prefix = instruction.map(|i| format!("{}\n\n", i)).unwrap_or_default();

        let user = if let Some(custom) = custom_user_prompt {
            let context_block = fmt.format_context_block();
            match task {
                | PromptTask::Inquire => {
                    let instruction = instruction.expect("inquire requires instruction");
                    format!("{custom}\n\n{context_block}\n\nInstruction: {instruction}")
                }
                | _ => format!("{custom}\n\n{context_block}"),
            }
        } else {
            match task {
                | PromptTask::Inquire => {
                    let instruction = instruction.expect("inquire requires instruction");
                    format!(
                        "{}\n\nInstruction: {instruction}\n\nProvide a response that addresses the instruction.",
                        fmt.build_user_body(task.default_user_intro()),
                        instruction = instruction
                    )
                }
                | _ => fmt.build_user_body(task.default_user_intro()),
            }
        };

        let system =
            custom_system_prompt.map(String::from).unwrap_or_else(|| task.default_system(presence));
        let system = match task {
            | PromptTask::Inquire => system,
            | _ => format!("{instruction_prefix}{system}"),
        };

        Self { system, user }
    }

    /// Build a reduce prompt from block context.
    pub(crate) fn reduce_from_context(
        context: &BlockContext, instruction: Option<&str>, custom_system_prompt: Option<&str>,
        custom_user_prompt: Option<&str>,
    ) -> Self {
        Self::from_context(
            PromptTask::Reduce,
            context,
            instruction,
            custom_system_prompt,
            custom_user_prompt,
        )
    }

    /// Build an expand prompt from block context.
    pub(crate) fn expand_from_context(
        context: &BlockContext, instruction: Option<&str>, custom_system_prompt: Option<&str>,
        custom_user_prompt: Option<&str>,
    ) -> Self {
        Self::from_context(
            PromptTask::Expand,
            context,
            instruction,
            custom_system_prompt,
            custom_user_prompt,
        )
    }

    /// Build a prompt for a one-time instruction inquiry.
    ///
    /// The inquiry prompt includes the block's lineage, direct children, and
    /// friend blocks as context, followed by the user's instruction. The
    /// response is a free-form text answer that can be applied as a rewrite to
    /// the block's point.
    pub(crate) fn inquire_from_context(
        context: &BlockContext, instruction: &str, custom_system_prompt: Option<&str>,
        custom_user_prompt: Option<&str>,
    ) -> Self {
        Self::from_context(
            PromptTask::Inquire,
            context,
            Some(instruction),
            custom_system_prompt,
            custom_user_prompt,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_prompt_labels_target_last() {
        let lineage =
            LineageContext::from_points(vec!["first".into(), "second".into(), "third".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::reduce_from_context(&context, None, None, None);
        assert!(prompt.user.contains("Parent: first"));
        assert!(prompt.user.contains("Parent: second"));
        assert!(prompt.user.contains("Target: third"));
    }

    #[test]
    fn expand_prompt_labels_target_last() {
        let lineage =
            LineageContext::from_points(vec!["first".into(), "second".into(), "third".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::expand_from_context(&context, None, None, None);
        assert!(prompt.user.contains("Parent: first"));
        assert!(prompt.user.contains("Parent: second"));
        assert!(prompt.user.contains("Target: third"));
    }

    #[test]
    fn expand_prompt_mentions_concise_and_non_overlapping_constraints() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::expand_from_context(&context, None, None, None);
        assert!(prompt.system.contains("one concise sentence"));
        assert!(prompt.system.contains("mutually non-overlapping"));
        assert!(prompt.system.contains("distinct subtopic"));
        assert!(prompt.system.contains("should not restate the rewrite"));
    }

    #[test]
    fn expand_prompt_includes_existing_children() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let children = vec!["existing child A".to_string(), "existing child B".to_string()];
        let ctx = BlockContext::new(lineage, children, vec![]);
        let prompt = Prompt::expand_from_context(&ctx, None, None, None);
        assert!(prompt.user.contains("Existing children:"));
        assert!(prompt.user.contains("[0] existing child A"));
        assert!(prompt.user.contains("[1] existing child B"));
        assert!(prompt.system.contains("MUST NOT overlap with the existing children"));
    }

    #[test]
    fn expand_prompt_without_children_omits_section() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let ctx = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::expand_from_context(&ctx, None, None, None);
        assert!(!prompt.user.contains("Existing children:"));
    }

    #[test]
    fn reduce_prompt_includes_existing_children() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let children = vec!["child A".to_string()];
        let ctx = BlockContext::new(lineage, children, vec![]);
        let prompt = Prompt::reduce_from_context(&ctx, None, None, None);
        assert!(prompt.user.contains("Existing children:"));
        assert!(prompt.user.contains("[0] child A"));
        assert!(prompt.system.contains("redundant_children"));
    }

    #[test]
    fn reduce_prompt_without_children_is_plain() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let ctx = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::reduce_from_context(&ctx, None, None, None);
        assert!(!prompt.user.contains("Existing children:"));
    }

    #[test]
    fn expand_prompt_includes_friend_blocks() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let friends = vec![
            FriendContext::with_context(
                "peer concept A".to_string(),
                Some("historical lens".to_string()),
                true,
                true,
                None,
                None,
            ),
            FriendContext::with_context("peer concept B".to_string(), None, true, true, None, None),
        ];
        let ctx = BlockContext::new(lineage, vec![], friends);
        let prompt = Prompt::expand_from_context(&ctx, None, None, None);
        assert!(prompt.user.contains("Friend blocks:"));
        assert!(prompt.user.contains("[0] peer concept A (perspective: historical lens)"));
        assert!(prompt.user.contains("[1] peer concept B"));
    }

    #[test]
    fn reduce_prompt_includes_friend_blocks() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let friends = vec![FriendContext::with_context(
            "supporting external detail".to_string(),
            Some("skeptical counterpoint".to_string()),
            true,
            true,
            None,
            None,
        )];
        let ctx = BlockContext::new(lineage, vec![], friends);
        let prompt = Prompt::reduce_from_context(&ctx, None, None, None);
        assert!(prompt.user.contains("Friend blocks:"));
        assert!(
            prompt
                .user
                .contains("[0] supporting external detail (perspective: skeptical counterpoint)")
        );
    }

    #[test]
    fn inquire_prompt_includes_existing_children() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let ctx = BlockContext::new(
            lineage,
            vec!["child one".to_string(), "child two".to_string()],
            vec![],
        );
        let prompt = Prompt::inquire_from_context(&ctx, "answer this", None, None);
        assert!(prompt.user.contains("Existing children:"));
        assert!(prompt.user.contains("[0] child one"));
        assert!(prompt.user.contains("[1] child two"));
    }

    #[test]
    fn inquire_prompt_omits_existing_children_when_empty() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let ctx = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::inquire_from_context(&ctx, "answer this", None, None);
        assert!(!prompt.user.contains("Existing children:"));
    }

    #[test]
    fn custom_prompts_override_defaults() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::reduce_from_context(
            &context,
            None,
            Some("Custom system prompt"),
            Some("Custom user prompt"),
        );
        assert!(prompt.system.contains("Custom system prompt"));
        assert!(prompt.user.contains("Custom user prompt"));
    }

    #[test]
    fn custom_system_only_uses_default_user() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::reduce_from_context(&context, None, Some("Custom system"), None);
        assert!(prompt.system.contains("Custom system"));
        assert!(prompt.user.contains("Reduce the target point with context:"));
    }

    #[test]
    fn custom_user_prompts_include_full_context_block() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let children = vec!["child A".to_string()];
        let friends = vec![FriendContext::with_context(
            "friend block".to_string(),
            Some("perspective".to_string()),
            true,
            true,
            None,
            None,
        )];
        let ctx = BlockContext::new(lineage, children, friends);
        let prompt =
            Prompt::inquire_from_context(&ctx, "answer this", None, Some("Custom user preamble"));
        assert!(prompt.user.contains("Custom user preamble"));
        assert!(prompt.user.contains("Existing children:"));
        assert!(prompt.user.contains("[0] child A"));
        assert!(prompt.user.contains("Friend blocks:"));
        assert!(prompt.user.contains("friend block"));
        assert!(prompt.user.contains("perspective"));
    }

    #[test]
    fn custom_user_only_uses_default_system() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let prompt =
            Prompt::reduce_from_context(&context, None, None, Some("Custom user preamble"));
        assert!(prompt.system.contains("You reduce a bullet point"));
        assert!(prompt.user.contains("Custom user preamble"));
    }
}
