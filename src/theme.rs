//! Calm paper-and-ink theme: palette, layout tokens, and per-widget style functions.
//!
//! Supports light and dark variants. The focused palette is determined by the
//! `Theme::mode()` at render time, so all style functions adapt automatically
//! when the system appearance changes.

use iced::theme::Base;
use iced::theme::Mode;
use iced::widget::{button, container, rule, text, text_editor};
use iced::{Color, Font, Theme, border};

pub const INTER: Font = Font::with_name("Inter");

// ── Palette ──────────────────────────────────────────────────────────

/// Semantic color slots shared by light and dark themes.
///
/// Every style function resolves colors through a `Palette` obtained via
/// [`focused_palette`], which inspects `Theme::mode()`.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Primary surface / background color.
    pub paper: Color,
    /// Primary text / foreground color.
    pub ink: Color,
    /// Brand accent — interactive elements, focus indicators.
    pub accent: Color,
    /// Subdued accent — secondary labels, disabled-ish text.
    pub accent_muted: Color,
    /// Subtle surface tint — hover backgrounds, panel fills.
    pub tint: Color,
    /// Structural gray — markers, placeholders, disabled text.
    pub spine: Color,
    /// Lighter structural gray — vertical rule lines.
    pub spine_light: Color,
    /// Danger / destructive action color.
    pub danger: Color,
    /// Success / positive feedback color.
    pub success: Color,
    /// Warning color.
    pub warning: Color,
    /// Very faint accent wash for focused block highlight.
    pub focus_wash: Color,
}

/// Light palette: warm off-white paper, near-black ink, soft blue accent.
pub const LIGHT: Palette = Palette {
    paper: Color::from_rgb(0.965, 0.957, 0.937),
    ink: Color::from_rgb(0.18, 0.17, 0.16),
    accent: Color::from_rgb(0.35, 0.48, 0.62),
    accent_muted: Color::from_rgb(0.55, 0.62, 0.70),
    tint: Color::from_rgb(0.935, 0.925, 0.905),
    spine: Color::from_rgb(0.65, 0.63, 0.60),
    spine_light: Color::from_rgb(0.78, 0.76, 0.73),
    danger: Color::from_rgb(0.75, 0.28, 0.22),
    success: Color::from_rgb(0.30, 0.60, 0.38),
    warning: Color::from_rgb(0.85, 0.65, 0.20),
    focus_wash: Color { r: 0.35, g: 0.48, b: 0.62, a: 0.06 },
};

/// Dark palette: deep charcoal surface, warm off-white text, desaturated blue accent.
pub const DARK: Palette = Palette {
    paper: Color::from_rgb(0.11, 0.11, 0.12),
    ink: Color::from_rgb(0.85, 0.83, 0.80),
    accent: Color::from_rgb(0.50, 0.65, 0.82),
    accent_muted: Color::from_rgb(0.45, 0.50, 0.58),
    tint: Color::from_rgb(0.15, 0.15, 0.16),
    spine: Color::from_rgb(0.38, 0.37, 0.35),
    spine_light: Color::from_rgb(0.25, 0.24, 0.23),
    danger: Color::from_rgb(0.85, 0.38, 0.32),
    success: Color::from_rgb(0.40, 0.72, 0.48),
    warning: Color::from_rgb(0.90, 0.72, 0.30),
    focus_wash: Color { r: 0.50, g: 0.65, b: 0.82, a: 0.08 },
};

/// Resolve the focused palette from the current theme's mode.
fn focused_palette(theme: &Theme) -> &'static Palette {
    match theme.mode() {
        | Mode::Dark => &DARK,
        | _ => &LIGHT,
    }
}

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

/// Padding inside buttons and tooltips.
pub const BUTTON_PAD: f32 = 4.0;
/// Shared square footprint for icon-like controls in block rows.
///
/// Used by fold toggles, non-foldable ring markers, and overflow/action glyph buttons.
pub const ICON_BUTTON_SIZE: f32 = 24.0;
/// Glyph size for non-foldable leaf ring markers.
pub const LEAF_RING_ICON_SIZE: f32 = 10.0;
/// Vertical nudge applied to row controls so their visual center matches
/// the first line of point text in the editor column.
pub const ROW_CONTROL_VERTICAL_PAD: f32 = 3.0;
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

/// Build the iced `Theme` for the given appearance mode.
///
/// The mode is embedded in the extended palette's `is_dark` flag so that
/// style functions can resolve the correct palette via [`focused_palette`].
pub fn app_theme(is_dark: bool) -> Theme {
    let pal = if is_dark { &DARK } else { &LIGHT };
    Theme::custom_with_fn(
        if is_dark { "bb night".to_string() } else { "bb paper".to_string() },
        iced::theme::Palette {
            background: pal.paper,
            text: pal.ink,
            primary: pal.accent,
            success: pal.success,
            warning: pal.warning,
            danger: pal.danger,
        },
        move |palette| {
            let mut ext = iced::theme::palette::Extended::generate(palette);
            ext.is_dark = is_dark;
            ext
        },
    )
}

// ── Button styles ────────────────────────────────────────────────────

/// Annotation-style button: no background, subtle ink text that darkens on hover.
/// Feels like a marginalia link rather than a toolbar control.
pub fn action_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = focused_palette(theme);
    let base = button::Style {
        background: None,
        text_color: p.accent_muted,
        border: border::rounded(3).width(0).color(Color::TRANSPARENT),
        shadow: Default::default(),
        snap: false,
    };
    match status {
        | button::Status::Active => base,
        | button::Status::Hovered => button::Style {
            text_color: p.ink,
            background: Some(p.tint.into()),
            border: border::rounded(3).width(1).color(p.spine),
            ..base
        },
        | button::Status::Pressed => button::Style {
            text_color: p.ink,
            background: Some(Color { a: 0.15, ..p.accent }.into()),
            border: border::rounded(3).width(1).color(p.accent_muted),
            ..base
        },
        | button::Status::Disabled => button::Style { text_color: p.spine, ..base },
    }
}

/// Destructive action variant — uses danger color on hover/press.
pub fn destructive_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = focused_palette(theme);
    let base = button::Style {
        background: None,
        text_color: p.accent_muted,
        border: border::rounded(3).width(0).color(Color::TRANSPARENT),
        shadow: Default::default(),
        snap: false,
    };
    match status {
        | button::Status::Active => base,
        | button::Status::Hovered => button::Style {
            text_color: p.danger,
            background: Some(Color { a: 0.08, ..p.danger }.into()),
            border: border::rounded(3).width(1).color(Color { a: 0.3, ..p.danger }),
            ..base
        },
        | button::Status::Pressed => button::Style {
            text_color: Color { a: 1.0, ..Color::from_rgb(0.9, 0.25, 0.18) },
            background: Some(Color { a: 0.14, ..p.danger }.into()),
            ..base
        },
        | button::Status::Disabled => button::Style { text_color: p.spine, ..base },
    }
}

// ── Container styles ─────────────────────────────────────────────────

/// Main canvas container — paper background.
pub fn canvas(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style { background: Some(p.paper.into()), ..Default::default() }
}

/// Draft / expansion panel — very subtle bordered region.
pub fn draft_panel(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(p.tint.into()),
        border: border::rounded(4).width(1).color(p.spine),
        ..Default::default()
    }
}

/// Error banner container.
pub fn error_banner(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(Color { a: 0.08, ..p.danger }.into()),
        border: border::rounded(4).width(1).color(Color { a: 0.3, ..p.danger }),
        text_color: Some(p.danger),
        ..Default::default()
    }
}

/// Focused block row — faint accent wash to indicate which block is selected.
pub fn focused_block(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(p.focus_wash.into()),
        border: border::rounded(4).width(0),
        ..Default::default()
    }
}

// ── Text editor style ────────────────────────────────────────────────

/// Borderless editor that blends with the paper surface.
pub fn point_editor(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let p = focused_palette(theme);
    let base = text_editor::Style {
        background: Color::TRANSPARENT.into(),
        border: border::rounded(2).width(0).color(Color::TRANSPARENT),
        placeholder: p.spine,
        value: p.ink,
        selection: Color { a: 0.18, ..p.accent },
    };
    match status {
        | text_editor::Status::Active => base,
        | text_editor::Status::Hovered => text_editor::Style {
            border: border::rounded(2).width(1).color(Color { a: 0.2, ..p.spine }),
            ..base
        },
        | text_editor::Status::Focused { .. } => {
            text_editor::Style { border: border::rounded(2).width(1).color(p.accent_muted), ..base }
        }
        | text_editor::Status::Disabled => text_editor::Style { value: p.accent_muted, ..base },
    }
}

// ── Text styles ──────────────────────────────────────────────────────

/// Spine / structural marker text — low-contrast gray.
pub fn spine_text(theme: &Theme) -> text::Style {
    let p = focused_palette(theme);
    text::Style { color: Some(p.spine) }
}

/// Status chip label text.
pub fn status_text(theme: &Theme) -> text::Style {
    let p = focused_palette(theme);
    text::Style { color: Some(p.accent_muted) }
}

/// Diff deletion container — red-tinted background for removed words.
pub fn diff_deletion(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(Color { a: 0.08, ..p.danger }.into()),
        text_color: Some(p.ink),
        ..Default::default()
    }
}

/// Diff addition container — green-tinted background for added words.
pub fn diff_addition(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(Color { a: 0.08, ..p.success }.into()),
        text_color: Some(p.ink),
        ..Default::default()
    }
}

/// Diff context text — neutral styling for unchanged words.
pub fn diff_context(theme: &Theme) -> text::Style {
    let p = focused_palette(theme);
    text::Style { color: Some(p.ink) }
}

// ── Rule styles ───────────────────────────────────────────────────────

/// Spine rule — a thin, low-contrast vertical line for tree structure.
/// Uses spine_light for subtlety; the bullet marker carries the stronger spine color.
pub fn spine_rule(theme: &Theme) -> rule::Style {
    let p = focused_palette(theme);
    rule::Style {
        color: p.spine_light,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

// ── Tooltip style ────────────────────────────────────────────────────

/// Tooltip container — inverted colors for high contrast against the surface.
pub fn tooltip(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(p.ink.into()),
        text_color: Some(p.paper),
        border: border::rounded(4).width(0),
        ..Default::default()
    }
}
