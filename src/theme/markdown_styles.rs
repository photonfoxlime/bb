//! Markdown preview helpers re-exported through [`crate::theme`].

use super::{DEFAULT_FONT, LINK_MARKDOWN_PREVIEW_TEXT_SIZE, palette_for_mode};
use iced::widget::markdown;

/// Build markdown renderer settings for inline reference-link previews.
///
/// Note: previews follow app typography (`DEFAULT_FONT`) instead of the
/// markdown widget defaults so mixed-script documents keep a consistent look.
pub(crate) fn markdown_preview_settings(is_dark: bool) -> markdown::Settings {
    let palette = palette_for_mode(is_dark);
    let mut style = markdown::Style::from_palette(iced::theme::Palette {
        background: palette.paper,
        text: palette.ink,
        primary: palette.accent,
        success: palette.success,
        danger: palette.danger,
        warning: palette.warning,
    });
    style.font = DEFAULT_FONT;
    style.inline_code_font = DEFAULT_FONT;
    style.code_block_font = DEFAULT_FONT;
    style.inline_code_color = palette.ink;
    style.inline_code_highlight.background = palette.tint.into();
    markdown::Settings::with_text_size(LINK_MARKDOWN_PREVIEW_TEXT_SIZE, style)
}
