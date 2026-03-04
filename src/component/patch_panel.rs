//! Generic panel component for LLM-produced draft suggestions.
//!
//! Accepts up to two independent sections, both optional:
//! - **Rewrite**: a proposed text change rendered as a word-level diff, or a
//!   free-form content element (e.g. an inquiry response), or a dismiss-only
//!   header row.
//! - **Children**: a list of text items the user can act on individually or
//!   in bulk (add new children, or delete existing ones).
//!
//! UI text is fully caller-controlled through the button/section structs, so
//! the same layout can serve amplify, atomize, distill, and probe operations.

use crate::app::diff::{WordChange, word_diff};
use crate::component::text_button::TextButton;
use crate::theme;
use iced::widget::{column, container, rich_text, row, space, span, text};
use iced::{Color, Element, Length, Padding};

/// Visual style for a panel action button.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PanelButtonStyle {
    Action,
    Destructive,
}

/// A labelled button specification for the patch panel.
pub(crate) struct PanelButton<Msg> {
    pub(crate) label: String,
    pub(crate) style: PanelButtonStyle,
    pub(crate) on_press: Msg,
}

/// The optional rewrite/response section of a patch panel.
pub(crate) enum RewriteSection<'a, Msg> {
    /// Word-level diff between the current and proposed point text.
    ///
    /// Both `old_text` and `new_text` are owned so callers can pass point
    /// text obtained from the store without lifetime coupling.
    Diff { title: String, old_text: String, new_text: String, buttons: Vec<PanelButton<Msg>> },
    /// Arbitrary pre-built content element (e.g. a scrollable inquiry
    /// response) paired with action buttons.
    Content { title: String, content: Element<'a, Msg>, buttons: Vec<PanelButton<Msg>> },
}

/// One item in a children list section.
pub(crate) struct ChildItem<Msg> {
    pub(crate) text: String,
    /// Primary action (e.g. "Keep" for add-children, "Delete" for delete-children).
    pub(crate) primary: PanelButton<Msg>,
    /// Secondary action (e.g. "Drop" / "Keep").
    pub(crate) secondary: PanelButton<Msg>,
}

/// A section listing child items with bulk and per-item action buttons.
///
/// Used both for *add-children* (amplify, atomize) and *delete-children*
/// (distill) by passing the appropriate button labels and messages.
pub(crate) struct ChildrenSection<Msg> {
    pub(crate) header: String,
    pub(crate) bulk_primary: PanelButton<Msg>,
    pub(crate) bulk_secondary: PanelButton<Msg>,
    pub(crate) items: Vec<ChildItem<Msg>>,
}

/// Render a patch panel inside a `draft_panel`-styled container.
///
/// Any section passed as `None` is omitted entirely. If a `ChildrenSection`
/// is provided but `items` is empty, that section is also omitted.
pub(crate) fn view<'a, Msg: Clone + 'a>(
    is_dark: bool, rewrite: Option<RewriteSection<'a, Msg>>, children: Option<ChildrenSection<Msg>>,
) -> Element<'a, Msg> {
    let mut panel = column![].spacing(theme::PANEL_INNER_GAP);

    if let Some(section) = rewrite {
        panel = panel.push(render_rewrite_section(is_dark, section));
    }

    if let Some(section) = children {
        if !section.items.is_empty() {
            let bulk_row = row![]
                .spacing(theme::PANEL_BUTTON_GAP)
                .push(container(text(section.header)).width(Length::Fill))
                .push(make_button(section.bulk_primary))
                .push(make_button(section.bulk_secondary));
            panel = panel.push(bulk_row);
            for item in section.items {
                let item_row = row![]
                    .spacing(theme::PANEL_BUTTON_GAP)
                    .push(container(text(item.text)).width(Length::Fill))
                    .push(make_button(item.primary))
                    .push(make_button(item.secondary));
                panel = panel.push(item_row);
            }
        }
    }

    container(panel)
        .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
        .style(theme::draft_panel)
        .into()
}

fn render_rewrite_section<'a, Msg: Clone + 'a>(
    is_dark: bool, section: RewriteSection<'a, Msg>,
) -> Element<'a, Msg> {
    match section {
        | RewriteSection::Diff { title, old_text, new_text, buttons } => {
            let diff = render_diff(is_dark, &old_text, &new_text);
            let mut header_row = row![]
                .width(Length::Fill)
                .spacing(theme::PANEL_BUTTON_GAP)
                .push(container(text(title)).width(Length::Fill));
            for btn in buttons {
                header_row = header_row.push(make_button(btn));
            }
            column![]
                .spacing(theme::PANEL_INNER_GAP)
                .push(header_row)
                .push(container(diff).width(Length::Fill))
                .into()
        }
        | RewriteSection::Content { title, content, buttons } => {
            let mut inner = column![]
                .spacing(theme::PANEL_INNER_GAP)
                .push(container(text(title)).width(Length::Fill))
                .push(content);
            if !buttons.is_empty() {
                let mut btn_row = row![]
                    .width(Length::Fill)
                    .spacing(theme::PANEL_BUTTON_GAP)
                    .push(space::horizontal());
                for btn in buttons {
                    btn_row = btn_row.push(make_button(btn));
                }
                inner = inner.push(btn_row);
            }
            inner.into()
        }
    }
}

fn make_button<'a, Msg: Clone + 'a>(btn: PanelButton<Msg>) -> Element<'a, Msg> {
    match btn.style {
        | PanelButtonStyle::Action => TextButton::action(btn.label, 13.0)
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(btn.on_press)
            .into(),
        | PanelButtonStyle::Destructive => TextButton::destructive(btn.label, 13.0)
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(btn.on_press)
            .into(),
    }
}

fn render_diff<'a, Msg: 'a>(is_dark: bool, old_text: &str, new_text: &str) -> Element<'a, Msg> {
    use iced::widget::text::Span as RichSpan;

    let changes = word_diff(old_text, new_text);
    let pal = theme::palette_for_mode(is_dark);
    let del_bg = Color { a: 0.08, ..pal.danger };
    let add_bg = Color { a: 0.08, ..pal.success };
    let ctx = pal.ink;

    let old_spans: Vec<RichSpan<'_>> = changes
        .iter()
        .filter_map(|c| match c {
            | WordChange::Unchanged(s) => Some(span(s.clone()).color(ctx)),
            | WordChange::Deleted(s) => Some(
                span(s.clone())
                    .color(ctx)
                    .background(del_bg)
                    .padding(Padding::from([0.0, theme::DIFF_HIGHLIGHT_PAD_H])),
            ),
            | WordChange::Added(_) => None,
        })
        .collect();

    let new_spans: Vec<RichSpan<'_>> = changes
        .iter()
        .filter_map(|c| match c {
            | WordChange::Unchanged(s) => Some(span(s.clone()).color(ctx)),
            | WordChange::Added(s) => Some(
                span(s.clone())
                    .color(ctx)
                    .background(add_bg)
                    .padding(Padding::from([0.0, theme::DIFF_HIGHLIGHT_PAD_H])),
            ),
            | WordChange::Deleted(_) => None,
        })
        .collect();

    container(
        column![
            rich_text(old_spans).width(Length::Fill),
            rich_text(new_spans).width(Length::Fill),
        ]
        .spacing(theme::DIFF_LINE_GAP),
    )
    .width(Length::Fill)
    .into()
}
