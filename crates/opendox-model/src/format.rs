//! Rich text: character formats and paragraph formats.
//!
//! **Character formats** are Peritext-style marks over text ranges: `bold` and `italic`
//! (`true`), `size` (points) and `font` (a family name). They expand after, so text typed at
//! the end of a bold run is bold. Text typed at the start of a paragraph takes the format of
//! the character after it, as in Google Docs.
//!
//! **Paragraph formats** (`p_align`, `p_line`, `p_before`, `p_after`) are marks on the
//! character that ends the paragraph: its `\n`, or a table placeholder. They never expand.
//! The last paragraph of a container has no such character, so its format is kept beside the
//! container: in the `body_tail` map for the body, and under `<column id>|p_*` keys in the
//! row map for a cell. Splitting a paragraph copies its format to the new line break; joining
//! two keeps the first one's format, as in Google Docs.

use std::collections::HashMap;
use std::ops::Range;

use loro::{LoroMap, LoroText, LoroValue, TextDelta, ValueOrContainer};

use crate::document::Document;
use crate::error::ModelError;
use crate::image::{ImageRef, IMAGE_MARK, IMAGE_PLACEHOLDER};
use crate::outline::{Outline, TextRef};

pub(crate) const BOLD: &str = "bold";
pub(crate) const ITALIC: &str = "italic";
pub(crate) const SIZE: &str = "size";
pub(crate) const FONT: &str = "font";
pub(crate) const P_ALIGN: &str = "p_align";
pub(crate) const P_LINE: &str = "p_line";
pub(crate) const P_BEFORE: &str = "p_before";
pub(crate) const P_AFTER: &str = "p_after";
pub(crate) const CHAR_KEYS: [&str; 4] = [BOLD, ITALIC, SIZE, FONT];
pub(crate) const PARAGRAPH_KEYS: [&str; 4] = [P_ALIGN, P_LINE, P_BEFORE, P_AFTER];
const BODY_TAIL: &str = "body_tail";

/// How a run of characters looks. `None` means the document default.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CharFormat {
    pub bold: bool,
    pub italic: bool,
    /// Font size in points.
    pub size: Option<f32>,
    /// Font family name.
    pub font: Option<String>,
}

/// One change to character formatting. `None` resets to the document default.
#[derive(Debug, Clone, PartialEq)]
pub enum FormatChange {
    Bold(bool),
    Italic(bool),
    Size(Option<f32>),
    Font(Option<String>),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// How a paragraph is laid out. Lengths in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParagraphFormat {
    pub align: Align,
    /// Multiple of the natural line height: 1.0 single, 2.0 double.
    pub line_spacing: f32,
    pub space_before: f32,
    pub space_after: f32,
}

impl Default for ParagraphFormat {
    fn default() -> Self {
        Self {
            align: Align::Left,
            line_spacing: 1.0,
            space_before: 0.0,
            space_after: 0.0,
        }
    }
}

/// One change to paragraph formatting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParagraphChange {
    Align(Align),
    LineSpacing(f32),
    SpaceBefore(f32),
    SpaceAfter(f32),
}

impl ParagraphFormat {
    fn apply(&mut self, change: ParagraphChange) {
        match change {
            ParagraphChange::Align(a) => self.align = a,
            ParagraphChange::LineSpacing(v) => self.line_spacing = v,
            ParagraphChange::SpaceBefore(v) => self.space_before = v,
            ParagraphChange::SpaceAfter(v) => self.space_after = v,
        }
    }

    fn from_attrs(get: impl Fn(&str) -> Option<LoroValue>) -> Self {
        let d = Self::default();
        Self {
            align: match get(P_ALIGN).as_ref().and_then(as_str).as_deref() {
                Some("center") => Align::Center,
                Some("right") => Align::Right,
                Some("justify") => Align::Justify,
                _ => Align::Left,
            },
            line_spacing: get(P_LINE).and_then(as_f32).unwrap_or(d.line_spacing),
            space_before: get(P_BEFORE).and_then(as_f32).unwrap_or(d.space_before),
            space_after: get(P_AFTER).and_then(as_f32).unwrap_or(d.space_after),
        }
    }

    /// The stored value for each key; `None` (the default) is stored as absent.
    fn to_attrs(self) -> [(&'static str, Option<LoroValue>); 4] {
        let d = Self::default();
        let num = |v: f32, default: f32| (v != default).then(|| LoroValue::from(f64::from(v)));
        let align = match self.align {
            Align::Left => None,
            Align::Center => Some("center"),
            Align::Right => Some("right"),
            Align::Justify => Some("justify"),
        };
        [
            (P_ALIGN, align.map(LoroValue::from)),
            (P_LINE, num(self.line_spacing, d.line_spacing)),
            (P_BEFORE, num(self.space_before, d.space_before)),
            (P_AFTER, num(self.space_after, d.space_after)),
        ]
    }
}

/// A run of characters in one paragraph sharing a format. `range` is in chars from the
/// paragraph's start.
#[derive(Debug, Clone, PartialEq)]
pub struct FormatRun {
    pub range: Range<usize>,
    pub format: CharFormat,
}

/// Formatting of one paragraph ([`Outline::slots`] entry): its non-default character runs and
/// its paragraph format.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SlotFormat {
    pub runs: Vec<FormatRun>,
    pub paragraph: ParagraphFormat,
    /// Images anchored in the paragraph, in text order.
    pub images: Vec<ImageRef>,
}

type Attrs = Vec<(String, LoroValue)>;

/// A container's marks as consecutive runs over char ranges.
struct Marks {
    runs: Vec<(Range<usize>, Attrs)>,
    len: usize,
}

impl Marks {
    fn of(text: &LoroText) -> Marks {
        let mut runs = Vec::new();
        let mut at = 0;
        for d in text.to_delta() {
            let TextDelta::Insert { insert, attributes } = d else {
                continue;
            };
            let n = insert.chars().count();
            let attrs: Attrs = attributes
                .map(|a| a.into_iter().filter(|(_, v)| !v.is_null()).collect())
                .unwrap_or_default();
            runs.push((at..at + n, attrs));
            at += n;
        }
        Marks { runs, len: at }
    }

    fn attrs_at(&self, i: usize) -> Option<&Attrs> {
        let k = self.runs.partition_point(|(r, _)| r.end <= i);
        self.runs
            .get(k)
            .filter(|(r, _)| r.contains(&i))
            .map(|(_, a)| a)
    }

    fn char_format(&self, i: usize) -> CharFormat {
        self.attrs_at(i).map(char_format).unwrap_or_default()
    }

    /// Non-default runs within `range`, relative to its start.
    fn slice(&self, range: Range<usize>) -> Vec<FormatRun> {
        let mut out: Vec<FormatRun> = Vec::new();
        let from = self.runs.partition_point(|(r, _)| r.end <= range.start);
        for (r, attrs) in &self.runs[from..] {
            if r.start >= range.end {
                break;
            }
            let format = char_format(attrs);
            if format == CharFormat::default() {
                continue;
            }
            let lo = r.start.max(range.start) - range.start;
            let hi = r.end.min(range.end) - range.start;
            out.push(FormatRun {
                range: lo..hi,
                format,
            });
        }
        out
    }
}

fn char_format(attrs: &Attrs) -> CharFormat {
    let get = |k: &str| attrs.iter().find(|(key, _)| key == k).map(|(_, v)| v);
    CharFormat {
        bold: matches!(get(BOLD), Some(LoroValue::Bool(true))),
        italic: matches!(get(ITALIC), Some(LoroValue::Bool(true))),
        size: get(SIZE).cloned().and_then(as_f32),
        font: get(FONT).and_then(as_str),
    }
}

fn as_f32(v: LoroValue) -> Option<f32> {
    match v {
        LoroValue::Double(d) => Some(d as f32),
        LoroValue::I64(i) => Some(i as f32),
        _ => None,
    }
}

fn as_str(v: &LoroValue) -> Option<String> {
    v.as_string().map(|s| s.to_string())
}

/// Where a paragraph's format is stored.
enum Holder {
    /// The paragraph's terminating character, at this char index.
    Char(usize),
    /// Beside the container: the paragraph is its last.
    Tail,
}

/// Formatting by flat position (see [`Outline`]). `outline` must describe the current state.
impl Document {
    /// Formatting for every paragraph in `outline.slots`, in the same order.
    pub fn formats(&self, outline: &Outline) -> Vec<SlotFormat> {
        let mut marks: HashMap<TextRef, Option<Marks>> = HashMap::new();
        outline
            .slots
            .iter()
            .map(|slot| {
                let Some(m) = marks
                    .entry(slot.container.clone())
                    .or_insert_with(|| self.container_marks(&slot.container))
                else {
                    return SlotFormat::default();
                };
                let n = slot.text.chars().count();
                let end = slot.offset + n;
                let paragraph = if end < m.len {
                    ParagraphFormat::from_attrs(|k| {
                        m.attrs_at(end)
                            .and_then(|a| a.iter().find(|(key, _)| key == k))
                            .map(|(_, v)| v.clone())
                    })
                } else {
                    self.tail_format(&slot.container)
                };
                let images = slot
                    .text
                    .chars()
                    .enumerate()
                    .filter(|(_, c)| *c == IMAGE_PLACEHOLDER)
                    .filter_map(|(k, _)| {
                        let id = m
                            .attrs_at(slot.offset + k)?
                            .iter()
                            .find(|(key, _)| key == IMAGE_MARK)
                            .and_then(|(_, v)| as_str(v))?;
                        self.image(&id)
                            .is_some()
                            .then_some(ImageRef { offset: k, id })
                    })
                    .collect();
                SlotFormat {
                    runs: m.slice(slot.offset..end),
                    paragraph,
                    images,
                }
            })
            .collect()
    }

    /// Applies a character format change to everything between two flat positions, across
    /// paragraphs, cells and tables.
    pub fn format_at(
        &self,
        outline: &Outline,
        a: usize,
        b: usize,
        change: &FormatChange,
    ) -> Result<(), ModelError> {
        let (lo, hi) = (a.min(b), a.max(b));
        for slot in &outline.slots {
            let (from, to) = (lo.max(slot.flat), hi.min(slot.flat_end()));
            if from >= to {
                continue;
            }
            let text = self.container(&slot.container)?;
            let range = slot.offset + from - slot.flat..slot.offset + to - slot.flat;
            apply_change(&text, range, change)?;
        }
        self.doc.commit();
        Ok(())
    }

    /// The character format at a caret, or across a selection: bold/italic only if every
    /// selected character is; size and font from the first character.
    ///
    /// At a caret it's the character before it, or after it at the start of a paragraph
    /// (what typing there would produce).
    pub fn char_format_at(&self, outline: &Outline, a: usize, b: usize) -> CharFormat {
        let (lo, hi) = (a.min(b), a.max(b));
        let mut marks: HashMap<TextRef, Option<Marks>> = HashMap::new();
        let mut out: Option<CharFormat> = None;
        for slot in &outline.slots {
            let (from, to) = (lo.max(slot.flat), hi.min(slot.flat_end()));
            if from >= to {
                continue;
            }
            let Some(m) = marks
                .entry(slot.container.clone())
                .or_insert_with(|| self.container_marks(&slot.container))
            else {
                continue;
            };
            for flat in from..to {
                let f = m.char_format(slot.offset + flat - slot.flat);
                match &mut out {
                    None => out = Some(f),
                    Some(o) => {
                        o.bold &= f.bold;
                        o.italic &= f.italic;
                    }
                }
            }
        }
        if let Some(f) = out {
            return f;
        }
        // A caret (or a selection holding no characters).
        let Some((i, off)) = outline.slot_at(outline.snap(lo)) else {
            return CharFormat::default();
        };
        let slot = &outline.slots[i];
        let n = slot.text.chars().count();
        let Some(m) = self.container_marks(&slot.container) else {
            return CharFormat::default();
        };
        match (off, n) {
            (_, 0) => CharFormat::default(),
            (0, _) => m.char_format(slot.offset),
            _ => m.char_format(slot.offset + off - 1),
        }
    }

    /// The paragraph format at flat position `pos`.
    pub fn paragraph_format_at(&self, outline: &Outline, pos: usize) -> ParagraphFormat {
        let loc = outline.locate(outline.snap(pos));
        self.paragraph_format_in(&loc.container, loc.offset)
    }

    /// Changes the format of every paragraph touched by the range between two flat positions
    /// (the caret's paragraph if they're equal).
    pub fn set_paragraph_format_at(
        &self,
        outline: &Outline,
        a: usize,
        b: usize,
        change: ParagraphChange,
    ) -> Result<(), ModelError> {
        let (lo, hi) = (a.min(b), a.max(b));
        let (lo, hi) = (outline.snap(lo), outline.snap(hi));
        for slot in &outline.slots {
            if slot.flat > hi || slot.flat_end() < lo {
                continue;
            }
            let mut f = self.paragraph_format_in(&slot.container, slot.offset);
            f.apply(change);
            self.write_paragraph_format(&slot.container, slot.offset, f)?;
        }
        self.doc.commit();
        Ok(())
    }
}

/// Internals used by editing (document.rs), which keeps formats intact across splits and joins.
impl Document {
    /// Format of the paragraph containing char `offset` of a container.
    pub(crate) fn paragraph_format_in(
        &self,
        container: &TextRef,
        offset: usize,
    ) -> ParagraphFormat {
        let Ok(text) = self.container(container) else {
            return ParagraphFormat::default();
        };
        match holder(&text.to_string(), offset, &self.table_chars(container)) {
            Holder::Char(i) => {
                let m = Marks::of(&text);
                ParagraphFormat::from_attrs(|k| {
                    m.attrs_at(i)
                        .and_then(|a| a.iter().find(|(key, _)| key == k))
                        .map(|(_, v)| v.clone())
                })
            }
            Holder::Tail => self.tail_format(container),
        }
    }

    /// Sets the format of the paragraph containing char `offset`, without committing.
    pub(crate) fn write_paragraph_format(
        &self,
        container: &TextRef,
        offset: usize,
        f: ParagraphFormat,
    ) -> Result<(), ModelError> {
        let text = self.container(container)?;
        match holder(&text.to_string(), offset, &self.table_chars(container)) {
            Holder::Char(i) => mark_paragraph(&text, i, f),
            Holder::Tail => {
                let (map, prefix) = self.tail_map(container)?;
                for (key, value) in f.to_attrs() {
                    let key = format!("{prefix}{key}");
                    match value {
                        Some(v) => map.insert(&key, v).map_err(crdt)?,
                        None => map.delete(&key).map_err(crdt)?,
                    }
                }
                Ok(())
            }
        }
    }

    /// Gives `range` of a container the format `f`, without committing.
    pub(crate) fn set_char_format(
        &self,
        text: &LoroText,
        range: Range<usize>,
        f: &CharFormat,
    ) -> Result<(), ModelError> {
        for change in [
            FormatChange::Bold(f.bold),
            FormatChange::Italic(f.italic),
            FormatChange::Size(f.size),
            FormatChange::Font(f.font.clone()),
        ] {
            apply_change(text, range.clone(), &change)?;
        }
        Ok(())
    }

    /// The format of the character at `i` of a container's text.
    pub(crate) fn char_format_in(&self, text: &LoroText, i: usize) -> CharFormat {
        Marks::of(text).char_format(i)
    }

    /// Removes the tail format keys of a deleted column from every row.
    pub(crate) fn delete_cell_tail(row: &LoroMap, column: &str) -> Result<(), ModelError> {
        for key in PARAGRAPH_KEYS {
            let key = format!("{column}|{key}");
            if row.get(&key).is_some() {
                row.delete(&key).map_err(crdt)?;
            }
        }
        Ok(())
    }

    /// Char indices of the table placeholders in a container (only the body holds tables).
    fn table_chars(&self, container: &TextRef) -> Vec<usize> {
        match container {
            TextRef::Body => self.placeholders().into_iter().map(|(i, _)| i).collect(),
            TextRef::Cell(_) => Vec::new(),
        }
    }

    fn container_marks(&self, container: &TextRef) -> Option<Marks> {
        match container {
            TextRef::Body => Some(Marks::of(&self.body())),
            TextRef::Cell(c) => self.existing_cell(c).map(|t| Marks::of(&t)),
        }
    }

    fn tail_format(&self, container: &TextRef) -> ParagraphFormat {
        let Ok((map, prefix)) = self.tail_map(container) else {
            return ParagraphFormat::default();
        };
        ParagraphFormat::from_attrs(|k| match map.get(&format!("{prefix}{k}"))? {
            ValueOrContainer::Value(v) => Some(v),
            ValueOrContainer::Container(_) => None,
        })
    }

    /// The map holding a container's tail format, and the key prefix within it.
    fn tail_map(&self, container: &TextRef) -> Result<(LoroMap, String), ModelError> {
        match container {
            TextRef::Body => Ok((self.doc.get_map(BODY_TAIL), String::new())),
            TextRef::Cell(c) => {
                let t = self.table(&c.table).ok_or(ModelError::NoSuchTable)?;
                let col = t
                    .columns()
                    .get(c.col)
                    .cloned()
                    .ok_or(ModelError::NoSuchCell)?;
                let row = t.rows().get(c.row).cloned().ok_or(ModelError::NoSuchCell)?;
                Ok((row, format!("{col}|")))
            }
        }
    }
}

/// Where the format of the paragraph containing `offset` lives: the first line break or table
/// placeholder (`tables`: their char indices) at or after it, or the container's tail. Image
/// placeholders use the same character as tables but sit inside paragraphs, so the character
/// alone can't tell.
fn holder(text: &str, offset: usize, tables: &[usize]) -> Holder {
    text.chars()
        .enumerate()
        .skip(offset)
        .find(|(i, c)| *c == '\n' || tables.contains(i))
        .map_or(Holder::Tail, |(i, _)| Holder::Char(i))
}

/// Writes a paragraph format onto its terminating character.
pub(crate) fn mark_paragraph(
    text: &LoroText,
    i: usize,
    f: ParagraphFormat,
) -> Result<(), ModelError> {
    for (key, value) in f.to_attrs() {
        match value {
            Some(v) => text.mark(i..i + 1, key, v).map_err(crdt)?,
            None => text.unmark(i..i + 1, key).map_err(crdt)?,
        }
    }
    Ok(())
}

fn apply_change(
    text: &LoroText,
    range: Range<usize>,
    change: &FormatChange,
) -> Result<(), ModelError> {
    if range.is_empty() {
        return Ok(());
    }
    let r = match change {
        FormatChange::Bold(true) => text.mark(range, BOLD, true),
        FormatChange::Bold(false) => text.unmark(range, BOLD),
        FormatChange::Italic(true) => text.mark(range, ITALIC, true),
        FormatChange::Italic(false) => text.unmark(range, ITALIC),
        FormatChange::Size(Some(s)) => text.mark(range, SIZE, f64::from(*s)),
        FormatChange::Size(None) => text.unmark(range, SIZE),
        FormatChange::Font(Some(f)) => text.mark(range, FONT, f.as_str()),
        FormatChange::Font(None) => text.unmark(range, FONT),
    };
    r.map_err(crdt)
}

fn crdt(e: impl std::fmt::Display) -> ModelError {
    ModelError::Crdt(e.to_string())
}
