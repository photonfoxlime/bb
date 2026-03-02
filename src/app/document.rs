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
//! - rewrite/reduce drafts render inline word-level diff panels.
//!
//! # Friend blocks UI
//!
//! Friend blocks are shown per block that has at least one friend:
//! - A "Friends" panel is rendered below the block row (same pattern as
//!   expansion/reduction draft panels), listing each friend's point text and
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
    AppState, DocumentMode, EditMessage, ErrorBanner, ErrorMessage, ExpandMessage, FindMessage,
    Message, MountFileMessage, NavigationMessage, OverlayMessage, ReduceMessage, ShortcutMessage,
    StructureMessage, UndoRedoMessage,
    action_bar::{
        ActionAvailability, ActionBarVm, ActionDescriptor, ActionId, RowContext, StatusChipVm,
        ViewportBucket, action_i18n_key, action_to_message, build_action_bar_vm,
        project_for_viewport, shortcut_to_action, status_error_i18n_key,
    },
    diff::{WordChange, word_diff},
    find_panel,
    friends_panel::{self, FriendPanelMessage},
    instruction_panel::{self, InstructionPanelMessage},
    settings::SettingsMessage,
};
use crate::{
    component::icon_button::IconButton,
    component::text_button::TextButton,
    store::{BlockId, BlockPanelBarState, ExpansionDraftRecord, ReductionDraftRecord},
    text::truncate_for_display,
    theme,
};
use iced::{
    Color, Element, Fill, Length, Padding,
    widget::{
        button, column, container, rich_text, row, rule, scrollable, space, span, stack, text,
        text_editor, tooltip,
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

        // Modebar buttons (normal, find, multiselect) - top-left corner
        // Document mode buttons reflect the underlying document mode, independent of find overlay
        let is_normal_mode = state.ui().document_mode == DocumentMode::Normal;
        let is_find_mode = state.ui().document_mode == DocumentMode::Find;
        let is_multiselect_mode = state.ui().document_mode == DocumentMode::Multiselect;

        let normal_mode_btn = IconButton::mode(
            icons::icon_mouse_pointer_2()
                .size(theme::TOOLBAR_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into(),
            is_normal_mode,
        )
        .on_press(Message::DocumentMode(DocumentMode::Normal));

        let find_mode_btn = IconButton::mode(
            icons::icon_search()
                .size(theme::TOOLBAR_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into(),
            is_find_mode,
        )
        .on_press(Message::Find(if is_find_mode {
            FindMessage::Close
        } else {
            FindMessage::Open
        }));

        let multiselect_mode_btn = IconButton::mode(
            icons::icon_square_check()
                .size(theme::TOOLBAR_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into(),
            is_multiselect_mode,
        )
        .on_press(Message::DocumentMode(if is_multiselect_mode {
            DocumentMode::Normal
        } else {
            DocumentMode::Multiselect
        }));

        let toolbar =
            row![normal_mode_btn, find_mode_btn, multiselect_mode_btn].spacing(theme::ACTION_GAP);

        let toolbar_container = container(
            container(toolbar)
                .align_y(iced::alignment::Vertical::Top)
                .align_x(iced::alignment::Horizontal::Left)
                .padding(
                    iced::Padding::new(theme::PANEL_PAD_H)
                        .left(theme::CANVAS_PAD)
                        .top(theme::CANVAS_TOP),
                ),
        )
        .width(Fill)
        .height(Fill);

        // Document tree
        let tree = TreeView::new(state).render_roots();
        let max_width = theme::canvas_max_width(state.ui().window_size.width);
        let content = container(tree).padding(theme::CANVAS_PAD).max_width(max_width);
        layout = layout.push(
            scrollable(
                container(content)
                    .width(Fill)
                    .center_x(Fill)
                    .padding(iced::Padding::ZERO.top(theme::CANVAS_TOP)),
            )
            .height(Fill),
        );

        let main_content = container(layout).style(theme::canvas).width(Fill).height(Fill);

        // Shortcut help button – top-right, before settings
        let show_shortcut_help = state.ui().show_shortcut_help;
        let help_button = IconButton::mode(
            icons::icon_circle_question_mark()
                .size(theme::TOOLBAR_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into(),
            show_shortcut_help,
        )
        .on_press(Message::Overlay(OverlayMessage::ToggleShortcutHelp));

        // Settings gear button – top-right corner
        let gear_button = IconButton::action(
            lucide_icons::iced::icon_settings()
                .size(theme::TOOLBAR_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into(),
        )
        .on_press(Message::Settings(SettingsMessage::Open));

        // Undo/redo buttons – top-right, before settings
        let can_undo = state.can_undo();
        let can_redo = state.can_redo();

        let mut undo_button = IconButton::action(
            icons::icon_undo_2()
                .size(theme::TOOLBAR_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into(),
        );
        if can_undo {
            undo_button = undo_button.on_press(Message::UndoRedo(UndoRedoMessage::Undo));
        }

        let mut redo_button = IconButton::action(
            icons::icon_redo_2()
                .size(theme::TOOLBAR_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into(),
        );
        if can_redo {
            redo_button = redo_button.on_press(Message::UndoRedo(UndoRedoMessage::Redo));
        }

        let top_right_buttons =
            row![undo_button, redo_button, help_button, gear_button].spacing(theme::ACTION_GAP);
        let floating_gear = container(
            container(top_right_buttons)
                .width(Fill)
                .align_y(iced::alignment::Vertical::Top)
                .align_x(iced::alignment::Horizontal::Right)
                .padding(
                    iced::Padding::new(theme::PANEL_PAD_H)
                        .right(theme::CANVAS_PAD)
                        .bottom(theme::PANEL_PAD_H),
                ),
        )
        .width(Fill)
        .height(Fill);

        // Shortcut help banner – bottom-right corner
        let shortcut_help_banner_element = self.render_shortcut_help_banner();

        // Error banner – bottom-right corner
        let error_banner_element = if let Some(error_banner) = ErrorBanner::from_state(state) {
            let mut banner_content = column![
                row![
                    text(error_banner.title()),
                    button(text(t!("ui_dismiss").to_string())).on_press(Message::Error(
                        ErrorMessage::DismissAt(error_banner.latest.index)
                    )),
                ]
                .spacing(theme::PANEL_BUTTON_GAP)
                .align_y(iced::Alignment::Center)
            ]
            .spacing(theme::INLINE_GAP);
            for entry in &error_banner.previous_entries {
                banner_content = banner_content.push(
                    row![
                        text(t!("error_earlier", message = entry.message.as_str()).to_string()),
                        button(text(t!("ui_dismiss").to_string()))
                            .on_press(Message::Error(ErrorMessage::DismissAt(entry.index))),
                    ]
                    .spacing(theme::PANEL_BUTTON_GAP)
                    .align_y(iced::Alignment::Center),
                );
            }
            if error_banner.hidden_previous_count > 0 {
                banner_content = banner_content.push(text(
                    t!("error_older_count", count = error_banner.hidden_previous_count).to_string(),
                ));
            }
            Some(container(banner_content).style(theme::error_banner).padding(theme::BANNER_PAD))
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
        let breadcrumbs = self.render_breadcrumbs();
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
            breadcrumbs_container,
            floating_bottom_right
        ]
        .width(Fill)
        .height(Fill)
        .into()
    }

    /// Render the bottom-right keyboard-shortcuts help banner when enabled.
    fn render_shortcut_help_banner(&self) -> Option<Element<'a, Message>> {
        if !self.state.ui().show_shortcut_help {
            return None;
        }

        let title = row![
            text(t!("shortcut_help_title").to_string())
                .size(theme::SHORTCUT_HELP_TITLE_SIZE)
                .font(theme::INTER),
            space::horizontal(),
            button(text(t!("ui_dismiss").to_string()))
                .on_press(Message::Overlay(OverlayMessage::ToggleShortcutHelp))
                .padding(theme::BUTTON_PAD)
        ]
        .spacing(theme::ACTION_GAP)
        .align_y(iced::Alignment::Center)
        .width(Fill);

        let global_section = column![
            text(t!("shortcut_help_section_global").to_string())
                .font(theme::INTER)
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_global_find").to_string()).size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_global_find_next").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_global_find_previous").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_global_undo").to_string()).size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_global_redo").to_string()).size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_global_escape").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
        ]
        .spacing(theme::SHORTCUT_HELP_ROW_GAP);

        let structure_section = column![
            text(t!("shortcut_help_section_structure").to_string())
                .font(theme::INTER)
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_structure_expand").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_structure_reduce").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_structure_add_child").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_structure_add_sibling").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_structure_accept_all").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
        ]
        .spacing(theme::SHORTCUT_HELP_ROW_GAP);

        let movement_section = column![
            text(t!("shortcut_help_section_movement").to_string())
                .font(theme::INTER)
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_movement_word_cursor").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_movement_focus").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_movement_reorder").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_movement_outdent").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_movement_indent").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
        ]
        .spacing(theme::SHORTCUT_HELP_ROW_GAP);

        let backspace_section = column![
            text(t!("shortcut_help_section_backspace").to_string())
                .font(theme::INTER)
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_backspace_enter_multiselect").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_backspace_delete_multiselect").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
        ]
        .spacing(theme::SHORTCUT_HELP_ROW_GAP);

        let banner = container(
            column![
                title,
                rule::horizontal(1),
                global_section,
                structure_section,
                movement_section,
                backspace_section,
            ]
            .spacing(theme::SHORTCUT_HELP_SECTION_GAP),
        )
        .style(theme::shortcut_help_banner)
        .max_width(theme::SHORTCUT_HELP_MAX_WIDTH)
        .padding(theme::BANNER_PAD);

        Some(banner.into())
    }

    /// Render the breadcrumb navigation bar.
    fn render_breadcrumbs(&self) -> Element<'a, Message> {
        let layers = self.state.navigation.layers();
        if layers.is_empty() {
            // At root, no breadcrumbs needed
            return row![].into();
        }

        let mut crumbs = row![].spacing(theme::ACTION_GAP).align_y(iced::Alignment::Center);

        // Home button
        let home_btn = IconButton::action(
            icons::icon_house()
                .size(theme::TOOLBAR_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into(),
        )
        .on_press(Message::Navigation(NavigationMessage::Home));
        crumbs = crumbs.push(home_btn);

        // Separator before breadcrumbs
        crumbs = crumbs.push(text("›").style(theme::spine_text));

        // Each layer as a clickable breadcrumb
        for (i, layer) in layers.iter().enumerate() {
            // Get block text for label
            let label = self.state.store.point(&layer.block_id).unwrap_or_default();
            let display_label = truncate_for_display(&label, 30);

            // Add file path indicator if present
            let full_label = if let Some(path) = &layer.path {
                if let Some(file_name) = path.file_name() {
                    format!("{} ({})", display_label, file_name.to_string_lossy())
                } else {
                    display_label.to_string()
                }
            } else {
                display_label.to_string()
            };

            let crumb_btn = button(text(full_label.clone()).style(theme::spine_text))
                .style(theme::action_button)
                .padding(theme::BUTTON_PAD);

            // Make clickable except for current layer
            if i < layers.len() - 1 {
                crumbs = crumbs
                    .push(crumb_btn.on_press(Message::Navigation(NavigationMessage::GoTo(i))));
            } else {
                // Current layer - not clickable, emphasize as plain text in a container
                let current_crumb: Element<'a, Message> = text(full_label).into();
                crumbs = crumbs.push(
                    container(current_crumb)
                        .padding(Padding::ZERO.top(theme::BREADCRUMB_CURRENT_TEXT_TOP_PAD)),
                );
            }

            // Separator (not after last item)
            if i < layers.len() - 1 {
                crumbs = crumbs.push(text("›").style(theme::spine_text));
            }
        }

        crumbs.into()
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

        let Some(editor_content) = self.state.editor_buffers.get(block_id) else {
            let fallback_text = self.state.store.point(block_id).unwrap_or_default();
            tracing::error!(block_id = ?block_id, "missing editor content for rendered block");
            return container(text(fallback_text).style(theme::spine_text)).into();
        };

        let block_id_for_edit = *block_id;
        let row_context = self.action_row_context(block_id, editor_content.text());
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
        let is_target_block =
            is_pick_friend_mode && self.state.focus().is_some_and(|s| s.block_id != *block_id);

        // Check if this block should be highlighted due to friend panel hover
        let is_hovered_friend = self.state.ui().hovered_friend_block.is_some_and(|hovered_id| {
            hovered_id == *block_id
                && self.state.store.is_visible(block_id)
                && self.state.navigation.is_in_current_view(&self.state.store, block_id)
        });

        let point_editor: Element<'a, Message> = if is_target_block {
            // In friend picker mode, render as plain text so the button wrapper can capture clicks
            let point_text = self.state.store.point(block_id).unwrap_or_default();
            container(text(point_text)).width(Fill).height(Length::Shrink).into()
        } else {
            let mut editor = text_editor(editor_content)
                .placeholder(t!("doc_placeholder_point").to_string())
                .style(theme::point_editor)
                .on_action(move |action| {
                    Message::Edit(EditMessage::PointEdited { block_id: block_id_for_edit, action })
                })
                .key_binding(move |key_press| editor_key_binding(block_id_for_edit, key_press))
                .height(Length::Shrink);
            if let Some(wid) = self.state.editor_buffers.widget_id(block_id) {
                editor = editor.id(wid.clone());
            }
            editor.into()
        };

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
        if let Some(draft) = self.state.store.expansion_draft(block_id) {
            block = block.push(self.render_expansion_panel(block_id, draft));
        }
        if let Some(draft) = self.state.store.reduction_draft(block_id) {
            block = block.push(self.render_reduction_panel(block_id, draft));
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
            | (DocumentMode::Normal | DocumentMode::Find, Some(focused)) if focused == *block_id => {
                // Render the block as the focused block
                container(block).style(theme::focused_block).into()
            }
            | (DocumentMode::Normal | DocumentMode::Find, _) if is_hovered_friend => {
                // Highlight block when friend panel hovers over it
                container(block).style(theme::friend_picker_hover).into()
            }
            | (DocumentMode::Normal | DocumentMode::Find, _) => block.into(),
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
                container(block).style(theme::focused_block).into()
            }
            | (DocumentMode::Multiselect, _) => block.into(),
            | (_, None) => block.into(),
        }
    }

    fn render_expansion_panel(
        &self, block_id: &BlockId, draft: &'a ExpansionDraftRecord,
    ) -> Element<'a, Message> {
        let mut panel = column![].spacing(theme::PANEL_INNER_GAP);

        if let Some(rewrite) = &draft.rewrite {
            let old_text = self.state.store.point(block_id).unwrap_or_default();
            let diff_content = self.render_diff_content(&old_text, rewrite);

            panel = panel.push(
                column![]
                    .spacing(theme::PANEL_INNER_GAP)
                    .push(container(text(t!("doc_rewrite").to_string())).width(Length::Fill))
                    .push(container(diff_content).width(Length::Fill))
                    .push(
                        row![]
                            .width(Length::Fill)
                            .spacing(theme::PANEL_BUTTON_GAP)
                            .push(space::horizontal())
                            .push(
                                TextButton::action(t!("doc_apply_rewrite").to_string(), 13.0)
                                    .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                    .on_press(Message::Expand(ExpandMessage::ApplyRewrite(
                                        *block_id,
                                    ))),
                            )
                            .push(
                                TextButton::destructive(
                                    t!("doc_dismiss_rewrite").to_string(),
                                    13.0,
                                )
                                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                .on_press(Message::Expand(ExpandMessage::RejectRewrite(*block_id))),
                            ),
                    ),
            );
        }

        if !draft.children.is_empty() {
            panel = panel.push(
                row![]
                    .spacing(theme::PANEL_BUTTON_GAP)
                    .push(
                        container(text(t!("doc_child_suggestions").to_string()))
                            .width(Length::Fill),
                    )
                    .push(
                        TextButton::action(t!("doc_accept_all").to_string(), 13.0)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Expand(ExpandMessage::AcceptAllChildren(*block_id))),
                    )
                    .push(
                        TextButton::destructive(t!("doc_discard_all").to_string(), 13.0)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Expand(ExpandMessage::DiscardAllChildren(
                                *block_id,
                            ))),
                    ),
            );

            for (index, child) in draft.children.iter().enumerate() {
                panel = panel.push(
                    row![]
                        .spacing(theme::PANEL_BUTTON_GAP)
                        .push(container(text(child.as_str())).width(Length::Fill))
                        .push(
                            TextButton::action(t!("doc_keep").to_string(), 13.0)
                                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                .on_press(Message::Expand(ExpandMessage::AcceptChild {
                                    block_id: *block_id,
                                    child_index: index,
                                })),
                        )
                        .push(
                            TextButton::destructive(t!("doc_drop").to_string(), 13.0)
                                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                .on_press(Message::Expand(ExpandMessage::RejectChild {
                                    block_id: *block_id,
                                    child_index: index,
                                })),
                        ),
                );
            }
        }

        container(panel)
            .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
            .style(theme::draft_panel)
            .into()
    }

    fn render_reduction_panel(
        &self, block_id: &BlockId, draft: &'a ReductionDraftRecord,
    ) -> Element<'a, Message> {
        let old_text = self.state.store.point(block_id).unwrap_or_default();
        let diff_content = self.render_diff_content(&old_text, &draft.reduction);

        let mut panel = column![].spacing(theme::PANEL_INNER_GAP);

        panel = panel
            .push(container(text(t!("doc_reduce").to_string())).width(Length::Fill))
            .push(container(diff_content).width(Length::Fill))
            .push(
                row![]
                    .width(Length::Fill)
                    .spacing(theme::PANEL_BUTTON_GAP)
                    .push(space::horizontal())
                    .push(
                        TextButton::action(t!("doc_apply_reduction").to_string(), 13.0)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Reduce(ReduceMessage::Apply(*block_id))),
                    )
                    .push(
                        TextButton::destructive(t!("doc_dismiss_reduction").to_string(), 13.0)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Reduce(ReduceMessage::Reject(*block_id))),
                    ),
            );

        let valid_children: Vec<(usize, &BlockId)> = draft
            .redundant_children
            .iter()
            .enumerate()
            .filter(|(_, id)| self.state.store.node(id).is_some())
            .collect();

        if !valid_children.is_empty() {
            panel = panel.push(
                row![]
                    .spacing(theme::PANEL_BUTTON_GAP)
                    .push(
                        container(text(t!("doc_redundant_children").to_string()))
                            .width(Length::Fill),
                    )
                    .push(
                        TextButton::destructive(t!("doc_delete_all").to_string(), 13.0)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Reduce(ReduceMessage::AcceptAllDeletions(
                                *block_id,
                            ))),
                    )
                    .push(
                        TextButton::action(t!("doc_keep_all").to_string(), 13.0)
                            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                            .on_press(Message::Reduce(ReduceMessage::RejectAllDeletions(
                                *block_id,
                            ))),
                    ),
            );

            for (index, child_id) in &valid_children {
                let child_text = self.state.store.point(child_id).unwrap_or_default();
                panel = panel.push(
                    row![]
                        .spacing(theme::PANEL_BUTTON_GAP)
                        .push(container(text(child_text)).width(Length::Fill))
                        .push(
                            TextButton::destructive(t!("doc_delete").to_string(), 13.0)
                                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                .on_press(Message::Reduce(ReduceMessage::AcceptChildDeletion {
                                    block_id: *block_id,
                                    child_index: *index,
                                })),
                        )
                        .push(
                            TextButton::action(t!("doc_keep").to_string(), 13.0)
                                .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
                                .on_press(Message::Reduce(ReduceMessage::RejectChildDeletion {
                                    block_id: *block_id,
                                    child_index: *index,
                                })),
                        ),
                );
            }
        }

        container(panel)
            .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
            .style(theme::draft_panel)
            .into()
    }

    fn action_row_context(&self, block_id: &BlockId, point_text: String) -> RowContext {
        let expansion_draft = self.state.store.expansion_draft(block_id);
        let reduction_draft = self.state.store.reduction_draft(block_id);
        let node = self.state.store.node(block_id);
        RowContext {
            block_id: *block_id,
            point_text,
            has_draft: expansion_draft.is_some() || reduction_draft.is_some(),
            draft_suggestion_count: expansion_draft.map(|d| d.children.len()).unwrap_or(0)
                + reduction_draft.map(|d| d.redundant_children.len()).unwrap_or(0),
            has_expand_error: self.state.llm_requests.has_expand_error(*block_id),
            has_reduce_error: self.state.llm_requests.has_reduce_error(*block_id),
            is_expanding: self.state.llm_requests.is_expanding(*block_id),
            is_reducing: self.state.llm_requests.is_reducing(*block_id),
            is_mounted: self.state.store.mount_table().entry(*block_id).is_some(),
            is_unexpanded_mount: node.is_some_and(|n| n.mount_path().is_some()),
            has_children: !self.state.store.children(block_id).is_empty(),
        }
    }

    /// Render an inline diff between old and new text as two `rich_text` lines.
    ///
    /// Uses `rich_text` + `span` instead of `row` + `text` so that long text
    /// wraps naturally within the panel width. Deletions and additions are
    /// highlighted with colored background spans.
    fn render_diff_content(&self, old_text: &str, new_text: &str) -> Element<'a, Message> {
        use iced::widget::text::Span as RichSpan;

        let changes = word_diff(old_text, new_text);
        let pal = theme::palette_for_mode(self.state.is_dark_mode());

        let del_bg: Color = Color { a: 0.08, ..pal.danger };
        let add_bg: Color = Color { a: 0.08, ..pal.success };
        let ctx_color: Color = pal.ink;

        // Old line: unchanged + deleted spans (skip added).
        let old_spans: Vec<RichSpan<'_>> = changes
            .iter()
            .filter_map(|change| match change {
                | WordChange::Unchanged(s) => Some(span(s.clone()).color(ctx_color)),
                | WordChange::Deleted(s) => Some(
                    span(s.clone())
                        .color(ctx_color)
                        .background(del_bg)
                        .padding(Padding::from([0.0, theme::DIFF_HIGHLIGHT_PAD_H])),
                ),
                | WordChange::Added(_) => None,
            })
            .collect();

        // New line: unchanged + added spans (skip deleted).
        let new_spans: Vec<RichSpan<'_>> = changes
            .iter()
            .filter_map(|change| match change {
                | WordChange::Unchanged(s) => Some(span(s.clone()).color(ctx_color)),
                | WordChange::Added(s) => Some(
                    span(s.clone())
                        .color(ctx_color)
                        .background(add_bg)
                        .padding(Padding::from([0.0, theme::DIFF_HIGHLIGHT_PAD_H])),
                ),
                | WordChange::Deleted(_) => None,
            })
            .collect();

        let diff_content = column![
            rich_text(old_spans).width(Length::Fill),
            rich_text(new_spans).width(Length::Fill),
        ]
        .spacing(theme::DIFF_LINE_GAP);

        container(diff_content).width(Length::Fill).into()
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
            | Some(StatusChipVm::Loading { op: ActionId::Expand }) => {
                t!("doc_status_expanding").to_string()
            }
            | Some(StatusChipVm::Loading { op: ActionId::Reduce }) => {
                t!("doc_status_reducing").to_string()
            }
            | Some(StatusChipVm::Loading { .. }) => t!("doc_status_working").to_string(),
            | Some(StatusChipVm::Error { op, .. }) => t!(status_error_i18n_key(*op)).to_string(),
            | Some(StatusChipVm::DraftActive { suggestion_count }) if *suggestion_count > 0 => {
                t!("doc_status_draft_ready").to_string()
            }
            | Some(StatusChipVm::DraftActive { .. }) => t!("doc_status_draft").to_string(),
            | None => String::new(),
        };

        container(
            text(label).size(theme::SMALL_TEXT_SIZE).font(theme::INTER).style(theme::status_text),
        )
        .padding(Padding::from([theme::CHIP_PAD_V, theme::CHIP_PAD_H]))
        .width(Length::Shrink)
        .into()
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
    /// Inline-all uses a two-step confirmation button to reduce accidental
    /// irreversible operations.
    fn render_mount_indicator(
        &self, block_id: &BlockId, mount_path: &'a std::path::Path,
    ) -> Element<'a, Message> {
        let is_inline_confirmation_armed =
            self.state.ui().pending_inline_mount_confirmation == Some(*block_id);
        let is_mount_action_overflow_open =
            self.state.ui().mount_action_overflow_block == Some(*block_id);

        let move_label = t!("action_move_mount_file").to_string();
        let inline_label = t!("action_inline_mount").to_string();
        let inline_all_label = if is_inline_confirmation_armed {
            t!("action_confirm_inline_mount_all").to_string()
        } else {
            t!("action_inline_mount_all").to_string()
        };

        let move_btn: Element<'a, Message> = {
            let btn = IconButton::action(
                icons::icon_folder_input()
                    .size(theme::TOOLBAR_ICON_SIZE)
                    .line_height(iced::widget::text::LineHeight::Relative(1.0))
                    .into(),
            )
            .on_press(Message::MountFile(MountFileMessage::MoveMount(*block_id)));
            tooltip(
                btn,
                text(move_label).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
                tooltip::Position::Bottom,
            )
            .style(theme::tooltip)
            .padding(theme::TOOLTIP_PAD)
            .gap(theme::TOOLTIP_GAP)
            .into()
        };

        let inline_btn: Element<'a, Message> = {
            let btn = IconButton::action(
                icons::icon_chevron_down()
                    .size(theme::TOOLBAR_ICON_SIZE)
                    .line_height(iced::widget::text::LineHeight::Relative(1.0))
                    .into(),
            )
            .on_press(Message::MountFile(MountFileMessage::InlineMount(*block_id)));
            tooltip(
                btn,
                text(inline_label).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
                tooltip::Position::Bottom,
            )
            .style(theme::tooltip)
            .padding(theme::TOOLTIP_PAD)
            .gap(theme::TOOLTIP_GAP)
            .into()
        };

        let inline_all_btn: Element<'a, Message> = {
            let btn = if is_inline_confirmation_armed {
                IconButton::destructive(
                    icons::icon_chevrons_down()
                        .size(theme::TOOLBAR_ICON_SIZE)
                        .line_height(iced::widget::text::LineHeight::Relative(1.0))
                        .into(),
                )
            } else {
                IconButton::action(
                    icons::icon_chevrons_down()
                        .size(theme::TOOLBAR_ICON_SIZE)
                        .line_height(iced::widget::text::LineHeight::Relative(1.0))
                        .into(),
                )
            }
            .on_press(Message::MountFile(MountFileMessage::InlineMountAll(*block_id)));
            tooltip(
                btn,
                text(inline_all_label).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
                tooltip::Position::Bottom,
            )
            .style(theme::tooltip)
            .padding(theme::TOOLTIP_PAD)
            .gap(theme::TOOLTIP_GAP)
            .into()
        };

        let overflow_toggle_btn: Element<'a, Message> = {
            let (icon, tooltip_label) = if is_mount_action_overflow_open {
                (icons::icon_x(), t!("ui_close").to_string())
            } else {
                (icons::icon_ellipsis(), t!("ui_more").to_string())
            };
            let btn = IconButton::action(
                icon.size(theme::TOOLBAR_ICON_SIZE)
                    .line_height(iced::widget::text::LineHeight::Relative(1.0))
                    .into(),
            )
            .on_press(Message::Overlay(OverlayMessage::ToggleMountActionsOverflow(*block_id)));
            tooltip(
                btn,
                text(tooltip_label).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
                tooltip::Position::Bottom,
            )
            .style(theme::tooltip)
            .padding(theme::TOOLTIP_PAD)
            .gap(theme::TOOLTIP_GAP)
            .into()
        };

        let confirm_close_btn: Element<'a, Message> = {
            let btn = IconButton::action(
                icons::icon_x()
                    .size(theme::TOOLBAR_ICON_SIZE)
                    .line_height(iced::widget::text::LineHeight::Relative(1.0))
                    .into(),
            )
            .on_press(Message::MountFile(MountFileMessage::CancelInlineMountAllConfirm(*block_id)));
            tooltip(
                btn,
                text(t!("ui_close").to_string()).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
                tooltip::Position::Bottom,
            )
            .style(theme::tooltip)
            .padding(theme::TOOLTIP_PAD)
            .gap(theme::TOOLTIP_GAP)
            .into()
        };

        let mut header = row![
            text(mount_path.display().to_string())
                .font(theme::INTER)
                .size(theme::MOUNT_HEADER_TEXT_SIZE)
                .style(theme::spine_text),
            space::horizontal(),
        ]
        .spacing(theme::ACTION_GAP)
        .align_y(iced::Alignment::Center);

        if is_inline_confirmation_armed {
            header = header.push(
                text(t!("mount_inline_confirm_hint").to_string())
                    .font(theme::INTER)
                    .size(theme::SMALL_TEXT_SIZE)
                    .style(theme::spine_text),
            );
            header = header.push(inline_all_btn);
            header = header.push(confirm_close_btn);
            return header.into();
        }

        if is_mount_action_overflow_open {
            header = header.push(move_btn);
            header = header.push(inline_btn);
            header = header.push(inline_all_btn);
        }
        header = header.push(overflow_toggle_btn);
        header.into()
    }
}

/// Resolve text-editor key bindings for one block row.
///
/// Structural Enter shortcuts are intentionally resolved here (instead of the
/// global subscription path) so they can target the exact focused block and be
/// dispatched exactly once.
fn editor_key_binding(
    block_id: BlockId, key_press: text_editor::KeyPress,
) -> Option<text_editor::Binding<Message>> {
    // Only the focused editor should resolve structural shortcuts.
    // Other editor instances must ignore the key press so one chord yields one
    // mutation for the active block.
    if !matches!(key_press.status, text_editor::Status::Focused { .. }) {
        return text_editor::Binding::from_key_press(key_press);
    }

    if let Some(action_id) = shortcut_to_action(key_press.key.clone(), key_press.modifiers) {
        // Design decision:
        // - `Cmd/Ctrl+Enter` uses a dedicated edit message so add-child behavior
        //   does not depend on asynchronous modifier-state updates.
        // - `Cmd/Ctrl+Shift+Enter` stays on shortcut dispatch so sibling
        //   insertion uses the same action semantics as the action bar.
        // - Shortcut dispatch is restricted to the focused editor above so a
        //   single keypress cannot fan out to every visible editor widget.
        return match action_id {
            | ActionId::AddChild => {
                Some(text_editor::Binding::Custom(Message::Edit(EditMessage::AddEmptyFirstChild {
                    block_id,
                })))
            }
            | ActionId::AddSibling => {
                Some(text_editor::Binding::Custom(Message::Shortcut(ShortcutMessage::ForBlock {
                    block_id,
                    action_id,
                })))
            }
            | _ => {
                Some(text_editor::Binding::Custom(Message::Shortcut(ShortcutMessage::ForBlock {
                    block_id,
                    action_id,
                })))
            }
        };
    }

    if let Some(direction) = word_cursor_direction_for_key_press(&key_press) {
        return Some(text_editor::Binding::Custom(Message::Edit(EditMessage::MoveCursorByWord {
            block_id,
            direction,
        })));
    }

    text_editor::Binding::from_key_press(key_press)
}

fn word_cursor_direction_for_key_press(
    key_press: &text_editor::KeyPress,
) -> Option<super::edit::WordCursorDirection> {
    let modifiers = key_press.modifiers;
    if !(modifiers.command() || modifiers.control()) || modifiers.alt() || modifiers.shift() {
        return None;
    }

    match key_press.key {
        | iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowLeft) => {
            Some(super::edit::WordCursorDirection::Left)
        }
        | iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowRight) => {
            Some(super::edit::WordCursorDirection::Right)
        }
        | _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enter_key_press(modifiers: iced::keyboard::Modifiers) -> text_editor::KeyPress {
        text_editor::KeyPress {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
            modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
            physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Enter),
            modifiers,
            text: None,
            status: text_editor::Status::Focused { is_hovered: false },
        }
    }

    fn arrow_key_press(
        named: iced::keyboard::key::Named, code: iced::keyboard::key::Code,
        modifiers: iced::keyboard::Modifiers,
    ) -> text_editor::KeyPress {
        text_editor::KeyPress {
            key: iced::keyboard::Key::Named(named),
            modified_key: iced::keyboard::Key::Named(named),
            physical_key: iced::keyboard::key::Physical::Code(code),
            modifiers,
            text: None,
            status: text_editor::Status::Focused { is_hovered: false },
        }
    }

    #[test]
    fn command_enter_maps_to_add_empty_first_child_edit_message() {
        let (_, root) = AppState::test_state();

        let binding = editor_key_binding(root, enter_key_press(iced::keyboard::Modifiers::COMMAND));

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(Message::Edit(
                EditMessage::AddEmptyFirstChild { block_id }
            ))) if block_id == root
        ));
    }

    #[test]
    fn command_shift_enter_maps_to_add_sibling_shortcut() {
        let (_, root) = AppState::test_state();

        let binding = editor_key_binding(
            root,
            enter_key_press(iced::keyboard::Modifiers::COMMAND | iced::keyboard::Modifiers::SHIFT),
        );

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(Message::Shortcut(ShortcutMessage::ForBlock {
                block_id,
                action_id: ActionId::AddSibling,
            }))) if block_id == root
        ));
    }

    #[test]
    fn ctrl_shift_enter_maps_to_add_sibling_shortcut() {
        let (_, root) = AppState::test_state();

        let binding = editor_key_binding(
            root,
            enter_key_press(iced::keyboard::Modifiers::CTRL | iced::keyboard::Modifiers::SHIFT),
        );

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(Message::Shortcut(ShortcutMessage::ForBlock {
                block_id,
                action_id: ActionId::AddSibling,
            }))) if block_id == root
        ));
    }

    #[test]
    fn command_shift_enter_ignores_non_focused_editor() {
        let (_, root) = AppState::test_state();

        let mut key_press =
            enter_key_press(iced::keyboard::Modifiers::COMMAND | iced::keyboard::Modifiers::SHIFT);
        key_press.status = text_editor::Status::Active;

        let binding = editor_key_binding(root, key_press);

        assert!(binding.is_none());
    }

    #[test]
    fn command_left_maps_to_word_left_edit_message() {
        let (_, root) = AppState::test_state();

        let binding = editor_key_binding(
            root,
            arrow_key_press(
                iced::keyboard::key::Named::ArrowLeft,
                iced::keyboard::key::Code::ArrowLeft,
                iced::keyboard::Modifiers::COMMAND,
            ),
        );

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(Message::Edit(EditMessage::MoveCursorByWord {
                block_id,
                direction: super::super::edit::WordCursorDirection::Left,
            }))) if block_id == root
        ));
    }

    #[test]
    fn ctrl_right_maps_to_word_right_edit_message() {
        let (_, root) = AppState::test_state();

        let binding = editor_key_binding(
            root,
            arrow_key_press(
                iced::keyboard::key::Named::ArrowRight,
                iced::keyboard::key::Code::ArrowRight,
                iced::keyboard::Modifiers::CTRL,
            ),
        );

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(Message::Edit(EditMessage::MoveCursorByWord {
                block_id,
                direction: super::super::edit::WordCursorDirection::Right,
            }))) if block_id == root
        ));
    }
}

fn action_icon<'a>(id: ActionId) -> Element<'a, Message> {
    let icon = match id {
        | ActionId::Expand => icons::icon_maximize_2(),
        | ActionId::Reduce => icons::icon_minimize_2(),
        | ActionId::Cancel => icons::icon_circle_x(),
        | ActionId::AddChild => icons::icon_corner_down_right(),
        | ActionId::AddParent => icons::icon_corner_up_left(),
        | ActionId::AcceptAll => icons::icon_check_check(),
        | ActionId::Retry => icons::icon_refresh_cw(),
        | ActionId::DismissDraft => icons::icon_x(),
        | ActionId::CollapseBranch => icons::icon_chevron_down(),
        | ActionId::ExpandBranch => icons::icon_chevron_right(),
        | ActionId::AddSibling => icons::icon_plus(),
        | ActionId::DuplicateBlock => icons::icon_copy(),
        | ActionId::ArchiveBlock => icons::icon_archive(),
        | ActionId::SaveToFile => icons::icon_hard_drive_download(),
        | ActionId::LoadFromFile => icons::icon_hard_drive_upload(),
        | ActionId::EnterBlock => icons::icon_log_in(),
    };
    icon.size(theme::TOOLBAR_ICON_SIZE)
        .line_height(iced::widget::text::LineHeight::Relative(1.0))
        .into()
}
