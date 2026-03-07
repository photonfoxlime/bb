//! Breadcrumb navigation bar component.
//!
//! Renders home button + separator + clickable layer labels for drill-down
//! navigation. Uses theme constants; labels are passed in from the parent.
//! The bar renders on an opaque surface so document content behind the overlay
//! does not compete with the navigation controls.

use crate::theme;
use iced::{
    Element, Padding,
    widget::{button, container, row, text},
};
use lucide_icons::iced as icons;

/// One breadcrumb layer: display label and whether it is the current (non-clickable) layer.
#[derive(Debug, Clone)]
pub struct BreadcrumbLayer {
    /// Label shown for this navigation layer.
    pub label: String,
    /// Whether this layer is the current location and therefore not clickable.
    pub is_current: bool,
}

/// Renders the breadcrumb bar.
pub struct Breadcrumbs;

impl Breadcrumbs {
    /// Build the breadcrumb element.
    ///
    /// - `layers`: navigation layers from root toward current; last is current.
    /// - `on_home`: message when home is pressed.
    /// - `on_go_to`: message when a prior layer is pressed; receives layer index.
    pub fn view<'a, Message: Clone + 'a>(
        layers: &[BreadcrumbLayer], on_home: Message, on_go_to: impl Fn(usize) -> Message + 'a,
    ) -> Element<'a, Message> {
        if layers.is_empty() {
            return row![].into();
        }

        let mut crumbs = row![].spacing(theme::ACTION_GAP).align_y(iced::Alignment::Center);

        let home_btn = crate::component::icon_button::IconButton::action(
            icons::icon_house()
                .size(theme::TOOLBAR_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into(),
        )
        .on_press(on_home);
        crumbs = crumbs.push(home_btn);

        crumbs = crumbs.push(text("›").style(theme::spine_text));

        for (i, layer) in layers.iter().enumerate() {
            let crumb_btn = button(text(layer.label.clone()).style(theme::spine_text))
                .style(theme::action_button)
                .padding(theme::BUTTON_PAD);

            if layer.is_current {
                let current_crumb: Element<'a, Message> = text(layer.label.clone()).into();
                crumbs = crumbs.push(
                    container(current_crumb)
                        .padding(Padding::ZERO.top(theme::BREADCRUMB_CURRENT_TEXT_TOP_PAD)),
                );
            } else {
                crumbs = crumbs.push(crumb_btn.on_press(on_go_to(i)));
            }

            if i < layers.len() - 1 {
                crumbs = crumbs.push(text("›").style(theme::spine_text));
            }
        }

        container(crumbs)
            .padding([theme::BREADCRUMB_BAR_PAD_V, theme::BREADCRUMB_BAR_PAD_H])
            .style(theme::breadcrumb_bar)
            .into()
    }
}
