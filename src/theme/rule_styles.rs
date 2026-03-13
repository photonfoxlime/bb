//! Rule style functions re-exported through [`crate::theme`].

use super::focused_palette;
use iced::Theme;
use iced::widget::rule;

/// Spine rule used for the tree structure column.
///
/// Uses `spine_light` for subtlety; the bullet marker carries the stronger
/// spine color.
pub fn spine_rule(theme: &Theme) -> rule::Style {
    let p = focused_palette(theme);
    rule::Style {
        color: p.spine_light,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}
