//! Reusable row widget for a friend relation inside the friends panel.
//!
//! This component owns the per-row layout for friend context entries:
//! point summary, perspective editor, telescope toggles, and remove action.
//! Extracting the row keeps the panel module focused on list assembly and
//! message/state routing, which is the seam needed for future merged
//! friend-plus-link panel work.

use crate::component::reference_list_row::ReferenceListRow;
use crate::component::{icon_button::IconButton, text_button::TextButton};
use crate::text::truncate_for_display;
use crate::theme;
use iced::widget::{Id, button, row, text, text_input, tooltip};
use iced::{Element, Length, Padding};
use lucide_icons::iced as icons;

/// Widget id used by the inline perspective editor.
///
/// Note: only one friend perspective editor may be active at a time, so one
/// stable id is sufficient for focus transfer.
pub const FRIEND_PERSPECTIVE_INPUT_ID: &str = "friend-perspective-input";

/// Return the widget [`Id`] for the inline perspective editor.
pub fn friend_perspective_input_id() -> Id {
    Id::new(FRIEND_PERSPECTIVE_INPUT_ID)
}

/// Per-friend relation row for the inline friends panel.
///
/// The caller owns all application-level messages. This keeps the row purely
/// presentational and avoids coupling the reusable view code to `app::Message`.
pub struct FriendRow<Message> {
    /// Full point text of the friend block.
    pub point_text: String,
    /// Persisted perspective label for the friend relation.
    pub perspective_label: String,
    /// Whether this row is currently showing the inline editor.
    pub is_editing: bool,
    /// Current inline-editor buffer when editing is active.
    pub current_input: String,
    /// Whether parent lineage is included in LLM context.
    pub parent_lineage_telescope: bool,
    /// Whether children are included in LLM context.
    pub children_telescope: bool,
    /// Localized placeholder for an empty perspective.
    pub perspective_placeholder: String,
    /// Localized relation label between point and perspective.
    pub relation_label: String,
    /// Localized tooltip for the parent-lineage toggle.
    pub parent_toggle_tooltip: String,
    /// Localized tooltip for the children toggle.
    pub children_toggle_tooltip: String,
    /// Localized remove button label.
    pub remove_label: String,
    /// Message to emit when the point summary is pressed.
    pub on_press_point: Message,
    /// Message to emit when perspective editing should start.
    pub on_start_editing: Message,
    /// Message to emit when the perspective should be cleared.
    pub on_clear_perspective: Message,
    /// Message to emit when the accept button is pressed.
    pub on_accept_perspective: Message,
    /// Message to emit when Enter is pressed in the input.
    pub on_submit_input: Message,
    /// Message to emit when the parent-lineage toggle is pressed.
    pub on_toggle_parent_lineage: Message,
    /// Message to emit when the children toggle is pressed.
    pub on_toggle_children: Message,
    /// Message to emit when the friend should be removed.
    pub on_remove_friend: Message,
    /// Message factory for inline perspective input changes.
    pub on_update_input: fn(String) -> Message,
}

impl<Message: Clone + 'static> FriendRow<Message> {
    /// Consume the struct and produce the friend row element.
    pub fn view(self) -> Element<'static, Message> {
        let Self {
            point_text,
            perspective_label,
            is_editing,
            current_input,
            parent_lineage_telescope,
            children_telescope,
            perspective_placeholder,
            relation_label,
            parent_toggle_tooltip,
            children_toggle_tooltip,
            remove_label,
            on_press_point,
            on_start_editing,
            on_clear_perspective,
            on_accept_perspective,
            on_submit_input,
            on_toggle_parent_lineage,
            on_toggle_children,
            on_remove_friend,
            on_update_input,
        } = self;

        let relation_content = Self::view_relation_content(
            is_editing,
            &current_input,
            &perspective_label,
            &perspective_placeholder,
            on_start_editing,
            on_clear_perspective,
            on_accept_perspective,
            on_submit_input,
            on_update_input,
        );
        let point_text_element = Self::view_point_button(&point_text, on_press_point);
        let controls = Self::view_controls(
            parent_lineage_telescope,
            children_telescope,
            &parent_toggle_tooltip,
            &children_toggle_tooltip,
            &remove_label,
            on_toggle_parent_lineage,
            on_toggle_children,
            on_remove_friend,
        );

        ReferenceListRow {
            primary: point_text_element,
            relation_label: Some(relation_label),
            detail: relation_content,
            controls,
        }
        .view()
    }

    /// Render the clickable summary of the referenced friend block.
    fn view_point_button(point_text: &str, on_press: Message) -> Element<'static, Message> {
        let truncated_point = truncate_for_display(point_text, theme::FRIEND_POINT_TRUNCATE);
        ReferenceListRow::summary_button(
            text(truncated_point).font(theme::INTER).size(theme::FRIEND_POINT_SIZE),
            on_press,
        )
    }

    /// Render the perspective area in either read or edit mode.
    fn view_relation_content(
        is_editing: bool, current_input: &str, perspective_label: &str,
        perspective_placeholder: &str, on_start_editing: Message, on_clear_perspective: Message,
        on_accept_perspective: Message, on_submit_input: Message,
        on_update_input: fn(String) -> Message,
    ) -> Element<'static, Message> {
        if is_editing {
            let input = text_input(perspective_placeholder, current_input)
                .id(friend_perspective_input_id())
                .font(theme::INTER)
                .size(theme::FRIEND_PERSPECTIVE_SIZE)
                .padding(Padding::ZERO)
                .width(Length::Fill)
                .on_input(on_update_input)
                .on_submit(on_submit_input);

            let accept_btn = IconButton::action_with_size(
                icons::icon_check().size(theme::FRIEND_PERSPECTIVE_ICON_SIZE).into(),
                theme::FRIEND_PERSPECTIVE_HEIGHT,
                theme::FRIEND_PERSPECTIVE_BUTTON_PAD,
            )
            .on_press(on_accept_perspective);

            let clear_btn = IconButton::destructive_with_size(
                icons::icon_x().size(theme::FRIEND_PERSPECTIVE_ICON_SIZE).into(),
                theme::FRIEND_PERSPECTIVE_HEIGHT,
                theme::FRIEND_PERSPECTIVE_BUTTON_PAD,
            )
            .on_press(on_clear_perspective);

            return row![]
                .spacing(theme::INLINE_GAP)
                .push(input)
                .push(accept_btn)
                .push(clear_btn)
                .into();
        }

        if perspective_label.is_empty() {
            return button(
                text(perspective_placeholder.to_owned())
                    .font(theme::INTER)
                    .size(theme::FRIEND_PERSPECTIVE_SIZE)
                    .style(theme::spine_text),
            )
            .style(theme::action_button)
            .height(Length::Fixed(theme::FRIEND_PERSPECTIVE_HEIGHT))
            .width(Length::Fill)
            .padding(Padding::ZERO)
            .on_press(on_start_editing)
            .into();
        }

        row![]
            .spacing(theme::FRIEND_ROW_GAP)
            .push(
                button(
                    text(perspective_label.to_owned())
                        .font(theme::INTER)
                        .size(theme::FRIEND_PERSPECTIVE_SIZE)
                        .style(theme::spine_text),
                )
                .style(theme::action_button)
                .height(Length::Fixed(theme::FRIEND_PERSPECTIVE_HEIGHT))
                .width(Length::Fill)
                .padding(Padding::ZERO)
                .on_press(on_start_editing),
            )
            .push(
                IconButton::destructive_with_size(
                    icons::icon_x().size(theme::FRIEND_PERSPECTIVE_ICON_SIZE).into(),
                    theme::FRIEND_PERSPECTIVE_HEIGHT,
                    theme::FRIEND_PERSPECTIVE_BUTTON_PAD,
                )
                .on_press(on_clear_perspective),
            )
            .into()
    }

    /// Render the telescope toggles and remove action.
    fn view_controls(
        parent_lineage_telescope: bool, children_telescope: bool, parent_toggle_tooltip: &str,
        children_toggle_tooltip: &str, remove_label: &str, on_toggle_parent_lineage: Message,
        on_toggle_children: Message, on_remove_friend: Message,
    ) -> Element<'static, Message> {
        row![]
            .spacing(theme::FRIEND_TOGGLE_GAP)
            .padding(Padding::ZERO.left(theme::TOOLTIP_PAD))
            .align_y(iced::alignment::Vertical::Center)
            .push(
                tooltip(
                    IconButton::toggle_with_size(
                        icons::icon_corner_up_left().size(theme::FRIEND_TOGGLE_ICON_SIZE).into(),
                        parent_lineage_telescope,
                        theme::FRIEND_TOGGLE_SIZE,
                        0.0,
                    )
                    .on_press(on_toggle_parent_lineage),
                    text(parent_toggle_tooltip.to_owned())
                        .size(theme::SMALL_TEXT_SIZE)
                        .font(theme::INTER),
                    tooltip::Position::Bottom,
                )
                .style(theme::tooltip)
                .padding(theme::TOOLTIP_PAD)
                .gap(theme::TOOLTIP_GAP),
            )
            .push(
                tooltip(
                    IconButton::toggle_with_size(
                        icons::icon_corner_down_right().size(theme::FRIEND_TOGGLE_ICON_SIZE).into(),
                        children_telescope,
                        theme::FRIEND_TOGGLE_SIZE,
                        0.0,
                    )
                    .on_press(on_toggle_children),
                    text(children_toggle_tooltip.to_owned())
                        .size(theme::SMALL_TEXT_SIZE)
                        .font(theme::INTER),
                    tooltip::Position::Bottom,
                )
                .style(theme::tooltip)
                .padding(theme::TOOLTIP_PAD)
                .gap(theme::TOOLTIP_GAP),
            )
            .push(
                TextButton::destructive(remove_label.to_string(), theme::FRIEND_POINT_SIZE)
                    .height(Length::Fixed(theme::FRIEND_PERSPECTIVE_HEIGHT))
                    .padding(Padding::ZERO)
                    .on_press(on_remove_friend),
            )
            .into()
    }
}
