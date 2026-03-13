//! Calm paper-and-ink theme: palette, layout tokens, and per-widget style functions.
//!
//! **Add all UI numeric values here** (sizes, padding, gaps, colors) rather than
//! hardcoding them in other modules. This ensures consistent theming across the app.
//!
//! Supports light and dark variants.
//! `Theme::mode()` at render time, so all style functions adapt automatically
//! when the system appearance changes.

mod button_styles;
mod container_styles;
mod editor_styles;
mod rule_styles;
mod text_styles;

pub use self::{
    button_styles::{
        action_button, close_button, destructive_button, mode_button, panel_toggle_button,
        toggle_button,
    },
    container_styles::{
        canvas, draft_panel, error_banner, focused_block, friend_picker_hover, lineage_highlight,
        multiselect_selected, overlay_bar, shortcut_help_banner, tooltip,
    },
    editor_styles::point_editor,
    rule_styles::spine_rule,
    text_styles::{spine_text, status_text},
};

use iced::theme::{Base, Mode};
use iced::{Color, Font, Theme};
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
    /// Very faint lavender wash for ancestor-lineage highlight.
    pub lineage_wash: Color,
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
    focus_wash: Color { r: 0.35, g: 0.48, b: 0.62, a: 0.09 },
    lineage_wash: Color { r: 0.52, g: 0.38, b: 0.68, a: 0.09 },
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
    lineage_wash: Color { r: 0.65, g: 0.50, b: 0.85, a: 0.07 },
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
/// Fraction of window width used for canvas max width in medium layout.
pub const CANVAS_MAX_WIDTH_RATIO: f32 = 0.6;
/// Top padding inside the scrollable viewport.
pub const CANVAS_TOP: f32 = 12.0;
/// Fraction of viewport height used as extra bottom padding so the last item
/// can scroll up to the vertical center when at max scroll.
pub const CANVAS_SCROLL_TAIL_RATIO: f32 = 0.5;

/// Shared layout for floating overlay panels (find, link, archive).
pub const FLOATING_PANEL_MARGIN: f32 = 16.0;
/// Maximum width for floating panels.
pub const FLOATING_PANEL_MAX_WIDTH: f32 = 680.0;
/// Vertical padding inside floating overlay panels.
pub const FLOATING_PANEL_PAD_V: f32 = 14.0;
/// Horizontal padding inside floating overlay panels.
pub const FLOATING_PANEL_PAD_H: f32 = 16.0;
/// Vertical spacing between sections (header, input, result list) in floating panels.
pub const FLOATING_PANEL_SECTION_GAP: f32 = 10.0;
/// Horizontal gap between control buttons in floating panel headers.
pub const FLOATING_PANEL_CONTROL_GAP: f32 = 6.0;

// --- Link panel ---
/// Fixed height for the link panel candidate list.
pub const LINK_PANEL_LIST_HEIGHT: f32 = 280.0;
/// Computes the effective canvas max width based on window width.
pub fn canvas_max_width(window_width: f32) -> f32 {
    if window_width <= CANVAS_THRESHOLD_STANDARD {
        CANVAS_MAX_WIDTH_STANDARD
    } else if window_width <= CANVAS_THRESHOLD_WIDE {
        window_width * CANVAS_MAX_WIDTH_RATIO
    } else {
        CANVAS_MAX_WIDTH_WIDE
    }
}

/// Vertical gap between error banner and content.
pub const LAYOUT_GAP: f32 = 12.0;
/// Vertical gap between sibling blocks.
pub const BLOCK_GAP: f32 = 1.0;
/// Top padding inside a block's own content line (before head row).
pub const BLOCK_LINE_PAD_TOP: f32 = 2.0;
/// Bottom padding inside a block's own content line (after last row before children).
pub const BLOCK_LINE_PAD_BOTTOM: f32 = 2.0;
/// Vertical gap between mount header and the block row.
pub const MOUNT_HEADER_ROW_GAP: f32 = 2.0;
/// Horizontal gap between items within a row (spine, marker, editor, actions).
pub const ROW_GAP: f32 = 6.0;
/// Horizontal gap between action buttons.
pub const ACTION_GAP: f32 = 6.0;
/// Horizontal gap between buttons inside draft panels.
pub const PANEL_BUTTON_GAP: f32 = 0.0;
/// Internal spacing for draft panel content.
pub const PANEL_INNER_GAP: f32 = 6.0;
/// Vertical spacing between diff lines.
pub const DIFF_LINE_GAP: f32 = 2.0;

/// Horizontal indent for child blocks / status / mount indicators.
pub const INDENT: f32 = 16.0;
/// Width of the spine rule column.
pub const SPINE_WIDTH: f32 = 4.0;
/// Thickness of horizontal/vertical rule lines (dividers).
pub const RULE_WIDTH: f32 = 1.0;

/// Padding inside buttons and tooltips.
pub const BUTTON_PAD: f32 = 4.0;
/// Vertical padding inside floating document overlay bars.
pub const OVERLAY_BAR_PAD_V: f32 = 6.0;
/// Horizontal padding inside floating document overlay bars.
pub const OVERLAY_BAR_PAD_H: f32 = 8.0;
/// Vertical offset applied to the current breadcrumb label to align with nav controls.
pub const BREADCRUMB_CURRENT_TEXT_TOP_PAD: f32 = 1.0;
/// Truncation budget for breadcrumb layer labels.
pub const BREADCRUMB_LABEL_TRUNCATE: usize = 30;
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
pub const PANEL_PAD_H: f32 = 8.0;
/// Horizontal padding for diff highlight spans.
pub const DIFF_HIGHLIGHT_PAD_H: f32 = 2.0;
/// Vertical padding for diff highlight spans.
pub const DIFF_HIGHLIGHT_PAD_V: f32 = 0.0;
/// Background alpha for deleted text in diff view.
pub const DIFF_DELETED_BG_ALPHA: f32 = 0.08;
/// Background alpha for added text in diff view.
pub const DIFF_ADDED_BG_ALPHA: f32 = 0.08;
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
/// Horizontal gap between the chord and description columns in one shortcut row.
pub const SHORTCUT_HELP_COLUMN_GAP: f32 = 12.0;
/// Fixed width of the shortcut-chord column in the help banner.
pub const SHORTCUT_HELP_CHORD_WIDTH: f32 = 190.0;

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
/// Gap between section title text and trailing icon (e.g. task settings).
pub const TITLE_ICON_GAP: f32 = 6.0;
/// Compact vertical padding.
pub const COMPACT_PAD_V: f32 = 6.0;
/// Compact horizontal padding.
pub const COMPACT_PAD_H: f32 = 10.0;

/// Fixed width for path labels in settings.
pub const PATH_LABEL_WIDTH: f32 = 90.0;

/// Font size for probe panel button text.
pub const INSTRUCTION_BUTTON_SIZE: f32 = 13.0;
/// Height for the instruction editor in the probe panel.
pub const INSTRUCTION_EDITOR_HEIGHT: f32 = 80.0;
/// Timeout for LLM requests started from the probe panel.
pub const INSTRUCTION_LLM_TIMEOUT: Duration = Duration::from_secs(30);

/// Point text truncation length in the references panel.
pub const FRIEND_POINT_TRUNCATE: usize = 30;
/// Gap between point text and "as" label in the references panel.
pub const FRIEND_AS_GAP: f32 = 6.0;
/// Font size for friend point text in the references panel.
pub const FRIEND_POINT_SIZE: f32 = 12.0;
/// Font size for friend perspective text in the references panel.
pub const FRIEND_PERSPECTIVE_SIZE: f32 = 12.0;
/// Height for friend perspective buttons and input.
pub const FRIEND_PERSPECTIVE_HEIGHT: f32 = 16.0;
/// Inner padding for compact perspective icon buttons.
pub const FRIEND_PERSPECTIVE_BUTTON_PAD: f32 = 2.0;
/// Icon size for perspective accept/cancel buttons.
pub const FRIEND_PERSPECTIVE_ICON_SIZE: f32 = 10.0;
/// Spacing inside a row in the references panel.
pub const FRIEND_ROW_GAP: f32 = 4.0;
/// Font size for friend visibility toggle icons.
pub const FRIEND_TOGGLE_ICON_SIZE: f32 = 10.0;
/// Font size for friend visibility toggle buttons.
pub const FRIEND_TOGGLE_SIZE: f32 = 14.0;
/// Gap between visibility toggles in the references panel.
pub const FRIEND_TOGGLE_GAP: f32 = 8.0;

/// Font size for find panel title text.
pub const FIND_TITLE_SIZE: f32 = 14.0;
/// Font size for find panel metadata and controls.
pub const FIND_META_SIZE: f32 = 12.0;
/// Square footprint for compact icon buttons in the find panel controls row.
pub const FIND_CONTROL_BUTTON_SIZE: f32 = 20.0;
/// Icon glyph size inside find panel control buttons.
pub const FIND_CONTROL_ICON_SIZE: f32 = 12.0;
/// Internal padding for find panel control icon buttons.
pub const FIND_CONTROL_BUTTON_PAD: f32 = 2.0;
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

// ── Context menu ──────────────────────────────────────────────────────

/// Context menu container width.
pub const CONTEXT_MENU_WIDTH: f32 = 180.0;
/// Context menu action buttons per row before wrapping.
pub const CONTEXT_MENU_ACTIONS_PER_ROW: usize = 5;
/// Spacing between context menu action buttons.
pub const CONTEXT_MENU_ACTION_GAP: f32 = 4.0;
/// Spacing between context menu item rows.
pub const CONTEXT_MENU_ITEM_SPACING: f32 = 2.0;
/// Padding inside context menu container.
pub const CONTEXT_MENU_PAD: f32 = 4.0;
/// Border radius for context menu container.
pub const CONTEXT_MENU_BORDER_RADIUS: f32 = 4.0;
/// Border radius for context menu buttons.
pub const CONTEXT_MENU_BUTTON_BORDER_RADIUS: f32 = 2.0;
/// Hover background opacity for context menu buttons.
pub const CONTEXT_MENU_BUTTON_HOVER_OPACITY: f32 = 0.08;
/// Pressed background opacity for context menu buttons.
pub const CONTEXT_MENU_BUTTON_PRESSED_OPACITY: f32 = 0.15;
/// Context menu shadow blur radius.
pub const CONTEXT_MENU_SHADOW_BLUR: f32 = 8.0;
/// Context menu shadow offset Y.
pub const CONTEXT_MENU_SHADOW_OFFSET_Y: f32 = 2.0;
/// Context menu shadow opacity.
pub const CONTEXT_MENU_SHADOW_OPACITY: f32 = 0.2;
/// Context menu border opacity.
pub const CONTEXT_MENU_BORDER_OPACITY: f32 = 0.15;
/// Icon size inside context menu rows.
pub const CONTEXT_MENU_ICON_SIZE: f32 = 14.0;
/// Horizontal padding for context menu button content.
pub const CONTEXT_MENU_BUTTON_PAD_H: f32 = 8.0;

// ── Reference Link ─────────────────────────────────────────────────

/// Icon size inside a reference-link summary.
pub const LINK_CHIP_ICON_SIZE: f32 = 14.0;
/// Inner padding of the markdown preview container for a reference link.
pub const LINK_CHIP_PAD: f32 = 4.0;
/// Base text size used by inline markdown previews under expanded reference links.
pub const LINK_MARKDOWN_PREVIEW_TEXT_SIZE: f32 = 13.0;

// ── Style tokens (used by style functions) ────────────────────────────

/// Border radius for action buttons, panel toggle, mode button, toggle, destructive.
pub const BORDER_RADIUS_BUTTON: f32 = 3.0;
/// Border radius for panels, blocks, tooltip, context menu.
pub const BORDER_RADIUS_PANEL: f32 = 4.0;
/// Border radius for shortcut help banner.
pub const BORDER_RADIUS_BANNER: f32 = 6.0;
/// Border radius for point editor.
pub const BORDER_RADIUS_EDITOR: f32 = 2.0;

/// Accent background opacity on button hover/press.
pub const BUTTON_ACCENT_BG_OPACITY: f32 = 0.15;
/// Danger background opacity on destructive button hover.
pub const BUTTON_DANGER_BG_HOVER_OPACITY: f32 = 0.08;
/// Danger border opacity on destructive button hover.
pub const BUTTON_DANGER_BORDER_HOVER_OPACITY: f32 = 0.3;
/// Danger background opacity on destructive button press.
pub const BUTTON_DANGER_BG_PRESSED_OPACITY: f32 = 0.14;
/// Danger background opacity on close-button hover.
pub const BUTTON_CLOSE_BG_HOVER_OPACITY: f32 = 0.05;
/// Danger border opacity on close-button hover.
pub const BUTTON_CLOSE_BORDER_HOVER_OPACITY: f32 = 0.18;
/// Error banner background opacity.
pub const ERROR_BANNER_BG_OPACITY: f32 = 0.15;
/// Error banner border opacity.
pub const ERROR_BANNER_BORDER_OPACITY: f32 = 0.5;
/// Shortcut help banner background opacity.
pub const SHORTCUT_HELP_BG_OPACITY: f32 = 0.96;
/// Shortcut help banner border opacity.
pub const SHORTCUT_HELP_BORDER_OPACITY: f32 = 0.5;
/// Text editor selection background opacity.
pub const EDITOR_SELECTION_OPACITY: f32 = 0.18;
/// Text editor hover border opacity.
pub const EDITOR_HOVER_BORDER_OPACITY: f32 = 0.2;

/// Destructive button pressed text color (hardcoded for high contrast).
pub const DESTRUCTIVE_PRESSED_R: f32 = 0.9;
pub const DESTRUCTIVE_PRESSED_G: f32 = 0.25;
pub const DESTRUCTIVE_PRESSED_B: f32 = 0.18;

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
