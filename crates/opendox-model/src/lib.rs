//! Document model for opendox, built on the Loro CRDT.
//!
//! Loro is an implementation detail: nothing outside this crate should name Loro types, so the
//! CRDT can be swapped without touching layout or the editor.
//!
//! # Positions
//!
//! Every position in this API is a **char index**: a count of Unicode scalar values from the
//! start of the document. Edits must start and end on grapheme-cluster boundaries, so a caret
//! can never split a visible character (an emoji, or a letter plus its accent).

mod clipboard;
mod comments;
mod document;
mod error;
mod footnote;
mod format;
mod grapheme;
mod header;
mod history;
mod image;
mod list;
mod outline;
mod page;
mod style;
mod table;

pub use clipboard::{Clip, ClipList, ClipParagraph, ClipRun};
pub use comments::{Comment, Reply};
pub use document::{Anchor, Document, Paragraph, Version};
pub use error::ModelError;
pub use format::{
    Align, CharFormat, FormatChange, FormatRun, ListFormat, ListKind, ParagraphChange,
    ParagraphFormat, Script, SlotFormat, MAX_LIST_LEVEL,
};
pub use header::{FieldKind, FieldRef};
pub use history::History;
pub use image::{ImageChange, ImageData, ImageProps, ImageRef, Recolor, Wrap, IMAGE_PLACEHOLDER};
pub use list::{list_indent, list_markers, marker_text, LIST_INDENT_STEP, LIST_MARKER_HANG};
pub use outline::{Block, FootnoteSlots, Location, Outline, Region, Slot, TableBlock, TextRef};
pub use page::PageSetup;
pub use style::{StyleLook, StyleName};
pub use table::{CellRef, TABLE_PLACEHOLDER};
