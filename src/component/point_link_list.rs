//! Reusable link-chip list for block point attachments.
//!
//! This component renders the non-text portion of a block point: clickable link
//! chips and their optional inline previews. It is separated from the text
//! editor so panels can reuse link rendering without carrying editor-specific
//! key binding and context-menu concerns.

use crate::store::{BlockId, LinkKind, PointLink};
use crate::theme;
use iced::widget::{button, column, container, markdown, row, text};
use iced::{Element, Fill, Length};
use lucide_icons::iced as icons;

/// Link-chip list for the attachments attached to one block point.
///
/// The caller owns all application-level messages. This keeps the component
/// reusable in both row editors and future panel-based attachment views.
pub struct PointLinkList<'a, Message> {
    /// Block whose point owns these links.
    pub block_id: BlockId,
    /// Links to render in order.
    pub links: &'a [PointLink],
    /// Currently expanded chip index, if any.
    pub expanded_link_index: Option<usize>,
    /// Parsed markdown items for the expanded markdown preview.
    pub expanded_markdown_preview: Option<&'a [markdown::Item]>,
    /// Whether dark-mode markdown preview colors should be used.
    pub is_dark_mode: bool,
    /// Message to emit when a chip is pressed.
    ///
    /// Arguments: `(block_id, link_index)`.
    pub on_link_chip_toggle: fn(BlockId, usize) -> Message,
    /// Message to emit when the remove button is pressed.
    ///
    /// Arguments: `(block_id, link_index)`.
    pub on_remove_link: fn(BlockId, usize) -> Message,
    /// Message to emit when a markdown preview link is activated.
    ///
    /// Arguments: `(block_id, href)`.
    pub on_markdown_preview_link: fn(BlockId, String) -> Message,
}

impl<'a, Message: Clone + 'static + 'a> PointLinkList<'a, Message> {
    /// Consume the struct and produce the link-chip list element.
    pub fn view(self) -> Element<'a, Message> {
        let Self {
            block_id,
            links,
            expanded_link_index,
            expanded_markdown_preview,
            is_dark_mode,
            on_link_chip_toggle,
            on_remove_link,
            on_markdown_preview_link,
        } = self;

        let mut list = column![].width(Fill);
        for (index, link) in links.iter().enumerate() {
            list = list.push(Self::view_link_row(
                block_id,
                index,
                link,
                expanded_link_index,
                expanded_markdown_preview,
                is_dark_mode,
                on_link_chip_toggle,
                on_remove_link,
                on_markdown_preview_link,
            ));
        }
        list.into()
    }

    /// Render one link chip row and its optional preview.
    ///
    /// Note: markdown preview data is shared for the whole block because the
    /// app currently allows at most one expanded link chip per block.
    fn view_link_row(
        block_id: BlockId, index: usize, link: &'a PointLink, expanded_link_index: Option<usize>,
        expanded_markdown_preview: Option<&'a [markdown::Item]>, is_dark_mode: bool,
        on_link_chip_toggle: fn(BlockId, usize) -> Message,
        on_remove_link: fn(BlockId, usize) -> Message,
        on_markdown_preview_link: fn(BlockId, String) -> Message,
    ) -> Element<'a, Message> {
        let expand_btn = button(
            row![
                Self::link_icon(link.kind),
                text(link.display_text().to_owned()).size(theme::LINK_CHIP_TEXT_SIZE)
            ]
            .spacing(theme::LINK_CHIP_ICON_GAP)
            .align_y(iced::Alignment::Center),
        )
        .style(theme::link_chip_button)
        .padding(theme::LINK_CHIP_PAD)
        .on_press(on_link_chip_toggle(block_id, index));

        let remove_btn = button(
            icons::icon_x()
                .size(theme::LINK_CHIP_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0)),
        )
        .style(theme::link_chip_button)
        .padding(theme::LINK_CHIP_PAD)
        .on_press(on_remove_link(block_id, index));

        let mut chip_col = column![
            row![expand_btn, remove_btn]
                .spacing(theme::LINK_CHIP_ICON_GAP)
                .align_y(iced::Alignment::Center)
        ];

        if expanded_link_index == Some(index) {
            chip_col = chip_col.push(Self::view_preview(
                block_id,
                link,
                expanded_markdown_preview,
                is_dark_mode,
                on_markdown_preview_link,
            ));
        }

        chip_col.into()
    }

    /// Render the inline preview for an expanded link chip.
    ///
    /// Note: generic path links intentionally render nothing until the app has
    /// a richer preview model for non-image, non-markdown resources.
    fn view_preview(
        block_id: BlockId, link: &'a PointLink,
        expanded_markdown_preview: Option<&'a [markdown::Item]>, is_dark_mode: bool,
        on_markdown_preview_link: fn(BlockId, String) -> Message,
    ) -> Element<'a, Message> {
        match link.kind {
            | LinkKind::Image => {
                iced::widget::image(iced::widget::image::Handle::from_path(&link.href))
                    .width(Fill)
                    .into()
            }
            | LinkKind::Markdown => {
                if let Some(markdown_preview) = expanded_markdown_preview {
                    let markdown_widget: Element<'a, Message> = markdown::view(
                        markdown_preview,
                        theme::markdown_preview_settings(is_dark_mode),
                    )
                    .map(move |uri| on_markdown_preview_link(block_id, uri))
                    .into();
                    container(markdown_widget).padding(theme::LINK_CHIP_PAD).width(Fill).into()
                } else {
                    iced::widget::Space::new().height(Length::Shrink).into()
                }
            }
            | LinkKind::Path => iced::widget::Space::new().height(Length::Shrink).into(),
        }
    }

    /// Render the icon corresponding to a link kind.
    fn link_icon(kind: LinkKind) -> Element<'a, Message> {
        match kind {
            | LinkKind::Image => icons::icon_image().size(theme::LINK_CHIP_ICON_SIZE).into(),
            | LinkKind::Markdown => icons::icon_file_text().size(theme::LINK_CHIP_ICON_SIZE).into(),
            | LinkKind::Path => icons::icon_link().size(theme::LINK_CHIP_ICON_SIZE).into(),
        }
    }
}
