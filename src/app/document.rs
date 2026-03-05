//! Immutable document and tree renderer from `AppState` to Iced elements.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! Rendering semantics:
//! - mount and fold state are represented through disclosure marker behavior,
//! - action bars are projected per-row via `action_bar` view-model pipeline,
//! - rewrite/distill drafts render inline word-level diff panels.
//!
//! # Friend blocks UI
//!
//! Friend blocks are shown per block that has at least one friend:
//! - A "Friends" panel is rendered below the block row (same pattern as
//!   amplify/distill draft panels), listing each friend's point text and
//!   optional perspective, with a remove button per friend.
//!
//! ## Inline Perspective Editor
//!
//! Each friend in the panel has an editable "perspective" field. This is a
//! user-authored framing string that describes how the source block should
//! interpret that friend block. For example, a friend might be viewed from
//! "historical lens", "skeptical counterpoint", or "supporting evidence" perspective.
//!
//! The perspective is rendered as a secondary line below the friend's point text.
//! When empty, a localized placeholder invites the user to "add perspective...".
//! Clicking the perspective area toggles an inline text input field. On blur
//! (or Enter key), the new perspective is saved via `StructureMessage::SetFriendPerspective`.
//!
//! Design rationale:
//! - Inline editing avoids navigating to a separate modal/dialog, keeping context visible.
//! - Immediate save on blur provides instant feedback without requiring explicit save actions.
//! - Empty state with placeholder makes the affordance discoverable without cluttering the UI.
//!
//! # Editor Shortcut Routing Invariants
//!
//! Point-editor Enter chords are owned by this key-binding layer:
//! - `Cmd/Ctrl+Enter` dispatches a dedicated edit message that inserts an empty
//!   first child.
//! - `Cmd/Ctrl+Shift+Enter` dispatches `ActionId::AddSibling`.
//! - `Cmd/Ctrl+ArrowLeft/ArrowRight` dispatches cached tokenizer-based
//!   cursor-by-word movement.
//!
//! To avoid duplicate mutations, this resolver only dispatches structural
//! shortcuts when the editor instance is focused. Non-focused editors return the
//! default binding.
//!
//! # Multiselect Visual Design
//!
//! In multiselect mode, blocks render as read-only text (no editors) so the
//! full row is clickable. Selected blocks use `multiselect_selected` style
//! (accent border and wash) distinct from normal focus. Unselected blocks are
//! plain; both are wrapped in buttons that send `MultiselectBlockClicked`.
//! A selection count badge appears next to the mode bar when blocks are selected.
//!
//! # Mode Bar Semantics
//!
//! Mode buttons represent document modes (Normal, PickFriend, Multiselect).
//! Find is an overlay toggle that can coexist with an underlying document mode.
//! Mode button active states reflect the underlying document mode independent of
//! the find overlay, allowing users to switch between modes while find is open.
//!
//! # Shortcut Help Banner
//!
//! The top-right help button toggles a large bottom-right banner that lists all
//! currently supported keyboard shortcuts and key-driven editing gestures.
//! Keeping this cheat-sheet in-document reduces mode-switching cost for users
//! while preserving discoverability of less obvious chords.

use super::{
    AppState, ContextMenuAction, ContextMenuMessage, DocumentMode, ErrorBanner, ErrorMessage,
    Message, MountFileMessage, NavigationMessage, OverlayMessage, StructureMessage,
    action_bar::{
        ActionAvailability, ActionBarVm, ActionDescriptor, ActionId, RowContext, StatusChipVm,
        ViewportBucket, action_i18n_key, action_icon, action_to_message, build_action_bar_vm,
        project_for_viewport, status_error_i18n_key,
    },
    archive_panel,
    find_panel,
    friends_panel::{self, FriendPanelMessage},
    instruction_panel::{self, InstructionPanelMessage},
    link_panel, point_editor,
};
use crate::{
    component::breadcrumbs::{BreadcrumbLayer, Breadcrumbs},
    component::context_menu_button::{ContextMenuButton, ContextMenuIcon},
    component::error_banner_view::{ErrorBannerContent, ErrorBannerEntry, ErrorBannerView},
    component::icon_button::IconButton,
    component::status_chip::StatusChip,
    component::text_button::TextButton,
    store::{BlockId, BlockPanelBarState, PointContent},
    text::truncate_for_display,
    theme,
};
use iced::{
    Element, Fill, Length, Padding, Point,
    widget::{
        button, column, container, mouse_area, row, rule, scrollable, stack, text, tooltip,
    },
};
use lucide_icons::iced as icons;
use rust_i18n::t;

/// Stateless view that borrows `AppState` to render the document.
///
/// All rendering methods return iced `Element`s; no mutation of state occurs.
pub(super) struct DocumentView<'a> {
    state: &'a AppState,
}

impl<'a> DocumentView<'a> {
    pub(super) fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub(super) fn view(&self) -> Element<'a, Message> {
        let Self { state } = self;
        // Floating overlay
        let mut layout = column![].spacing(theme::LAYOUT_GAP);

        let toolbar_container = super::document_toolbar::view(super::document_toolbar::DocumentToolbarVm {
            document_mode: state.ui().document_mode,
            multiselect_count: state.ui().multiselect_selected_blocks.len(),
            focused_block_id: state.focus().map(|f| f.block_id),
        });

        // Document tree
        let tree = TreeView::new(state).render_roots();
        let max_width = theme::canvas_max_width(state.ui().window_size.width);
        let content = container(tree).padding(theme::CANVAS_PAD).max_width(max_width);
        let scroll_tail = self.state.ui().window_size.height * theme::CANVAS_SCROLL_TAIL_RATIO;
        layout = layout.push(
            scrollable(
                container(content)
                    .width(Fill)
                    .center_x(Fill)
                    .padding(iced::Padding::ZERO.top(theme::CANVAS_TOP).bottom(scroll_tail)),
            )
            .height(Fill),
        );

        let main_content = container(layout).style(theme::canvas).width(Fill).height(Fill);

        let show_shortcut_help = state.ui().show_shortcut_help;
        let floating_gear = super::document_top_right::view(
            super::document_top_right::DocumentTopRightVm {
                can_undo: state.can_undo(),
                can_redo: state.can_redo(),
                show_shortcut_help,
            },
        );

        // Shortcut help banner – bottom-right corner
        let shortcut_help_banner_element = if show_shortcut_help {
            Some(super::shortcut_help_banner::ShortcutHelpBanner::view(Message::Overlay(
                OverlayMessage::ToggleShortcutHelp,
            )))
        } else {
            None
        };

        // Error banner – bottom-right corner
        let error_banner_element = if let Some(eb) = ErrorBanner::from_state(state) {
            let content = ErrorBannerContent {
                title: eb.title(),
                latest_index: eb.latest.index,
                previous_entries: eb
                    .previous_entries
                    .iter()
                    .map(|e| ErrorBannerEntry { index: e.index, message: e.message.clone() })
                    .collect(),
                hidden_previous_count: eb.hidden_previous_count,
            };
            Some(ErrorBannerView::view(&content, |i| Message::Error(ErrorMessage::DismissAt(i))))
        } else {
            None
        };

        let mut bottom_right_content =
            column![].spacing(theme::LAYOUT_GAP).align_x(iced::alignment::Horizontal::Right);
        let mut has_bottom_right_overlay = false;

        if let Some(help_banner) = shortcut_help_banner_element {
            bottom_right_content = bottom_right_content.push(help_banner);
            has_bottom_right_overlay = true;
        }
        if let Some(error_banner) = error_banner_element {
            bottom_right_content = bottom_right_content.push(error_banner);
            has_bottom_right_overlay = true;
        }

        let floating_bottom_right = if has_bottom_right_overlay {
            container(
                container(bottom_right_content)
                    .width(Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .padding(
                        iced::Padding::new(16.0).right(theme::CANVAS_PAD).bottom(theme::CANVAS_PAD),
                    ),
            )
            .align_y(iced::alignment::Vertical::Bottom)
            .align_x(iced::alignment::Horizontal::Right)
            .width(Fill)
            .height(Fill)
        } else {
            container(iced::widget::Space::new()).width(Fill).height(Fill)
        };

        // Breadcrumb navigation - bottom-left corner
        let breadcrumbs = self.render_breadcrumbs_elements();
        let breadcrumbs_container =
            container(container(breadcrumbs).align_x(iced::alignment::Horizontal::Left).padding(
                iced::Padding::new(16.0).left(theme::CANVAS_PAD).bottom(theme::CANVAS_PAD),
            ))
            .align_y(iced::alignment::Vertical::Bottom)
            .width(Fill)
            .height(Fill);

        stack![
            main_content,
            floating_gear,
            toolbar_container,
            find_panel::floating_overlay(state),
            archive_panel::floating_overlay(state),
            link_panel::floating_overlay(state),
            breadcrumbs_container,
            floating_bottom_right,
            self.render_context_menu(),
        ]
        .width(Fill)
        .height(Fill)
        .into()
    }

    /// Render the breadcrumb navigation bar.
    fn render_breadcrumbs_elements(&self) -> Element<'a, Message> {
        let nav_layers = self.state.navigation.layers();
        let layer_count = nav_layers.len();
        let layers: Vec<BreadcrumbLayer> = nav_layers
            .iter()
            .enumerate()
            .map(|(i, layer)| {
                let label = self.state.store.point(&layer.block_id).unwrap_or_default();
                let display_label = truncate_for_display(&label, 30);
                let full_label = if let Some(path) = &layer.path {
                    if let Some(file_name) = path.file_name() {
                        format!("{} ({})", display_label, file_name.to_string_lossy())
                    } else {
                        display_label.to_string()
                    }
                } else {
                    display_label.to_string()
                };
                BreadcrumbLayer { label: full_label, is_current: i == layer_count - 1 }
            })
            .collect();

        Breadcrumbs::view(&layers, Message::Navigation(NavigationMessage::Home), |i| {
            Message::Navigation(NavigationMessage::GoTo(i))
        })
    }

    /// Render the context menu overlay when visible.
    fn render_context_menu(&self) -> Element<'a, Message> {
        let Some((block_id, position)) = self.state.ui().context_menu else {
            return container(iced::widget::Space::new()).width(Fill).height(Fill).into();
        };

        // Build action bar for this block
        let point_text = self.state.store.point(&block_id).unwrap_or_default().to_string();
        let amplification_draft = self.state.store.amplification_draft(&block_id);
        let atomization_draft = self.state.store.atomization_draft(&block_id);
        let distillation_draft = self.state.store.distillation_draft(&block_id);
        let node = self.state.store.node(&block_id);
        let row_context = RowContext {
            block_id,
            point_text,
            has_draft: amplification_draft.is_some()
                || atomization_draft.is_some()
                || distillation_draft.is_some(),
            draft_suggestion_count: amplification_draft.map(|d| d.children.len()).unwrap_or(0)
                + atomization_draft.map(|d| d.points.len()).unwrap_or(0)
                + distillation_draft.map(|d| d.redundant_children.len()).unwrap_or(0),
            has_amplify_error: self.state.llm_requests.has_amplify_error(block_id),
            has_distill_error: self.state.llm_requests.has_distill_error(block_id),
            has_atomize_error: self.state.llm_requests.has_atomize_error(block_id),
            is_amplifying: self.state.llm_requests.is_amplifying(block_id),
            is_distilling: self.state.llm_requests.is_distilling(block_id),
            is_atomizing: self.state.llm_requests.is_atomizing(block_id),
            is_mounted: self.state.store.mount_table().entry(block_id).is_some(),
            is_unexpanded_mount: node.is_some_and(|n| n.mount_path().is_some()),
            has_children: !self.state.store.children(&block_id).is_empty(),
        };
        let viewport_bucket = {
            let width = self.state.ui().window_size.width;
            if width <= theme::VIEWPORT_TOUCH_COMPACT_MAX_WIDTH {
                ViewportBucket::TouchCompact
            } else if width <= theme::VIEWPORT_COMPACT_MAX_WIDTH {
                ViewportBucket::Compact
            } else if width <= theme::VIEWPORT_MEDIUM_MAX_WIDTH {
                ViewportBucket::Medium
            } else {
                ViewportBucket::Wide
            }
        };
        let action_bar = project_for_viewport(build_action_bar_vm(&row_context), viewport_bucket);

        // Collect all enabled actions (visible + overflow)
        let mut enabled_actions = Vec::new();
        for descriptor in action_bar.visible_actions() {
            if descriptor.availability == ActionAvailability::Enabled {
                if let Some(message) = action_to_message(self.state, &block_id, &descriptor) {
                    enabled_actions.push((descriptor.id, message));
                }
            }
        }
        for descriptor in &action_bar.overflow {
            if descriptor.availability == ActionAvailability::Enabled {
                if let Some(message) = action_to_message(self.state, &block_id, descriptor) {
                    enabled_actions.push((descriptor.id, message));
                }
            }
        }

        // Group action buttons into rows of 5
        let mut action_buttons_column = column![].spacing(theme::CONTEXT_MENU_ACTION_GAP).padding(
            Padding::ZERO
                .top(theme::CONTEXT_MENU_PAD)
                .left(theme::CONTEXT_MENU_PAD)
                .right(theme::CONTEXT_MENU_PAD),
        );
        for chunk in enabled_actions.chunks(theme::CONTEXT_MENU_ACTIONS_PER_ROW) {
            let mut row_buttons = row![].spacing(theme::CONTEXT_MENU_ACTION_GAP);
            for (action_id, message) in chunk {
                let btn: Element<'a, Message> =
                    IconButton::action(action_icon(*action_id)).on_press(message.clone()).into();
                row_buttons = row_buttons.push(btn);
            }
            action_buttons_column = action_buttons_column.push(row_buttons.width(Fill));
        }

        // Context menu action buttons
        let is_link = self.state.store.point_content(&block_id).map_or(false, |pc| pc.is_link());
        let toggle_item: Element<'a, Message> = if is_link {
            ContextMenuButton::view(
                t!("ctx_convert_to_text").to_string(),
                ContextMenuIcon::ConvertToText,
                Message::ContextMenu(ContextMenuMessage::Action(ContextMenuAction::ConvertToText)),
            )
        } else {
            ContextMenuButton::view(
                t!("ctx_convert_to_link").to_string(),
                ContextMenuIcon::ConvertToLink,
                Message::ContextMenu(ContextMenuMessage::Action(ContextMenuAction::ConvertToLink)),
            )
        };
        let menu_items: Element<'a, Message> = column![
            ContextMenuButton::view(
                t!("ctx_undo").to_string(),
                ContextMenuIcon::Undo,
                Message::ContextMenu(ContextMenuMessage::Action(ContextMenuAction::Undo)),
            ),
            ContextMenuButton::view(
                t!("ctx_redo").to_string(),
                ContextMenuIcon::Redo,
                Message::ContextMenu(ContextMenuMessage::Action(ContextMenuAction::Redo)),
            ),
            rule::horizontal(1),
            ContextMenuButton::view(
                t!("ctx_cut").to_string(),
                ContextMenuIcon::Cut,
                Message::ContextMenu(ContextMenuMessage::Action(ContextMenuAction::Cut)),
            ),
            ContextMenuButton::view(
                t!("ctx_copy").to_string(),
                ContextMenuIcon::Copy,
                Message::ContextMenu(ContextMenuMessage::Action(ContextMenuAction::Copy)),
            ),
            ContextMenuButton::view(
                t!("ctx_paste").to_string(),
                ContextMenuIcon::Paste,
                Message::ContextMenu(ContextMenuMessage::Action(ContextMenuAction::Paste)),
            ),
            rule::horizontal(1),
            ContextMenuButton::view(
                t!("ctx_select_all").to_string(),
                ContextMenuIcon::SelectAll,
                Message::ContextMenu(ContextMenuMessage::Action(ContextMenuAction::SelectAll)),
            ),
            rule::horizontal(1),
            toggle_item,
        ]
        .spacing(theme::CONTEXT_MENU_ITEM_SPACING)
        .padding(theme::CONTEXT_MENU_PAD)
        .into();

        // Combine action buttons and menu items
        let content = if enabled_actions.len() > 0 {
            column![action_buttons_column, rule::horizontal(1), menu_items]
                .spacing(theme::CONTEXT_MENU_ITEM_SPACING)
        } else {
            column![menu_items].spacing(theme::CONTEXT_MENU_ITEM_SPACING)
        };

        // Width should fit content but have reasonable bounds
        let menu = container(content)
            .style(theme::context_menu)
            .width(Length::Fixed(theme::CONTEXT_MENU_WIDTH));

        // Use stack to position menu at cursor location
        // Layer 1: Background with click-to-dismiss
        let background: Element<'a, Message> = mouse_area(
            container(iced::widget::Space::new())
                .width(Fill)
                .height(Fill)
                .style(theme::transparent),
        )
        .on_press(Message::ContextMenu(ContextMenuMessage::Hide))
        .into();

        // Layer 2: Menu positioned at cursor
        let menu_container = container(menu)
            .align_x(iced::alignment::Horizontal::Left)
            .align_y(iced::alignment::Vertical::Top)
            .padding(iced::Padding::new(0.0).left(position.x as f32).top(position.y as f32));

        stack![background, menu_container].into()
    }
}

/// Stateless view that borrows `AppState` to render the block tree.
///
/// All rendering methods return iced `Element`s; no mutation of state occurs.
pub(super) struct TreeView<'a> {
    state: &'a AppState,
}

impl<'a> TreeView<'a> {
    pub(super) fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub(super) fn render_roots(&self) -> Element<'a, Message> {
        // Render from the current navigation layer's block
        let current_block = self.state.navigation.current_block_id();
        let children = if let Some(block_id) = current_block {
            self.state.store.children(&block_id)
        } else {
            self.state.store.roots()
        };
        self.render_line(children)
    }

    fn render_line(&self, ids: &'a [BlockId]) -> Element<'a, Message> {
        let mut col = column![].spacing(theme::BLOCK_GAP);
        for id in ids {
            if self.state.store.node(id).is_none() {
                continue;
            }
            col = col.push(self.render_block(id));
        }
        col.into()
    }

    fn render_block(&self, block_id: &BlockId) -> Element<'a, Message> {
        let Some(node) = self.state.store.node(block_id) else {
            return container(text("")).into();
        };

        let is_expanded_mount = self.state.store.mount_table().entry(*block_id).is_some();
        let unexpanded_mount_path = node.mount_path();
        let mount_display_path = unexpanded_mount_path.or_else(|| {
            self.state.store.mount_table().entry(*block_id).map(|entry| entry.rel_path.as_path())
        });

        // Link blocks bypass the editor buffer entirely — they render a chip
        // widget instead of a text editor. Checking here avoids the "missing
        // editor content" error that would otherwise fire for blocks whose
        // buffer was removed on link conversion.
        let is_link_block =
            self.state.store.point_content(block_id).map_or(false, PointContent::is_link);

        // Editor content is only needed for non-link blocks.
        let editor_content = if is_link_block {
            None
        } else {
            let content = self.state.editor_buffers.get(block_id);
            if content.is_none() {
                let fallback_text = self.state.store.point(block_id).unwrap_or_default();
                tracing::error!(block_id = ?block_id, "missing editor content for rendered block");
                return container(text(fallback_text).style(theme::spine_text)).into();
            }
            content
        };

        let block_id_for_edit = *block_id;

        // For action bar context, use editor text or link display text.
        let point_text_for_context = if is_link_block {
            self.state.store.point(block_id).unwrap_or_default()
        } else {
            editor_content.unwrap().text()
        };
        let row_context = self.action_row_context(block_id, point_text_for_context);
        let action_bar =
            project_for_viewport(build_action_bar_vm(&row_context), self.viewport_bucket());

        let spine = container(rule::vertical(1).style(theme::spine_rule))
            .width(Length::Fixed(theme::SPINE_WIDTH))
            .align_x(iced::alignment::Horizontal::Center);
        let has_children = !self.state.store.children(block_id).is_empty();
        let is_collapsed = self.state.store.is_collapsed(block_id);
        let is_foldable = has_children || is_expanded_mount || unexpanded_mount_path.is_some();

        let marker: Element<'a, Message> = if is_foldable {
            let icon = if is_collapsed || unexpanded_mount_path.is_some() {
                ActionId::ExpandBranch
            } else {
                ActionId::CollapseBranch
            };
            let msg = if unexpanded_mount_path.is_some() {
                Message::MountFile(MountFileMessage::ExpandMount(*block_id))
            } else if is_expanded_mount {
                Message::MountFile(MountFileMessage::CollapseMount(*block_id))
            } else {
                Message::Structure(StructureMessage::ToggleFold(*block_id))
            };
            IconButton::action(action_icon(icon)).on_press(msg).into()
        } else {
            let ring_icon: Element<'a, Message> = icons::icon_circle()
                .size(theme::LEAF_RING_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into();
            IconButton::action(ring_icon).into()
        };

        let is_focused = self.state.focus().map_or(false, |s| s.block_id == *block_id);

        // Only render action bar when block is focused
        let action_buttons: Element<'a, Message> = if is_focused {
            self.render_action_buttons(block_id, &action_bar)
        } else {
            container(iced::widget::Space::new())
                .padding(
                    Padding::ZERO
                        .top(theme::ROW_CONTROL_VERTICAL_PAD)
                        .bottom(theme::ROW_CONTROL_VERTICAL_PAD),
                )
                .into()
        };

        let is_pick_friend_mode = self.state.ui().document_mode == DocumentMode::PickFriend;
        let is_multiselect_mode = self.state.ui().document_mode == DocumentMode::Multiselect;
        let is_target_block =
            is_pick_friend_mode && self.state.focus().is_some_and(|s| s.block_id != *block_id);

        // Check if this block should be highlighted due to friend panel hover
        let is_hovered_friend = self.state.ui().hovered_friend_block.is_some_and(|hovered_id| {
            hovered_id == *block_id
                && self.state.store.is_visible(block_id)
                && self.state.navigation.is_in_current_view(&self.state.store, block_id)
        });

        let point_editor = point_editor::view(
            block_id_for_edit,
            is_target_block || is_multiselect_mode,
            self.state.store.point(block_id).unwrap_or_default(),
            self.state.store.point_content(block_id),
            editor_content,
            self.state.editor_buffers.widget_id(block_id),
            self.state.ui().cursor_position.unwrap_or(Point::ORIGIN),
            self.state.ui().expanded_links.contains(block_id),
        );

        let row_content = row![]
            .spacing(theme::ROW_GAP)
            .width(Fill)
            .align_y(iced::Alignment::Start)
            .push(spine)
            .push(
                container(marker).padding(
                    Padding::ZERO
                        .top(theme::ROW_CONTROL_VERTICAL_PAD)
                        .bottom(theme::ROW_CONTROL_VERTICAL_PAD),
                ),
            )
            .push(point_editor);

        // Panel bar (left) and action bar (right) in one row
        let panel_bar = self.render_panel_bar_only(block_id, is_focused);
        let bar_row = row![]
            .spacing(theme::ROW_GAP)
            .width(Fill)
            .push(container(panel_bar).width(Length::Fill))
            .push(action_buttons);

        // Panel row (shown only when a panel is open)
        let panel_row = self.render_panel_row(block_id, is_focused);

        let head_row: Element<'a, Message> = if let Some(mount_path) = mount_display_path {
            column![
                container(self.render_mount_indicator(block_id, mount_path))
                    .padding(Padding::ZERO.left(theme::INDENT)),
                row_content,
            ]
            .spacing(theme::MOUNT_HEADER_ROW_GAP)
            .into()
        } else {
            row_content.into()
        };

        let mut block = column![head_row, bar_row, panel_row].spacing(theme::BLOCK_INNER_GAP);
        if action_bar.status_chip.is_some() {
            block = block.push(
                container(self.render_status_chip(&action_bar))
                    .padding(Padding::ZERO.left(theme::INDENT)),
            );
        }
        if let Some(draft) = self.state.store.atomization_draft(block_id) {
            block = block.push(super::patch::render_patch_panel(
                self.state,
                block_id,
                super::patch::PatchDraft::Atomize(draft),
            ));
        }
        if let Some(draft) = self.state.store.amplification_draft(block_id) {
            block = block.push(super::patch::render_patch_panel(
                self.state,
                block_id,
                super::patch::PatchDraft::Amplify(draft),
            ));
        }
        if let Some(draft) = self.state.store.distillation_draft(block_id) {
            block = block.push(super::patch::render_patch_panel(
                self.state,
                block_id,
                super::patch::PatchDraft::Distill(draft),
            ));
        }

        // Render children only when not folded.
        if !is_collapsed {
            let children = self.state.store.children(block_id);
            if !children.is_empty() {
                block = block.push(
                    container(self.render_line(children))
                        .padding(Padding::ZERO.left(theme::INDENT)),
                );
            }
        }

        let is_multiselect_selected =
            self.state.ui().multiselect_selected_blocks.contains(block_id);

        match (self.state.ui().document_mode, self.state.focus().map(|s| s.block_id)) {
            | (
                DocumentMode::Normal
                | DocumentMode::Find
                | DocumentMode::LinkInput
                | DocumentMode::Archive,
                Some(focused),
            ) if focused == *block_id => {
                // Render the block as the focused block
                container(block).style(theme::focused_block).into()
            }
            | (
                DocumentMode::Normal
                | DocumentMode::Find
                | DocumentMode::LinkInput
                | DocumentMode::Archive,
                _,
            ) if is_hovered_friend => {
                // Highlight block when friend panel hovers over it
                container(block).style(theme::friend_picker_hover).into()
            }
            | (
                DocumentMode::Normal
                | DocumentMode::Find
                | DocumentMode::LinkInput
                | DocumentMode::Archive,
                _,
            ) => block.into(),
            | (DocumentMode::PickFriend, Some(focused)) if focused == *block_id => {
                // Render the picker block itself as is
                block.into()
            }
            | (DocumentMode::PickFriend, Some(target)) => {
                // Render the block as a friend picker target
                button(container(block).style(theme::friend_picker_hover))
                    .on_press(Message::Structure(StructureMessage::AddFriendBlock {
                        target,
                        friend_id: *block_id,
                    }))
                    .padding(0)
                    .style(theme::action_button)
                    .into()
            }
            | (DocumentMode::Multiselect, _) if is_multiselect_selected => {
                button(container(block).style(theme::multiselect_selected))
                    .on_press(Message::MultiselectBlockClicked(*block_id))
                    .padding(0)
                    .style(theme::action_button)
                    .into()
            }
            | (DocumentMode::Multiselect, _) => button(container(block))
                .on_press(Message::MultiselectBlockClicked(*block_id))
                .padding(0)
                .style(theme::action_button)
                .into(),
            | (_, None) => block.into(),
        }
    }

    fn action_row_context(&self, block_id: &BlockId, point_text: String) -> RowContext {
        let amplification_draft = self.state.store.amplification_draft(block_id);
        let atomization_draft = self.state.store.atomization_draft(block_id);
        let distillation_draft = self.state.store.distillation_draft(block_id);
        let node = self.state.store.node(block_id);
        RowContext {
            block_id: *block_id,
            point_text,
            has_draft: amplification_draft.is_some()
                || atomization_draft.is_some()
                || distillation_draft.is_some(),
            draft_suggestion_count: amplification_draft.map(|d| d.children.len()).unwrap_or(0)
                + atomization_draft.map(|d| d.points.len()).unwrap_or(0)
                + distillation_draft.map(|d| d.redundant_children.len()).unwrap_or(0),
            has_amplify_error: self.state.llm_requests.has_amplify_error(*block_id),
            has_distill_error: self.state.llm_requests.has_distill_error(*block_id),
            has_atomize_error: self.state.llm_requests.has_atomize_error(*block_id),
            is_amplifying: self.state.llm_requests.is_amplifying(*block_id),
            is_distilling: self.state.llm_requests.is_distilling(*block_id),
            is_atomizing: self.state.llm_requests.is_atomizing(*block_id),
            is_mounted: self.state.store.mount_table().entry(*block_id).is_some(),
            is_unexpanded_mount: node.is_some_and(|n| n.mount_path().is_some()),
            has_children: !self.state.store.children(block_id).is_empty(),
        }
    }

    fn viewport_bucket(&self) -> ViewportBucket {
        let width = self.state.ui().window_size.width;
        if width <= 0.0 {
            return ViewportBucket::Wide;
        }
        if width <= theme::VIEWPORT_TOUCH_COMPACT_MAX_WIDTH {
            ViewportBucket::TouchCompact
        } else if width <= theme::VIEWPORT_COMPACT_MAX_WIDTH {
            ViewportBucket::Compact
        } else if width <= theme::VIEWPORT_MEDIUM_MAX_WIDTH {
            ViewportBucket::Medium
        } else {
            ViewportBucket::Wide
        }
    }

    fn render_status_chip(&self, vm: &ActionBarVm) -> Element<'a, Message> {
        let label = match &vm.status_chip {
            | Some(StatusChipVm::Loading { op: ActionId::Amplify }) => {
                t!("doc_status_amplifying").to_string()
            }
            | Some(StatusChipVm::Loading { op: ActionId::Atomize }) => {
                t!("doc_status_atomizing").to_string()
            }
            | Some(StatusChipVm::Loading { op: ActionId::Distill }) => {
                t!("doc_status_distilling").to_string()
            }
            | Some(StatusChipVm::Loading { .. }) => t!("doc_status_working").to_string(),
            | Some(StatusChipVm::Error { op, .. }) => t!(status_error_i18n_key(*op)).to_string(),
            | Some(StatusChipVm::DraftActive { suggestion_count }) if *suggestion_count > 0 => {
                t!("doc_status_draft_ready").to_string()
            }
            | Some(StatusChipVm::DraftActive { .. }) => t!("doc_status_draft").to_string(),
            | None => return Element::from(iced::widget::Space::new()),
        };
        StatusChip::view(label)
    }

    /// Renders the panel bar containing toggle buttons for overlay panels.
    ///
    /// This component lives in the bar row and provides toggles for panels
    /// that appear in the panel row below.
    ///
    /// The toggle buttons reflect panel-open state independently:
    /// - `Friends` is highlighted only when [`BlockPanelBarState::Friends`] is open.
    /// - `Instruction` is highlighted only when [`BlockPanelBarState::Instruction`] is open.
    fn render_panel_bar_only(&self, block_id: &BlockId, is_focused: bool) -> Element<'a, Message> {
        if !is_focused {
            return column![].into();
        }

        let friends_panel_open = matches!(
            self.state.store.block_panel_state(block_id),
            Some(BlockPanelBarState::Friends)
        );
        let instruction_panel_open = matches!(
            self.state.store.block_panel_state(block_id),
            Some(BlockPanelBarState::Instruction)
        );

        let button_row = row![]
            .spacing(theme::PANEL_BUTTON_GAP)
            .push(
                TextButton::panel_toggle(t!("ui_friends").to_string(), 13.0, friends_panel_open)
                    .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                    .on_press(Message::FriendPanel(FriendPanelMessage::Toggle(*block_id))),
            )
            .push(
                TextButton::panel_toggle(
                    t!("ui_instruction").to_string(),
                    13.0,
                    instruction_panel_open,
                )
                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::InstructionPanel(*block_id, InstructionPanelMessage::Toggle)),
            );

        container(button_row).padding(Padding::ZERO.right(theme::INDENT)).into()
    }

    /// Renders the overlay panel row containing the active panel content.
    ///
    /// This component lives in the panel row below the bar row.
    /// Only renders content when a panel is open.
    fn render_panel_row(&self, block_id: &BlockId, is_focused: bool) -> Element<'a, Message> {
        if !is_focused {
            return column![].into();
        }

        match self.state.store.block_panel_state(block_id) {
            | Some(BlockPanelBarState::Friends) => {
                container(friends_panel::view(self.state)).width(Length::Fill).into()
            }
            | Some(BlockPanelBarState::Instruction) => {
                container(instruction_panel::view(self.state)).width(Length::Fill).into()
            }
            | None => column![].into(),
        }
    }

    #[allow(dead_code)]
    /// Renders the overlay panel bar containing toggle buttons for overlay panels.
    ///
    /// This component lives below each block's editor and provides toggles for panels
    /// that can be shown inline (as opposed to draft panels which appear below).
    ///
    /// The toggle buttons reflect panel-open state independently:
    /// - `Friends` is highlighted only when [`BlockPanelBarState::Friends`] is open.
    /// - `Instruction` is highlighted only when [`BlockPanelBarState::Instruction`] is open.
    fn render_overlay_panel_bar(
        &self, block_id: &BlockId, is_focused: bool,
    ) -> Element<'a, Message> {
        if !is_focused {
            return column![].into();
        }

        let friends_panel_open = matches!(
            self.state.store.block_panel_state(block_id),
            Some(BlockPanelBarState::Friends)
        );
        let instruction_panel_open = matches!(
            self.state.store.block_panel_state(block_id),
            Some(BlockPanelBarState::Instruction)
        );

        let mut button_row = row![].spacing(theme::PANEL_BUTTON_GAP);
        button_row = button_row.push(
            TextButton::panel_toggle(t!("ui_friends").to_string(), 13.0, friends_panel_open)
                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                .on_press(Message::FriendPanel(FriendPanelMessage::Toggle(*block_id))),
        );
        button_row = button_row.push(
            TextButton::panel_toggle(
                t!("ui_instruction").to_string(),
                13.0,
                instruction_panel_open,
            )
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(Message::InstructionPanel(*block_id, InstructionPanelMessage::Toggle)),
        );

        let mut col =
            column![].push(container(button_row).padding(Padding::ZERO.right(theme::INDENT)));

        match self.state.store.block_panel_state(block_id) {
            | Some(BlockPanelBarState::Friends) => {
                col = col.push(container(friends_panel::view(self.state)).width(Length::Fill));
            }
            | Some(BlockPanelBarState::Instruction) => {
                col = col.push(container(instruction_panel::view(self.state)).width(Length::Fill));
            }
            | None => {}
        }

        col.into()
    }

    fn render_action_buttons(&self, block_id: &BlockId, vm: &ActionBarVm) -> Element<'a, Message> {
        let is_overflow_open =
            self.state.focus().is_some_and(|s| s.block_id == *block_id && s.overflow_open);
        let mut actions_row = row![].spacing(theme::ACTION_GAP);

        // Always show primary actions
        for descriptor in vm.visible_actions() {
            actions_row = actions_row.push(self.render_action_button(block_id, &descriptor));
        }

        // Show "More" button when closed, or "Close" button at end when open
        if !vm.overflow.is_empty() {
            if is_overflow_open {
                // When open, show overflow actions first, then close button at the end
                for descriptor in &vm.overflow {
                    actions_row = actions_row.push(self.render_action_button(block_id, descriptor));
                }
                let btn = IconButton::action(
                    icons::icon_x()
                        .size(theme::TOOLBAR_ICON_SIZE)
                        .line_height(iced::widget::text::LineHeight::Relative(1.0))
                        .into(),
                )
                .on_press(Message::Overlay(OverlayMessage::ToggleOverflow(*block_id)));

                actions_row = actions_row.push(
                    tooltip(
                        btn,
                        text(t!("ui_close").to_string())
                            .size(theme::SMALL_TEXT_SIZE)
                            .font(theme::INTER),
                        tooltip::Position::Bottom,
                    )
                    .style(theme::tooltip)
                    .padding(theme::TOOLTIP_PAD)
                    .gap(theme::TOOLTIP_GAP),
                );
            } else {
                // When closed, show "More" button
                let btn = IconButton::action(
                    icons::icon_ellipsis()
                        .size(theme::TOOLBAR_ICON_SIZE)
                        .line_height(iced::widget::text::LineHeight::Relative(1.0))
                        .into(),
                )
                .on_press(Message::Overlay(OverlayMessage::ToggleOverflow(*block_id)));

                actions_row = actions_row.push(
                    tooltip(
                        btn,
                        text(t!("ui_more").to_string())
                            .size(theme::SMALL_TEXT_SIZE)
                            .font(theme::INTER),
                        tooltip::Position::Bottom,
                    )
                    .style(theme::tooltip)
                    .padding(theme::TOOLTIP_PAD)
                    .gap(theme::TOOLTIP_GAP),
                );
            }
        }

        actions_row.into()
    }

    fn render_action_button(
        &self, block_id: &BlockId, descriptor: &ActionDescriptor,
    ) -> Element<'a, Message> {
        let base = if descriptor.destructive {
            IconButton::destructive(action_icon(descriptor.id))
        } else {
            IconButton::action(action_icon(descriptor.id))
        };
        let btn = if descriptor.availability == ActionAvailability::Enabled {
            if let Some(message) = action_to_message(self.state, block_id, descriptor) {
                base.on_press(message)
            } else {
                base
            }
        } else {
            base
        };
        let label = t!(action_i18n_key(descriptor.id)).to_string();
        tooltip(
            btn,
            text(label).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
            tooltip::Position::Bottom,
        )
        .style(theme::tooltip)
        .padding(theme::TOOLTIP_PAD)
        .gap(theme::TOOLTIP_GAP)
        .into()
    }

    /// Render a mount header showing file path and mount actions.
    ///
    /// Displayed above mount-backed nodes (both expanded and unexpanded).
    /// Provides an overflow menu for mount relocation, shallow inline, and
    /// recursive inline-all.
    fn render_mount_indicator(
        &self, block_id: &BlockId, mount_path: &'a std::path::Path,
    ) -> Element<'a, Message> {
        super::mount_indicator::view(super::mount_indicator::MountIndicatorVm {
            block_id: *block_id,
            mount_path,
            is_inline_confirmation_armed: self.state.ui().pending_inline_mount_confirmation
                == Some(*block_id),
            is_overflow_open: self.state.ui().mount_action_overflow_block == Some(*block_id),
        })
    }
}
