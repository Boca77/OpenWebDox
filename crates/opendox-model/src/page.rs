//! Page setup (size and margins) and page breaks.
//!
//! The page setup is part of the document, so everyone editing it sees the same pages: a
//! `page` map with one key per value, so two people changing different margins at once both
//! keep their change. A page break is a paragraph format ([`ParagraphFormat::page_break_before`]).
//!
//! [`ParagraphFormat::page_break_before`]: crate::ParagraphFormat::page_break_before

use loro::{LoroValue, ValueOrContainer};

use crate::document::Document;
use crate::error::ModelError;
use crate::format::ParagraphChange;
use crate::outline::Outline;

const PAGE: &str = "page";

/// Page size and margins, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
}

impl Default for PageSetup {
    /// US Letter, 1 in margins (Google Docs' default).
    fn default() -> Self {
        Self {
            width: 612.0,
            height: 792.0,
            margin_top: 72.0,
            margin_right: 72.0,
            margin_bottom: 72.0,
            margin_left: 72.0,
        }
    }
}

impl PageSetup {
    fn fields(&mut self) -> [(&'static str, &mut f32); 6] {
        [
            ("width", &mut self.width),
            ("height", &mut self.height),
            ("margin_top", &mut self.margin_top),
            ("margin_right", &mut self.margin_right),
            ("margin_bottom", &mut self.margin_bottom),
            ("margin_left", &mut self.margin_left),
        ]
    }

    /// Whether the page leaves room for text: positive size, margins that don't meet.
    pub fn is_valid(&self) -> bool {
        let all = [
            self.width,
            self.height,
            self.margin_top,
            self.margin_right,
            self.margin_bottom,
            self.margin_left,
        ];
        all.iter().all(|v| v.is_finite() && *v >= 0.0)
            && self.width - self.margin_left - self.margin_right >= 36.0
            && self.height - self.margin_top - self.margin_bottom >= 36.0
    }
}

impl Document {
    /// The document's page size and margins (the default where none were set).
    pub fn page_setup(&self) -> PageSetup {
        let map = self.doc.get_map(PAGE);
        let mut p = PageSetup::default();
        for (key, value) in p.fields() {
            if let Some(ValueOrContainer::Value(v)) = map.get(key) {
                match v {
                    LoroValue::Double(d) => *value = d as f32,
                    LoroValue::I64(i) => *value = i as f32,
                    _ => {}
                }
            }
        }
        if p.is_valid() {
            p
        } else {
            // Concurrent changes can combine into margins wider than the page (one person
            // widening the left margin while another narrows the page): fall back rather
            // than lay out text in no room.
            PageSetup::default()
        }
    }

    /// Changes the page size and margins. Only values that differ are written, so a
    /// concurrent change to another value isn't overwritten.
    pub fn set_page_setup(&self, setup: PageSetup) -> Result<(), ModelError> {
        if !setup.is_valid() {
            return Err(ModelError::InvalidPageSetup);
        }
        let map = self.doc.get_map(PAGE);
        let mut old = self.page_setup();
        let mut new = setup;
        for ((key, was), (_, now)) in old.fields().into_iter().zip(new.fields()) {
            if *was != *now {
                map.insert(key, f64::from(*now)).map_err(crdt)?;
            }
        }
        self.doc.commit();
        Ok(())
    }

    /// Ctrl+Enter: splits the paragraph at flat position `pos` and starts the second half on a
    /// new page, as one change (one undo step). Returns the caret position after the break.
    pub fn insert_page_break_at(&self, outline: &Outline, pos: usize) -> Result<usize, ModelError> {
        let loc = outline.locate(pos);
        self.raw_insert(&loc.container, loc.offset, "\n", None)?;
        let after = loc.offset + 1;
        let mut f = self.paragraph_format_in(&loc.container, after);
        f.apply(ParagraphChange::PageBreakBefore(true));
        self.write_paragraph_format(&loc.container, after, f)?;
        self.doc.commit();
        Ok(pos + 1)
    }
}

fn crdt(e: impl std::fmt::Display) -> ModelError {
    ModelError::Crdt(e.to_string())
}
