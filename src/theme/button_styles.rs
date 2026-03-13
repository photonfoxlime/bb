//! Button style functions re-exported through [`crate::theme`].
//!
//! Keeping button chrome here leaves `theme.rs` focused on tokens and palette
//! definitions while preserving the existing `theme::...` call surface.

use super::{
    BORDER_RADIUS_BUTTON, BUTTON_ACCENT_BG_OPACITY, BUTTON_CLOSE_BG_HOVER_OPACITY,
    BUTTON_CLOSE_BORDER_HOVER_OPACITY, BUTTON_DANGER_BG_HOVER_OPACITY,
    BUTTON_DANGER_BG_PRESSED_OPACITY, BUTTON_DANGER_BORDER_HOVER_OPACITY, DESTRUCTIVE_PRESSED_B,
    DESTRUCTIVE_PRESSED_G, DESTRUCTIVE_PRESSED_R, focused_palette,
};
use iced::widget::button;
use iced::{Color, Theme, border};

/// Annotation-style button: no background, subtle ink text that darkens on hover.
///
/// Feels like a marginalia link rather than a toolbar control.
pub fn action_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = focused_palette(theme);
    let base = button::Style {
        background: None,
        text_color: p.accent_muted,
        border: border::rounded(BORDER_RADIUS_BUTTON).width(0).color(Color::TRANSPARENT),
        shadow: Default::default(),
        snap: false,
    };
    match status {
        | button::Status::Active => base,
        | button::Status::Hovered => button::Style {
            text_color: p.ink,
            background: Some(p.tint.into()),
            border: border::rounded(BORDER_RADIUS_BUTTON).width(1).color(p.spine),
            ..base
        },
        | button::Status::Pressed => button::Style {
            text_color: p.ink,
            background: Some(Color { a: BUTTON_ACCENT_BG_OPACITY, ..p.accent }.into()),
            border: border::rounded(BORDER_RADIUS_BUTTON).width(1).color(p.accent_muted),
            ..base
        },
        | button::Status::Disabled => button::Style { text_color: p.spine, ..base },
    }
}

/// Panel toggle button style highlighted when the panel is open.
///
/// When `is_active` is true, the button shows with accent color text and border,
/// indicating the panel is currently open.
pub fn panel_toggle_button(
    theme: &Theme, status: button::Status, is_active: bool,
) -> button::Style {
    let p = focused_palette(theme);
    let base = button::Style {
        background: if is_active { Some(p.tint.into()) } else { None },
        text_color: if is_active { p.accent } else { p.accent_muted },
        border: border::rounded(BORDER_RADIUS_BUTTON)
            .width(if is_active { 1 } else { 0 })
            .color(if is_active { p.accent } else { Color::TRANSPARENT }),
        shadow: Default::default(),
        snap: false,
    };
    match status {
        | button::Status::Active => base,
        | button::Status::Hovered => button::Style {
            text_color: p.ink,
            background: Some(p.tint.into()),
            border: border::rounded(BORDER_RADIUS_BUTTON).width(1).color(p.spine),
            ..base
        },
        | button::Status::Pressed => button::Style {
            text_color: p.ink,
            background: Some(Color { a: BUTTON_ACCENT_BG_OPACITY, ..p.accent }.into()),
            border: border::rounded(BORDER_RADIUS_BUTTON).width(1).color(p.accent_muted),
            ..base
        },
        | button::Status::Disabled => button::Style { text_color: p.spine, ..base },
    }
}

/// Mode button style for the mode bar.
///
/// When `is_active` is true, the button shows with accent color text and border,
/// indicating that mode is currently active.
pub fn mode_button(theme: &Theme, status: button::Status, is_active: bool) -> button::Style {
    let p = focused_palette(theme);
    let base = button::Style {
        background: if is_active { Some(p.tint.into()) } else { None },
        text_color: if is_active { p.accent } else { p.accent_muted },
        border: border::rounded(BORDER_RADIUS_BUTTON)
            .width(if is_active { 1 } else { 0 })
            .color(if is_active { p.accent } else { Color::TRANSPARENT }),
        shadow: Default::default(),
        snap: false,
    };
    match status {
        | button::Status::Active => base,
        | button::Status::Hovered => button::Style {
            text_color: p.ink,
            background: Some(p.tint.into()),
            border: border::rounded(BORDER_RADIUS_BUTTON).width(1).color(p.spine),
            ..base
        },
        | button::Status::Pressed => button::Style {
            text_color: p.ink,
            background: Some(Color { a: BUTTON_ACCENT_BG_OPACITY, ..p.accent }.into()),
            border: border::rounded(BORDER_RADIUS_BUTTON).width(1).color(p.accent_muted),
            ..base
        },
        | button::Status::Disabled => button::Style { text_color: p.spine, ..base },
    }
}

/// Destructive action variant using danger color on hover and press.
pub fn destructive_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = focused_palette(theme);
    let base = button::Style {
        background: None,
        text_color: p.accent_muted,
        border: border::rounded(BORDER_RADIUS_BUTTON).width(0).color(Color::TRANSPARENT),
        shadow: Default::default(),
        snap: false,
    };
    match status {
        | button::Status::Active => base,
        | button::Status::Hovered => button::Style {
            text_color: p.danger,
            background: Some(Color { a: BUTTON_DANGER_BG_HOVER_OPACITY, ..p.danger }.into()),
            border: border::rounded(BORDER_RADIUS_BUTTON)
                .width(1)
                .color(Color { a: BUTTON_DANGER_BORDER_HOVER_OPACITY, ..p.danger }),
            ..base
        },
        | button::Status::Pressed => button::Style {
            text_color: Color {
                a: 1.0,
                ..Color::from_rgb(
                    DESTRUCTIVE_PRESSED_R,
                    DESTRUCTIVE_PRESSED_G,
                    DESTRUCTIVE_PRESSED_B,
                )
            },
            background: Some(Color { a: BUTTON_DANGER_BG_PRESSED_OPACITY, ..p.danger }.into()),
            ..base
        },
        | button::Status::Disabled => button::Style { text_color: p.spine, ..base },
    }
}

/// Close-button variant with a softer red hover than destructive actions.
pub fn close_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = focused_palette(theme);
    let base = button::Style {
        background: None,
        text_color: p.accent_muted,
        border: border::rounded(BORDER_RADIUS_BUTTON).width(0).color(Color::TRANSPARENT),
        shadow: Default::default(),
        snap: false,
    };
    match status {
        | button::Status::Active => base,
        | button::Status::Hovered => button::Style {
            text_color: p.danger,
            background: Some(Color { a: BUTTON_CLOSE_BG_HOVER_OPACITY, ..p.danger }.into()),
            border: border::rounded(BORDER_RADIUS_BUTTON)
                .width(1)
                .color(Color { a: BUTTON_CLOSE_BORDER_HOVER_OPACITY, ..p.danger }),
            ..base
        },
        | button::Status::Pressed => button::Style {
            text_color: Color {
                a: 1.0,
                ..Color::from_rgb(
                    DESTRUCTIVE_PRESSED_R,
                    DESTRUCTIVE_PRESSED_G,
                    DESTRUCTIVE_PRESSED_B,
                )
            },
            background: Some(Color { a: BUTTON_DANGER_BG_PRESSED_OPACITY, ..p.danger }.into()),
            border: border::rounded(BORDER_RADIUS_BUTTON)
                .width(1)
                .color(Color { a: BUTTON_DANGER_BORDER_HOVER_OPACITY, ..p.danger }),
            ..base
        },
        | button::Status::Disabled => button::Style { text_color: p.spine, ..base },
    }
}

/// Toggle button style highlighted when active and muted when inactive.
pub fn toggle_button(is_on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status: button::Status| {
        let p = focused_palette(theme);
        let base = button::Style {
            background: if is_on { Some(p.tint.into()) } else { None },
            text_color: if is_on { p.accent } else { p.accent_muted },
            border: border::rounded(BORDER_RADIUS_BUTTON)
                .width(if is_on { 1 } else { 0 })
                .color(if is_on { p.accent } else { Color::TRANSPARENT }),
            shadow: Default::default(),
            snap: false,
        };
        match status {
            | button::Status::Active => base,
            | button::Status::Hovered => button::Style {
                text_color: p.ink,
                background: Some(p.tint.into()),
                border: border::rounded(BORDER_RADIUS_BUTTON).width(1).color(p.spine),
                ..base
            },
            | button::Status::Pressed => button::Style {
                text_color: p.ink,
                background: Some(Color { a: BUTTON_ACCENT_BG_OPACITY, ..p.accent }.into()),
                border: border::rounded(BORDER_RADIUS_BUTTON).width(1).color(p.accent_muted),
                ..base
            },
            | button::Status::Disabled => button::Style { text_color: p.spine, ..base },
        }
    }
}
