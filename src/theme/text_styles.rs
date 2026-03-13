//! Text style functions re-exported through [`crate::theme`].

use super::focused_palette;
use iced::Theme;
use iced::widget::text;

/// Spine or structural marker text with low-contrast gray.
pub fn spine_text(theme: &Theme) -> text::Style {
    let p = focused_palette(theme);
    text::Style { color: Some(p.spine) }
}

/// Status-chip label text.
pub fn status_text(theme: &Theme) -> text::Style {
    let p = focused_palette(theme);
    text::Style { color: Some(p.accent_muted) }
}
