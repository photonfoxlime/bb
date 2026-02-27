//! LLM prompt construction from context.

use crate::llm::context::{BlockContext, FriendContext, Lineage};

/// System + user prompt pair sent to the chat completions endpoint.
pub struct Prompt {
    pub(crate) system: String,
    pub(crate) user: String,
}

impl Prompt {
    /// Format lineage items as labeled lines.
    fn format_lineage_lines(lineage: &Lineage) -> String {
        let mut lines = String::new();
        let total = lineage.items.len();
        for (index, item) in lineage.iter().enumerate() {
            let label = if index + 1 == total { "Target" } else { "Parent" };
            lines.push_str(&format!("{label}: {}\n", item.point()));
        }
        lines
    }

    /// Format existing children as indexed lines.
    fn format_children_lines(children: &[String]) -> String {
        let mut lines = String::new();
        for (index, child) in children.iter().enumerate() {
            lines.push_str(&format!("[{index}] {child}\n"));
        }
        lines
    }

    fn format_friend_blocks_lines(friend_blocks: &[FriendContext]) -> String {
        let mut lines = String::new();
        for (index, friend_block) in friend_blocks.iter().enumerate() {
            let mut line = format!("[{}] {}", index, friend_block.point());

            // Add lineage if visible
            if friend_block.parent_lineage_telescope {
                if let Some(lineage) = friend_block.friend_lineage() {
                    let lineage_str = lineage.points().collect::<Vec<_>>().join(" > ");
                    line.push_str(&format!(" (lineage: {})", lineage_str));
                }
            }

            // Add children if visible
            if friend_block.children_telescope {
                if let Some(children) = friend_block.friend_children() {
                    if !children.is_empty() {
                        let children_str = children.join("; ");
                        line.push_str(&format!(" (children: {})", children_str));
                    }
                }
            }

            // Add perspective
            if let Some(perspective) = friend_block.perspective() {
                line.push_str(&format!(" (perspective: {})", perspective));
            }

            lines.push_str(&line);
            lines.push('\n');
        }
        lines
    }

    /// Build a reduce prompt from block context.
    pub(crate) fn reduce_from_context(context: &BlockContext, instruction: Option<&str>) -> Self {
        let lineage_lines = Self::format_lineage_lines(&context.lineage);
        let friend_lines = Self::format_friend_blocks_lines(&context.friend_blocks);

        let instruction_prefix = instruction.map(|i| format!("{}\n\n", i)).unwrap_or_default();

        if context.existing_children.is_empty() && context.friend_blocks.is_empty() {
            return Self {
                system: format!(
                    "{}You reduce a bullet point using its ancestors as context. Return strict JSON only: {{\"reduction\": string}}. The reduction must be a single concise sentence. No markdown, no extra keys.",
                    instruction_prefix
                ),
                user: format!("Reduce the target point with context:\n{lineage_lines}"),
            };
        }

        if context.existing_children.is_empty() {
            return Self {
                system: format!(
                    "{}You reduce a bullet point using its ancestors plus friend blocks as context. Return strict JSON only: {{\"reduction\": string}}. The reduction must be a single concise sentence. Friend blocks are user-selected related context and are not children of the target. Each friend block may include an optional perspective describing how the target views that friend block; use it when helpful. No markdown, no extra keys.",
                    instruction_prefix
                ),
                user: format!(
                    "Reduce the target point with context:\n{lineage_lines}\nFriend blocks:\n{friend_lines}"
                ),
            };
        }

        let children_lines = Self::format_children_lines(&context.existing_children);
        let friend_context = if context.friend_blocks.is_empty() {
            String::new()
        } else {
            format!("\nFriend blocks:\n{friend_lines}")
        };
        Self {
            system: format!(
                "{}You reduce a bullet point using its ancestors, existing children, and optional friend blocks as context. Return strict JSON only: {{\"reduction\": string, \"redundant_children\": number[]}}. The reduction must be a single concise sentence that captures the essential meaning. redundant_children: 0-based indices of existing children whose information is fully captured by the reduction and can be safely removed. Friend blocks are additional context only and must never appear in redundant_children. Friend blocks may include optional perspective text that can refine interpretation. Only mark a child redundant when its content is genuinely subsumed. No markdown, no extra keys.",
                instruction_prefix
            ),
            user: format!(
                "Reduce the target point with context:\n{lineage_lines}\nExisting children:\n{children_lines}{friend_context}"
            ),
        }
    }

    /// Build an expand prompt from block context.
    pub(crate) fn expand_from_context(context: &BlockContext, instruction: Option<&str>) -> Self {
        let lineage_lines = Self::format_lineage_lines(&context.lineage);
        let friend_lines = Self::format_friend_blocks_lines(&context.friend_blocks);

        let instruction_prefix = instruction.map(|i| format!("{}\n\n", i)).unwrap_or_default();

        if context.existing_children.is_empty() && context.friend_blocks.is_empty() {
            return Self {
                system: format!(
                    "{}You expand one target bullet point using its ancestors as context. Return strict JSON only with this shape: {{\"rewrite\": string|null, \"children\": string[]}}. Keep rewrite to one concise sentence. Generate 3-6 concise child points. Children must be mutually non-overlapping, each focused on a distinct subtopic, and should not restate the rewrite. No markdown, no extra keys.",
                    instruction_prefix
                ),
                user: format!("Expand the target point with context:\n{lineage_lines}"),
            };
        }

        if context.existing_children.is_empty() {
            return Self {
                system: format!(
                    "{}You expand one target bullet point using its ancestors plus friend blocks as context. Return strict JSON only with this shape: {{\"rewrite\": string|null, \"children\": string[]}}. Keep rewrite to one concise sentence. Generate 3-6 concise child points. Children must be mutually non-overlapping, each focused on a distinct subtopic, and should not restate the rewrite. Friend blocks are user-selected related context and are not children of the target. Friend blocks may include an optional perspective describing how the target views that friend block; use it when relevant. No markdown, no extra keys.",
                    instruction_prefix
                ),
                user: format!(
                    "Expand the target point with context:\n{lineage_lines}\nFriend blocks:\n{friend_lines}"
                ),
            };
        }

        let children_lines = Self::format_children_lines(&context.existing_children);
        let friend_context = if context.friend_blocks.is_empty() {
            String::new()
        } else {
            format!("\nFriend blocks:\n{friend_lines}")
        };
        Self {
            system: format!(
                "{}You expand one target bullet point using its ancestors, existing children, and optional friend blocks as context. Return strict JSON only with this shape: {{\"rewrite\": string|null, \"children\": string[]}}. Keep rewrite to one concise sentence. Generate 3-6 concise NEW child points. Children must be mutually non-overlapping, each focused on a distinct subtopic, should not restate the rewrite, and MUST NOT overlap with the existing children listed below. Friend blocks are additional context only and are not children. Friend blocks may include optional perspective text that can refine interpretation. No markdown, no extra keys.",
                instruction_prefix
            ),
            user: format!(
                "Expand the target point with context:\n{lineage_lines}\nExisting children:\n{children_lines}{friend_context}"
            ),
        }
    }

    /// Build a prompt for a one-time instruction inquiry.
    ///
    /// The inquiry prompt includes the block's lineage and friend blocks as context,
    /// followed by the user's instruction. The response is a free-form text answer
    /// that can be applied as a rewrite to the block's point.
    pub(crate) fn inquire_from_context(context: &BlockContext, instruction: &str) -> Self {
        let lineage_lines = Self::format_lineage_lines(&context.lineage);
        let friend_lines = Self::format_friend_blocks_lines(&context.friend_blocks);

        let friend_context = if context.friend_blocks.is_empty() {
            String::new()
        } else {
            format!("\nFriend blocks:\n{friend_lines}")
        };

        Self {
            system: "You are a helpful writing assistant. Respond to the user's instruction based on the provided context.".to_string(),
            user: format!(
                "Context:\n{lineage_lines}{friend_context}\n\nInstruction: {instruction}\n\nProvide a response that addresses the instruction."
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_prompt_labels_target_last() {
        let lineage = Lineage::from_points(vec!["first".into(), "second".into(), "third".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::reduce_from_context(&context, None);
        assert!(prompt.user.contains("Parent: first"));
        assert!(prompt.user.contains("Parent: second"));
        assert!(prompt.user.contains("Target: third"));
    }

    #[test]
    fn expand_prompt_labels_target_last() {
        let lineage = Lineage::from_points(vec!["first".into(), "second".into(), "third".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::expand_from_context(&context, None);
        assert!(prompt.user.contains("Parent: first"));
        assert!(prompt.user.contains("Parent: second"));
        assert!(prompt.user.contains("Target: third"));
    }

    #[test]
    fn expand_prompt_mentions_concise_and_non_overlapping_constraints() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::expand_from_context(&context, None);
        assert!(prompt.system.contains("one concise sentence"));
        assert!(prompt.system.contains("mutually non-overlapping"));
        assert!(prompt.system.contains("distinct subtopic"));
        assert!(prompt.system.contains("should not restate the rewrite"));
    }

    #[test]
    fn expand_prompt_includes_existing_children() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
        let children = vec!["existing child A".to_string(), "existing child B".to_string()];
        let ctx = BlockContext::new(lineage, children, vec![]);
        let prompt = Prompt::expand_from_context(&ctx, None);
        assert!(prompt.user.contains("Existing children:"));
        assert!(prompt.user.contains("[0] existing child A"));
        assert!(prompt.user.contains("[1] existing child B"));
        assert!(prompt.system.contains("MUST NOT overlap with the existing children"));
    }

    #[test]
    fn expand_prompt_without_children_omits_section() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
        let ctx = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::expand_from_context(&ctx, None);
        assert!(!prompt.user.contains("Existing children:"));
    }

    #[test]
    fn reduce_prompt_includes_existing_children() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
        let children = vec!["child A".to_string()];
        let ctx = BlockContext::new(lineage, children, vec![]);
        let prompt = Prompt::reduce_from_context(&ctx, None);
        assert!(prompt.user.contains("Existing children:"));
        assert!(prompt.user.contains("[0] child A"));
        assert!(prompt.system.contains("redundant_children"));
    }

    #[test]
    fn reduce_prompt_without_children_is_plain() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
        let ctx = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::reduce_from_context(&ctx, None);
        assert!(!prompt.user.contains("Existing children:"));
    }

    #[test]
    fn expand_prompt_includes_friend_blocks() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
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
        let prompt = Prompt::expand_from_context(&ctx, None);
        assert!(prompt.user.contains("Friend blocks:"));
        assert!(prompt.user.contains("[0] peer concept A (perspective: historical lens)"));
        assert!(prompt.user.contains("[1] peer concept B"));
    }

    #[test]
    fn reduce_prompt_includes_friend_blocks() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
        let friends = vec![FriendContext::with_context(
            "supporting external detail".to_string(),
            Some("skeptical counterpoint".to_string()),
            true,
            true,
            None,
            None,
        )];
        let ctx = BlockContext::new(lineage, vec![], friends);
        let prompt = Prompt::reduce_from_context(&ctx, None);
        assert!(prompt.user.contains("Friend blocks:"));
        assert!(
            prompt
                .user
                .contains("[0] supporting external detail (perspective: skeptical counterpoint)")
        );
    }
}
