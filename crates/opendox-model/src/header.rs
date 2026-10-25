//! Page headers and footers, and the page number fields in them.
//!
//! The header and footer are rich-text containers of their own (`header`, `footer`), drawn
//! on every page. Whether the document has them is a flag in the `page` map, so an empty
//! header can exist (and be typed into) and removing one is a single change. A field is the
//! object placeholder character marked `field = page | pages`; what it shows depends on the
//! page, so layout fills it in.

use loro::{LoroValue, ValueOrContainer};

use crate::document::Document;
use crate::error::ModelError;
use crate::image::IMAGE_PLACEHOLDER;
use crate::outline::{Outline, Region, TextRef};

pub(crate) const HEADER: &str = "header";
pub(crate) const FOOTER: &str = "footer";
pub(crate) const FIELD_MARK: &str = "field";
const PAGE: &str = "page";

/// What a field shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKind {
    /// The number of the page it's drawn on (from 1).
    PageNumber,
    /// How many pages the document has.
    PageCount,
}

impl FieldKind {
    fn key(self) -> &'static str {
        match self {
            FieldKind::PageNumber => "page",
            FieldKind::PageCount => "pages",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<FieldKind> {
        match s {
            "page" => Some(FieldKind::PageNumber),
            "pages" => Some(FieldKind::PageCount),
            _ => None,
        }
    }
}

/// A field in a paragraph: its char offset from the paragraph's start, and what it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldRef {
    pub offset: usize,
    pub kind: FieldKind,
}

impl Document {
    pub fn has_header(&self) -> bool {
        self.page_flag(HEADER)
    }

    pub fn has_footer(&self) -> bool {
        self.page_flag(FOOTER)
    }

    /// Adds a header (empty if it never had text) or removes it with its text.
    pub fn set_header(&self, on: bool) -> Result<(), ModelError> {
        self.set_region(TextRef::Header, HEADER, on)
    }

    /// Adds a footer or removes it with its text.
    pub fn set_footer(&self, on: bool) -> Result<(), ModelError> {
        self.set_region(TextRef::Footer, FOOTER, on)
    }

    /// Inserts a page number or page count field at flat position `pos`, which must be in
    /// the header or footer. Returns the caret position after it.
    pub fn insert_field_at(
        &self,
        outline: &Outline,
        pos: usize,
        kind: FieldKind,
    ) -> Result<usize, ModelError> {
        if outline.region(pos) == Region::Body {
            return Err(ModelError::FieldOutsideHeader);
        }
        let loc = outline.locate(pos);
        let text = self.container(&loc.container)?;
        // Raw insert: `insert_at` drops placeholder characters, which typing must never add.
        self.raw_insert(
            &loc.container,
            loc.offset,
            &IMAGE_PLACEHOLDER.to_string(),
            None,
        )?;
        text.mark(loc.offset..loc.offset + 1, FIELD_MARK, kind.key())
            .map_err(crdt)?;
        self.doc.commit();
        Ok(pos + 1)
    }

    fn page_flag(&self, key: &str) -> bool {
        matches!(
            self.doc.get_map(PAGE).get(key),
            Some(ValueOrContainer::Value(LoroValue::Bool(true)))
        )
    }

    fn set_region(&self, container: TextRef, key: &str, on: bool) -> Result<(), ModelError> {
        let map = self.doc.get_map(PAGE);
        if on {
            map.insert(key, true).map_err(crdt)?;
        } else {
            map.delete(key).map_err(crdt)?;
            let text = self.container(&container)?;
            let n = text.len_unicode();
            if n > 0 {
                text.delete(0, n).map_err(crdt)?;
            }
        }
        self.doc.commit();
        Ok(())
    }
}

fn crdt(e: impl std::fmt::Display) -> ModelError {
    ModelError::Crdt(e.to_string())
}
