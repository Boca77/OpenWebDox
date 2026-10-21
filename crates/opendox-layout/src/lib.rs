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
mod navigation;
mod pagination;
mod paragraph;

pub use fonts::{FontId, FontLibrary, GlyphOutline};
pub use navigation::{CaretRect, PlacedLine, Rect, SelectionRect, TextPosition};
pub use pagination::{
    BlockInput, Fragment, Layer, Page, PageGeometry, Paginator, ParagraphInput, PlacedGlyph,
    PlacedObject, Rule, TableInput, BREAK_MARGIN, CELL_PADDING, WRAP_MARGIN,
};
pub use paragraph::{
    inline_size, Align, CaretStop, Exclusion, Glyph, GlyphRun, LayoutEngine, Line, LineObject,
    ObjectInput, ObjectMode, ParagraphLayout, ParagraphStyle, StyleSpan, TextStyle, MIN_WRAP_WIDTH,
    OBJECT_PLACEHOLDER,
};
