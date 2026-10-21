//! Export for opendox documents.
//!
//! - [`pdf`]: print-accurate PDF drawn from layout output, with subsetted embedded fonts and
//!   searchable text.
//! - [`docx`]: the document's text, formatting, tables and images as a Word file, for further
//!   editing. Word lays it out again itself, so it's faithful in content, not in exact line
//!   breaks.

mod docx;
mod image;
mod pdf;
mod zip;

pub use docx::{docx, DocxOptions};
pub use image::{adjust, ExportImage};
pub use pdf::{pdf, PdfInfo};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExportError {
    /// Layout used a font id the font library doesn't have.
    #[error("font {0} used by layout is not in the font library")]
    MissingFont(u32),

    #[error("could not embed font: {0}")]
    Font(String),
}
