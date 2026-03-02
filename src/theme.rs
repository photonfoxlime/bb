//! Calm paper-and-ink theme: palette, layout tokens, and per-widget style functions.
//!
//! **Add all UI numeric values here** (sizes, padding, gaps, colors) rather than
//! hardcoding them in other modules. This ensures consistent theming across the app.
//!
//! Supports light and dark variants.
//! `Theme::mode()` at render time, so all style functions adapt automatically
//! when the system appearance changes.

use iced::theme::{Base, Mode};
use iced::widget::{button, container, rule, text, text_editor};
use iced::{Color, Font, Theme, border};
use std::time::Duration;

pub const INTER: Font = Font::with_name("Inter");
pub const LXGW_WENKAI: Font = Font::with_name("LXGW WenKai");
pub const DEFAULT_FONT: Font = LXGW_WENKAI;

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
pub(crate) fn focused_palette(theme: &Theme) -> &'static Palette {
    match theme.mode() {
        | Mode::Dark => &DARK,
        | Mode::Light | Mode::None => &LIGHT,
    }
}

/// Resolve the palette from a dark-mode flag.
///
/// Useful in contexts where the Iced `Theme` reference is not available
/// (e.g. building `rich_text` `Span`s that require concrete colors).
pub(crate) fn palette_for_mode(is_dark: bool) -> &'static Palette {
    if is_dark { &DARK } else { &LIGHT }
}

// ── Layout tokens ────────────────────────────────────────────────────

/// Outer padding around the document canvas.
pub const CANVAS_PAD: f32 = 24.0;
/// Maximum content width for readability on standard screens.
pub const CANVAS_MAX_WIDTH_STANDARD: f32 = 720.0;
/// Maximum content width for readability on wide screens.
pub const CANVAS_MAX_WIDTH_WIDE: f32 = 1080.0;
/// Window width threshold for switching to wide layout.
pub const CANVAS_THRESHOLD_STANDARD: f32 = 1200.0;
/// Window width threshold for switching to ultra wide layout.
pub const CANVAS_THRESHOLD_WIDE: f32 = 1800.0;
/// Top padding inside the scrollable viewport.
pub const CANVAS_TOP: f32 = 12.0;

/// Computes the effective canvas max width based on window width.
pub fn canvas_max_width(window_width: f32) -> f32 {
    if window_width <= CANVAS_THRESHOLD_STANDARD {
        CANVAS_MAX_WIDTH_STANDARD
    } else if window_width <= CANVAS_THRESHOLD_WIDE {
        window_width * 0.6
    } else {
        CANVAS_MAX_WIDTH_WIDE
    }
}

/// Vertical gap between error banner and content.
pub const LAYOUT_GAP: f32 = 12.0;
/// Vertical gap between sibling blocks.
pub const BLOCK_GAP: f32 = 6.0;
/// Vertical gap between rows inside a single block (row, status, panels, children).
pub const BLOCK_INNER_GAP: f32 = 0.0;
/// Vertical gap between mount header and the block row.
pub const MOUNT_HEADER_ROW_GAP: f32 = 2.0;
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
/// Vertical offset applied to the current breadcrumb label to align with nav controls.
pub const BREADCRUMB_CURRENT_TEXT_TOP_PAD: f32 = 1.0;
/// Shared square footprint for icon-like controls in block rows.
///
/// Used by fold toggles, non-foldable ring markers, and overflow/action glyph buttons.
pub const ICON_BUTTON_SIZE: f32 = 24.0;
/// Icon size for toolbar buttons (select, move, etc).
pub const TOOLBAR_ICON_SIZE: f32 = 16.0;
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
/// Fixed width for the settings appearance-mode slider and labels.
pub const SETTINGS_APPEARANCE_SLIDER_WIDTH: f32 = 220.0;
/// Fixed width for the settings token-limit text input field.
pub const SETTINGS_TOKEN_INPUT_WIDTH: f32 = 80.0;
/// Vertical/horizontal padding inside draft panels.
pub const PANEL_PAD_V: f32 = 8.0;
pub const PANEL_PAD_H: f32 = 16.0;
/// Horizontal padding for diff highlight spans.
pub const DIFF_HIGHLIGHT_PAD_H: f32 = 2.0;
/// Padding inside the error banner.
pub const BANNER_PAD: f32 = 8.0;
/// Maximum width for the keyboard-shortcuts help banner.
pub const SHORTCUT_HELP_MAX_WIDTH: f32 = 560.0;
/// Font size for keyboard-shortcuts help banner title.
pub const SHORTCUT_HELP_TITLE_SIZE: f32 = 16.0;
/// Font size for keyboard-shortcuts help banner content rows.
pub const SHORTCUT_HELP_TEXT_SIZE: f32 = 13.0;
/// Vertical gap between sections in the keyboard-shortcuts help banner.
pub const SHORTCUT_HELP_SECTION_GAP: f32 = 8.0;
/// Vertical gap between shortcut rows inside one section.
pub const SHORTCUT_HELP_ROW_GAP: f32 = 4.0;

/// Page title font size.
pub const PAGE_TITLE_SIZE: f32 = 20.0;
/// Section title font size.
pub const SECTION_TITLE_SIZE: f32 = 16.0;
/// Label text size for secondary text.
pub const LABEL_TEXT_SIZE: f32 = 13.0;
/// Input and body text size.
pub const INPUT_TEXT_SIZE: f32 = 14.0;
/// Line height multiplier for text editors.
/// Set to 1.5 to accommodate CJK characters and ensure consistent cursor alignment.
pub const EDITOR_LINE_HEIGHT: f32 = 1.5;
/// Small text size for metadata and labels.
pub const SMALL_TEXT_SIZE: f32 = 12.0;

/// Vertical gap between major page sections.
pub const PAGE_SECTION_GAP: f32 = 24.0;
/// Vertical gap between form rows.
pub const FORM_ROW_GAP: f32 = 10.0;
/// Vertical gap between form sections.
pub const FORM_SECTION_GAP: f32 = 12.0;
/// Inline element gap.
pub const INLINE_GAP: f32 = 4.0;
/// Compact vertical padding.
pub const COMPACT_PAD_V: f32 = 6.0;
/// Compact horizontal padding.
pub const COMPACT_PAD_H: f32 = 10.0;

/// Fixed width for path labels in settings.
pub const PATH_LABEL_WIDTH: f32 = 90.0;

/// Font size for instruction panel button text.
pub const INSTRUCTION_BUTTON_SIZE: f32 = 13.0;
/// Height for instruction editor in the instruction panel.
pub const INSTRUCTION_EDITOR_HEIGHT: f32 = 80.0;
/// Timeout for LLM requests in instruction panel.
pub const INSTRUCTION_LLM_TIMEOUT: Duration = Duration::from_secs(30);

/// Point text truncation length in friends panel.
pub const FRIEND_POINT_TRUNCATE: usize = 30;
/// Gap between point text and "as" label in friends panel.
pub const FRIEND_AS_GAP: f32 = 6.0;
/// Font size for friend point text in friends panel.
pub const FRIEND_POINT_SIZE: f32 = 12.0;
/// Font size for friend perspective text in friends panel.
pub const FRIEND_PERSPECTIVE_SIZE: f32 = 12.0;
/// Height for friend perspective buttons and input.
pub const FRIEND_PERSPECTIVE_HEIGHT: f32 = 16.0;
/// Inner padding for compact perspective icon buttons.
pub const FRIEND_PERSPECTIVE_BUTTON_PAD: f32 = 2.0;
/// Icon size for perspective accept/cancel buttons.
pub const FRIEND_PERSPECTIVE_ICON_SIZE: f32 = 10.0;
/// Spacing inside friend row in friends panel.
pub const FRIEND_ROW_GAP: f32 = 4.0;
/// Font size for friend visibility toggle icons.
pub const FRIEND_TOGGLE_ICON_SIZE: f32 = 10.0;
/// Font size for friend visibility toggle buttons.
pub const FRIEND_TOGGLE_SIZE: f32 = 14.0;
/// Gap between visibility toggles in friends panel.
pub const FRIEND_TOGGLE_GAP: f32 = 8.0;

/// Outer margin for the floating find panel overlay.
pub const FIND_PANEL_MARGIN: f32 = 16.0;
/// Top offset ratio for the floating find panel (`0.382` = 38.2% of viewport height).
pub const FIND_PANEL_TOP_RATIO: f32 = 0.382;
/// Maximum width for the floating find panel.
pub const FIND_PANEL_MAX_WIDTH: f32 = 680.0;
/// Font size for find panel title text.
pub const FIND_TITLE_SIZE: f32 = 14.0;
/// Font size for find panel metadata and controls.
pub const FIND_META_SIZE: f32 = 12.0;
/// Font size for the find query text input.
pub const FIND_QUERY_SIZE: f32 = 14.0;
/// Padding for the find query text input.
pub const FIND_QUERY_PAD: f32 = 8.0;
/// Height of the find result list viewport.
pub const FIND_RESULT_LIST_HEIGHT: f32 = 280.0;
/// Vertical/horizontal padding for one find result row.
pub const FIND_RESULT_PAD_V: f32 = 6.0;
pub const FIND_RESULT_PAD_H: f32 = 8.0;
/// Vertical gap between point text and lineage text in one find result row.
pub const FIND_RESULT_LINE_GAP: f32 = 2.0;
/// Font size for primary find result point text.
pub const FIND_RESULT_POINT_SIZE: f32 = 13.0;
/// Font size for secondary find result lineage text.
pub const FIND_RESULT_META_SIZE: f32 = 11.0;
/// Truncation budget for primary find result point text.
pub const FIND_RESULT_POINT_TRUNCATE: usize = 72;
/// Truncation budget per lineage segment in find results.
pub const FIND_RESULT_LINEAGE_TRUNCATE: usize = 20;

/// Window width threshold for medium action layout.
pub const VIEWPORT_MEDIUM_MAX_WIDTH: f32 = 1200.0;
/// Window width threshold for compact action layout.
pub const VIEWPORT_COMPACT_MAX_WIDTH: f32 = 820.0;
/// Window width threshold for touch-compact action layout.
pub const VIEWPORT_TOUCH_COMPACT_MAX_WIDTH: f32 = 560.0;

/// Font size for mount header path labels.
pub const MOUNT_HEADER_TEXT_SIZE: f32 = 13.0;

// ── Theme constructor ────────────────────────────────────────────────

impl crate::app::AppState {
    /// Build the iced `Theme` for the given appearance mode.
    ///
    /// The mode is embedded in the extended palette's `is_dark` flag so that
    /// style functions can resolve the correct palette via [`focused_palette`].
    pub fn theme(is_dark: bool) -> Theme {
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

/// Panel toggle button style - highlighted when the panel is open.
///
/// When `is_active` is true, the button shows with accent color text and border,
/// indicating the panel is currently open. This provides visual feedback without
/// needing to change the button text.
pub fn panel_toggle_button(
    theme: &Theme, status: button::Status, is_active: bool,
) -> button::Style {
    let p = focused_palette(theme);
    let base = button::Style {
        background: if is_active { Some(p.tint.into()) } else { None },
        text_color: if is_active { p.accent } else { p.accent_muted },
        border: border::rounded(3).width(if is_active { 1 } else { 0 }).color(if is_active {
            p.accent
        } else {
            Color::TRANSPARENT
        }),
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

/// Mode button style for the modebar (normal/pick friend).
///
/// When `is_active` is true, the button shows with accent color text and border,
/// indicating that mode is currently active.
pub fn mode_button(theme: &Theme, status: button::Status, is_active: bool) -> button::Style {
    let p = focused_palette(theme);
    let base = button::Style {
        background: if is_active { Some(p.tint.into()) } else { None },
        text_color: if is_active { p.accent } else { p.accent_muted },
        border: border::rounded(3).width(if is_active { 1 } else { 0 }).color(if is_active {
            p.accent
        } else {
            Color::TRANSPARENT
        }),
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

/// Toggle button style - highlighted when active (on), muted when inactive (off).
pub fn toggle_button(is_on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status: button::Status| {
        let p = focused_palette(theme);
        let active = is_on;
        let base = button::Style {
            background: if active { Some(p.tint.into()) } else { None },
            text_color: if active { p.accent } else { p.accent_muted },
            border: border::rounded(3).width(if active { 1 } else { 0 }).color(if active {
                p.accent
            } else {
                Color::TRANSPARENT
            }),
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
        background: Some(Color { a: 0.15, ..p.danger }.into()),
        border: border::rounded(4).width(1).color(Color { a: 0.5, ..p.danger }),
        text_color: Some(p.danger),
        ..Default::default()
    }
}

/// Keyboard-shortcuts help banner container.
pub fn shortcut_help_banner(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(Color { a: 0.96, ..p.paper }.into()),
        border: border::rounded(6).width(1).color(Color { a: 0.5, ..p.accent }),
        text_color: Some(p.ink),
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

/// Friend picker hover — indicates block is clickable to select as friend.
pub fn friend_picker_hover(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(p.tint.into()),
        border: border::rounded(4).width(1).color(p.accent),
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

// ── Context menu styles ───────────────────────────────────────────────

/// Context menu container — elevated surface with subtle border.
pub fn context_menu(theme: &Theme) -> container::Style {
    let p = focused_palette(theme);
    container::Style {
        background: Some(p.paper.into()),
        text_color: Some(p.ink),
        border: border::rounded(4).width(1).color(Color { a: 0.15, ..p.ink }),
        shadow: iced::Shadow {
            color: Color { a: 0.2, ..Color::BLACK },
            offset: iced::Vector { x: 0.0, y: 2.0 },
            blur_radius: 8.0,
        },
        snap: false,
    }
}

/// Transparent container — for click-through overlays.
pub fn transparent(_theme: &Theme) -> container::Style {
    container::Style {
        background: None,
        text_color: None,
        border: border::rounded(0).width(0),
        snap: false,
        ..Default::default()
    }
}

// ── Context menu button style ────────────────────────────────────────

/// Context menu button — minimal hover effect.
pub fn context_menu_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = focused_palette(theme);
    match status {
        | button::Status::Active => button::Style {
            background: None,
            text_color: p.ink,
            border: border::rounded(2).width(0),
            snap: false,
            ..Default::default()
        },
        | button::Status::Hovered => button::Style {
            background: Some(Color { a: 0.08, ..p.ink }.into()),
            text_color: p.ink,
            border: border::rounded(2).width(0),
            snap: false,
            ..Default::default()
        },
        | button::Status::Pressed => button::Style {
            background: Some(Color { a: 0.15, ..p.ink }.into()),
            text_color: p.ink,
            border: border::rounded(2).width(0),
            snap: false,
            ..Default::default()
        },
        | button::Status::Disabled => button::Style {
            background: None,
            text_color: p.spine_light,
            border: border::rounded(2).width(0),
            snap: false,
            ..Default::default()
        },
    }
}
