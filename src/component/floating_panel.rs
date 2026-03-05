//! Reusable floating overlay panel for find, link, archive, and similar overlays.
//!
//! Provides consistent positioning (centered horizontally, offset from top),
//! sizing (viewport-aware max width), and styling (`draft_panel`).

use crate::theme;
use iced::widget::container;
use iced::{Element, Length, Padding};

/// Wraps content in the standard floating panel chrome.
///
/// Positions the panel centered horizontally, with top offset computed from
/// viewport height. Width is clamped to [`theme::FLOATING_PANEL_MAX_WIDTH`]
/// with margins on the sides.
pub fn wrap<'a, M: 'a>(content: impl Into<Element<'a, M>>, viewport_width: f32, viewport_height: f32) -> Element<'a, M> {
    let panel_width = if viewport_width > 0.0 {
        (viewport_width - (theme::FLOATING_PANEL_MARGIN * 2.0)).min(theme::FLOATING_PANEL_MAX_WIDTH)
    } else {
        theme::FLOATING_PANEL_MAX_WIDTH
    };
    let panel_top = if viewport_height > 0.0 {
        (viewport_height * theme::FLOATING_PANEL_TOP_RATIO).max(theme::FLOATING_PANEL_MARGIN)
    } else {
        theme::FLOATING_PANEL_MARGIN
    };

    let panel = container(content)
        .style(theme::draft_panel)
        .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
        .width(Length::Fixed(panel_width));

    container(container(panel).padding(Padding::new(theme::FLOATING_PANEL_MARGIN).top(panel_top)))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Top)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
