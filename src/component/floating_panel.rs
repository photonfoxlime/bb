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

/// Viewport-aware layout helper for floating overlays.
///
/// The shared floating panels all follow the same contract:
/// - horizontal size is clamped by [`theme::FLOATING_PANEL_MAX_WIDTH`],
/// - vertical size must stay inside the viewport margins,
/// - scrollable subregions should consume only the leftover height after the
///   panel's non-scroll chrome (header, inputs, hints) is accounted for.
///
/// `viewport_height == 0.0` is treated as "unknown yet" so the initial render
/// does not collapse the panel before the first window-resize event arrives.
#[derive(Debug, Clone, Copy)]
pub struct FloatingPanelLayout {
    viewport_width: f32,
    viewport_height: f32,
}

impl FloatingPanelLayout {
    /// Create a layout helper from the current window dimensions.
    pub fn new(viewport_width: f32, viewport_height: f32) -> Self {
        Self { viewport_width, viewport_height }
    }

    /// Compute the panel width after applying side margins and max width.
    pub fn panel_width(&self) -> f32 {
        if self.viewport_width > 0.0 {
            (self.viewport_width - (theme::FLOATING_PANEL_MARGIN * 2.0))
                .min(theme::FLOATING_PANEL_MAX_WIDTH)
        } else {
            theme::FLOATING_PANEL_MAX_WIDTH
        }
    }

    /// Maximum panel height available inside the viewport margins, if known.
    pub fn panel_max_height(&self) -> Option<f32> {
        (self.viewport_height > 0.0)
            .then(|| (self.viewport_height - (theme::FLOATING_PANEL_MARGIN * 2.0)).max(0.0))
    }

    /// Cap one scrollable body region so the panel chrome stays visible.
    ///
    /// `preferred_height` is the normal list viewport height for the panel.
    /// `reserved_height` is the estimated height of the non-scrollable sections
    /// inside the panel content column.
    ///
    /// Note: this deliberately reserves the panel padding and section gaps in
    /// one place so callers do not each reimplement slightly different viewport
    /// math. The returned height falls back to `preferred_height` until the
    /// first non-zero viewport height is known.
    pub fn list_height(&self, preferred_height: f32, reserved_height: f32) -> f32 {
        let Some(max_height) = self.panel_max_height() else {
            return preferred_height;
        };

        let available_height =
            (max_height - (theme::FLOATING_PANEL_PAD_V * 2.0) - reserved_height).max(0.0);

        preferred_height.min(available_height)
    }
}

/// Wraps content in the standard floating panel chrome.
///
/// Positions the panel centered horizontally and vertically. Width is clamped to
/// [`theme::FLOATING_PANEL_MAX_WIDTH`] with margins on the sides.
pub fn wrap<'a, M: 'a>(
    content: impl Into<Element<'a, M>>, layout: FloatingPanelLayout,
) -> Element<'a, M> {
    let panel = container(content)
        .style(theme::draft_panel)
        .padding(Padding::from([theme::FLOATING_PANEL_PAD_V, theme::FLOATING_PANEL_PAD_H]))
        .width(Length::Fixed(layout.panel_width()));
    let panel = if let Some(max_height) = layout.panel_max_height() {
        panel.max_height(max_height)
    } else {
        panel
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_width_respects_side_margins_and_max_width() {
        let layout = FloatingPanelLayout::new(480.0, 720.0);
        assert_eq!(layout.panel_width(), 448.0);

        let layout = FloatingPanelLayout::new(2000.0, 720.0);
        assert_eq!(layout.panel_width(), theme::FLOATING_PANEL_MAX_WIDTH);
    }

    #[test]
    fn list_height_uses_preferred_height_until_viewport_height_is_known() {
        let layout = FloatingPanelLayout::new(900.0, 0.0);
        assert_eq!(layout.list_height(280.0, 120.0), 280.0);
    }

    #[test]
    fn list_height_caps_scroll_region_to_remaining_panel_space() {
        let layout = FloatingPanelLayout::new(900.0, 260.0);
        assert_eq!(layout.list_height(280.0, 120.0), 80.0);
    }
}
