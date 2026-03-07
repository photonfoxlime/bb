//! Markdown Mount v1 render and parse.
//!
//! The markdown format is a two-space indented bullet list where each block
//! point is a double-quoted, escape-encoded scalar.  A required preamble
//! comment identifies the format version.

use super::{BlockId, BlockNode, BlockStore};
use rustc_hash::FxHashMap;
use thiserror::Error;

/// Errors produced while parsing Markdown Mount v1 format.
#[derive(Debug, Error)]
pub enum MarkdownParseError {
    /// The first non-empty line is not the required preamble comment.
    #[error("line {line}: missing markdown mount preamble '<!-- bb-mount format=markdown v1 -->'")]
    MissingPreamble { line: usize },
    /// The input is empty or whitespace-only; no preamble was found.
    #[error("missing markdown mount preamble '<!-- bb-mount format=markdown v1 -->'")]
    EmptyDocument,
    /// Leading spaces on a list item are not a multiple of two.
    #[error("line {line}: indentation must be multiples of two spaces")]
    OddIndentation { line: usize },
    /// A line does not match the expected `- "..."` list item syntax.
    #[error("line {line}: expected '- \"...\"' markdown list item")]
    InvalidListItem { line: usize },
    /// A child is indented more than one level deeper than its predecessor.
    #[error("line {line}: indentation depth jumps more than one level")]
    DepthJump { line: usize },
    /// No parent block was recorded at the expected depth.
    #[error("line {line}: missing parent block at depth {depth}")]
    MissingParent { line: usize, depth: usize },
    /// The parent block id was not found in the nodes map.
    #[error("line {line}: parent block does not exist")]
    ParentNotFound { line: usize },
    /// The parent node is a mount variant and cannot accept children.
    #[error("line {line}: parent block is not a children node")]
    ParentNotChildren { line: usize },
    /// The preamble was found but no list items followed.
    #[error("markdown mount file contains no block items")]
    NoBlockItems,
    /// A trailing backslash with no character after it inside a quoted point.
    #[error("line {line}: trailing backslash in escaped point")]
    TrailingBackslash { line: usize },
    /// An unrecognized `\X` escape sequence inside a quoted point.
    #[error("line {line}: unsupported escape sequence \\{ch}")]
    UnsupportedEscape { line: usize, ch: char },
}

impl BlockStore {
    /// Render a projected mount store into Markdown Mount v1.
    ///
    /// Mapping rules (block graph -> markdown):
    ///
    /// 1. Emit a required preamble line:
    ///    `<!-- bb-mount format=markdown v1 -->`.
    /// 2. Emit each root block as a top-level list item in root order.
    /// 3. Emit each child block as a nested list item in child order.
    /// 4. Indent nested list items by two spaces per depth level.
    /// 5. Serialize each block point as a double-quoted scalar:
    ///    `- "<escaped-point>"`.
    /// 6. Escape point text with [`Self::escape_markdown_point`].
    ///
    /// Notes:
    /// - This projection intentionally writes only structural hierarchy and
    ///   point text for parser-friendly, deterministic output.
    /// - Runtime-only metadata (drafts, fold state, mount table) is excluded.
    pub(crate) fn render_markdown_mount_store(store: &BlockStore) -> String {
        let mut output = String::from("<!-- bb-mount format=markdown v1 -->\n");
        for &root in store.roots() {
            Self::render_markdown_node(store, root, 0, &mut output);
        }
        output
    }

    /// Parse Markdown Mount v1 into a projected mount store.
    ///
    /// The parser accepts exactly the markdown structure emitted by
    /// [`Self::render_markdown_mount_store`]: preamble line + two-space nested
    /// bullet list with quoted and escaped point text.
    pub(crate) fn parse_markdown_mount_store(
        markdown: &str,
    ) -> Result<BlockStore, MarkdownParseError> {
        let mut nodes: FxHashMap<BlockId, BlockNode> = FxHashMap::default();
        let mut points: FxHashMap<BlockId, String> = FxHashMap::default();
        let mut roots: Vec<BlockId> = Vec::new();
        let mut path_by_depth: Vec<BlockId> = Vec::new();

        let mut saw_preamble = false;
        let mut saw_item = false;

        for (line_index, raw_line) in markdown.lines().enumerate() {
            let line = line_index + 1;
            let trimmed_line = raw_line.trim_end();
            if trimmed_line.trim().is_empty() {
                continue;
            }
            if !saw_preamble {
                if trimmed_line == "<!-- bb-mount format=markdown v1 -->" {
                    saw_preamble = true;
                    continue;
                }
                return Err(MarkdownParseError::MissingPreamble { line });
            }

            let depth_spaces = raw_line.chars().take_while(|ch| *ch == ' ').count();
            if depth_spaces % 2 != 0 {
                return Err(MarkdownParseError::OddIndentation { line });
            }
            let depth = depth_spaces / 2;
            let content = &raw_line[depth_spaces..];

            if !content.starts_with("- \"") || !content.ends_with('"') {
                return Err(MarkdownParseError::InvalidListItem { line });
            }

            let quoted_content = &content[3..content.len() - 1];
            let point = Self::unescape_markdown_point(quoted_content).map_err(|e| match e {
                | UnescapeError::TrailingBackslash => {
                    MarkdownParseError::TrailingBackslash { line }
                }
                | UnescapeError::UnsupportedEscape(ch) => {
                    MarkdownParseError::UnsupportedEscape { line, ch }
                }
            })?;

            if depth > path_by_depth.len() {
                return Err(MarkdownParseError::DepthJump { line });
            }
            path_by_depth.truncate(depth);

            let id = BlockStore::insert_node(&mut nodes, BlockNode::with_children(vec![]));
            points.insert(id, point);
            saw_item = true;

            if depth == 0 {
                roots.push(id);
            } else {
                let Some(parent_id) = path_by_depth.get(depth - 1).copied() else {
                    return Err(MarkdownParseError::MissingParent { line, depth: depth - 1 });
                };
                let Some(parent) = nodes.get_mut(&parent_id) else {
                    return Err(MarkdownParseError::ParentNotFound { line });
                };
                let Some(children) = parent.children_mut() else {
                    return Err(MarkdownParseError::ParentNotChildren { line });
                };
                children.push(id);
            }

            path_by_depth.push(id);
        }

        if !saw_preamble {
            return Err(MarkdownParseError::EmptyDocument);
        }
        if !saw_item {
            return Err(MarkdownParseError::NoBlockItems);
        }

        Ok(BlockStore::new(roots, nodes, points))
    }

    /// Emit one block as a markdown list item, then recurse into children.
    fn render_markdown_node(store: &BlockStore, id: BlockId, depth: usize, out: &mut String) {
        let indent = "  ".repeat(depth);
        let point = store.point(&id).unwrap_or_default();
        let escaped = Self::escape_markdown_point(&point);
        out.push_str(&indent);
        out.push_str("- \"");
        out.push_str(&escaped);
        out.push_str("\"\n");
        for child in store.children(&id) {
            Self::render_markdown_node(store, *child, depth + 1, out);
        }
    }

    /// Escape point text used in markdown quoted scalars.
    ///
    /// Escapes: `\\`, `"`, `\n`, `\r`, and `\t`.
    fn escape_markdown_point(point: &str) -> String {
        let mut escaped = String::with_capacity(point.len());
        for ch in point.chars() {
            match ch {
                | '\\' => escaped.push_str("\\\\"),
                | '"' => escaped.push_str("\\\""),
                | '\n' => escaped.push_str("\\n"),
                | '\r' => escaped.push_str("\\r"),
                | '\t' => escaped.push_str("\\t"),
                | _ => escaped.push(ch),
            }
        }
        escaped
    }

    /// Unescape point text parsed from markdown quoted scalars.
    ///
    /// Supports the exact escapes emitted by [`Self::escape_markdown_point`].
    fn unescape_markdown_point(point: &str) -> Result<String, UnescapeError> {
        let mut chars = point.chars();
        let mut out = String::with_capacity(point.len());

        while let Some(ch) = chars.next() {
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            let Some(next) = chars.next() else {
                return Err(UnescapeError::TrailingBackslash);
            };
            match next {
                | '\\' => out.push('\\'),
                | '"' => out.push('"'),
                | 'n' => out.push('\n'),
                | 'r' => out.push('\r'),
                | 't' => out.push('\t'),
                | other => {
                    return Err(UnescapeError::UnsupportedEscape(other));
                }
            }
        }

        Ok(out)
    }
}

/// Internal error type for [`BlockStore::unescape_markdown_point`], mapped to
/// [`MarkdownParseError`] variants with line context at the call site.
enum UnescapeError {
    TrailingBackslash,
    UnsupportedEscape(char),
}
