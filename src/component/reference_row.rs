//! Reusable row widget for one reference-panel entry.
//!
//! Friend relations and point links share the same summary-plus-perspective
//! layout. This component owns that shared layout while callers provide the
//! reference-specific summary content and trailing controls.

use crate::component::reference_list_row::ReferenceListRow;
use crate::component::reference_perspective::ReferencePerspectiveEditor;
use crate::theme;
use iced::Element;
use iced::widget::text;

/// Shared row widget for one item in the inline reference panel.
///
/// The caller owns all application-level messages and any specialized trailing
/// controls. This keeps the row reusable across friend and link entries.
pub struct ReferenceRow<Message> {
    /// Primary clickable summary content.
    pub primary: Element<'static, Message>,
    /// Persisted perspective label for the reference relation.
    pub perspective_label: String,
    /// Whether this row is currently showing the inline editor.
    pub is_editing: bool,
    /// Current inline-editor buffer when editing is active.
    pub current_input: String,
    /// Localized placeholder for an empty perspective.
    pub perspective_placeholder: String,
    /// Localized relation label between summary and perspective.
    pub relation_label: String,
    /// Trailing row-local controls.
    pub controls: Element<'static, Message>,
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

impl<Message: Clone + 'static> ReferenceRow<Message> {
    /// Consume the struct and produce the row element.
    pub fn view(self) -> Element<'static, Message> {
        let Self {
            primary,
            perspective_label,
            is_editing,
            current_input,
            perspective_placeholder,
            relation_label,
            controls,
            on_start_editing,
            on_clear_perspective,
            on_accept_perspective,
            on_submit_input,
            on_update_input,
        } = self;

        let detail = ReferencePerspectiveEditor {
            perspective_label,
            is_editing,
            current_input,
            perspective_placeholder,
            on_start_editing,
            on_clear_perspective,
            on_accept_perspective,
            on_submit_input,
            on_update_input,
        }
        .view();

        ReferenceListRow { primary, relation_label: Some(relation_label), detail, controls }.view()
    }

    /// Render a clickable text summary with the shared reference-row style.
    pub fn text_summary_button(label: String, on_press: Message) -> Element<'static, Message> {
        ReferenceListRow::summary_button(
            text(label).font(theme::INTER).size(theme::FRIEND_POINT_SIZE),
            on_press,
        )
    }
}
