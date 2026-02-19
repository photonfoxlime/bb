//! Calm paper-and-ink theme: palette, custom `Theme`, and per-widget style functions.

use iced::widget::{button, container, rule, text, text_editor};
use iced::{Color, Font, Theme, border};

pub const INTER: Font = Font::with_name("Inter");

pub const PAPER: Color = Color::from_rgb(0.965, 0.957, 0.937);
pub const INK: Color = Color::from_rgb(0.18, 0.17, 0.16);
pub const ACCENT: Color = Color::from_rgb(0.35, 0.48, 0.62);
pub const ACCENT_MUTED: Color = Color::from_rgb(0.55, 0.62, 0.70);
pub const TINT: Color = Color::from_rgb(0.935, 0.925, 0.905);
pub const SPINE: Color = Color::from_rgb(0.65, 0.63, 0.60);
pub const DANGER: Color = Color::from_rgb(0.75, 0.28, 0.22);
pub const SUCCESS: Color = Color::from_rgb(0.30, 0.60, 0.38);
pub fn app_theme() -> Theme {
    Theme::custom_with_fn(
        "bb paper".to_string(),
        iced::theme::Palette {
            background: PAPER,
            text: INK,
            primary: ACCENT,
            success: SUCCESS,
            warning: Color::from_rgb(0.85, 0.65, 0.20),
            danger: DANGER,
        },
        |palette| {
            // Generate the extended palette from our custom base, then
            // mark it as a light theme so built-in styles pick sensible
            // defaults.
            let mut ext = iced::theme::palette::Extended::generate(palette);
            ext.is_dark = false;
            ext
        },
    )
}

// ── Button styles ────────────────────────────────────────────────────

/// Annotation-style button: no background, subtle ink text that darkens on hover.
/// Feels like a marginalia link rather than a toolbar control.
pub fn action_button(theme: &Theme, status: button::Status) -> button::Style {
    let _ = theme;
    let base = button::Style {
        background: None,
        text_color: ACCENT_MUTED,
        border: border::rounded(3).width(0).color(Color::TRANSPARENT),
        shadow: Default::default(),
        snap: false,
    };
    match status {
        | button::Status::Active => base,
        | button::Status::Hovered => button::Style {
            text_color: INK,
            background: Some(TINT.into()),
            border: border::rounded(3).width(1).color(SPINE),
            ..base
        },
        | button::Status::Pressed => button::Style {
            text_color: INK,
            background: Some(Color { a: 0.15, ..ACCENT }.into()),
            border: border::rounded(3).width(1).color(ACCENT_MUTED),
            ..base
        },
        | button::Status::Disabled => button::Style { text_color: SPINE, ..base },
    }
}

/// Destructive action variant — uses danger color on hover/press.
pub fn destructive_button(theme: &Theme, status: button::Status) -> button::Style {
    let _ = theme;
    let base = button::Style {
        background: None,
        text_color: ACCENT_MUTED,
        border: border::rounded(3).width(0).color(Color::TRANSPARENT),
        shadow: Default::default(),
        snap: false,
    };
    match status {
        | button::Status::Active => base,
        | button::Status::Hovered => button::Style {
            text_color: DANGER,
            background: Some(Color { a: 0.08, ..DANGER }.into()),
            border: border::rounded(3).width(1).color(Color { a: 0.3, ..DANGER }),
            ..base
        },
        | button::Status::Pressed => button::Style {
            text_color: Color::from_rgb(0.9, 0.25, 0.18),
            background: Some(Color { a: 0.14, ..DANGER }.into()),
            ..base
        },
        | button::Status::Disabled => button::Style { text_color: SPINE, ..base },
    }
}

// ── Container styles ─────────────────────────────────────────────────

/// Main canvas container — paper background.
pub fn canvas(_theme: &Theme) -> container::Style {
    container::Style { background: Some(PAPER.into()), ..Default::default() }
}

/// Draft / expansion panel — very subtle bordered region.
pub fn draft_panel(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(TINT.into()),
        border: border::rounded(4).width(1).color(SPINE),
        ..Default::default()
    }
}

/// Error banner container.
pub fn error_banner(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color { a: 0.08, ..DANGER }.into()),
        border: border::rounded(4).width(1).color(Color { a: 0.3, ..DANGER }),
        text_color: Some(DANGER),
        ..Default::default()
    }
}

// ── Text editor style ────────────────────────────────────────────────

/// Borderless editor that blends with the paper surface.
pub fn point_editor(_theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let base = text_editor::Style {
        background: Color::TRANSPARENT.into(),
        border: border::rounded(2).width(0).color(Color::TRANSPARENT),
        placeholder: SPINE,
        value: INK,
        selection: Color { a: 0.18, ..ACCENT },
    };
    match status {
        | text_editor::Status::Active => base,
        | text_editor::Status::Hovered => text_editor::Style {
            border: border::rounded(2).width(1).color(Color { a: 0.2, ..SPINE }),
            ..base
        },
        | text_editor::Status::Focused { .. } => {
            text_editor::Style { border: border::rounded(2).width(1).color(ACCENT_MUTED), ..base }
        }
        | text_editor::Status::Disabled => text_editor::Style { value: ACCENT_MUTED, ..base },
    }
}

// ── Text styles ──────────────────────────────────────────────────────

/// Spine / structural marker text — low-contrast gray.
pub fn spine_text(_theme: &Theme) -> text::Style {
    text::Style { color: Some(SPINE) }
}

/// Status chip label text.
pub fn status_text(_theme: &Theme) -> text::Style {
    text::Style { color: Some(ACCENT_MUTED) }
}

// ── Rule styles ───────────────────────────────────────────────────────

/// Spine rule — a thin, low-contrast vertical line for tree structure.
pub fn spine_rule(_theme: &Theme) -> rule::Style {
    rule::Style { color: SPINE, radius: 0.0.into(), fill_mode: rule::FillMode::Full, snap: true }
}

// ── Tooltip style ────────────────────────────────────────────────────

/// Tooltip container — ink background with paper text for high contrast.
pub fn tooltip(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(INK.into()),
        text_color: Some(PAPER),
        border: border::rounded(4).width(0),
        ..Default::default()
    }
}
