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

mod document;
mod error;
mod grapheme;
mod outline;
mod table;

pub use document::{Anchor, Document, Paragraph, Version};
pub use error::ModelError;
pub use outline::{Block, Location, Outline, Slot, TableBlock, TextRef};
pub use table::{CellRef, TABLE_PLACEHOLDER};
