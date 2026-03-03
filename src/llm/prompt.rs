//! LLM prompt construction from context.
//!
//! Custom prompts support partial override: set only `system_prompt`, only
//! `user_prompt`, or both. Lineage and block context are always appended to
//! the user message.

use crate::llm::context::ContextFormatter;
use crate::llm::context::BlockContext;
#[cfg(test)]
use crate::llm::context::{FriendContext, LineageContext};

/// System + user prompt pair sent to the chat completions endpoint.
pub struct Prompt {
    pub(crate) system: String,
    pub(crate) user: String,
}

impl Prompt {
    /// Build a reduce prompt from block context.
    pub(crate) fn reduce_from_context(
        context: &BlockContext, instruction: Option<&str>, custom_system_prompt: Option<&str>,
        custom_user_prompt: Option<&str>,
    ) -> Self {
        let fmt = ContextFormatter::from_block_context(context);
        let presence = fmt.presence();

        let instruction_prefix = instruction.map(|i| format!("{}\n\n", i)).unwrap_or_default();

        let user = if let Some(custom) = custom_user_prompt {
            format!("{custom}\n\n{}", fmt.lineage_lines())
        } else {
            fmt.build_user_body("Reduce the target point with context:")
        };

        if let Some(system) = custom_system_prompt {
            return Self {
                system: format!("{instruction_prefix}{system}"),
                user,
            };
        }

        let context_qualifier = match (presence.has_children, presence.has_friends) {
            (false, false) => " as context",
            (false, true) => " plus friend blocks as context",
            (true, _) => ", existing children, and optional friend blocks as context",
        };
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
        let friends_explanation = if presence.has_friends {
            if presence.has_children {
                " Friend blocks are additional context only and must never appear in redundant_children. Friend blocks may include optional perspective text that can refine interpretation."
            } else {
                " Friend blocks are user-selected related context and are not children of the target. Each friend block may include an optional perspective describing how the target views that friend block; use it when helpful."
            }
        } else {
            ""
        };

        let system = format!(
            "You reduce a bullet point using its ancestors{context_qualifier}. Return strict JSON only: {json_schema}. The reduction must be a single concise sentence{reduction_qualifier}.{children_explanation}{friends_explanation} No markdown, no extra keys."
        );

        Self {
            system: format!("{instruction_prefix}{system}"),
            user,
        }
    }

    /// Build an expand prompt from block context.
    pub(crate) fn expand_from_context(
        context: &BlockContext, instruction: Option<&str>, custom_system_prompt: Option<&str>,
        custom_user_prompt: Option<&str>,
    ) -> Self {
        let fmt = ContextFormatter::from_block_context(context);
        let presence = fmt.presence();

        let instruction_prefix = instruction.map(|i| format!("{}\n\n", i)).unwrap_or_default();

        let user = if let Some(custom) = custom_user_prompt {
            format!("{custom}\n\n{}", fmt.lineage_lines())
        } else {
            fmt.build_user_body("Expand the target point with context:")
        };

        if let Some(system) = custom_system_prompt {
            return Self {
                system: format!("{instruction_prefix}{system}"),
                user,
            };
        }

        let context_qualifier = match (presence.has_children, presence.has_friends) {
            (false, false) => " as context",
            (false, true) => " plus friend blocks as context",
            (true, _) => ", existing children, and optional friend blocks as context",
        };
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
        let friends_explanation = if presence.has_friends {
            if presence.has_children {
                " Friend blocks are additional context only and are not children. Friend blocks may include optional perspective text that can refine interpretation."
            } else {
                " Friend blocks are user-selected related context and are not children of the target. Friend blocks may include an optional perspective describing how the target views that friend block; use it when relevant."
            }
        } else {
            ""
        };

        let system = format!(
            "You expand one target bullet point using its ancestors{context_qualifier}. Return strict JSON only with this shape: {{\"rewrite\": string|null, \"children\": string[]}}. Keep rewrite to one concise sentence.{children_qualifier} Children must be mutually non-overlapping, each focused on a distinct subtopic, and should not restate the rewrite{children_constraint}.{friends_explanation} No markdown, no extra keys."
        );

        Self {
            system: format!("{instruction_prefix}{system}"),
            user,
        }
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
        let fmt = ContextFormatter::from_block_context(context);

        let user = if let Some(custom) = custom_user_prompt {
            format!("{custom}\n\n{}\n\nInstruction: {instruction}", fmt.lineage_lines())
        } else {
            format!(
                "{}\n\nInstruction: {instruction}\n\nProvide a response that addresses the instruction.",
                fmt.build_user_body("Context:"),
                instruction = instruction
            )
        };

        let system = custom_system_prompt
            .map(String::from)
            .unwrap_or_else(|| {
                "You are a helpful writing assistant. Respond to the user's instruction based on the provided context.".to_string()
            });
        Self { system, user }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_prompt_labels_target_last() {
        let lineage = LineageContext::from_points(vec!["first".into(), "second".into(), "third".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::reduce_from_context(&context, None, None, None);
        assert!(prompt.user.contains("Parent: first"));
        assert!(prompt.user.contains("Parent: second"));
        assert!(prompt.user.contains("Target: third"));
    }

    #[test]
    fn expand_prompt_labels_target_last() {
        let lineage = LineageContext::from_points(vec!["first".into(), "second".into(), "third".into()]);
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
        let prompt =
            Prompt::reduce_from_context(&context, None, Some("Custom system"), None);
        assert!(prompt.system.contains("Custom system"));
        assert!(prompt.user.contains("Reduce the target point with context:"));
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
