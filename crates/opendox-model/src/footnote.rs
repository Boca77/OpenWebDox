//! Footnotes: a numbered reference in the text, and the note's own text, drawn at the foot of
//! the page holding the reference.
//!
//! A reference is the object placeholder character marked `footnote = <id>`, in body or cell
//! text; the note's text is a rich-text container in the `footnotes` map under that id. Notes
//! are numbered by where their references are, in reading order. Deleting a reference removes
//! its note from the document; the note's text stays in the map, so undo brings both back.

use loro::{Container, LoroText, ValueOrContainer};

use crate::document::Document;
use crate::error::ModelError;
use crate::format::attrs_of;
use crate::image::IMAGE_PLACEHOLDER;
use crate::outline::{Outline, Region, TextRef};

pub(crate) const FOOTNOTE_MARK: &str = "footnote";
pub(crate) const FOOTNOTES: &str = "footnotes";
/// Where the last paragraph of each note keeps its format, under `<id>|<key>`.
pub(crate) const FOOTNOTE_TAIL: &str = "footnote_tail";

impl Document {
    /// Inserts a footnote reference at flat position `pos` (in body or cell text) with an
    /// empty note, and returns the note's id. Its text is then at
    /// [`Outline::footnote`]`(id)` in the next outline.
    pub fn insert_footnote_at(&self, outline: &Outline, pos: usize) -> Result<String, ModelError> {
        if outline.region(pos) != Region::Body {
            return Err(ModelError::FootnoteOutsideBody);
        }
        let loc = outline.locate(outline.snap(pos));
        let id = self.new_id("n");
        // Raw insert: `insert_at` drops placeholder characters, which typing must never add.
        self.raw_insert(
            &loc.container,
            loc.offset,
            &IMAGE_PLACEHOLDER.to_string(),
            None,
        )?;
        self.container(&loc.container)?
            .mark(loc.offset..loc.offset + 1, FOOTNOTE_MARK, id.as_str())
            .map_err(crdt)?;
        self.doc
            .get_map(FOOTNOTES)
            .insert_container(&id, LoroText::new())
            .map_err(crdt)?;
        self.doc.commit();
        Ok(id)
    }

    /// Removes a footnote: its reference, and with it the note. Undo brings both back.
    pub fn delete_footnote(&self, outline: &Outline, id: &str) -> Result<(), ModelError> {
        let note = outline.footnote(id).ok_or(ModelError::NoSuchFootnote)?;
        self.delete_range_at(outline, note.reference, note.reference + 1)?;
        Ok(())
    }

    /// A note's text container.
    pub(crate) fn note_text(&self, id: &str) -> Option<LoroText> {
        match self.doc.get_map(FOOTNOTES).get(id)? {
            ValueOrContainer::Container(Container::Text(t)) => Some(t),
            _ => None,
        }
    }

    /// Every footnote reference in reading order, `(flat position, id)`: those in the body
    /// (`body`, as body char index and id) and in cells. Each note counts once, at its first
    /// reference, and only if its text exists.
    pub(crate) fn note_refs(
        &self,
        outline: &Outline,
        body: Vec<(usize, String)>,
    ) -> Vec<(usize, String)> {
        let mut refs: Vec<(usize, String)> = body
            .into_iter()
            .map(|(i, id)| (outline.flat_of_body(i), id))
            .collect();
        for slot in &outline.slots {
            let TextRef::Cell(c) = &slot.container else {
                continue;
            };
            if !slot.text.contains(IMAGE_PLACEHOLDER) {
                continue;
            }
            let Some(text) = self.existing_cell(c) else {
                continue;
            };
            for (k, ch) in slot.text.chars().enumerate() {
                if ch != IMAGE_PLACEHOLDER {
                    continue;
                }
                let id = attrs_of(&text, slot.offset + k)
                    .into_iter()
                    .find(|(key, _)| key == FOOTNOTE_MARK)
                    .and_then(|(_, v)| v.as_string().map(|s| s.to_string()));
                if let Some(id) = id {
                    refs.push((slot.flat + k, id));
                }
            }
        }
        refs.sort_by_key(|(flat, _)| *flat);
        let mut seen = std::collections::HashSet::new();
        refs.retain(|(_, id)| self.note_text(id).is_some() && seen.insert(id.clone()));
        refs
    }
}

fn crdt(e: impl std::fmt::Display) -> ModelError {
    ModelError::Crdt(e.to_string())
}
