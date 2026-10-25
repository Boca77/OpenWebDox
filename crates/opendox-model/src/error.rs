use thiserror::Error;

/// Errors returned by [`crate::Document`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ModelError {
    #[error("position {pos} is past the end of the document (length {len})")]
    OutOfBounds { pos: usize, len: usize },

    #[error("position {pos} is inside a grapheme cluster")]
    NotGraphemeBoundary { pos: usize },

    #[error("anchor no longer resolves to a position in this document")]
    AnchorLost,

    #[error("no such table")]
    NoSuchTable,

    #[error("page setup leaves no room for text")]
    InvalidPageSetup,

    #[error("page number fields only go in a header or footer")]
    FieldOutsideHeader,

    #[error("footnotes go in body or table text, not in a header, footer or footnote")]
    FootnoteOutsideBody,

    #[error("no such footnote")]
    NoSuchFootnote,

    #[error("not a clipboard clip: {0}")]
    BadClip(String),

    #[error("a comment needs some text selected")]
    EmptyComment,

    #[error("no such comment")]
    NoSuchComment,

    #[error("no such table cell")]
    NoSuchCell,

    #[error("no such image")]
    NoSuchImage,

    #[error("a table needs at least one row and one column")]
    InvalidTable,

    #[error("could not decode update: {0}")]
    Import(String),

    /// Bytes from [`crate::Anchor::encode`] or [`crate::Version::encode`] were malformed.
    #[error("could not decode: {0}")]
    Decode(String),

    /// An error from the underlying CRDT that callers can't act on beyond reporting it.
    #[error("CRDT error: {0}")]
    Crdt(String),
}
