//! Markdown Mount v1 render and parse.
//!
//! The markdown format is a two-space indented bullet list where each block
//! point is a double-quoted, escape-encoded scalar.  A required preamble
//! comment identifies the format version.

use super::{BlockId, BlockNode, BlockStore};
use slotmap::{SecondaryMap, SlotMap};

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
    pub(crate) fn parse_markdown_mount_store(markdown: &str) -> Result<BlockStore, String> {
        let mut nodes: SlotMap<BlockId, BlockNode> = SlotMap::with_key();
        let mut points: SecondaryMap<BlockId, String> = SecondaryMap::new();
        let mut roots: Vec<BlockId> = Vec::new();
        let mut path_by_depth: Vec<BlockId> = Vec::new();

        let mut saw_preamble = false;
        let mut saw_item = false;

        for (line_index, raw_line) in markdown.lines().enumerate() {
            let line_no = line_index + 1;
            let line = raw_line.trim_end();
            if line.trim().is_empty() {
                continue;
            }
            if !saw_preamble {
                if line == "<!-- bb-mount format=markdown v1 -->" {
                    saw_preamble = true;
                    continue;
                }
                return Err(format!(
                    "line {}: missing markdown mount preamble '<!-- bb-mount format=markdown v1 -->'",
                    line_no
                ));
            }

            let depth_spaces = raw_line.chars().take_while(|ch| *ch == ' ').count();
            if depth_spaces % 2 != 0 {
                return Err(format!(
                    "line {}: indentation must be multiples of two spaces",
                    line_no
                ));
            }
            let depth = depth_spaces / 2;
            let trimmed = &raw_line[depth_spaces..];

            if !trimmed.starts_with("- \"") || !trimmed.ends_with('"') {
                return Err(format!("line {}: expected '- \"...\"' markdown list item", line_no));
            }

            let quoted_content = &trimmed[3..trimmed.len() - 1];
            let point = Self::unescape_markdown_point(quoted_content)
                .map_err(|reason| format!("line {}: {}", line_no, reason))?;

            if depth > path_by_depth.len() {
                return Err(format!(
                    "line {}: indentation depth jumps more than one level",
                    line_no
                ));
            }
            path_by_depth.truncate(depth);

            let id = nodes.insert(BlockNode::with_children(vec![]));
            points.insert(id, point);
            saw_item = true;

            if depth == 0 {
                roots.push(id);
            } else {
                let Some(parent_id) = path_by_depth.get(depth - 1).copied() else {
                    return Err(format!(
                        "line {}: missing parent block at depth {}",
                        line_no,
                        depth - 1
                    ));
                };
                let Some(parent) = nodes.get_mut(parent_id) else {
                    return Err(format!("line {}: parent block does not exist", line_no));
                };
                let Some(children) = parent.children_mut() else {
                    return Err(format!("line {}: parent block is not a children node", line_no));
                };
                children.push(id);
            }

            path_by_depth.push(id);
        }

        if !saw_preamble {
            return Err("missing markdown mount preamble '<!-- bb-mount format=markdown v1 -->'"
                .to_string());
        }
        if !saw_item {
            return Err("markdown mount file contains no block items".to_string());
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
    fn unescape_markdown_point(point: &str) -> Result<String, String> {
        let mut chars = point.chars();
        let mut out = String::with_capacity(point.len());

        while let Some(ch) = chars.next() {
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            let Some(next) = chars.next() else {
                return Err("trailing backslash in escaped point".to_string());
            };
            match next {
                | '\\' => out.push('\\'),
                | '"' => out.push('"'),
                | 'n' => out.push('\n'),
                | 'r' => out.push('\r'),
                | 't' => out.push('\t'),
                | other => {
                    return Err(format!("unsupported escape sequence \\{}", other));
                }
            }
        }

        Ok(out)
    }
}
