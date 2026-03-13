//! Shared row shell for items shown in the inline reference panel.
//!
//! Friend relations and point links now share one horizontal layout model:
//! a clickable primary summary on the left, optional metadata in the middle,
//! and row-local controls on the right.

use crate::theme;
use iced::widget::{button, row};
use iced::{Element, Length, Padding};

/// Row shell for one reference-panel item.
pub struct ReferenceListRow<Message> {
    /// Primary clickable summary content.
    pub primary: Element<'static, Message>,
    /// Optional text label between primary and detail content.
    pub relation_label: Option<String>,
    /// Detail content rendered after the optional relation label.
    pub detail: Element<'static, Message>,
    /// Trailing row-local controls.
    pub controls: Element<'static, Message>,
}

impl<Message: Clone + 'static> ReferenceListRow<Message> {
    /// Consume the struct and produce the row element.
    pub fn view(self) -> Element<'static, Message> {
        let Self { primary, relation_label, detail, controls } = self;

        let mut main = row![]
            .spacing(theme::FRIEND_ROW_GAP)
            .align_y(iced::alignment::Vertical::Top)
            .push(primary);

        if let Some(relation_label) = relation_label {
            main = main
                .push(iced::widget::Space::new().width(Length::Fixed(theme::FRIEND_AS_GAP)))
                .push(
                    iced::widget::text(relation_label)
                        .style(theme::spine_text)
                        .font(theme::INTER)
                        .size(theme::FRIEND_POINT_SIZE),
                )
                .push(iced::widget::Space::new().width(Length::Fixed(theme::FRIEND_AS_GAP)));
        }

        row![]
            .spacing(theme::PANEL_BUTTON_GAP)
            .align_y(iced::alignment::Vertical::Top)
            .push(main.push(detail).width(Length::Fill))
            .push(controls)
            .into()
    }

    /// Render the shared clickable summary button used by both friend and link rows.
    pub fn summary_button(
        content: impl Into<Element<'static, Message>>, on_press: Message,
    ) -> Element<'static, Message> {
        button(content.into())
            .style(move |theme, status| {
                let palette = theme::focused_palette(theme);
                button::Style {
                    background: if matches!(status, button::Status::Hovered) {
                        Some(palette.tint.into())
                    } else {
                        None
                    },
                    text_color: palette.ink,
                    border: iced::Border::default(),
                    shadow: Default::default(),
                    snap: false,
                }
            })
            .height(Length::Fixed(theme::FRIEND_PERSPECTIVE_HEIGHT))
            .padding(Padding::ZERO)
            .on_press(on_press)
            .into()
    }
}
