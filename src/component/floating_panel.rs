//! Reusable floating overlay panel for find, link, archive, and similar overlays.
//!
//! Provides consistent positioning (centered horizontally and vertically),
//! sizing (viewport-aware max width), and styling (`draft_panel`).
//!
//! # Building blocks
//!
//! - [`wrap`] — low-level chrome (positioning + `draft_panel` style).
//! - [`invisible_spacer`] — hidden state for conditional overlays in `stack!`.
//! - [`PanelHeader`] — title-left / controls-right header row.
//! - [`SelectableRow`] — candidate row with keyboard-selection highlight.

use crate::theme;
use iced::widget::{button, container, row};
use iced::{Alignment, Element, Length, Padding};

/// Wraps content in the standard floating panel chrome.
///
/// Positions the panel centered horizontally and vertically. Width is clamped to
/// [`theme::FLOATING_PANEL_MAX_WIDTH`] with margins on the sides.
pub fn wrap<'a, M: 'a>(
    content: impl Into<Element<'a, M>>, viewport_width: f32, _viewport_height: f32,
) -> Element<'a, M> {
    let panel_width = if viewport_width > 0.0 {
        (viewport_width - (theme::FLOATING_PANEL_MARGIN * 2.0)).min(theme::FLOATING_PANEL_MAX_WIDTH)
    } else {
        theme::FLOATING_PANEL_MAX_WIDTH
    };

    let panel = container(content)
        .style(theme::draft_panel)
        .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
        .width(Length::Fixed(panel_width));

    container(container(panel).padding(Padding::new(theme::FLOATING_PANEL_MARGIN)))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Return an invisible spacer that fills the overlay layer without consuming events.
///
/// Used as the "hidden" state for conditional floating overlays in `stack!`.
pub fn invisible_spacer<'a, M: 'a>() -> Element<'a, M> {
    container(iced::widget::Space::new()).width(Length::Fill).height(Length::Fill).into()
}

/// Title-left / controls-right header row for floating panels.
///
/// Renders `title` flush-left and `controls` flush-right, vertically centered.
pub struct PanelHeader;

impl PanelHeader {
    pub fn new<'a, M: 'a>(
        title: impl Into<Element<'a, M>>, controls: impl Into<Element<'a, M>>,
    ) -> Element<'a, M> {
        row![
            title.into(),
            container(controls.into())
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right)
        ]
        .align_y(Alignment::Center)
        .into()
    }
}

/// Clickable candidate row with keyboard-selection highlight.
///
/// Wraps arbitrary content in a padded container with optional
/// [`theme::friend_picker_hover`] highlight, inside a full-width
/// [`theme::action_button`]. Used by floating search panels (find, link)
/// for their scrollable result lists.
pub struct SelectableRow;

impl SelectableRow {
    pub fn new<'a, M: Clone + 'a>(
        content: impl Into<Element<'a, M>>, is_selected: bool, on_press: M,
    ) -> Element<'a, M> {
        let row_container = container(content)
            .width(Length::Fill)
            .padding(Padding::from([theme::FIND_RESULT_PAD_V, theme::FIND_RESULT_PAD_H]));
        let row_container = if is_selected {
            row_container.style(theme::friend_picker_hover)
        } else {
            row_container
        };
        button(row_container)
            .style(theme::action_button)
            .padding(Padding::ZERO)
            .width(Length::Fill)
            .on_press(on_press)
            .into()
    }
}
