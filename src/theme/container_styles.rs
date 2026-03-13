//! Container style functions re-exported through [`crate::theme`].

use super::{
    BORDER_RADIUS_BANNER, BORDER_RADIUS_PANEL, ERROR_BANNER_BG_OPACITY,
    ERROR_BANNER_BORDER_OPACITY, SHORTCUT_HELP_BG_OPACITY, SHORTCUT_HELP_BORDER_OPACITY,
    focused_palette,
};
use iced::widget::container;
use iced::{Color, Theme, border};

/// Main canvas container with the paper background.
pub fn canvas(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style { background: Some(p.paper.into()), ..Default::default() }
}

/// Draft or expansion panel shown as a subtle bordered region.
pub fn draft_panel(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(p.tint.into()),
        border: border::rounded(BORDER_RADIUS_PANEL).width(1).color(p.spine),
        ..Default::default()
    }
}

/// Error banner container.
pub fn error_banner(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(Color { a: ERROR_BANNER_BG_OPACITY, ..p.danger }.into()),
        border: border::rounded(BORDER_RADIUS_PANEL)
            .width(1)
            .color(Color { a: ERROR_BANNER_BORDER_OPACITY, ..p.danger }),
        text_color: Some(p.danger),
        ..Default::default()
    }
}

/// Keyboard-shortcuts help banner container.
pub fn shortcut_help_banner(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(Color { a: SHORTCUT_HELP_BG_OPACITY, ..p.paper }.into()),
        border: border::rounded(BORDER_RADIUS_BANNER)
            .width(1)
            .color(Color { a: SHORTCUT_HELP_BORDER_OPACITY, ..p.accent }),
        text_color: Some(p.ink),
        ..Default::default()
    }
}

/// Floating document overlay bar container.
///
/// Note: this surface stays fully opaque so the scrollable document content
/// behind corner-mounted controls cannot reduce legibility.
pub fn overlay_bar(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(p.paper.into()),
        border: border::rounded(BORDER_RADIUS_BANNER).width(0),
        text_color: Some(p.ink),
        ..Default::default()
    }
}

/// Focused block row with a faint accent wash.
pub fn focused_block(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(p.focus_wash.into()),
        border: border::rounded(BORDER_RADIUS_PANEL).width(0),
        ..Default::default()
    }
}

/// Ancestor block own-line with a very faint lineage tint.
pub fn lineage_highlight(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(p.lineage_wash.into()),
        border: border::rounded(BORDER_RADIUS_PANEL).width(0),
        ..Default::default()
    }
}

/// Multiselect selected block with accent border and wash.
pub fn multiselect_selected(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(p.focus_wash.into()),
        border: border::rounded(BORDER_RADIUS_PANEL).width(1).color(p.accent),
        ..Default::default()
    }
}

/// Friend picker hover state showing clickable selection.
pub fn friend_picker_hover(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(p.tint.into()),
        border: border::rounded(BORDER_RADIUS_PANEL).width(1).color(p.accent),
        ..Default::default()
    }
}

/// Tooltip container with inverted colors for contrast.
pub fn tooltip(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(p.ink.into()),
        text_color: Some(p.paper),
        border: border::rounded(BORDER_RADIUS_PANEL).width(0),
        ..Default::default()
    }
}
