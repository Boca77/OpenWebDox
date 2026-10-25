//! Rich text: character formats and paragraph formats.
//!
//! **Character formats** are Peritext-style marks over text ranges: `bold`, `italic`,
//! `underline` and `strike` (`true`), `size` (points), `font` (a family name), `color` and
//! `highlight` (`#rrggbb`), `script` (`super` / `sub`) and `link` (a URL). They expand after,
//! so text typed at the end of a bold run is bold, except links, which never expand: typing
//! just after a link isn't part of it, as in Google Docs. Text typed at the start of a
//! paragraph takes the format of the character after it (but not its link).
//!
//! **Paragraph formats** (`p_align`, `p_line`, `p_before`, `p_after`, the indents and the
//! list keys) are marks on the character that ends the paragraph: its `\n`, or a table
//! placeholder. They never expand.
//! The last paragraph of a container has no such character, so its format is kept beside the
//! container: in the `body_tail` map for the body, and under `<column id>|p_*` keys in the
//! row map for a cell. Splitting a paragraph copies its format to the new line break; joining
//! two keeps the first one's format, as in Google Docs.

use std::collections::HashMap;
use std::ops::Range;

use loro::cursor::PosType;
use loro::{LoroMap, LoroText, LoroValue, TextDelta, ValueOrContainer};

use crate::document::Document;
use crate::error::ModelError;
use crate::header::{FieldKind, FieldRef, FIELD_MARK};
use crate::image::{ImageRef, IMAGE_MARK, IMAGE_PLACEHOLDER};
use crate::outline::{Outline, TextRef};
use crate::style::StyleName;
use crate::table::{TABLE_MARK, TABLE_PLACEHOLDER};

pub(crate) const BOLD: &str = "bold";
pub(crate) const ITALIC: &str = "italic";
pub(crate) const SIZE: &str = "size";
pub(crate) const FONT: &str = "font";
const UNDERLINE: &str = "underline";
const STRIKE: &str = "strike";
const COLOR: &str = "color";
const HIGHLIGHT: &str = "highlight";
const SCRIPT: &str = "script";
pub(crate) const LINK: &str = "link";
pub(crate) const P_ALIGN: &str = "p_align";
pub(crate) const P_LINE: &str = "p_line";
pub(crate) const P_BEFORE: &str = "p_before";
pub(crate) const P_AFTER: &str = "p_after";
const P_INDENT_START: &str = "p_ind_start";
const P_INDENT_END: &str = "p_ind_end";
const P_INDENT_FIRST: &str = "p_ind_first";
const P_LIST: &str = "p_list";
const P_LEVEL: &str = "p_level";
const P_LIST_ID: &str = "p_list_id";
const P_CHECKED: &str = "p_checked";
const P_STYLE: &str = "p_style";
const P_BREAK: &str = "p_break";
/// Character formats that expand after (every one but [`LINK`]).
pub(crate) const CHAR_KEYS: [&str; 9] = [
    BOLD, ITALIC, SIZE, FONT, UNDERLINE, STRIKE, COLOR, HIGHLIGHT, SCRIPT,
];
pub(crate) const PARAGRAPH_KEYS: [&str; 13] = [
    P_ALIGN,
    P_LINE,
    P_BEFORE,
    P_AFTER,
    P_INDENT_START,
    P_INDENT_END,
    P_INDENT_FIRST,
    P_LIST,
    P_LEVEL,
    P_LIST_ID,
    P_CHECKED,
    P_STYLE,
    P_BREAK,
];
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
    pub underline: bool,
    pub strike: bool,
    /// Text colour as `#rrggbb`.
    pub color: Option<String>,
    /// Background colour as `#rrggbb`.
    pub highlight: Option<String>,
    pub script: Script,
    /// The URL the text links to.
    pub link: Option<String>,
}

/// Superscript or subscript.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Script {
    #[default]
    Normal,
    Super,
    Sub,
}

/// One change to character formatting. `None` resets to the document default.
#[derive(Debug, Clone, PartialEq)]
pub enum FormatChange {
    Bold(bool),
    Italic(bool),
    Size(Option<f32>),
    Font(Option<String>),
    Underline(bool),
    Strike(bool),
    /// `#rrggbb`.
    Color(Option<String>),
    /// `#rrggbb`.
    Highlight(Option<String>),
    Script(Script),
    Link(Option<String>),
}

impl CharFormat {
    /// The format with one change applied.
    pub fn apply(&mut self, change: FormatChange) {
        match change {
            FormatChange::Bold(b) => self.bold = b,
            FormatChange::Italic(i) => self.italic = i,
            FormatChange::Size(s) => self.size = s,
            FormatChange::Font(f) => self.font = f,
            FormatChange::Underline(u) => self.underline = u,
            FormatChange::Strike(s) => self.strike = s,
            FormatChange::Color(c) => self.color = c,
            FormatChange::Highlight(h) => self.highlight = h,
            FormatChange::Script(s) => self.script = s,
            FormatChange::Link(l) => self.link = l,
        }
    }

    /// Every property as a change, to give a range exactly this format.
    fn changes(&self) -> [FormatChange; 10] {
        [
            FormatChange::Bold(self.bold),
            FormatChange::Italic(self.italic),
            FormatChange::Size(self.size),
            FormatChange::Font(self.font.clone()),
            FormatChange::Underline(self.underline),
            FormatChange::Strike(self.strike),
            FormatChange::Color(self.color.clone()),
            FormatChange::Highlight(self.highlight.clone()),
            FormatChange::Script(self.script),
            FormatChange::Link(self.link.clone()),
        ]
    }
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
    /// Extra indent of every line from the left margin. A list item is indented further by
    /// its level (see [`crate::list_indent`]).
    pub indent_start: f32,
    /// Indent from the right margin.
    pub indent_end: f32,
    /// Where the first line starts, relative to the other lines: positive indents it,
    /// negative hangs it out to the left.
    pub indent_first: f32,
    /// The list the paragraph is an item of, if any.
    pub list: Option<ListFormat>,
    /// The paragraph style (heading level, title…): how its text looks unless formatted
    /// otherwise. See [`crate::StyleName::look`].
    pub style: StyleName,
    /// Starts on a new page (a page break before it, as Ctrl+Enter makes).
    pub page_break_before: bool,
}

impl Default for ParagraphFormat {
    fn default() -> Self {
        Self {
            align: Align::Left,
            line_spacing: 1.0,
            space_before: 0.0,
            space_after: 0.0,
            indent_start: 0.0,
            indent_end: 0.0,
            indent_first: 0.0,
            list: None,
            style: StyleName::Normal,
            page_break_before: false,
        }
    }
}

/// What a list item's marker looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListKind {
    /// ● ○ ■ by level.
    Bullet,
    /// 1. a. i. by level.
    Number,
    /// A checkbox, ticked when [`ListFormat::checked`].
    Check,
}

/// A paragraph's place in a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListFormat {
    pub kind: ListKind,
    /// Nesting depth, `0..=`[`MAX_LIST_LEVEL`].
    pub level: u8,
    /// Which list the item belongs to: items of one list are numbered together, even with
    /// other paragraphs between them, and separate lists number separately.
    pub id: u64,
    /// Only for checklists.
    pub checked: bool,
}

/// Deepest list level (nine levels, as in Google Docs and Word).
pub const MAX_LIST_LEVEL: u8 = 8;

/// One change to paragraph formatting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParagraphChange {
    Align(Align),
    LineSpacing(f32),
    SpaceBefore(f32),
    SpaceAfter(f32),
    IndentStart(f32),
    IndentEnd(f32),
    IndentFirst(f32),
    /// Makes the paragraph a list item (or a plain paragraph with `None`).
    List(Option<ListFormat>),
    /// Ticks or clears a checklist item; no effect on other paragraphs.
    Checked(bool),
    Style(StyleName),
    PageBreakBefore(bool),
}

impl ParagraphFormat {
    pub(crate) fn apply(&mut self, change: ParagraphChange) {
        match change {
            ParagraphChange::Align(a) => self.align = a,
            ParagraphChange::LineSpacing(v) => self.line_spacing = v,
            ParagraphChange::SpaceBefore(v) => self.space_before = v,
            ParagraphChange::SpaceAfter(v) => self.space_after = v,
            ParagraphChange::IndentStart(v) => self.indent_start = v,
            ParagraphChange::IndentEnd(v) => self.indent_end = v,
            ParagraphChange::IndentFirst(v) => self.indent_first = v,
            ParagraphChange::List(l) => self.list = l,
            ParagraphChange::Style(s) => self.style = s,
            ParagraphChange::PageBreakBefore(b) => self.page_break_before = b,
            ParagraphChange::Checked(c) => {
                if let Some(l) = &mut self.list {
                    l.checked = c && l.kind == ListKind::Check;
                }
            }
        }
    }

    fn from_attrs(get: impl Fn(&str) -> Option<LoroValue>) -> Self {
        let d = Self::default();
        let list = match get(P_LIST).as_ref().and_then(as_str).as_deref() {
            Some("bullet") => Some(ListKind::Bullet),
            Some("number") => Some(ListKind::Number),
            Some("check") => Some(ListKind::Check),
            _ => None,
        }
        .map(|kind| ListFormat {
            kind,
            level: get(P_LEVEL)
                .and_then(as_f32)
                .map_or(0, |v| v.clamp(0.0, f32::from(MAX_LIST_LEVEL)) as u8),
            id: get(P_LIST_ID)
                .as_ref()
                .and_then(as_str)
                .and_then(|s| u64::from_str_radix(&s, 16).ok())
                .unwrap_or(0),
            checked: kind == ListKind::Check
                && matches!(get(P_CHECKED), Some(LoroValue::Bool(true))),
        });
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
            indent_start: get(P_INDENT_START).and_then(as_f32).unwrap_or(0.0),
            indent_end: get(P_INDENT_END).and_then(as_f32).unwrap_or(0.0),
            indent_first: get(P_INDENT_FIRST).and_then(as_f32).unwrap_or(0.0),
            list,
            style: get(P_STYLE)
                .as_ref()
                .and_then(as_str)
                .and_then(|s| StyleName::parse(&s))
                .unwrap_or_default(),
            page_break_before: matches!(get(P_BREAK), Some(LoroValue::Bool(true))),
        }
    }

    /// The stored value for each key; `None` (the default) is stored as absent.
    fn to_attrs(self) -> [(&'static str, Option<LoroValue>); 13] {
        let d = Self::default();
        let num = |v: f32, default: f32| (v != default).then(|| LoroValue::from(f64::from(v)));
        let align = match self.align {
            Align::Left => None,
            Align::Center => Some("center"),
            Align::Right => Some("right"),
            Align::Justify => Some("justify"),
        };
        let l = self.list;
        let kind = l.map(|l| match l.kind {
            ListKind::Bullet => "bullet",
            ListKind::Number => "number",
            ListKind::Check => "check",
        });
        [
            (P_ALIGN, align.map(LoroValue::from)),
            (P_LINE, num(self.line_spacing, d.line_spacing)),
            (P_BEFORE, num(self.space_before, d.space_before)),
            (P_AFTER, num(self.space_after, d.space_after)),
            (P_INDENT_START, num(self.indent_start, 0.0)),
            (P_INDENT_END, num(self.indent_end, 0.0)),
            (P_INDENT_FIRST, num(self.indent_first, 0.0)),
            (P_LIST, kind.map(LoroValue::from)),
            (
                P_LEVEL,
                l.filter(|l| l.level > 0)
                    .map(|l| LoroValue::from(i64::from(l.level))),
            ),
            (P_LIST_ID, l.map(|l| LoroValue::from(format!("{:x}", l.id)))),
            (
                P_CHECKED,
                l.filter(|l| l.checked).map(|_| LoroValue::from(true)),
            ),
            (
                P_STYLE,
                (self.style != StyleName::Normal).then(|| LoroValue::from(self.style.key())),
            ),
            (
                P_BREAK,
                self.page_break_before.then(|| LoroValue::from(true)),
            ),
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
    /// Page number fields in the paragraph (headers and footers), in text order.
    pub fields: Vec<FieldRef>,
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
        underline: matches!(get(UNDERLINE), Some(LoroValue::Bool(true))),
        strike: matches!(get(STRIKE), Some(LoroValue::Bool(true))),
        color: get(COLOR).and_then(as_str),
        highlight: get(HIGHLIGHT).and_then(as_str),
        script: match get(SCRIPT).and_then(as_str).as_deref() {
            Some("super") => Script::Super,
            Some("sub") => Script::Sub,
            _ => Script::Normal,
        },
        link: get(LINK).and_then(as_str),
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
                let fields = slot
                    .text
                    .chars()
                    .enumerate()
                    .filter(|(_, c)| *c == IMAGE_PLACEHOLDER)
                    .filter_map(|(k, _)| {
                        let kind = m
                            .attrs_at(slot.offset + k)?
                            .iter()
                            .find(|(key, _)| key == FIELD_MARK)
                            .and_then(|(_, v)| as_str(v))
                            .and_then(|v| FieldKind::parse(&v))?;
                        Some(FieldRef { offset: k, kind })
                    })
                    .collect();
                SlotFormat {
                    runs: m.slice(slot.offset..end),
                    paragraph,
                    images,
                    fields,
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

    /// The character format at a caret, or across a selection: bold, italic, underline and
    /// strikethrough only if every selected character has them; the rest from the first
    /// character.
    ///
    /// At a caret it's the character before it, or after it at the start of a paragraph
    /// (what typing there would produce).
    pub fn char_format_at(&self, outline: &Outline, a: usize, b: usize) -> CharFormat {
        let (lo, hi) = (a.min(b), a.max(b));
        let mut out: Option<CharFormat> = None;
        // Only the selected stretch of each paragraph is read, not whole containers.
        for slot in &outline.slots {
            let (from, to) = (lo.max(slot.flat), hi.min(slot.flat_end()));
            if from >= to {
                continue;
            }
            let Some(text) = self.existing_container(&slot.container) else {
                continue;
            };
            let (start, end) = (slot.offset + from - slot.flat, slot.offset + to - slot.flat);
            let Ok(delta) = text.slice_delta(start, end, PosType::Unicode) else {
                continue;
            };
            for d in delta {
                let TextDelta::Insert { attributes, .. } = d else {
                    continue;
                };
                let attrs: Attrs = attributes
                    .map(|a| a.into_iter().filter(|(_, v)| !v.is_null()).collect())
                    .unwrap_or_default();
                let f = char_format(&attrs);
                match &mut out {
                    None => out = Some(f),
                    Some(o) => {
                        o.bold &= f.bold;
                        o.italic &= f.italic;
                        o.underline &= f.underline;
                        o.strike &= f.strike;
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
        let Some(text) = self.existing_container(&slot.container) else {
            return CharFormat::default();
        };
        match (off, n) {
            (_, 0) => CharFormat::default(),
            (0, _) => self.char_format_in(&text, slot.offset),
            _ => self.char_format_in(&text, slot.offset + off - 1),
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
        match self.holder(&text, container, offset) {
            Holder::Char(i) => {
                let attrs = attrs_of(&text, i);
                ParagraphFormat::from_attrs(|k| {
                    attrs
                        .iter()
                        .find(|(key, _)| key == k)
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
        match self.holder(&text, container, offset) {
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
        for change in f.changes() {
            apply_change(text, range.clone(), &change)?;
        }
        Ok(())
    }

    /// The format of the character at `i` of a container's text.
    pub(crate) fn char_format_in(&self, text: &LoroText, i: usize) -> CharFormat {
        char_format(&attrs_of(text, i))
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

    /// Where the format of the paragraph containing char `offset` lives: the first line break
    /// or table placeholder at or after it, or the container's tail. Image placeholders use
    /// the same character as tables but sit inside paragraphs, so a placeholder counts only
    /// if it's marked as a table of the body. Reads forward from `offset` in chunks: the cost
    /// is the rest of the paragraph, not the document.
    fn holder(&self, text: &LoroText, container: &TextRef, offset: usize) -> Holder {
        const CHUNK: usize = 1024;
        let len = text.len_unicode();
        let mut at = offset;
        while at < len {
            let end = (at + CHUNK).min(len);
            let Ok(s) = text.slice(at, end) else {
                break;
            };
            for (k, c) in s.chars().enumerate() {
                let i = at + k;
                if c == '\n' {
                    return Holder::Char(i);
                }
                if c == TABLE_PLACEHOLDER
                    && *container == TextRef::Body
                    && self.is_table_at(text, i)
                {
                    return Holder::Char(i);
                }
            }
            at = end;
        }
        Holder::Tail
    }

    /// Whether body char `i` is a table's placeholder (marked with an existing table's id).
    fn is_table_at(&self, text: &LoroText, i: usize) -> bool {
        attrs_of(text, i)
            .iter()
            .find(|(k, _)| k == TABLE_MARK)
            .and_then(|(_, v)| as_str(v))
            .is_some_and(|id| self.table(&id).is_some())
    }

    /// A container's text, if it exists (a never-written cell has none).
    fn existing_container(&self, container: &TextRef) -> Option<LoroText> {
        match container {
            TextRef::Cell(c) => self.existing_cell(c),
            _ => self.container(container).ok(),
        }
    }

    fn container_marks(&self, container: &TextRef) -> Option<Marks> {
        match container {
            TextRef::Body => Some(Marks::of(&self.body())),
            TextRef::Cell(c) => self.existing_cell(c).map(|t| Marks::of(&t)),
            TextRef::Header | TextRef::Footer | TextRef::Footnote(_) => {
                self.container(container).ok().map(|t| Marks::of(&t))
            }
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
            TextRef::Header => Ok((self.doc.get_map("header_tail"), String::new())),
            TextRef::Footer => Ok((self.doc.get_map("footer_tail"), String::new())),
            TextRef::Footnote(id) => Ok((
                self.doc.get_map(crate::footnote::FOOTNOTE_TAIL),
                format!("{id}|"),
            )),
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

/// The marks on char `i` of a text (none past its end).
pub(crate) fn attrs_of(text: &LoroText, i: usize) -> Attrs {
    let Ok(delta) = text.slice_delta(i, i + 1, PosType::Unicode) else {
        return Attrs::new();
    };
    delta
        .into_iter()
        .find_map(|d| match d {
            TextDelta::Insert { attributes, .. } => attributes,
            _ => None,
        })
        .map(|a| a.into_iter().filter(|(_, v)| !v.is_null()).collect())
        .unwrap_or_default()
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
        FormatChange::Underline(true) => text.mark(range, UNDERLINE, true),
        FormatChange::Underline(false) => text.unmark(range, UNDERLINE),
        FormatChange::Strike(true) => text.mark(range, STRIKE, true),
        FormatChange::Strike(false) => text.unmark(range, STRIKE),
        FormatChange::Color(Some(c)) => text.mark(range, COLOR, c.as_str()),
        FormatChange::Color(None) => text.unmark(range, COLOR),
        FormatChange::Highlight(Some(c)) => text.mark(range, HIGHLIGHT, c.as_str()),
        FormatChange::Highlight(None) => text.unmark(range, HIGHLIGHT),
        FormatChange::Script(Script::Super) => text.mark(range, SCRIPT, "super"),
        FormatChange::Script(Script::Sub) => text.mark(range, SCRIPT, "sub"),
        FormatChange::Script(Script::Normal) => text.unmark(range, SCRIPT),
        FormatChange::Link(Some(url)) => text.mark(range, LINK, url.as_str()),
        FormatChange::Link(None) => text.unmark(range, LINK),
    };
    r.map_err(crdt)
}

fn crdt(e: impl std::fmt::Display) -> ModelError {
    ModelError::Crdt(e.to_string())
}
