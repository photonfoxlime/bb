//! Text-editor style functions re-exported through [`crate::theme`].

use super::{
    BORDER_RADIUS_EDITOR, EDITOR_HOVER_BORDER_OPACITY, EDITOR_SELECTION_OPACITY, RULE_WIDTH,
    focused_palette,
};
use iced::widget::text_editor;
use iced::{Color, Theme, border};

/// Borderless point editor that blends with the paper surface.
pub fn point_editor(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let p = focused_palette(theme);
    let base = text_editor::Style {
        background: Color::TRANSPARENT.into(),
        border: border::rounded(BORDER_RADIUS_EDITOR).width(0).color(Color::TRANSPARENT),
        placeholder: p.spine,
        value: p.ink,
        selection: Color { a: EDITOR_SELECTION_OPACITY, ..p.accent },
    };
    match status {
        | text_editor::Status::Active => base,
        | text_editor::Status::Hovered => text_editor::Style {
            border: border::rounded(BORDER_RADIUS_EDITOR)
                .width(RULE_WIDTH)
                .color(Color { a: EDITOR_HOVER_BORDER_OPACITY, ..p.spine }),
            ..base
        },
        | text_editor::Status::Focused { .. } => text_editor::Style {
            border: border::rounded(BORDER_RADIUS_EDITOR).width(RULE_WIDTH).color(p.accent_muted),
            ..base
        },
        | text_editor::Status::Disabled => text_editor::Style { value: p.accent_muted, ..base },
    }
}
