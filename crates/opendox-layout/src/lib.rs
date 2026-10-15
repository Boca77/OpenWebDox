//! Layout engine for opendox: shaping, line breaking, pagination and hit testing.
//!
//! Takes plain paragraphs and style spans in, returns geometry out. Must not depend on the CRDT
//! or `opendox-model`. All geometry is in points (1/72 inch) as `f32`; conversion to device
//! pixels happens in the renderer.
//!
//! Only fonts registered in a [`FontLibrary`] are used — never system fonts — so the same input
//! produces the same line breaks on every machine and in every browser.
//!
//! Text offsets in this crate are **UTF-8 byte offsets** into the paragraph text.

mod fonts;
mod paragraph;

pub use fonts::{FontId, FontLibrary};
pub use paragraph::{Glyph, GlyphRun, LayoutEngine, Line, ParagraphLayout, StyleSpan, TextStyle};
