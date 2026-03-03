//! Typed point content for blocks.
//!
//! A block's "point" is its primary content. Historically this was always a
//! plain [`String`]. This module introduces [`PointContent`] to distinguish
//! between plain text and link references while keeping the existing API
//! surface (which traffics in `String`) intact for the majority of callers.
//!
//! # Variants
//!
//! - [`PointContent::Text`] -- plain text (the default, backward-compatible form).
//! - [`PointContent::Link`] -- a reference to an external resource described by
//!   a [`PointLink`] (href + inferred kind + optional label).
//!
//! # Serde contract
//!
//! Backward compatibility with existing JSON store files is critical:
//!
//! - **Deserialize**: a bare JSON string is read as `Text`; a JSON object with
//!   at least an `href` field is read as `Link`.
//! - **Serialize**: `Text(s)` writes a bare JSON string; `Link(link)` writes
//!   a JSON object.
//!
//! This means old files round-trip unchanged, and new files with links are
//! ignored (treated as text) by older versions only if they fall back to
//! the raw string representation -- which they will because `serde_json`
//! rejects an unexpected object for a `String` field gracefully.

use serde::de::{self, Deserializer, Visitor};
use serde::ser::Serializer;
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

/// The typed content of a block's point.
///
/// Most of the codebase accesses points through [`BlockStore::point()`] which
/// returns `Option<String>` (via [`Self::display_text`]) so existing callers
/// do not need to match on this enum. Only UI rendering and the toggle action
/// need to inspect the variant via [`BlockStore::point_content()`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointContent {
    /// Plain text (the historical default).
    Text(String),
    /// A link to an external resource.
    Link(PointLink),
}

impl PointContent {
    /// The user-visible text for this point.
    ///
    /// - `Text(s)` returns `s`.
    /// - `Link(link)` returns `link.display_text()` (label or href).
    pub fn display_text(&self) -> &str {
        match self {
            | Self::Text(s) => s,
            | Self::Link(link) => link.display_text(),
        }
    }

    /// True when the content is an empty text point (the default for new blocks).
    pub fn is_empty_text(&self) -> bool {
        matches!(self, Self::Text(s) if s.is_empty())
    }

    /// True when the content is a [`PointContent::Link`].
    pub fn is_link(&self) -> bool {
        matches!(self, Self::Link(_))
    }

    /// Return the inner link reference, if this is a `Link` variant.
    pub fn as_link(&self) -> Option<&PointLink> {
        match self {
            | Self::Link(link) => Some(link),
            | Self::Text(_) => None,
        }
    }
}

impl Default for PointContent {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

impl From<String> for PointContent {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for PointContent {
    fn from(s: &str) -> Self {
        Self::Text(s.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Serde: backward-compatible serialization
// ---------------------------------------------------------------------------

impl Serialize for PointContent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            | Self::Text(s) => serializer.serialize_str(s),
            | Self::Link(link) => link.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for PointContent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(PointContentVisitor)
    }
}

/// Visitor that distinguishes bare strings (Text) from objects (Link).
struct PointContentVisitor;

impl<'de> Visitor<'de> for PointContentVisitor {
    type Value = PointContent;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a string (text point) or an object with href/kind fields (link point)")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(PointContent::Text(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(PointContent::Text(value))
    }

    fn visit_map<A: de::MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        let link = PointLink::deserialize(de::value::MapAccessDeserializer::new(map))?;
        Ok(PointContent::Link(link))
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
        let text = PointContent::Text("hello".into());
        assert_eq!(text.display_text(), "hello");
        let link = PointContent::Link(PointLink::infer("pic.jpg").with_label("Photo"));
        assert_eq!(link.display_text(), "Photo");
    }

    #[test]
    fn serde_text_round_trip() {
        let original = PointContent::Text("hello world".into());
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(json, r#""hello world""#);
        let parsed: PointContent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn serde_link_round_trip() {
        let original = PointContent::Link(PointLink::infer("diagram.png").with_label("Diagram"));
        let json = serde_json::to_string(&original).unwrap();
        let parsed: PointContent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn serde_link_without_label_round_trip() {
        let original = PointContent::Link(PointLink::infer("notes.md"));
        let json = serde_json::to_string(&original).unwrap();
        assert!(!json.contains("label"));
        let parsed: PointContent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn serde_backward_compat_bare_string() {
        let json = r#""existing text point""#;
        let parsed: PointContent = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, PointContent::Text("existing text point".into()));
    }

    #[test]
    fn is_empty_text() {
        assert!(PointContent::Text(String::new()).is_empty_text());
        assert!(!PointContent::Text("non-empty".into()).is_empty_text());
        assert!(!PointContent::Link(PointLink::infer("a.png")).is_empty_text());
    }
}
