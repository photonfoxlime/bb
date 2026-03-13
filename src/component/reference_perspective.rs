//! Shared inline perspective editor for reference-panel rows.
//!
//! Friend relations and point links both expose an optional perspective string.
//! This component keeps the editing affordance visually identical across both
//! row types while leaving outer-row controls to the caller.

use crate::component::icon_button::IconButton;
use crate::theme;
use iced::widget::{Id, button, row, text, text_input};
use iced::{Element, Length, Padding};
use lucide_icons::iced as icons;

/// Widget id used by the inline reference perspective editor.
///
/// Note: only one reference perspective editor may be active at a time, so one
/// stable id is sufficient for focus transfer.
pub const REFERENCE_PERSPECTIVE_INPUT_ID: &str = "reference-perspective-input";

/// Return the widget [`Id`] for the inline reference perspective editor.
pub fn reference_perspective_input_id() -> Id {
    Id::new(REFERENCE_PERSPECTIVE_INPUT_ID)
}

/// Shared inline perspective editor used by reference rows.
pub struct ReferencePerspectiveEditor<Message> {
    /// Persisted perspective label for the reference relation.
    pub perspective_label: String,
    /// Whether this row is currently showing the inline editor.
    pub is_editing: bool,
    /// Current inline-editor buffer when editing is active.
    pub current_input: String,
    /// Localized placeholder for an empty perspective.
    pub perspective_placeholder: String,
    /// Message to emit when perspective editing should start.
    pub on_start_editing: Message,
    /// Message to emit when the perspective should be cleared.
    pub on_clear_perspective: Message,
    /// Message to emit when the accept button is pressed.
    pub on_accept_perspective: Message,
    /// Message to emit when Enter is pressed in the input.
    pub on_submit_input: Message,
    /// Message factory for inline perspective input changes.
    pub on_update_input: fn(String) -> Message,
}

impl<Message: Clone + 'static> ReferencePerspectiveEditor<Message> {
    /// Consume the struct and produce the perspective element.
    pub fn view(self) -> Element<'static, Message> {
        let Self {
            perspective_label,
            is_editing,
            current_input,
            perspective_placeholder,
            on_start_editing,
            on_clear_perspective,
            on_accept_perspective,
            on_submit_input,
            on_update_input,
        } = self;

        if is_editing {
            let input = text_input(&perspective_placeholder, &current_input)
                .id(reference_perspective_input_id())
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
                text(perspective_placeholder)
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
                    text(perspective_label)
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
}
