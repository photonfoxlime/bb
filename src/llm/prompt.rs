//! LLM prompt construction from context.
//!
//! All four tasks (reduce, atomize, expand, inquire) receive the same block context:
//! lineage (Parent/Target), existing children, and friend blocks. Custom
//! prompts support partial override; the full context block is always appended.
//!
//! Construction is unified via [`TaskKind`]; task-specific defaults apply
//! per variant.

use crate::llm::config::TaskKind;
use crate::llm::context::BlockContext;
use crate::llm::context::{ContextFormatter, ContextPresence};
#[cfg(test)]
use crate::llm::context::{FriendContext, LineageContext};

/// System + user prompt pair sent to the chat completions endpoint.
pub struct Prompt {
    pub(crate) system: String,
    pub(crate) user: String,
}

/// Default system prompt for the given task, with simplest context (no children, no friends).
/// Used as a foldable hint in the settings UI.
pub fn default_system_prompt_hint(task: TaskKind) -> String {
    let presence = ContextPresence { has_children: false, has_friends: false };
    task.default_system(presence)
}

/// Default user prompt intro for the given task.
pub fn default_user_prompt_hint(task: TaskKind) -> &'static str {
    task.default_user_intro()
}

/// Per-task prompt configuration: [`TaskKind`] plus its optional custom prompts.
///
/// Each task has its own `system_prompt` and `user_prompt` in config;
/// this struct bundles them for prompt construction.
#[derive(Clone, Debug)]
pub struct TaskPromptConfig {
    pub task: TaskKind,
    pub custom_system_prompt: Option<String>,
    pub custom_user_prompt: Option<String>,
}

impl TaskPromptConfig {
    fn optional(s: &str) -> Option<String> {
        if s.is_empty() { None } else { Some(s.to_string()) }
    }

    /// Config for reduce task with its custom prompts.
    pub fn reduce(system_prompt: &str, user_prompt: &str) -> Self {
        Self {
            task: TaskKind::Reduce,
            custom_system_prompt: Self::optional(system_prompt),
            custom_user_prompt: Self::optional(user_prompt),
        }
    }

    /// Config for atomize task with its custom prompts.
    pub fn atomize(system_prompt: &str, user_prompt: &str) -> Self {
        Self {
            task: TaskKind::Atomize,
            custom_system_prompt: Self::optional(system_prompt),
            custom_user_prompt: Self::optional(user_prompt),
        }
    }

    /// Config for expand task with its custom prompts.
    pub fn expand(system_prompt: &str, user_prompt: &str) -> Self {
        Self {
            task: TaskKind::Expand,
            custom_system_prompt: Self::optional(system_prompt),
            custom_user_prompt: Self::optional(user_prompt),
        }
    }

    /// Config for inquire task with its custom prompts.
    pub fn inquire(system_prompt: &str, user_prompt: &str) -> Self {
        Self {
            task: TaskKind::Inquire,
            custom_system_prompt: Self::optional(system_prompt),
            custom_user_prompt: Self::optional(user_prompt),
        }
    }
}

impl TaskKind {
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

    pub(crate) fn default_system(self, presence: ContextPresence) -> String {
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
            Self::Atomize => {
                let context_qualifier = Self::context_qualifier(presence);
                let friends_explanation = self.friends_explanation(presence);
                format!(
                    "You atomize one target bullet point using its ancestors{context_qualifier}. Break the text into a list of distinct information points without dropping details. Return strict JSON only: {{\"points\": string[]}}. Each point must be a single, self-contained fact or idea. Preserve all semantic content; do not summarize or condense.{friends_explanation} No markdown, no extra keys."
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

    pub(crate) fn default_user_intro(self) -> &'static str {
        match self {
            | Self::Reduce => "Reduce the target point with context:",
            | Self::Atomize => "Atomize the target point with context:",
            | Self::Expand => "Expand the target point with context:",
            | Self::Inquire => "Context:",
        }
    }
}

impl Prompt {
    /// Build a prompt from block context using a unified construction flow.
    ///
    /// Uses [`TaskPromptConfig`] for task identity and custom prompts.
    /// `instruction` is optional for reduce/expand (prefixed to system);
    /// required for inquire (embedded in user message).
    pub(crate) fn from_context(
        config: &TaskPromptConfig, context: &BlockContext, instruction: Option<&str>,
    ) -> Self {
        let task = config.task;
        let fmt = ContextFormatter::from_block_context(context);
        let presence = fmt.presence();
        let instruction_prefix = instruction.map(|i| format!("{}\n\n", i)).unwrap_or_default();
        let custom_system = config.custom_system_prompt.as_deref();
        let custom_user = config.custom_user_prompt.as_deref();

            let user = if let Some(custom) = custom_user {
            let context_block = fmt.format_context_block();
            match task {
                | TaskKind::Inquire => {
                    let instruction = instruction.expect("inquire requires instruction");
                    format!("{custom}\n\n{context_block}\n\nInstruction: {instruction}")
                }
                | _ => format!("{custom}\n\n{context_block}"),
            }
        } else {
            match task {
                | TaskKind::Inquire => {
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
            custom_system.map(String::from).unwrap_or_else(|| task.default_system(presence));
        let system = match task {
            | TaskKind::Inquire => system,
            | _ => format!("{instruction_prefix}{system}"),
        };

        Self { system, user }
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
        let config = TaskPromptConfig::reduce("", "");
        let prompt = Prompt::from_context(&config, &context, None);
        assert!(prompt.user.contains("Parent: first"));
        assert!(prompt.user.contains("Parent: second"));
        assert!(prompt.user.contains("Target: third"));
    }

    #[test]
    fn expand_prompt_labels_target_last() {
        let lineage =
            LineageContext::from_points(vec!["first".into(), "second".into(), "third".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let config = TaskPromptConfig::expand("", "");
        let prompt = Prompt::from_context(&config, &context, None);
        assert!(prompt.user.contains("Parent: first"));
        assert!(prompt.user.contains("Parent: second"));
        assert!(prompt.user.contains("Target: third"));
    }

    #[test]
    fn expand_prompt_mentions_concise_and_non_overlapping_constraints() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let config = TaskPromptConfig::expand("", "");
        let prompt = Prompt::from_context(&config, &context, None);
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
        let config = TaskPromptConfig::expand("", "");
        let prompt = Prompt::from_context(&config, &ctx, None);
        assert!(prompt.user.contains("Existing children:"));
        assert!(prompt.user.contains("[0] existing child A"));
        assert!(prompt.user.contains("[1] existing child B"));
        assert!(prompt.system.contains("MUST NOT overlap with the existing children"));
    }

    #[test]
    fn expand_prompt_without_children_omits_section() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let ctx = BlockContext::new(lineage, vec![], vec![]);
        let config = TaskPromptConfig::expand("", "");
        let prompt = Prompt::from_context(&config, &ctx, None);
        assert!(!prompt.user.contains("Existing children:"));
    }

    #[test]
    fn reduce_prompt_includes_existing_children() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let children = vec!["child A".to_string()];
        let ctx = BlockContext::new(lineage, children, vec![]);
        let config = TaskPromptConfig::reduce("", "");
        let prompt = Prompt::from_context(&config, &ctx, None);
        assert!(prompt.user.contains("Existing children:"));
        assert!(prompt.user.contains("[0] child A"));
        assert!(prompt.system.contains("redundant_children"));
    }

    #[test]
    fn reduce_prompt_without_children_is_plain() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let ctx = BlockContext::new(lineage, vec![], vec![]);
        let config = TaskPromptConfig::reduce("", "");
        let prompt = Prompt::from_context(&config, &ctx, None);
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
        let config = TaskPromptConfig::expand("", "");
        let prompt = Prompt::from_context(&config, &ctx, None);
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
        let config = TaskPromptConfig::reduce("", "");
        let prompt = Prompt::from_context(&config, &ctx, None);
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
        let config = TaskPromptConfig::inquire("", "");
        let prompt = Prompt::from_context(&config, &ctx, Some("answer this"));
        assert!(prompt.user.contains("Existing children:"));
        assert!(prompt.user.contains("[0] child one"));
        assert!(prompt.user.contains("[1] child two"));
    }

    #[test]
    fn inquire_prompt_omits_existing_children_when_empty() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let ctx = BlockContext::new(lineage, vec![], vec![]);
        let config = TaskPromptConfig::inquire("", "");
        let prompt = Prompt::from_context(&config, &ctx, Some("answer this"));
        assert!(!prompt.user.contains("Existing children:"));
    }

    #[test]
    fn custom_prompts_override_defaults() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let config = TaskPromptConfig::reduce("Custom system prompt", "Custom user prompt");
        let prompt = Prompt::from_context(&config, &context, None);
        assert!(prompt.system.contains("Custom system prompt"));
        assert!(prompt.user.contains("Custom user prompt"));
    }

    #[test]
    fn custom_system_only_uses_default_user() {
        let lineage = LineageContext::from_points(vec!["root".into(), "target".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let config = TaskPromptConfig::reduce("Custom system", "");
        let prompt = Prompt::from_context(&config, &context, None);
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
        let config = TaskPromptConfig::inquire("", "Custom user preamble");
        let prompt = Prompt::from_context(&config, &ctx, Some("answer this"));
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
        let config = TaskPromptConfig::reduce("", "Custom user preamble");
        let prompt = Prompt::from_context(&config, &context, None);
        assert!(prompt.system.contains("You reduce a bullet point"));
        assert!(prompt.user.contains("Custom user preamble"));
    }
}
