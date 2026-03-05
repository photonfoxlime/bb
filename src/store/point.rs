//! Typed point content for blocks.
//!
//! A block's "point" is its primary content: always a combination of editable
//! text and zero or more [`PointLink`] references to external resources.
//!
//! # Shape
//!
//! [`PointContent`] is a struct with two fields:
//! - `text` — the editable plain text of the block.
//! - `links` — a `Vec<PointLink>` rendered as chips floating above the text
//!   editor. Empty by default.
//!
//! # Serde contract
//!
//! Three wire formats are accepted for backward compatibility:
//!
//! | Wire format | Deserializes as |
//! |---|---|
//! | `"bare string"` (old `Text` variant) | `{ text: "bare string", links: [] }` |
//! | `{ "href": ..., "kind": ... }` (old `Link` variant) | `{ text: "", links: [PointLink { ... }] }` |
//! | `{ "text": ..., "links": [...] }` (current) | parsed normally |
//!
//! Serialization always writes the current struct format, omitting `links`
//! when empty.

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

// ---------------------------------------------------------------------------
// LinkKind
// ---------------------------------------------------------------------------

/// Discriminant for the type of resource a [`PointLink`] references.
///
/// Inferred from the href's file extension by [`PointLink::infer`].
/// The kind drives UI rendering (image preview, markdown preview, or
/// generic clickable chip).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkKind {
    /// Image file (png, jpg, jpeg, gif, svg, webp, bmp, ico).
    Image,
    /// Markdown document (md, markdown).
    Markdown,
    /// Any other file or URL.
    Path,
}

impl LinkKind {
    /// Infer the link kind from a file extension string (case-insensitive).
    ///
    /// Returns `None` if the extension is absent or not recognized, in which
    /// case the caller should default to [`LinkKind::Path`].
    fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            | "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "ico" => Some(Self::Image),
            | "md" | "markdown" => Some(Self::Markdown),
            | _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// PointLink
// ---------------------------------------------------------------------------

/// A reference to an external resource attached to a block.
///
/// Created via [`PointLink::infer`] which derives [`LinkKind`] from the href's
/// file extension. The optional `label` is user-provided display text; when
/// absent, the UI should show the href directly.
///
/// # Invariants
///
/// - `href` is non-empty.
/// - `kind` agrees with the extension of `href` at construction time (but is
///   not re-validated after deserialization to allow manual overrides in JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointLink {
    /// The target resource (file path, URL, etc.).
    pub href: String,
    /// Inferred or overridden resource kind.
    pub kind: LinkKind,
    /// Optional human-readable label. When `None`, the UI displays `href`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl PointLink {
    /// Create a link, inferring [`LinkKind`] from the href extension.
    ///
    /// Unknown or missing extensions default to [`LinkKind::Path`].
    pub fn infer(href: impl Into<String>) -> Self {
        let href = href.into();
        let kind = Path::new(&href)
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(LinkKind::from_extension)
            .unwrap_or(LinkKind::Path);
        Self { href, kind, label: None }
    }

    /// Create a link with an explicit label.
    ///
    /// Note: currently only used in tests. Kept as public API for future
    /// callers (e.g. user-provided link labels).
    #[allow(dead_code)]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The text shown to the user: label if present, otherwise href.
    pub fn display_text(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.href)
    }
}

// ---------------------------------------------------------------------------
// PointContent
// ---------------------------------------------------------------------------

/// The content of a block's point: always editable text plus zero or more links.
///
/// Links are rendered as chips floating above the text editor. The text field
/// is always independently editable regardless of link count.
///
/// # Invariants
///
/// - `text` may be empty (e.g. a block that is purely a link collection).
/// - `links` is append-only via [`Self::add_link`]; removal is index-based
///   via [`Self::remove_link`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointContent {
    /// The plain-text body of the block.
    pub text: String,
    /// External resource links attached to this block.
    pub links: Vec<PointLink>,
}

impl PointContent {
    /// The user-visible text for this point.
    pub fn display_text(&self) -> &str {
        &self.text
    }

    /// True when the text field is empty.
    pub fn is_empty_text(&self) -> bool {
        self.text.is_empty()
    }

    /// Append a link to this point.
    pub fn add_link(&mut self, link: PointLink) {
        self.links.push(link);
    }

    /// Remove the link at `index`.
    ///
    /// No-op if `index` is out of bounds.
    pub fn remove_link(&mut self, index: usize) {
        if index < self.links.len() {
            self.links.remove(index);
        }
    }
}

impl Default for PointContent {
    fn default() -> Self {
        Self { text: String::new(), links: vec![] }
    }
}

impl From<String> for PointContent {
    fn from(s: String) -> Self {
        Self { text: s, links: vec![] }
    }
}

impl From<&str> for PointContent {
    fn from(s: &str) -> Self {
        Self { text: s.to_owned(), links: vec![] }
    }
}

// ---------------------------------------------------------------------------
// Serde: backward-compatible serialization
// ---------------------------------------------------------------------------

impl Serialize for PointContent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.links.is_empty() {
            // Compact form: omit `links` field entirely.
            let mut s = serializer.serialize_struct("PointContent", 1)?;
            s.serialize_field("text", &self.text)?;
            s.end()
        } else {
            let mut s = serializer.serialize_struct("PointContent", 2)?;
            s.serialize_field("text", &self.text)?;
            s.serialize_field("links", &self.links)?;
            s.end()
        }
    }
}

impl<'de> Deserialize<'de> for PointContent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(PointContentVisitor)
    }
}

/// Visitor that handles three wire formats:
/// - bare string → `PointContent { text: s, links: [] }`
/// - object with `href` field (old Link variant) → `PointContent { text: "", links: [link] }`
/// - object with `text` field (current format) → parsed normally
struct PointContentVisitor;

impl<'de> Visitor<'de> for PointContentVisitor {
    type Value = PointContent;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a string (legacy text point), an object with href/kind (legacy link), \
             or an object with text/links fields (current format)",
        )
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(PointContent { text: value.to_owned(), links: vec![] })
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(PointContent { text: value, links: vec![] })
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        // Peek at the first key to distinguish legacy Link objects from current format.
        // Legacy: first key is "href" or "kind".
        // Current: first key is "text".
        //
        // Note: we must buffer remaining key-value pairs because serde visitors
        // consume the map sequentially.
        let mut text: Option<String> = None;
        let mut links: Option<Vec<PointLink>> = None;
        let mut legacy_href: Option<String> = None;
        let mut legacy_kind: Option<LinkKind> = None;
        let mut legacy_label: Option<String> = None;

        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                | "text" => {
                    text = Some(map.next_value()?);
                }
                | "links" => {
                    links = Some(map.next_value()?);
                }
                // Legacy Link fields.
                | "href" => {
                    legacy_href = Some(map.next_value()?);
                }
                | "kind" => {
                    legacy_kind = Some(map.next_value()?);
                }
                | "label" => {
                    legacy_label = Some(map.next_value()?);
                }
                | _ => {
                    map.next_value::<de::IgnoredAny>()?;
                }
            }
        }

        if let Some(href) = legacy_href {
            // Old Link variant: migrate to PointContent with one link.
            let kind = legacy_kind.unwrap_or(LinkKind::Path);
            let link = PointLink { href, kind, label: legacy_label };
            Ok(PointContent { text: String::new(), links: vec![link] })
        } else {
            // Current format (or partial).
            Ok(PointContent { text: text.unwrap_or_default(), links: links.unwrap_or_default() })
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_image_kind() {
        let link = PointLink::infer("photo.png");
        assert_eq!(link.kind, LinkKind::Image);
        let link = PointLink::infer("/path/to/pic.JPG");
        assert_eq!(link.kind, LinkKind::Image);
    }

    #[test]
    fn infer_markdown_kind() {
        let link = PointLink::infer("notes.md");
        assert_eq!(link.kind, LinkKind::Markdown);
        let link = PointLink::infer("README.markdown");
        assert_eq!(link.kind, LinkKind::Markdown);
    }

    #[test]
    fn infer_path_kind_for_unknown() {
        let link = PointLink::infer("archive.tar.gz");
        assert_eq!(link.kind, LinkKind::Path);
        let link = PointLink::infer("https://example.com");
        assert_eq!(link.kind, LinkKind::Path);
    }

    #[test]
    fn display_text_prefers_label() {
        let link = PointLink::infer("photo.png").with_label("My Photo");
        assert_eq!(link.display_text(), "My Photo");
    }

    #[test]
    fn display_text_falls_back_to_href() {
        let link = PointLink::infer("photo.png");
        assert_eq!(link.display_text(), "photo.png");
    }

    #[test]
    fn point_content_display_text() {
        let content = PointContent { text: "hello".into(), links: vec![] };
        assert_eq!(content.display_text(), "hello");
    }

    #[test]
    fn add_and_remove_link() {
        let mut content = PointContent::default();
        assert!(content.links.is_empty());
        content.add_link(PointLink::infer("pic.jpg"));
        assert!(!content.links.is_empty());
        assert_eq!(content.links.len(), 1);
        content.remove_link(0);
        assert!(content.links.is_empty());
    }

    #[test]
    fn remove_link_oob_is_noop() {
        let mut content = PointContent::default();
        content.remove_link(5); // should not panic
    }

    #[test]
    fn serde_text_round_trip() {
        let original = PointContent { text: "hello world".into(), links: vec![] };
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(json, r#"{"text":"hello world"}"#);
        let parsed: PointContent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn serde_with_links_round_trip() {
        let mut original = PointContent { text: "my note".into(), links: vec![] };
        original.add_link(PointLink::infer("diagram.png").with_label("Diagram"));
        let json = serde_json::to_string(&original).unwrap();
        let parsed: PointContent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn serde_backward_compat_bare_string() {
        let json = r#""existing text point""#;
        let parsed: PointContent = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.text, "existing text point");
        assert!(parsed.links.is_empty());
    }

    #[test]
    fn serde_backward_compat_old_link_object() {
        let json = r#"{"href":"/path/to/photo.png","kind":"image","label":"Photo"}"#;
        let parsed: PointContent = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.text, "");
        assert_eq!(parsed.links.len(), 1);
        assert_eq!(parsed.links[0].href, "/path/to/photo.png");
        assert_eq!(parsed.links[0].kind, LinkKind::Image);
        assert_eq!(parsed.links[0].label.as_deref(), Some("Photo"));
    }

    #[test]
    fn serde_backward_compat_old_link_no_label() {
        let json = r#"{"href":"notes.md","kind":"markdown"}"#;
        let parsed: PointContent = serde_json::from_str(json).unwrap();
        assert!(parsed.text.is_empty());
        assert_eq!(parsed.links.len(), 1);
        assert_eq!(parsed.links[0].kind, LinkKind::Markdown);
        assert!(parsed.links[0].label.is_none());
    }

    #[test]
    fn is_empty_text() {
        assert!(PointContent::default().is_empty_text());
        assert!(!PointContent { text: "non-empty".into(), links: vec![] }.is_empty_text());
    }
}
