//! Calm paper-and-ink theme: palette, layout tokens, and per-widget style functions.

use iced::widget::{button, container, rule, text, text_editor};
use iced::{Color, Font, Theme, border};

pub const INTER: Font = Font::with_name("Inter");

// ── Palette ──────────────────────────────────────────────────────────

pub const PAPER: Color = Color::from_rgb(0.965, 0.957, 0.937);
pub const INK: Color = Color::from_rgb(0.18, 0.17, 0.16);
pub const ACCENT: Color = Color::from_rgb(0.35, 0.48, 0.62);
pub const ACCENT_MUTED: Color = Color::from_rgb(0.55, 0.62, 0.70);
pub const TINT: Color = Color::from_rgb(0.935, 0.925, 0.905);
pub const SPINE: Color = Color::from_rgb(0.65, 0.63, 0.60);
/// Lighter spine color for structural lines (less visual weight than markers).
pub const SPINE_LIGHT: Color = Color::from_rgb(0.78, 0.76, 0.73);
pub const DANGER: Color = Color::from_rgb(0.75, 0.28, 0.22);
pub const SUCCESS: Color = Color::from_rgb(0.30, 0.60, 0.38);
/// Very faint accent wash for active-block highlight.
pub const FOCUS_WASH: Color = Color { r: 0.35, g: 0.48, b: 0.62, a: 0.06 };

// ── Layout tokens ────────────────────────────────────────────────────

/// Outer padding around the document canvas.
pub const CANVAS_PAD: f32 = 24.0;
/// Maximum content width for readability.
pub const CANVAS_MAX_WIDTH: f32 = 720.0;
/// Top padding inside the scrollable viewport.
pub const CANVAS_TOP: f32 = 12.0;

/// Vertical gap between error banner and content.
pub const LAYOUT_GAP: f32 = 12.0;
/// Vertical gap between sibling blocks.
pub const BLOCK_GAP: f32 = 10.0;
/// Vertical gap between elements inside a single block (row, status, panels, children).
pub const BLOCK_INNER_GAP: f32 = 4.0;
/// Horizontal gap between items within a row (spine, marker, editor, actions).
pub const ROW_GAP: f32 = 6.0;
/// Horizontal gap between action buttons.
pub const ACTION_GAP: f32 = 6.0;
/// Horizontal gap between buttons inside draft panels.
pub const PANEL_BUTTON_GAP: f32 = 8.0;
/// Internal spacing for draft panel content.
pub const PANEL_INNER_GAP: f32 = 6.0;
/// Vertical spacing between diff lines.
pub const DIFF_LINE_GAP: f32 = 2.0;

/// Horizontal indent for child blocks / status / mount indicators.
pub const INDENT: f32 = 16.0;
/// Width of the spine rule column.
pub const SPINE_WIDTH: f32 = 4.0;
/// Width of the bullet marker column.
pub const MARKER_WIDTH: f32 = 12.0;
/// Top offset to vertically align the bullet marker with text.
pub const MARKER_TOP: f32 = 3.0;

/// Padding inside buttons and tooltips.
pub const BUTTON_PAD: f32 = 4.0;
/// Padding around tooltip text.
pub const TOOLTIP_PAD: f32 = 6.0;
/// Gap between tooltip and anchor.
pub const TOOLTIP_GAP: f32 = 4.0;
/// Padding inside status chips.
pub const CHIP_PAD_V: f32 = 2.0;
pub const CHIP_PAD_H: f32 = 8.0;
/// Vertical/horizontal padding inside draft panels.
pub const PANEL_PAD_V: f32 = 8.0;
pub const PANEL_PAD_H: f32 = 16.0;
/// Horizontal padding for diff highlight spans.
pub const DIFF_HIGHLIGHT_PAD_H: f32 = 2.0;
/// Padding inside the error banner.
pub const BANNER_PAD: f32 = 8.0;
/// Vertical padding for overflow section.
pub const OVERFLOW_PAD_V: f32 = 4.0;

// ── Theme constructor ────────────────────────────────────────────────

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

/// Active block row — faint accent wash to indicate which block is selected.
pub fn active_block(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(FOCUS_WASH.into()),
        border: border::rounded(4).width(0),
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

/// Diff deletion container — red-tinted background for removed words.
pub fn diff_deletion(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color { a: 0.08, ..DANGER }.into()),
        text_color: Some(INK),
        ..Default::default()
    }
}

/// Diff addition container — green-tinted background for added words.
pub fn diff_addition(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Color { a: 0.08, ..SUCCESS }.into()),
        text_color: Some(INK),
        ..Default::default()
    }
}

/// Diff context text — neutral styling for unchanged words.
pub fn diff_context(_theme: &Theme) -> text::Style {
    text::Style { color: Some(INK) }
}

// ── Rule styles ───────────────────────────────────────────────────────

/// Spine rule — a thin, low-contrast vertical line for tree structure.
/// Uses SPINE_LIGHT for subtlety; the bullet marker carries the stronger SPINE color.
pub fn spine_rule(_theme: &Theme) -> rule::Style {
    rule::Style {
        color: SPINE_LIGHT,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
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
