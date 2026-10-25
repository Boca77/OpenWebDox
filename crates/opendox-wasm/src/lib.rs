//! WebAssembly bindings: one [`Editor`] owns a document replica, the layout engine and the
//! paginator, and hands the renderer flat glyph data.
//!
//! Positions passed in from JS are **flat positions** (see `opendox_model::Outline`): one number
//! per caret position, counting body text and table cells together in reading order.

use opendox_export::{DocxOptions, ExportImage, PdfHeading, PdfInfo, PdfLink};
use opendox_layout::{
    BlockInput, DecorationKind, FontId, FontLibrary, Layer, LayoutEngine, Marker, ObjectInput,
    ObjectMode, PageGeometry, Paginator, ParagraphInput, ParagraphStyle, StyleSpan, TableInput,
    TextPosition, TextStyle,
};
use opendox_model::{
    list_indent, list_markers, Align, Anchor, Block, CharFormat, Clip, Comment, Document,
    FieldKind, FormatChange, FormatRun, History, ImageChange, ImageData, ImageProps, ListKind,
    Outline, PageSetup, ParagraphChange, ParagraphFormat, Recolor, Region, Script, SlotFormat,
    StyleName, TextRef, Wrap,
};
use opendox_sync::{Presence, SyncClient};
use wasm_bindgen::prelude::*;

/// Floats per glyph in [`Editor::page_glyphs`].
pub const GLYPH_STRIDE: usize = 7;

/// Floats per highlight, underline or strikethrough in [`Editor::page_decorations`].
pub const DECORATION_STRIDE: usize = 6;

/// How link text looks unless it has its own colour (Google Docs' link blue).
const LINK_COLOR: u32 = 0x1155CC;

/// How ticked checklist items look: struck through in this grey, as in Google Docs.
const TICKED_COLOR: u32 = 0x80868B;

/// Superscript and subscript size, as a share of the text's size.
const SCRIPT_SCALE: f32 = 0.65;

/// Floats per rectangle in [`Editor::selection_rects`].
pub const RECT_STRIDE: usize = 5;

/// Floats per border line in [`Editor::page_rules`].
pub const RULE_STRIDE: usize = 4;

/// Floats per image in [`Editor::page_objects`].
pub const OBJECT_STRIDE: usize = 9;

#[wasm_bindgen]
pub struct Editor {
    doc: Document,
    engine: LayoutEngine,
    paginator: Paginator,
    style: TextStyle,
    /// Document structure as of the last [`Editor::layout`]; maps flat positions to layout.
    outline: Outline,
    sync: SyncClient,
    /// A message produced while handling a server message, waiting for [`Editor::sync_flush`].
    sync_reply: Option<Vec<u8>>,
    history: History,
    /// Where the last typing-like edit left the caret; typing on from there joins its undo
    /// step.
    last_edit_end: Option<usize>,
    /// A format chosen with nothing selected (Ctrl+B at a caret), applied to the next text
    /// typed at that position.
    pending: Option<(usize, CharFormat)>,
    /// Image ids by the key layout was given for them, as of the last layout.
    object_ids: Vec<String>,
    /// Family list appended to list markers' fonts, for bullets and checkboxes the text
    /// fonts lack.
    symbol_font: Option<String>,
    /// Each paragraph's formatting as of the last layout, for link queries.
    formats: Vec<SlotFormat>,
    /// Local edits are refused (a view-only connection, or the app's choice).
    read_only: bool,
    /// Comments as of the last layout.
    comments: Vec<Comment>,
    /// The comment shown as selected (its text highlighted more strongly).
    active_comment: Option<String>,
}

/// Bit set in [`Editor::sync_receive`]'s flags when the document changed.
pub const SYNC_DOC_CHANGED: u32 = 1;
/// Bit set when a remote caret appeared, moved or left.
pub const SYNC_PEERS_CHANGED: u32 = 2;
/// Bit set when the server said this connection may only view the document.
pub const SYNC_READ_ONLY: u32 = 4;

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Editor {
    /// An empty Letter-size document using `"Noto Sans"` at 12pt. Register fonts before layout.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Editor {
        let doc = Document::new();
        let outline = doc.outline();
        let history = History::new(&doc);
        Editor {
            doc,
            engine: LayoutEngine::new(FontLibrary::new()),
            paginator: Paginator::new(PageGeometry::letter()),
            style: TextStyle {
                font_family: "Noto Sans".into(),
                font_size: 12.0,
                font_weight: 400.0,
                italic: false,
                ..TextStyle::default()
            },
            outline,
            sync: SyncClient::new(),
            sync_reply: None,
            history,
            last_edit_end: None,
            pending: None,
            symbol_font: None,
            formats: Vec::new(),
            read_only: false,
            comments: Vec::new(),
            active_comment: None,
            object_ids: Vec::new(),
        }
    }

    /// Registers a font file and returns its id. Fonts may arrive after the first layout; the
    /// next `layout` call then reshapes every paragraph so text waiting on this face picks it up.
    pub fn register_font(&mut self, data: Vec<u8>) -> u32 {
        self.paginator.invalidate();
        self.engine.fonts_mut().register(data).0
    }

    /// Sets the default font as a CSS-style family list and a size in points.
    /// Font family list for list markers (● ○ ■ ☐ ☑) the text fonts lack, e.g.
    /// `"Noto Sans Symbols 2"`. Used for markers only: adding it to the text's family list
    /// would also change which font draws characters no font has, and with it line heights.
    pub fn set_symbol_font(&mut self, family: String) {
        self.symbol_font = (!family.is_empty()).then_some(family);
    }

    pub fn set_default_font(&mut self, family: String, size: f32) {
        self.style.font_family = family;
        self.style.font_size = size;
    }

    /// All text, paragraphs and cells separated by `\n`.
    pub fn text(&self) -> String {
        let o = self.doc.outline();
        o.slice(0, o.len)
    }

    /// Largest flat position in the body (the header and footer come after it).
    pub fn len(&self) -> usize {
        self.outline.len
    }

    /// Largest flat position anywhere: the end of the footer, header or body, whichever is
    /// last.
    pub fn end(&self) -> usize {
        self.outline
            .slots
            .last()
            .map_or(self.outline.len, |s| s.flat_end())
    }

    pub fn is_empty(&self) -> bool {
        self.outline.len == 0
    }

    /// The text between two positions (for copy).
    pub fn slice(&self, start: usize, end: usize) -> String {
        self.outline.slice(start, end)
    }

    /// The selection as HTML for the clipboard: tags and styles other editors understand,
    /// plus the exact content for pasting into opendox (see `paste`).
    pub fn copy_html(&self, start: usize, end: usize) -> String {
        self.doc.copy_at(&self.outline, start, end).to_html()
    }

    /// The selection in opendox's clipboard format (JSON).
    pub fn copy_json(&self, start: usize, end: usize) -> String {
        self.doc.copy_at(&self.outline, start, end).to_json()
    }

    /// Pastes content in opendox's clipboard format (JSON, from `copy_json` or the
    /// `data-opendox-clip` attribute of `copy_html`, or built from other HTML) at `pos`, as
    /// one undo step. Returns the caret after it.
    pub fn paste(&mut self, pos: usize, json: &str) -> Result<usize, JsError> {
        let clip = Clip::from_json(json)?;
        self.begin(pos, pos, false)?;
        self.last_edit_end = None;
        self.pending = None;
        Ok(self.doc.paste_at(&self.outline, pos, &clip)?)
    }

    // --- editing -------------------------------------------------------------------------

    /// Inserts `text` at `pos` and returns the caret position just after it.
    pub fn insert(&mut self, pos: usize, text: &str) -> Result<usize, JsError> {
        self.begin(pos, pos, true)?;
        let after = match self.pending.take().filter(|(p, _)| *p == pos) {
            Some((_, f)) => self.doc.insert_formatted_at(&self.outline, pos, text, &f)?,
            None => self.doc.insert_at(&self.outline, pos, text)?,
        };
        self.last_edit_end = Some(after);
        Ok(after)
    }

    /// Deletes between two positions; returns where the caret goes. Spanning body text and a
    /// table deletes the whole table; spanning cells of one table clears them.
    pub fn delete(&mut self, start: usize, end: usize) -> Result<usize, JsError> {
        self.begin(start, end, false)?;
        let at = self.doc.delete_range_at(&self.outline, start, end)?;
        // Typing over a selection undoes together with the deletion.
        self.last_edit_end = Some(at);
        Ok(at)
    }

    /// Backspace: returns the new caret position.
    pub fn delete_backward(&mut self, pos: usize) -> Result<usize, JsError> {
        self.begin(pos, pos, true)?;
        let at = self.doc.delete_backward_at(&self.outline, pos)?;
        self.last_edit_end = Some(at);
        Ok(at)
    }

    /// Delete key: returns the new caret position.
    pub fn delete_forward(&mut self, pos: usize) -> Result<usize, JsError> {
        self.begin(pos, pos, true)?;
        let at = self.doc.delete_forward_at(&self.outline, pos)?;
        self.last_edit_end = Some(at);
        Ok(at)
    }

    // --- formatting ----------------------------------------------------------------------

    /// Bold on or off for the selection. With nothing selected it applies to the next text
    /// typed at the caret, as in Google Docs.
    pub fn set_bold(&mut self, anchor: usize, focus: usize, on: bool) -> Result<(), JsError> {
        self.format(anchor, focus, FormatChange::Bold(on))
    }

    pub fn set_italic(&mut self, anchor: usize, focus: usize, on: bool) -> Result<(), JsError> {
        self.format(anchor, focus, FormatChange::Italic(on))
    }

    /// Font size in points; 0 returns to the default size.
    pub fn set_font_size(&mut self, anchor: usize, focus: usize, size: f32) -> Result<(), JsError> {
        self.format(
            anchor,
            focus,
            FormatChange::Size((size > 0.0).then_some(size)),
        )
    }

    /// Font family name (a registered family); empty returns to the default font.
    pub fn set_font(&mut self, anchor: usize, focus: usize, family: &str) -> Result<(), JsError> {
        let f = (!family.is_empty()).then(|| family.to_owned());
        self.format(anchor, focus, FormatChange::Font(f))
    }

    pub fn set_underline(&mut self, anchor: usize, focus: usize, on: bool) -> Result<(), JsError> {
        self.format(anchor, focus, FormatChange::Underline(on))
    }

    pub fn set_strikethrough(
        &mut self,
        anchor: usize,
        focus: usize,
        on: bool,
    ) -> Result<(), JsError> {
        self.format(anchor, focus, FormatChange::Strike(on))
    }

    /// Text colour as `#rrggbb`; empty returns to the default.
    pub fn set_text_color(
        &mut self,
        anchor: usize,
        focus: usize,
        color: &str,
    ) -> Result<(), JsError> {
        self.format(anchor, focus, FormatChange::Color(hex_color(color)))
    }

    /// Highlight colour as `#rrggbb`; empty removes it.
    pub fn set_highlight(
        &mut self,
        anchor: usize,
        focus: usize,
        color: &str,
    ) -> Result<(), JsError> {
        self.format(anchor, focus, FormatChange::Highlight(hex_color(color)))
    }

    /// 0 normal, 1 superscript, 2 subscript.
    pub fn set_script(&mut self, anchor: usize, focus: usize, script: u32) -> Result<(), JsError> {
        let s = match script {
            1 => Script::Super,
            2 => Script::Sub,
            _ => Script::Normal,
        };
        self.format(anchor, focus, FormatChange::Script(s))
    }

    /// Links the selection to `url` (empty removes the link). With nothing selected: inside
    /// a link, changes or removes that whole link; elsewhere inserts `url` as linked text.
    /// Returns the linked range as `[start, end]`, to select.
    pub fn set_link(
        &mut self,
        anchor: usize,
        focus: usize,
        url: &str,
    ) -> Result<Vec<u32>, JsError> {
        let link = (!url.is_empty()).then(|| url.to_owned());
        let (mut lo, mut hi) = (anchor.min(focus), anchor.max(focus));
        if lo == hi {
            if let Some([a, b]) = self.link_bounds(lo) {
                (lo, hi) = (a, b);
            } else if let Some(url) = &link {
                self.begin(lo, lo, false)?;
                self.last_edit_end = None;
                let end = self.doc.insert_at(&self.outline, lo, url)?;
                let fresh = self.doc.outline();
                self.doc
                    .format_at(&fresh, lo, end, &FormatChange::Link(link.clone()))?;
                return Ok(vec![lo as u32, end as u32]);
            } else {
                return Ok(vec![lo as u32, hi as u32]);
            }
        }
        self.begin(lo, hi, false)?;
        self.last_edit_end = None;
        self.pending = None;
        self.doc
            .format_at(&self.outline, lo, hi, &FormatChange::Link(link))?;
        Ok(vec![lo as u32, hi as u32])
    }

    /// The URL of the link at the caret (the character after it, or before it at a link's
    /// end), per the last layout.
    pub fn link_at(&self, pos: usize) -> Option<String> {
        let (slot, off) = self.outline.slot_at(self.outline.snap(pos))?;
        let runs = &self.formats.get(slot)?.runs;
        let at = |i: usize| {
            runs.iter()
                .find(|r| r.range.contains(&i))
                .and_then(|r| r.format.link.clone())
        };
        at(off).or_else(|| at(off.checked_sub(1)?))
    }

    /// `[start, end]` of the whole link at the caret, or `undefined`.
    pub fn link_range(&self, pos: usize) -> Option<Vec<u32>> {
        self.link_bounds(pos).map(|r| r.map(|p| p as u32).to_vec())
    }

    /// `[bold, italic, size, underline, strikethrough, script]` at the caret or across the
    /// selection (bold, italic, underline and strikethrough only if all of it has them). Size
    /// is the text's own, else its paragraph style's, else 0 for the document default;
    /// script 0 normal, 1 superscript, 2 subscript. Includes a pending format at the caret.
    pub fn char_format(&self, anchor: usize, focus: usize) -> Vec<f32> {
        let f = self.effective_format(anchor, focus);
        let flag = |b: bool| f32::from(u8::from(b));
        let look = self
            .doc
            .paragraph_format_at(&self.outline, anchor.min(focus))
            .style
            .look();
        vec![
            flag(f.bold),
            flag(f.italic || look.italic),
            f.size.or(look.size).unwrap_or(0.0),
            flag(f.underline),
            flag(f.strike),
            match f.script {
                Script::Normal => 0.0,
                Script::Super => 1.0,
                Script::Sub => 2.0,
            },
        ]
    }

    /// Text colour (`#rrggbb`) at the caret or the start of the selection; empty for default.
    pub fn char_color(&self, anchor: usize, focus: usize) -> String {
        self.effective_format(anchor, focus)
            .color
            .unwrap_or_default()
    }

    /// Highlight colour (`#rrggbb`) at the caret or the start of the selection; empty for none.
    pub fn char_highlight(&self, anchor: usize, focus: usize) -> String {
        self.effective_format(anchor, focus)
            .highlight
            .unwrap_or_default()
    }

    /// Font family at the caret or the start of the selection; empty for the default.
    pub fn char_font(&self, anchor: usize, focus: usize) -> String {
        self.effective_format(anchor, focus)
            .font
            .unwrap_or_default()
    }

    /// Alignment of the paragraphs touched by the selection: 0 left, 1 centre, 2 right,
    /// 3 justify.
    pub fn set_align(&mut self, anchor: usize, focus: usize, align: u32) -> Result<(), JsError> {
        let a = match align {
            1 => Align::Center,
            2 => Align::Right,
            3 => Align::Justify,
            _ => Align::Left,
        };
        self.paragraph(anchor, focus, ParagraphChange::Align(a))
    }

    /// Line spacing as a multiple of single spacing (1, 1.15, 1.5, 2 …).
    pub fn set_line_spacing(&mut self, anchor: usize, focus: usize, v: f32) -> Result<(), JsError> {
        self.paragraph(anchor, focus, ParagraphChange::LineSpacing(v.max(0.5)))
    }

    /// Space before the paragraphs, in points.
    pub fn set_space_before(
        &mut self,
        anchor: usize,
        focus: usize,
        pt: f32,
    ) -> Result<(), JsError> {
        self.paragraph(anchor, focus, ParagraphChange::SpaceBefore(pt.max(0.0)))
    }

    /// Space after the paragraphs, in points.
    pub fn set_space_after(&mut self, anchor: usize, focus: usize, pt: f32) -> Result<(), JsError> {
        self.paragraph(anchor, focus, ParagraphChange::SpaceAfter(pt.max(0.0)))
    }

    /// `[align, line spacing, space before, space after, indent start, indent end, first line
    /// indent, list, list level, checked]` of the paragraph at `pos`; align as in
    /// [`Editor::set_align`], list as in [`Editor::set_list`]. Indents are the paragraph's
    /// own, without what its list level adds.
    pub fn paragraph_format(&self, pos: usize) -> Vec<f32> {
        let f = self.doc.paragraph_format_at(&self.outline, pos);
        let align = match f.align {
            Align::Left => 0.0,
            Align::Center => 1.0,
            Align::Right => 2.0,
            Align::Justify => 3.0,
        };
        let l = f.list;
        vec![
            align,
            f.line_spacing,
            f.space_before,
            f.space_after,
            f.indent_start,
            f.indent_end,
            f.indent_first,
            list_code(l.map(|l| l.kind)) as f32,
            l.map_or(0.0, |l| f32::from(l.level)),
            f32::from(u8::from(l.is_some_and(|l| l.checked))),
            style_code(f.style) as f32,
            f32::from(u8::from(f.page_break_before)),
        ]
    }

    /// Ctrl+Enter: splits the paragraph at `pos` and starts the rest on a new page. Returns
    /// the caret position after the break.
    pub fn insert_page_break(&mut self, pos: usize) -> Result<usize, JsError> {
        self.begin(pos, pos, false)?;
        self.last_edit_end = None;
        Ok(self.doc.insert_page_break_at(&self.outline, pos)?)
    }

    /// Adds or removes the page break before the paragraph at `pos` (Backspace at its start
    /// removes it).
    pub fn set_page_break(&mut self, pos: usize, on: bool) -> Result<(), JsError> {
        self.paragraph(pos, pos, ParagraphChange::PageBreakBefore(on))
    }

    /// `[width, height, top, right, bottom, left]`: the page size and margins in points.
    pub fn page_setup(&self) -> Vec<f32> {
        let p = self.doc.page_setup();
        vec![
            p.width,
            p.height,
            p.margin_top,
            p.margin_right,
            p.margin_bottom,
            p.margin_left,
        ]
    }

    /// Sets the page size and margins in points for everyone editing the document. Fails if
    /// the margins leave less than half an inch for text.
    pub fn set_page_setup(
        &mut self,
        width: f32,
        height: f32,
        top: f32,
        right: f32,
        bottom: f32,
        left: f32,
    ) -> Result<(), JsError> {
        self.begin(0, 0, false)?;
        self.last_edit_end = None;
        Ok(self.doc.set_page_setup(PageSetup {
            width,
            height,
            margin_top: top,
            margin_right: right,
            margin_bottom: bottom,
            margin_left: left,
        })?)
    }

    /// Gives the paragraphs touched by the selection a style: 0 normal text, 1 title,
    /// 2 subtitle, 3–8 heading 1–6.
    pub fn set_paragraph_style(
        &mut self,
        anchor: usize,
        focus: usize,
        style: u32,
    ) -> Result<(), JsError> {
        self.paragraph(anchor, focus, ParagraphChange::Style(style_from(style)))
    }

    /// The document's titles and headings per the last layout, flattened as `[flat start,
    /// level]` pairs in reading order; level 0 is a title, 1–6 headings. For an outline.
    pub fn headings(&self) -> Vec<u32> {
        self.outline
            .slots
            .iter()
            .zip(&self.formats)
            .filter_map(|(slot, f)| {
                Some([
                    slot.flat as u32,
                    u32::from(f.paragraph.style.outline_level()?),
                ])
            })
            .flatten()
            .collect()
    }

    /// Makes the paragraphs touched by the selection list items: 0 none (plain paragraphs),
    /// 1 bullets, 2 numbers, 3 checklist.
    pub fn set_list(&mut self, anchor: usize, focus: usize, kind: u32) -> Result<(), JsError> {
        self.begin(anchor, focus, false)?;
        self.last_edit_end = None;
        Ok(self
            .doc
            .set_list_at(&self.outline, anchor, focus, list_from(kind))?)
    }

    /// Indents (`deeper`) or outdents the paragraphs touched by the selection: list items by
    /// a level (outdenting the first level ends the list), other paragraphs by 0.5 in.
    pub fn indent(&mut self, anchor: usize, focus: usize, deeper: bool) -> Result<(), JsError> {
        self.begin(anchor, focus, false)?;
        self.last_edit_end = None;
        Ok(self.doc.indent_at(&self.outline, anchor, focus, deeper)?)
    }

    /// Ticks or clears the checklist item at `pos`.
    pub fn set_checked(&mut self, pos: usize, checked: bool) -> Result<(), JsError> {
        self.paragraph(pos, pos, ParagraphChange::Checked(checked))
    }

    /// Indents of the paragraphs touched by the selection, in points: from the left margin,
    /// from the right margin, and of the first line relative to the others.
    pub fn set_indents(
        &mut self,
        anchor: usize,
        focus: usize,
        start: f32,
        end: f32,
        first: f32,
    ) -> Result<(), JsError> {
        self.begin(anchor, focus, false)?;
        self.last_edit_end = None;
        for change in [
            ParagraphChange::IndentStart(start.max(0.0)),
            ParagraphChange::IndentEnd(end.max(0.0)),
            ParagraphChange::IndentFirst(first),
        ] {
            self.doc
                .set_paragraph_format_at(&self.outline, anchor, focus, change)?;
        }
        Ok(())
    }

    /// `[start, end]`: flat positions of the start and end of the paragraph holding `pos`.
    pub fn paragraph_range(&self, pos: usize) -> Vec<u32> {
        let o = &self.outline;
        o.slot_at(o.snap(pos)).map_or_else(
            || vec![pos as u32, pos as u32],
            |(i, _)| vec![o.slots[i].flat as u32, o.slots[i].flat_end() as u32],
        )
    }

    /// The flat position of the start of the list item whose marker is at `(x, y)` points on
    /// `page`, or `undefined` (a click on a checkbox ticks it).
    pub fn marker_at(&self, page: usize, x: f32, y: f32) -> Option<usize> {
        self.paginator
            .placed_lines()
            .iter()
            .filter(|l| l.page == page && l.line == 0 && l.top <= y && y <= l.bottom)
            .find_map(|l| {
                let m = self.paginator.paragraph(l.paragraph)?.marker.as_ref()?;
                let (x0, x1) = (l.left + m.x, l.left + m.x + m.width);
                // A little slack: markers are small targets.
                (x0 - 2.0 <= x && x <= x1 + 2.0).then(|| self.outline.slots.get(l.paragraph))?
            })
            .map(|s| s.flat)
    }

    // --- undo ----------------------------------------------------------------------------

    /// Undoes this user's last change (never other people's). `anchor`/`focus` are the
    /// current selection. Returns the selection to restore as `[anchor, focus]`, or
    /// `undefined` if there was nothing to undo. Call [`Editor::layout`] after.
    pub fn undo(&mut self, anchor: usize, focus: usize) -> Result<Option<Vec<u32>>, JsError> {
        self.history_step(anchor, focus, true)
    }

    /// Redoes the last undone change; like [`Editor::undo`].
    pub fn redo(&mut self, anchor: usize, focus: usize) -> Result<Option<Vec<u32>>, JsError> {
        self.history_step(anchor, focus, false)
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// The whole document (with its history of edits, so it merges with other copies) as
    /// bytes, for saving on this device. See `load_state`.
    pub fn save_state(&self) -> Result<Vec<u8>, JsError> {
        Ok(self.doc.export_snapshot()?)
    }

    /// Merges a saved copy (from `save_state`) into the document: load it before connecting,
    /// and the sync handshake sends the server whatever it lacks (edits made offline).
    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        Ok(self.doc.import(bytes)?)
    }

    /// Comments the selection; returns the new comment's id. `created` is milliseconds since
    /// the Unix epoch (`Date.now()`).
    pub fn add_comment(
        &mut self,
        anchor: usize,
        focus: usize,
        author: &str,
        text: &str,
        created: f64,
    ) -> Result<String, JsError> {
        self.begin(anchor, focus, false)?;
        self.last_edit_end = None;
        Ok(self
            .doc
            .add_comment(&self.outline, anchor, focus, author, text, created)?)
    }

    pub fn reply_comment(
        &mut self,
        id: &str,
        author: &str,
        text: &str,
        created: f64,
    ) -> Result<(), JsError> {
        self.comment_edit()?;
        Ok(self.doc.reply_to_comment(id, author, text, created)?)
    }

    pub fn resolve_comment(&mut self, id: &str, resolved: bool) -> Result<(), JsError> {
        self.comment_edit()?;
        Ok(self.doc.resolve_comment(id, resolved)?)
    }

    pub fn edit_comment(&mut self, id: &str, text: &str) -> Result<(), JsError> {
        self.comment_edit()?;
        Ok(self.doc.edit_comment(id, text)?)
    }

    pub fn delete_comment(&mut self, id: &str) -> Result<(), JsError> {
        self.comment_edit()?;
        Ok(self.doc.delete_comment(id)?)
    }

    /// Every comment as of the last layout, as a JSON array of `{id, start, end, author,
    /// text, created, resolved, replies: [{author, text, created}]}` in reading order (`start`
    /// and `end` are flat positions).
    pub fn comments_json(&self) -> String {
        serde_json::to_string(&self.comments).unwrap_or_else(|_| "[]".into())
    }

    /// The id of the open (unresolved) comment covering `pos`, the most recent if several.
    pub fn comment_at(&self, pos: usize) -> Option<String> {
        self.comments
            .iter()
            .filter(|c| !c.resolved && c.start <= pos && pos < c.end.max(c.start + 1))
            .max_by(|a, b| a.created.total_cmp(&b.created))
            .map(|c| c.id.clone())
    }

    /// Shows comment `id` as selected (its text highlighted more strongly), or none with an
    /// empty id. Call [`Editor::layout`] after.
    pub fn set_active_comment(&mut self, id: &str) {
        self.active_comment = (!id.is_empty()).then(|| id.to_owned());
    }

    /// Refuses (or allows again) every local edit: commands then fail with an error. The
    /// sync connection sets it when the server allows viewing only.
    pub fn set_read_only(&mut self, on: bool) {
        self.read_only = on;
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Forgets undo history, e.g. after building the initial document.
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.last_edit_end = None;
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// One grapheme left, crossing into the previous paragraph or cell.
    pub fn move_left(&self, pos: usize) -> usize {
        self.outline.move_left(pos)
    }

    /// Ctrl+Right / Ctrl+Left: to the next word's start, or the start of the word before.
    pub fn move_word(&self, pos: usize, forward: bool) -> usize {
        if forward {
            self.outline.word_right(pos)
        } else {
            self.outline.word_left(pos)
        }
    }

    /// `[start, end]` of the word (or run of spaces or punctuation) at `pos`: what a
    /// double-click selects.
    pub fn word_range(&self, pos: usize) -> Vec<u32> {
        let r = self.outline.word_at(pos);
        vec![r.start as u32, r.end as u32]
    }

    /// Ctrl+Backspace / Ctrl+Delete: deletes to the word boundary before or after `pos`
    /// (joining paragraphs at their edges, like Backspace and Delete). Returns the caret.
    pub fn delete_word(&mut self, pos: usize, forward: bool) -> Result<usize, JsError> {
        let to = self.move_word(pos, forward);
        let (lo, hi) = (pos.min(to), pos.max(to));
        let same = self.outline.slot_at(lo).map(|s| s.0) == self.outline.slot_at(hi).map(|s| s.0);
        if lo == hi || !same {
            return if forward {
                self.delete_forward(pos)
            } else {
                self.delete_backward(pos)
            };
        }
        self.begin(pos, pos, false)?;
        self.last_edit_end = None;
        Ok(self.doc.delete_range_at(&self.outline, lo, hi)?)
    }

    /// One grapheme right, crossing into the next paragraph or cell.
    pub fn move_right(&self, pos: usize) -> usize {
        self.outline.move_right(pos)
    }

    // --- tables --------------------------------------------------------------------------

    /// Inserts a table at the caret (after the current table if the caret is in one) and
    /// returns the position of its first cell.
    pub fn insert_table(&mut self, pos: usize, rows: usize, cols: usize) -> Result<usize, JsError> {
        self.begin(pos, pos, false)?;
        self.last_edit_end = None;
        let body_pos = match self.outline.table_at(pos) {
            Some(t) => t.placeholder + 1,
            None => self.outline.locate(pos).offset,
        };
        let id = self.doc.insert_table(body_pos, rows, cols)?;
        let fresh = self.doc.outline();
        Ok(fresh.cell_start(&id, 0, 0).unwrap_or(pos))
    }

    /// `[row, col, rows, cols]` of the cell holding `pos`, or `undefined` outside tables.
    pub fn table_info(&self, pos: usize) -> Option<Vec<u32>> {
        let t = self.outline.table_at(pos)?;
        let c = self.outline.cell_at(pos)?;
        Some(vec![
            c.row as u32,
            c.col as u32,
            t.rows() as u32,
            t.columns as u32,
        ])
    }

    /// Start of the next cell (Tab), or `undefined` in the last cell or outside a table.
    pub fn next_cell(&self, pos: usize) -> Option<usize> {
        self.outline.next_cell(pos)
    }

    /// Start of the previous cell (Shift+Tab).
    pub fn prev_cell(&self, pos: usize) -> Option<usize> {
        self.outline.prev_cell(pos)
    }

    /// Inserts a row above or below the caret's row; returns the caret in the new row.
    pub fn table_insert_row(&mut self, pos: usize, below: bool) -> Result<usize, JsError> {
        self.begin(pos, pos, false)?;
        self.last_edit_end = None;
        let c = self.cell(pos)?;
        let row = c.row + usize::from(below);
        self.doc.insert_row(&c.table, row)?;
        Ok(self
            .doc
            .outline()
            .cell_start(&c.table, row, c.col)
            .unwrap_or(pos))
    }

    /// Inserts a column left or right of the caret's column; returns the caret in it.
    pub fn table_insert_column(&mut self, pos: usize, right: bool) -> Result<usize, JsError> {
        self.begin(pos, pos, false)?;
        self.last_edit_end = None;
        let c = self.cell(pos)?;
        let col = c.col + usize::from(right);
        self.doc.insert_column(&c.table, col)?;
        Ok(self
            .doc
            .outline()
            .cell_start(&c.table, c.row, col)
            .unwrap_or(pos))
    }

    /// Deletes the caret's row (the whole table if it's the only row).
    pub fn table_delete_row(&mut self, pos: usize) -> Result<usize, JsError> {
        self.begin(pos, pos, false)?;
        self.last_edit_end = None;
        let c = self.cell(pos)?;
        let rows = self.outline.table(&c.table).map_or(0, |t| t.rows());
        if rows <= 1 {
            return self.table_delete(pos);
        }
        self.doc.delete_row(&c.table, c.row)?;
        let row = c.row.min(rows - 2);
        Ok(self
            .doc
            .outline()
            .cell_start(&c.table, row, c.col)
            .unwrap_or(pos))
    }

    /// Deletes the caret's column (the whole table if it's the only column).
    pub fn table_delete_column(&mut self, pos: usize) -> Result<usize, JsError> {
        self.begin(pos, pos, false)?;
        self.last_edit_end = None;
        let c = self.cell(pos)?;
        let cols = self.outline.table(&c.table).map_or(0, |t| t.columns);
        if cols <= 1 {
            return self.table_delete(pos);
        }
        self.doc.delete_column(&c.table, c.col)?;
        let col = c.col.min(cols - 2);
        Ok(self
            .doc
            .outline()
            .cell_start(&c.table, c.row, col)
            .unwrap_or(pos))
    }

    /// Deletes the table holding the caret; returns the caret where the table was.
    pub fn table_delete(&mut self, pos: usize) -> Result<usize, JsError> {
        self.begin(pos, pos, false)?;
        self.last_edit_end = None;
        let t = self
            .outline
            .table_at(pos)
            .ok_or_else(|| JsError::new("not in a table"))?;
        let (id, placeholder) = (t.id.clone(), t.placeholder);
        self.doc.delete_table(&id)?;
        let fresh = self.doc.outline();
        Ok(fresh.snap(fresh.flat_of_body(placeholder)))
    }

    // --- images ---------------------------------------------------------------------------

    /// Inserts an image in line at `pos` (replacing nothing) and returns its id. `data` is a
    /// PNG or JPEG file (`mime`), `px_width` × `px_height` pixels, shown at `width` × `height`
    /// points.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_image(
        &mut self,
        pos: usize,
        data: Vec<u8>,
        mime: &str,
        px_width: u32,
        px_height: u32,
        width: f32,
        height: f32,
    ) -> Result<String, JsError> {
        self.begin(pos, pos, false)?;
        self.last_edit_end = None;
        let image = ImageData {
            bytes: data,
            mime: mime.to_owned(),
            px_width,
            px_height,
        };
        Ok(self
            .doc
            .insert_image(&self.outline, pos, &image, width, height)?)
    }

    /// `[width, height, rotation, wrap, x, y, recolor, brightness, contrast, transparency,
    /// px width, px height, in table]` of an image, or `undefined` if it isn't in the document.
    /// Lengths in points; wrap as in [`Editor::set_image_wrap`], recolor as in
    /// [`Editor::set_image_recolor`]; `in table` is 1 for an image in a table cell, which is
    /// always laid out in line.
    pub fn image_props(&self, id: &str) -> Option<Vec<f32>> {
        let p = self.doc.image_props(id)?;
        let pos = self.doc.image_position(&self.outline, id)?;
        let data = self.doc.image_data(id);
        let in_table = matches!(self.outline.locate(pos).container, TextRef::Cell(_));
        Some(vec![
            p.width,
            p.height,
            p.rotation,
            wrap_code(p.wrap) as f32,
            p.x,
            p.y,
            recolor_code(p.recolor) as f32,
            p.brightness,
            p.contrast,
            p.transparency,
            data.as_ref().map_or(0.0, |d| d.px_width as f32),
            data.as_ref().map_or(0.0, |d| d.px_height as f32),
            f32::from(u8::from(in_table)),
        ])
    }

    /// An image's description for people who can't see it.
    pub fn image_alt(&self, id: &str) -> String {
        self.doc.image_props(id).map(|p| p.alt).unwrap_or_default()
    }

    /// An image's file bytes (they never change for an id, so callers may cache by id).
    pub fn image_data(&self, id: &str) -> Vec<u8> {
        self.doc.image_data(id).map(|d| d.bytes).unwrap_or_default()
    }

    /// An image's file type: `image/png` or `image/jpeg`.
    pub fn image_mime(&self, id: &str) -> String {
        self.doc.image_data(id).map(|d| d.mime).unwrap_or_default()
    }

    /// Flat position of an image's placeholder, or `undefined` if it isn't in the document.
    pub fn image_position(&self, id: &str) -> Option<usize> {
        self.doc.image_position(&self.outline, id)
    }

    /// Size in points, before rotation.
    pub fn set_image_size(&mut self, id: &str, width: f32, height: f32) -> Result<(), JsError> {
        self.image_edit(id, vec![ImageChange::Size(width, height)])
    }

    /// Degrees clockwise.
    pub fn set_image_rotation(&mut self, id: &str, degrees: f32) -> Result<(), JsError> {
        self.image_edit(id, vec![ImageChange::Rotation(degrees)])
    }

    /// 0 in line, 1 wrap text, 2 break text, 3 behind text, 4 in front of text.
    ///
    /// Leaving in-line places the image where it was drawn, so it doesn't jump.
    pub fn set_image_wrap(&mut self, id: &str, wrap: u32) -> Result<(), JsError> {
        let wrap = wrap_from(wrap);
        let mut changes = vec![ImageChange::Wrap(wrap)];
        let props = self
            .doc
            .image_props(id)
            .ok_or_else(|| JsError::new("no such image"))?;
        if props.wrap == Wrap::Inline && wrap != Wrap::Inline {
            if let Some((x, y)) = self.offset_where_drawn(id) {
                changes.push(ImageChange::Offset(x, y));
            }
        }
        self.image_edit(id, changes)
    }

    /// Offset in points of a positioned image's bounding box from the left margin and from
    /// the top of its paragraph.
    pub fn set_image_offset(&mut self, id: &str, x: f32, y: f32) -> Result<(), JsError> {
        self.image_edit(id, vec![ImageChange::Offset(x, y)])
    }

    /// Size, rotation and offset together, as one undo step (a resize from a corner, or a
    /// turn, that also moves the image's bounding box).
    pub fn set_image_geometry(
        &mut self,
        id: &str,
        width: f32,
        height: f32,
        rotation: f32,
        x: f32,
        y: f32,
    ) -> Result<(), JsError> {
        self.image_edit(
            id,
            vec![
                ImageChange::Size(width, height),
                ImageChange::Rotation(rotation),
                ImageChange::Offset(x, y),
            ],
        )
    }

    /// 0 none, 1 grayscale, 2 sepia, 3 negative.
    pub fn set_image_recolor(&mut self, id: &str, recolor: u32) -> Result<(), JsError> {
        let r = match recolor {
            1 => Recolor::Grayscale,
            2 => Recolor::Sepia,
            3 => Recolor::Negative,
            _ => Recolor::None,
        };
        self.image_edit(id, vec![ImageChange::Recolor(r)])
    }

    /// Brightness and contrast (`-100..=100`) and transparency (`0..=100`).
    pub fn set_image_adjustments(
        &mut self,
        id: &str,
        brightness: f32,
        contrast: f32,
        transparency: f32,
    ) -> Result<(), JsError> {
        self.image_edit(
            id,
            vec![
                ImageChange::Brightness(brightness),
                ImageChange::Contrast(contrast),
                ImageChange::Transparency(transparency),
            ],
        )
    }

    pub fn set_image_alt(&mut self, id: &str, alt: &str) -> Result<(), JsError> {
        self.image_edit(id, vec![ImageChange::Alt(alt.to_owned())])
    }

    /// Moves an image dragged to `(x, y)` points on `page`. An in-line image goes to the caret
    /// position nearest that point. A positioned image puts its bounding box's top-left corner
    /// there, anchored to the paragraph it lands beside; one that breaks text moves to the
    /// start of the line there.
    pub fn place_image(&mut self, id: &str, page: usize, x: f32, y: f32) -> Result<(), JsError> {
        let props = self
            .doc
            .image_props(id)
            .ok_or_else(|| JsError::new("no such image"))?;
        let from = self
            .doc
            .image_position(&self.outline, id)
            .ok_or_else(|| JsError::new("no such image"))?;
        let g = *self.paginator.geometry();
        let in_table = matches!(self.outline.locate(from).container, TextRef::Cell(_));
        let (to, changes) = match props.wrap {
            _ if in_table => (self.hit_flat(page, x, y), vec![]),
            Wrap::Inline => (self.hit_flat(page, x, y), vec![]),
            Wrap::Break => {
                let at = self.hit_flat(page, g.margin_left + 1.0, y);
                let start = self
                    .position(at, false)
                    .and_then(|tp| self.paginator.line_start(tp))
                    .map_or(at, |tp| self.caret_out(tp)[0] as usize);
                (start, vec![ImageChange::Offset(x - g.margin_left, props.y)])
            }
            Wrap::Wrap | Wrap::Behind | Wrap::Front => {
                let (slot, top) = self.anchor_paragraph(page, y);
                let to = if self.outline.slot_at(from).map(|(s, _)| s) == Some(slot) {
                    from
                } else {
                    self.outline.slots[slot].flat
                };
                (to, vec![ImageChange::Offset(x - g.margin_left, y - top)])
            }
        };
        self.begin(from, from, false)?;
        self.last_edit_end = None;
        Ok(self.doc.move_image(&self.outline, id, to, changes)?)
    }

    /// Removes an image; returns where the caret goes.
    pub fn delete_image(&mut self, id: &str) -> Result<usize, JsError> {
        let at = self
            .doc
            .image_position(&self.outline, id)
            .ok_or_else(|| JsError::new("no such image"))?;
        self.begin(at, at, false)?;
        self.last_edit_end = None;
        self.doc.delete_image(&self.outline, id)?;
        Ok(at)
    }

    /// Images on page `index` from the last layout, in drawing order (behind the text, among
    /// it, in front of it), flattened in groups of [`OBJECT_STRIDE`]: `key, layer, x, y,
    /// width, height, drawn width, drawn height, rotation`. `x, y, width, height` is the
    /// bounding box in points from the page's top-left corner; draw the image `drawn width` ×
    /// `drawn height`, rotated by `rotation` degrees about the box's centre. Layer: 0 behind,
    /// 1 text, 2 front. [`Editor::object_id`] turns a key into the image id.
    pub fn page_objects(&self, index: usize) -> Vec<f32> {
        let Some(page) = self.paginator.pages().get(index) else {
            return Vec::new();
        };
        let mut objects: Vec<_> = page.objects.iter().collect();
        objects.sort_by_key(|o| layer_code(o.layer));
        let mut out = Vec::with_capacity(objects.len() * OBJECT_STRIDE);
        for o in objects {
            out.extend_from_slice(&[
                o.key as f32,
                layer_code(o.layer) as f32,
                o.bounds.x,
                o.bounds.y,
                o.bounds.width,
                o.bounds.height,
                o.width,
                o.height,
                o.rotation,
            ]);
        }
        out
    }

    /// The image id for a key from [`Editor::page_objects`].
    pub fn object_id(&self, key: usize) -> Option<String> {
        self.object_ids.get(key).cloned()
    }

    /// Whether `(x, y)` points on `page` is over a line's text (not just its empty end), so a
    /// click there means the text rather than an image behind it.
    pub fn is_over_text(&self, page: usize, x: f32, y: f32) -> bool {
        self.paginator
            .placed_lines()
            .iter()
            .filter(|l| l.page == page && l.top <= y && y <= l.bottom)
            .any(|l| {
                let Some(line) = self
                    .paginator
                    .paragraph(l.paragraph)
                    .and_then(|p| p.lines.get(l.line))
                else {
                    return false;
                };
                let xs = line.carets.iter().map(|c| l.left + c.x);
                let (lo, hi) = xs.fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| {
                    (a.min(v), b.max(v))
                });
                lo <= x && x <= hi
            })
    }

    // --- export ---------------------------------------------------------------------------

    /// The document as a PDF, page for page as the editor lays it out, with the fonts
    /// embedded. `title` goes in the PDF's metadata (empty for none).
    pub fn export_pdf(&mut self, title: &str) -> Result<Vec<u8>, JsError> {
        self.layout();
        let texts: Vec<&str> = self.outline.slots.iter().map(|s| s.text.as_str()).collect();
        let files: Vec<Option<(ImageData, ImageProps)>> = self
            .object_ids
            .iter()
            .map(|id| Some((self.doc.image_data(id)?, self.doc.image_props(id)?)))
            .collect();
        let images: Vec<Option<ExportImage>> = files
            .iter()
            .map(|f| {
                f.as_ref().map(|(d, p)| ExportImage {
                    data: &d.bytes,
                    mime: &d.mime,
                    recolor: p.recolor,
                    brightness: p.brightness,
                    contrast: p.contrast,
                    transparency: p.transparency,
                    alt: &p.alt,
                })
            })
            .collect();
        // Links by paragraph and byte range, from the formats of the layout just done.
        let mut links: Vec<PdfLink> = Vec::new();
        for (i, (slot, f)) in self.outline.slots.iter().zip(&self.formats).enumerate() {
            let byte = |c: usize| {
                slot.text
                    .char_indices()
                    .nth(c)
                    .map_or(slot.text.len(), |(b, _)| b)
            };
            for r in &f.runs {
                if let Some(url) = &r.format.link {
                    links.push(PdfLink {
                        paragraph: i,
                        bytes: byte(r.range.start)..byte(r.range.end),
                        url,
                    });
                }
            }
        }
        let headings: Vec<PdfHeading> = self
            .outline
            .slots
            .iter()
            .zip(&self.formats)
            .enumerate()
            .filter_map(|(i, (slot, f))| {
                Some(PdfHeading {
                    paragraph: i,
                    level: f.paragraph.style.outline_level()?,
                    title: &slot.text,
                })
            })
            .collect();
        let info = PdfInfo {
            title: (!title.is_empty()).then_some(title),
            images: &images,
            links: &links,
            headings: &headings,
        };
        Ok(opendox_export::pdf(
            &self.paginator,
            self.engine.fonts(),
            &texts,
            &info,
        )?)
    }

    /// The document as a Word (.docx) file: text, formatting, tables and page setup. Word
    /// lays it out itself, so line breaks may differ; fonts are referenced by name.
    pub fn export_docx(&self, title: &str) -> Result<Vec<u8>, JsError> {
        let options = DocxOptions {
            default_font: &self.style.font_family,
            default_size: self.style.font_size,
            page: *self.paginator.geometry(),
            title: (!title.is_empty()).then_some(title),
        };
        Ok(opendox_export::docx(&self.doc, &options)?)
    }

    // --- collaboration --------------------------------------------------------------------

    /// Starts a sync session on a new connection (also after a reconnect). Send the returned
    /// bytes first. Editing works the whole time; edits made offline go out once synced.
    pub fn sync_connect(&mut self) -> Vec<u8> {
        self.sync_reply = None;
        self.sync.connect(&self.doc)
    }

    /// The connection dropped: stop sending and forget remote carets.
    pub fn sync_disconnected(&mut self) {
        self.sync_reply = None;
        self.sync.disconnected();
    }

    /// Whether the handshake has finished and edits are flowing.
    pub fn sync_is_synced(&self) -> bool {
        self.sync.is_synced()
    }

    /// The next message to send: a pending reply, or every local edit not yet sent, batched
    /// into one message. `undefined` when there's nothing to send. Call after
    /// [`Editor::sync_receive`] and on a short timer after local edits.
    pub fn sync_flush(&mut self) -> Result<Option<Vec<u8>>, JsError> {
        if let Some(reply) = self.sync_reply.take() {
            return Ok(Some(reply));
        }
        Ok(self.sync.flush(&self.doc)?)
    }

    /// Handles a message from the server. `anchor` and `focus` are the local selection (per
    /// the last layout); returns `[flags, anchor, focus]` with the selection moved to follow
    /// remote edits. `flags` combines [`SYNC_DOC_CHANGED`] and [`SYNC_PEERS_CHANGED`]. Call
    /// [`Editor::layout`] after a document change.
    pub fn sync_receive(
        &mut self,
        message: &[u8],
        anchor: usize,
        focus: usize,
    ) -> Result<Vec<u32>, JsError> {
        // Pin the selection to the text around it before remote edits shift positions.
        let pinned = [anchor, focus].map(|p| self.doc.anchor_at(&self.outline, p).ok());
        let r = self.sync.receive(&self.doc, message)?;
        if r.reply.is_some() {
            self.sync_reply = r.reply;
        }
        let mut out = vec![0, anchor as u32, focus as u32];
        if r.doc_changed {
            out[0] |= SYNC_DOC_CHANGED;
            let fresh = self.doc.outline();
            for (i, (pin, old)) in pinned.iter().zip([anchor, focus]).enumerate() {
                let pos = pin
                    .as_ref()
                    .and_then(|a| self.doc.resolve_at(&fresh, a))
                    // Its cell or table was deleted: stay roughly where it was.
                    .unwrap_or_else(|| fresh.snap(old.min(fresh.len)));
                out[i + 1] = pos as u32;
            }
        }
        if r.peers_changed {
            out[0] |= SYNC_PEERS_CHANGED;
        }
        if r.read_only {
            out[0] |= SYNC_READ_ONLY;
            self.read_only = true;
        }
        Ok(out)
    }

    /// This client's presence message: name, CSS colour and selection (positions per the last
    /// layout). With `visible` false the caret is hidden from others (e.g. editor unfocused).
    pub fn sync_presence(
        &self,
        name: &str,
        color: &str,
        anchor: usize,
        focus: usize,
        visible: bool,
    ) -> Result<Vec<u8>, JsError> {
        let selection = if visible {
            let a = self.doc.anchor_at(&self.outline, anchor)?;
            let f = self.doc.anchor_at(&self.outline, focus)?;
            Some((a.encode(), f.encode()))
        } else {
            None
        };
        Ok(SyncClient::presence_message(&Presence {
            name: name.to_owned(),
            color: color.to_owned(),
            selection,
        }))
    }

    /// Connection ids of the other people on this document.
    pub fn peer_ids(&self) -> Vec<u32> {
        self.sync.peers().keys().copied().collect()
    }

    pub fn peer_name(&self, id: u32) -> Option<String> {
        self.sync.peers().get(&id).map(|p| p.name.clone())
    }

    pub fn peer_color(&self, id: u32) -> Option<String> {
        self.sync.peers().get(&id).map(|p| p.color.clone())
    }

    /// A peer's selection as `[anchor, focus]` per the last layout, or `undefined` if it's
    /// hidden or points at text this replica doesn't have (yet).
    pub fn peer_selection(&self, id: u32) -> Option<Vec<u32>> {
        let (a, f) = self.sync.peers().get(&id)?.selection.as_ref()?;
        let resolve = |bytes: &[u8]| {
            let anchor = Anchor::decode(bytes).ok()?;
            self.doc.resolve_at(&self.outline, &anchor)
        };
        Some(vec![resolve(a)? as u32, resolve(f)? as u32])
    }

    // --- layout and geometry -------------------------------------------------------------

    /// Lays out the document and returns the page count. Call after every edit, before any
    /// position or geometry query.
    pub fn layout(&mut self) -> usize {
        // Before anything borrows the outline: comment ranges come from the new structure.
        let fresh = self.doc.outline();
        self.comments = self.doc.comments(&fresh);
        let p = self.doc.page_setup();
        let g = PageGeometry {
            width: p.width,
            height: p.height,
            margin_top: p.margin_top,
            margin_right: p.margin_right,
            margin_bottom: p.margin_bottom,
            margin_left: p.margin_left,
            ..PageGeometry::letter()
        };
        if *self.paginator.geometry() != g {
            self.paginator.set_geometry(g);
        }
        self.outline = fresh;
        let formats = self.doc.formats(&self.outline);
        let o = &self.outline;
        // Ticked checklist items are drawn struck through and grey, all of their text.
        // Each paragraph's base look: its style's size, colour and slant; ticked checklist
        // items grey and struck through, all of their text.
        let styled: Vec<TextStyle> = formats
            .iter()
            .map(|f| {
                let look = f.paragraph.style.look();
                let mut b = TextStyle {
                    font_size: look.size.unwrap_or(self.style.font_size),
                    color: look.color,
                    italic: look.italic,
                    ..self.style.clone()
                };
                if f.paragraph.list.is_some_and(|l| l.checked) {
                    b.color = Some(TICKED_COLOR);
                    b.strikethrough = true;
                }
                b
            })
            .collect();
        let bases: Vec<&TextStyle> = styled.iter().collect();
        let spans: Vec<Vec<StyleSpan>> = o
            .slots
            .iter()
            .zip(&formats)
            .zip(&bases)
            .map(|((slot, f), base)| {
                let marks = comment_marks(&self.comments, slot, self.active_comment.as_deref());
                style_spans(base, &slot.text, &with_comments(&f.runs, &marks))
            })
            .collect();
        self.object_ids.clear();
        let mut objects: Vec<Vec<ObjectInput>> = Vec::with_capacity(o.slots.len());
        for (slot, f) in o.slots.iter().zip(&formats) {
            let mut list = Vec::with_capacity(f.images.len());
            for r in &f.images {
                let Some(p) = self.doc.image_props(&r.id) else {
                    continue;
                };
                let byte = slot
                    .text
                    .char_indices()
                    .nth(r.offset)
                    .map_or(slot.text.len(), |(b, _)| b);
                list.push(object_input(&p, byte, self.object_ids.len()));
                self.object_ids.push(r.id.clone());
            }
            for fr in &f.fields {
                let byte = slot
                    .text
                    .char_indices()
                    .nth(fr.offset)
                    .map_or(slot.text.len(), |(b, _)| b);
                list.push(ObjectInput {
                    byte,
                    // Fields are drawn as text, never looked up as images.
                    key: usize::MAX,
                    width: 0.0,
                    height: 0.0,
                    rotation: 0.0,
                    mode: ObjectMode::Field(match fr.kind {
                        FieldKind::PageNumber => opendox_layout::FieldKind::PageNumber,
                        FieldKind::PageCount => opendox_layout::FieldKind::PageCount,
                    }),
                    x: 0.0,
                    y: 0.0,
                });
            }
            list.sort_by_key(|o| o.byte);
            objects.push(list);
        }
        let markers = list_markers(&formats);
        // A marker looks like the start of its item's text (bold if that's bold), as in
        // Google Docs.
        let marker_styles: Vec<Option<TextStyle>> = spans
            .iter()
            .zip(&markers)
            .zip(&bases)
            .map(|((sp, m), base)| {
                m.as_ref()?;
                let first = sp
                    .iter()
                    .find(|s| s.byte_range.start == 0 && !s.byte_range.is_empty())
                    .map_or(*base, |s| &s.style);
                // Its colour, size and weight; never its underline, highlight or shift.
                let mut s = TextStyle {
                    underline: false,
                    strikethrough: false,
                    highlight: None,
                    baseline_shift: 0.0,
                    ..first.clone()
                };
                if let Some(symbols) = &self.symbol_font {
                    s.font_family = format!("{}, {symbols}", s.font_family);
                }
                Some(s)
            })
            .collect();
        let para = |i: usize| ParagraphInput {
            text: &o.slots[i].text,
            base: bases[i],
            spans: &spans[i],
            style: formats
                .get(i)
                .map_or_else(ParagraphStyle::default, |f| paragraph_style(&f.paragraph)),
            objects: &objects[i],
            marker: markers[i]
                .as_deref()
                .zip(marker_styles[i].as_ref())
                .map(|(text, style)| Marker { text, style }),
        };
        let blocks: Vec<BlockInput> = o
            .blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph(i) => BlockInput::Paragraph(para(*i)),
                Block::Table(t) => BlockInput::Table(TableInput {
                    rows: t
                        .cells
                        .iter()
                        .map(|row| {
                            row.iter()
                                .map(|cell| cell.clone().map(para).collect())
                                .collect()
                        })
                        .collect(),
                }),
            })
            .collect();
        let header: Vec<ParagraphInput> = o.header.clone().map(para).collect();
        let footer: Vec<ParagraphInput> = o.footer.clone().map(para).collect();
        let pages = self
            .paginator
            .layout_document(&mut self.engine, &blocks, &header, &footer)
            .len();
        self.formats = formats;
        pages
    }

    /// The characters that no registered font could draw in the last layout (each once, in
    /// order of appearance): they show as the missing-glyph box. An app can load a font that
    /// covers them (emoji, CJK…) and lay out again.
    pub fn missing_chars(&self) -> String {
        let mut out = String::new();
        for (i, slot) in self.outline.slots.iter().enumerate() {
            let Some(p) = self.paginator.paragraph(i) else {
                continue;
            };
            for run in p.lines.iter().flat_map(|l| &l.runs) {
                for (g, r) in run.glyphs.iter().zip(&run.glyph_text) {
                    if g.id != 0 {
                        continue;
                    }
                    for c in slot.text.get(r.clone()).unwrap_or("").chars() {
                        // Joiners and variation selectors ride along with what they join.
                        if !out.contains(c) && !matches!(c, '\u{200D}' | '\u{FE0E}' | '\u{FE0F}') {
                            out.push(c);
                        }
                    }
                }
            }
        }
        out
    }

    /// Page width in points.
    pub fn page_width(&self) -> f32 {
        self.paginator.geometry().width
    }

    /// Page height in points.
    pub fn page_height(&self) -> f32 {
        self.paginator.geometry().height
    }

    /// Glyphs on page `index` from the last layout, flattened into groups of [`GLYPH_STRIDE`]:
    /// `font id, face index, glyph id, x, y, font size, colour` — positions in points from the
    /// page's top-left corner, `(x, y)` being the glyph's baseline origin; colour `0xRRGGBB`
    /// (exact in an f32) or -1 for the default ink.
    pub fn page_glyphs(&self, index: usize) -> Vec<f32> {
        let glyphs = self.paginator.placed_glyphs(index);
        let mut out = Vec::with_capacity(glyphs.len() * GLYPH_STRIDE);
        for g in glyphs {
            out.extend_from_slice(&[
                g.font.0 as f32,
                g.face_index as f32,
                g.id as f32,
                g.x,
                g.y,
                g.font_size,
                g.color.map_or(-1.0, |c| c as f32),
            ]);
        }
        out
    }

    /// Highlights, underlines and strikethroughs on page `index`, highlights first, in groups
    /// of [`DECORATION_STRIDE`]: `kind, x, y, width, height, colour` — a filled rectangle in
    /// points; kind 0 highlight (draw before the text), 1 underline, 2 strikethrough; colour
    /// as in [`Editor::page_glyphs`].
    pub fn page_decorations(&self, index: usize) -> Vec<f32> {
        let decorations = self.paginator.placed_decorations(index);
        let mut out = Vec::with_capacity(decorations.len() * DECORATION_STRIDE);
        for d in decorations {
            out.extend_from_slice(&[
                match d.kind {
                    DecorationKind::Highlight => 0.0,
                    DecorationKind::Underline => 1.0,
                    DecorationKind::Strikethrough => 2.0,
                },
                d.rect.x,
                d.rect.y,
                d.rect.width,
                d.rect.height,
                d.color.map_or(-1.0, |c| c as f32),
            ]);
        }
        out
    }

    /// Table borders on page `index`, in groups of [`RULE_STRIDE`]: `x0, y0, x1, y1` in points.
    pub fn page_rules(&self, index: usize) -> Vec<f32> {
        let Some(page) = self.paginator.pages().get(index) else {
            return Vec::new();
        };
        page.rules
            .iter()
            .flat_map(|r| [r.x0, r.y0, r.x1, r.y1])
            .collect()
    }

    /// SVG path data for a glyph in font units with y pointing down, or `undefined` if the
    /// font or glyph is unknown. Scale by `font_size / units_per_em`.
    pub fn glyph_path(&self, font: u32, face_index: u32, glyph: u32) -> Option<String> {
        self.engine
            .fonts()
            .glyph_outline(FontId(font), face_index, glyph)
            .map(|o| o.svg_path)
    }

    /// Units per em of a font face, or 0 if unknown.
    pub fn units_per_em(&self, font: u32, face_index: u32) -> u16 {
        // Glyph 0 (.notdef) exists in every font; its outline carries the font's em size.
        self.engine
            .fonts()
            .glyph_outline(FontId(font), face_index, 0)
            .map_or(0, |o| o.units_per_em)
    }

    /// The caret position nearest to `(x, y)` points on `page`, as `[pos, upstream]`.
    pub fn hit_test(&self, page: usize, x: f32, y: f32) -> Option<Vec<u32>> {
        let hit = self.paginator.hit_test(page, x, y)?;
        let pos = self.caret_out(hit);
        // A click in the header or footer: its caret is drawn on this page's copy.
        if self.outline.region(pos[0] as usize) != Region::Body {
            self.paginator.set_margin_page(page);
        }
        Some(pos)
    }

    /// Adds an empty header to every page, or removes the header and its text.
    pub fn set_header(&mut self, on: bool) -> Result<(), JsError> {
        self.begin(0, 0, false)?;
        self.last_edit_end = None;
        Ok(self.doc.set_header(on)?)
    }

    /// Adds an empty footer to every page, or removes the footer and its text.
    pub fn set_footer(&mut self, on: bool) -> Result<(), JsError> {
        self.begin(0, 0, false)?;
        self.last_edit_end = None;
        Ok(self.doc.set_footer(on)?)
    }

    pub fn has_header(&self) -> bool {
        self.doc.has_header()
    }

    pub fn has_footer(&self) -> bool {
        self.doc.has_footer()
    }

    /// Inserts a field at `pos` in the header or footer: 0 page number, 1 page count.
    /// Returns the caret after it.
    pub fn insert_field(&mut self, pos: usize, kind: u32) -> Result<usize, JsError> {
        self.begin(pos, pos, false)?;
        self.last_edit_end = None;
        let k = if kind == 1 {
            FieldKind::PageCount
        } else {
            FieldKind::PageNumber
        };
        Ok(self.doc.insert_field_at(&self.outline, pos, k)?)
    }

    /// Which part of the document `pos` is in: 0 body, 1 header, 2 footer, 3 a footnote.
    pub fn region(&self, pos: usize) -> u32 {
        match self.outline.region(pos) {
            Region::Body => 0,
            Region::Header => 1,
            Region::Footer => 2,
            Region::Footnote => 3,
        }
    }

    /// `[first, last]` flat positions of `pos`'s part of the document (select all, Ctrl+Home
    /// and Ctrl+End stay in it).
    pub fn region_bounds(&self, pos: usize) -> Vec<u32> {
        let (a, b) = self.outline.region_bounds(pos);
        vec![a as u32, b as u32]
    }

    /// Start of the header (1) or footer (2) as of the last layout, or `undefined` if the
    /// document has none.
    pub fn region_start(&self, region: u32) -> Option<usize> {
        let range = match region {
            1 => &self.outline.header,
            2 => &self.outline.footer,
            _ => return Some(0),
        };
        self.outline.slots.get(range.start).map(|s| s.flat)
    }

    /// Where to draw the caret: `[page, x, top, bottom]` in points.
    pub fn caret_rect(&self, pos: usize, upstream: bool) -> Option<Vec<f32>> {
        let r = self.paginator.caret_rect(self.position(pos, upstream)?)?;
        Some(vec![r.page as f32, r.x, r.top, r.bottom])
    }

    /// One line up or down, aiming for `goal_x` points: `[pos, upstream]`, or `undefined` at
    /// the first or last line.
    pub fn move_vertical(
        &self,
        pos: usize,
        upstream: bool,
        goal_x: f32,
        down: bool,
    ) -> Option<Vec<u32>> {
        let to = self
            .paginator
            .move_vertical(self.position(pos, upstream)?, goal_x, down)?;
        Some(self.caret_out(to))
    }

    /// Start of the visual line (Home): `[pos, upstream]`.
    pub fn line_start(&self, pos: usize, upstream: bool) -> Option<Vec<u32>> {
        let to = self.paginator.line_start(self.position(pos, upstream)?)?;
        Some(self.caret_out(to))
    }

    /// End of the visual line (End): `[pos, upstream]`.
    pub fn line_end(&self, pos: usize, upstream: bool) -> Option<Vec<u32>> {
        let to = self.paginator.line_end(self.position(pos, upstream)?)?;
        Some(self.caret_out(to))
    }

    /// Highlight boxes for the selection between two positions, flattened in groups of
    /// [`RECT_STRIDE`]: `page, x, y, width, height` in points. A selection reaching from body
    /// text into a table covers the whole table, matching what deleting it would remove.
    pub fn selection_rects(&self, a: usize, b: usize) -> Vec<f32> {
        let (a, b) = self.visible_range(a, b);
        let (Some(pa), Some(pb)) = (self.position(a, false), self.position(b, false)) else {
            return Vec::new();
        };
        let rects = self.paginator.selection_rects(pa, pb);
        let mut out = Vec::with_capacity(rects.len() * RECT_STRIDE);
        for r in rects {
            out.extend_from_slice(&[r.page as f32, r.x, r.y, r.width, r.height]);
        }
        out
    }
}

impl Editor {
    /// Before a comment change: refused when read-only; its own undo step.
    fn comment_edit(&mut self) -> Result<(), JsError> {
        self.begin(0, 0, false)?;
        self.last_edit_end = None;
        Ok(())
    }

    /// Flat bounds of the link at the caret: the run holding it, joined with neighbouring
    /// runs linking to the same URL.
    fn link_bounds(&self, pos: usize) -> Option<[usize; 2]> {
        let url = self.link_at(pos)?;
        let (slot, off) = self.outline.slot_at(self.outline.snap(pos))?;
        let runs = &self.formats.get(slot)?.runs;
        let same = |i: usize| runs[i].format.link.as_deref() == Some(url.as_str());
        let hit = runs
            .iter()
            .position(|r| r.range.contains(&off) && r.format.link.as_deref() == Some(&url))
            .or_else(|| {
                runs.iter().position(|r| {
                    off > 0
                        && r.range.contains(&(off - 1))
                        && r.format.link.as_deref() == Some(&url)
                })
            })?;
        let (mut a, mut b) = (hit, hit);
        while a > 0 && same(a - 1) && runs[a - 1].range.end == runs[a].range.start {
            a -= 1;
        }
        while b + 1 < runs.len() && same(b + 1) && runs[b].range.end == runs[b + 1].range.start {
            b += 1;
        }
        let flat = self.outline.slots[slot].flat;
        Some([flat + runs[a].range.start, flat + runs[b].range.end])
    }

    /// One undoable change to an image.
    fn image_edit(&mut self, id: &str, changes: Vec<ImageChange>) -> Result<(), JsError> {
        let at = self.doc.image_position(&self.outline, id).unwrap_or(0);
        self.begin(at, at, false)?;
        self.last_edit_end = None;
        Ok(self.doc.set_image_many(id, changes)?)
    }

    /// The flat caret position nearest `(x, y)` on `page`.
    fn hit_flat(&self, page: usize, x: f32, y: f32) -> usize {
        self.paginator
            .hit_test(page, x, y)
            .map_or(0, |tp| self.caret_out(tp)[0] as usize)
    }

    /// The offset that keeps an image where the last layout drew it, once positioned.
    fn offset_where_drawn(&self, id: &str) -> Option<(f32, f32)> {
        let key = self.object_ids.iter().position(|k| k == id)?;
        let (page, placed) = self
            .paginator
            .pages()
            .iter()
            .enumerate()
            .find_map(|(i, p)| p.objects.iter().find(|o| o.key == key).map(|o| (i, *o)))?;
        let b = placed.bounds;
        let (slot, top) = self.anchor_paragraph(page, b.y);
        let own = self
            .doc
            .image_position(&self.outline, id)
            .and_then(|p| self.outline.slot_at(p))
            .map(|(s, _)| s);
        // Measured from its own paragraph, which it stays in.
        let top = if own == Some(slot) {
            top
        } else {
            own.and_then(|s| self.paragraph_top(s))
                .map_or(top, |(_, t)| t)
        };
        Some((b.x - self.paginator.geometry().margin_left, b.y - top))
    }

    /// The body paragraph to anchor an image positioned at height `y` on `page`, and that
    /// paragraph's top: the last paragraph starting on the page at or above `y`, else the first
    /// one starting on the page, else the one at `y`.
    fn anchor_paragraph(&self, page: usize, y: f32) -> (usize, f32) {
        let firsts = self
            .outline
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.container == TextRef::Body)
            .filter_map(|(i, _)| Some((i, self.paragraph_top(i)?)))
            .filter(|(_, (p, _))| *p == page);
        let mut best: Option<(usize, f32)> = None;
        let mut first: Option<(usize, f32)> = None;
        for (i, (_, top)) in firsts {
            first.get_or_insert((i, top));
            if top <= y {
                best = Some((i, top));
            }
        }
        best.or(first).unwrap_or_else(|| {
            let g = self.paginator.geometry();
            let at = self.hit_flat(page, g.margin_left + 1.0, y);
            let slot = self.outline.slot_at(at).map_or(0, |(s, _)| s);
            let top = self.paragraph_top(slot).map_or(g.margin_top, |(_, t)| t);
            (slot, top)
        })
    }

    /// Page and y of a paragraph's first line, per the last layout.
    fn paragraph_top(&self, slot: usize) -> Option<(usize, f32)> {
        let l = self
            .paginator
            .placed_lines()
            .iter()
            .find(|l| l.paragraph == slot && l.line == 0)?;
        // A first line that moved below a wrapped image: its paragraph starts above it.
        let line_top = self
            .paginator
            .paragraph(slot)
            .and_then(|p| p.lines.first())
            .map_or(0.0, |line| line.top);
        Some((l.page, l.top - line_top))
    }

    /// Before every local edit: refuses it if the document is read-only, records the selection
    /// for undo, and lets the edit join the previous undo step only when it's typing that
    /// continues where the last edit ended.
    fn begin(&mut self, anchor: usize, focus: usize, typing: bool) -> Result<(), JsError> {
        if self.read_only {
            return Err(JsError::new("the document is read-only"));
        }
        let merge = typing && anchor == focus && self.last_edit_end == Some(focus);
        self.history
            .before_edit(&self.doc, &self.outline, anchor, focus, merge);
        Ok(())
    }

    fn format(&mut self, anchor: usize, focus: usize, change: FormatChange) -> Result<(), JsError> {
        if anchor == focus {
            let mut f = self.effective_format(anchor, focus);
            f.apply(change);
            // A link needs text to sit on; typing at a caret is never linked by a pending
            // format.
            f.link = None;
            self.pending = Some((anchor, f));
            return Ok(());
        }
        self.begin(anchor, focus, false)?;
        self.pending = None;
        self.last_edit_end = None;
        Ok(self.doc.format_at(&self.outline, anchor, focus, &change)?)
    }

    fn paragraph(
        &mut self,
        anchor: usize,
        focus: usize,
        change: ParagraphChange,
    ) -> Result<(), JsError> {
        self.begin(anchor, focus, false)?;
        self.last_edit_end = None;
        Ok(self
            .doc
            .set_paragraph_format_at(&self.outline, anchor, focus, change)?)
    }

    /// The format typing would produce: a pending one at the caret, else the document's.
    fn effective_format(&self, anchor: usize, focus: usize) -> CharFormat {
        match &self.pending {
            Some((p, f)) if anchor == focus && *p == focus => f.clone(),
            _ => self.doc.char_format_at(&self.outline, anchor, focus),
        }
    }

    fn history_step(
        &mut self,
        anchor: usize,
        focus: usize,
        undo: bool,
    ) -> Result<Option<Vec<u32>>, JsError> {
        self.pending = None;
        self.last_edit_end = None;
        let step = if undo {
            self.history.undo(&self.doc, &self.outline, anchor, focus)?
        } else {
            self.history.redo(&self.doc, &self.outline, anchor, focus)?
        };
        let Some(restored) = step else {
            return Ok(None);
        };
        let fresh = self.doc.outline();
        let at = |a: Option<&Anchor>, fallback: usize| {
            a.and_then(|a| self.doc.resolve_at(&fresh, a))
                .unwrap_or_else(|| fresh.snap(fallback.min(fresh.len))) as u32
        };
        Ok(Some(match &restored {
            Some((a, f)) => vec![at(Some(a), anchor), at(Some(f), focus)],
            None => vec![at(None, anchor), at(None, focus)],
        }))
    }

    fn cell(&self, pos: usize) -> Result<opendox_model::CellRef, JsError> {
        self.outline
            .cell_at(pos)
            .ok_or_else(|| JsError::new("not in a table"))
    }

    /// Flat position → paragraph + byte offset, per the last layout.
    fn position(&self, pos: usize, upstream: bool) -> Option<TextPosition> {
        let (slot, chars) = self.outline.slot_at(self.outline.snap(pos))?;
        let text = &self.outline.slots[slot].text;
        let byte = text
            .char_indices()
            .nth(chars)
            .map_or(text.len(), |(b, _)| b);
        Some(TextPosition {
            paragraph: slot,
            byte,
            upstream,
        })
    }

    /// Paragraph + byte offset → `[flat position, upstream]`.
    fn caret_out(&self, tp: TextPosition) -> Vec<u32> {
        let pos = self.outline.slots.get(tp.paragraph).map_or(0, |s| {
            s.flat + s.text.get(..tp.byte).map_or(0, |t| t.chars().count())
        });
        vec![pos as u32, u32::from(tp.upstream)]
    }

    /// Widens a selection that crosses a table edge to cover the whole table.
    fn visible_range(&self, a: usize, b: usize) -> (usize, usize) {
        let o = &self.outline;
        let (ta, tb) = (o.table_at(a).map(|t| &t.id), o.table_at(b).map(|t| &t.id));
        if ta == tb {
            return (a, b);
        }
        let widen = |p: usize, other: usize| match o.table_at(p) {
            Some(t) if other > p => t.flat_first,
            Some(t) => t.flat_last,
            None => p,
        };
        (widen(a, b), widen(b, a))
    }
}

/// `#rrggbb` (or `rrggbb`) normalised to lower case; anything else is `None`.
fn hex_color(s: &str) -> Option<String> {
    let h = s.trim().trim_start_matches('#');
    (h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("#{}", h.to_ascii_lowercase()))
}

fn color_value(s: &Option<String>) -> Option<u32> {
    s.as_deref()
        .and_then(|c| u32::from_str_radix(c.trim_start_matches('#'), 16).ok())
}

/// Layout spans for a paragraph's formatted runs: char ranges to byte ranges, formats to
/// text styles. A chosen font goes first in the family list, with the default list after it
/// so characters that font lacks (Arabic, say) still fall back.
/// How commented text is highlighted (Google Docs' yellows), and the selected comment's.
const COMMENT_HIGHLIGHT: &str = "#fff2cc";
const ACTIVE_COMMENT_HIGHLIGHT: &str = "#ffd966";

/// The open comments' ranges within a paragraph, as char ranges with their highlight colour.
fn comment_marks(
    comments: &[Comment],
    slot: &opendox_model::Slot,
    active: Option<&str>,
) -> Vec<(std::ops::Range<usize>, &'static str)> {
    let end = slot.flat_end();
    comments
        .iter()
        .filter(|c| !c.resolved)
        .filter_map(|c| {
            let (s, e) = (c.start.max(slot.flat), c.end.min(end));
            let color = if active == Some(c.id.as_str()) {
                ACTIVE_COMMENT_HIGHLIGHT
            } else {
                COMMENT_HIGHLIGHT
            };
            (s < e).then(|| (s - slot.flat..e - slot.flat, color))
        })
        .collect()
}

/// A paragraph's formatted runs with commented stretches highlighted (where the text has no
/// highlight of its own), split where the comments start and end.
fn with_comments(runs: &[FormatRun], marks: &[(std::ops::Range<usize>, &str)]) -> Vec<FormatRun> {
    if marks.is_empty() {
        return runs.to_vec();
    }
    let mut cuts: Vec<usize> = runs
        .iter()
        .flat_map(|r| [r.range.start, r.range.end])
        .chain(marks.iter().flat_map(|(r, _)| [r.start, r.end]))
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut out = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let mut format = runs
            .iter()
            .find(|r| r.range.start <= a && b <= r.range.end)
            .map(|r| r.format.clone())
            .unwrap_or_default();
        // The selected comment's colour wins where comments overlap.
        let color = marks
            .iter()
            .filter(|(r, _)| r.start <= a && b <= r.end)
            .map(|(_, c)| *c)
            .max_by_key(|c| *c == ACTIVE_COMMENT_HIGHLIGHT);
        if let Some(c) = color {
            format.highlight.get_or_insert_with(|| c.to_owned());
        }
        if format != CharFormat::default() {
            out.push(FormatRun {
                range: a..b,
                format,
            });
        }
    }
    out
}

fn style_spans(base: &TextStyle, text: &str, runs: &[FormatRun]) -> Vec<StyleSpan> {
    if runs.is_empty() {
        return Vec::new();
    }
    let bytes: Vec<usize> = text
        .char_indices()
        .map(|(b, _)| b)
        .chain([text.len()])
        .collect();
    runs.iter()
        .filter_map(|r| {
            let start = *bytes.get(r.range.start)?;
            let end = *bytes.get(r.range.end)?;
            let f = &r.format;
            let size = f.size.unwrap_or(base.font_size);
            // Raised or lowered by about a third / a seventh of the text's size, as in Word.
            let (font_size, baseline_shift) = match f.script {
                Script::Normal => (size, 0.0),
                Script::Super => (size * SCRIPT_SCALE, size * 0.33),
                Script::Sub => (size * SCRIPT_SCALE, -size * 0.14),
            };
            let link = f.link.is_some();
            // A ticked checklist item stays grey whatever colour its text has.
            let color = if base.strikethrough && base.color == Some(TICKED_COLOR) {
                base.color
            } else {
                color_value(&f.color)
                    .or(link.then_some(LINK_COLOR))
                    .or(base.color)
            };
            Some(StyleSpan {
                byte_range: start..end,
                style: TextStyle {
                    font_family: f.font.as_ref().map_or_else(
                        || base.font_family.clone(),
                        |name| format!("{name}, {}", base.font_family),
                    ),
                    font_size,
                    font_weight: if f.bold { 700.0 } else { base.font_weight },
                    italic: f.italic || base.italic,
                    color,
                    highlight: color_value(&f.highlight),
                    underline: f.underline || link,
                    strikethrough: f.strike || base.strikethrough,
                    baseline_shift,
                },
            })
        })
        .collect()
}

fn object_input(p: &ImageProps, byte: usize, key: usize) -> ObjectInput {
    ObjectInput {
        byte,
        key,
        width: p.width,
        height: p.height,
        rotation: p.rotation,
        mode: match p.wrap {
            Wrap::Inline => ObjectMode::Inline,
            Wrap::Wrap => ObjectMode::Wrap,
            Wrap::Break => ObjectMode::Break,
            Wrap::Behind => ObjectMode::Behind,
            Wrap::Front => ObjectMode::Front,
        },
        x: p.x,
        y: p.y,
    }
}

fn wrap_code(w: Wrap) -> u32 {
    match w {
        Wrap::Inline => 0,
        Wrap::Wrap => 1,
        Wrap::Break => 2,
        Wrap::Behind => 3,
        Wrap::Front => 4,
    }
}

fn wrap_from(code: u32) -> Wrap {
    match code {
        1 => Wrap::Wrap,
        2 => Wrap::Break,
        3 => Wrap::Behind,
        4 => Wrap::Front,
        _ => Wrap::Inline,
    }
}

fn recolor_code(r: Recolor) -> u32 {
    match r {
        Recolor::None => 0,
        Recolor::Grayscale => 1,
        Recolor::Sepia => 2,
        Recolor::Negative => 3,
    }
}

fn layer_code(l: Layer) -> u32 {
    match l {
        Layer::Behind => 0,
        Layer::Text => 1,
        Layer::Front => 2,
    }
}

fn list_code(kind: Option<ListKind>) -> u32 {
    match kind {
        None => 0,
        Some(ListKind::Bullet) => 1,
        Some(ListKind::Number) => 2,
        Some(ListKind::Check) => 3,
    }
}

fn list_from(code: u32) -> Option<ListKind> {
    match code {
        1 => Some(ListKind::Bullet),
        2 => Some(ListKind::Number),
        3 => Some(ListKind::Check),
        _ => None,
    }
}

fn style_code(s: StyleName) -> u32 {
    match s {
        StyleName::Normal => 0,
        StyleName::Title => 1,
        StyleName::Subtitle => 2,
        StyleName::Heading(n) => 2 + u32::from(n.clamp(1, 6)),
    }
}

fn style_from(code: u32) -> StyleName {
    match code {
        1 => StyleName::Title,
        2 => StyleName::Subtitle,
        3..=8 => StyleName::Heading((code - 2) as u8),
        _ => StyleName::Normal,
    }
}

fn paragraph_style(f: &ParagraphFormat) -> ParagraphStyle {
    // A list level indents on top of the paragraph's own indent.
    let (list_start, list_first) = f.list.map_or((0.0, 0.0), |l| list_indent(l.level));
    // The style's spacing where the paragraph sets none of its own.
    let look = f.style.look();
    let or = |own: f32, style: f32| if own == 0.0 { style } else { own };
    ParagraphStyle {
        indent_start: f.indent_start + list_start,
        indent_end: f.indent_end,
        indent_first: f.indent_first + list_first,
        align: match f.align {
            Align::Left => opendox_layout::Align::Left,
            Align::Center => opendox_layout::Align::Center,
            Align::Right => opendox_layout::Align::Right,
            Align::Justify => opendox_layout::Align::Justify,
        },
        line_spacing: f.line_spacing,
        space_before: or(f.space_before, look.space_before),
        space_after: or(f.space_after, look.space_after),
        page_break_before: f.page_break_before,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendox_model::TextRef;

    /// Which container a position is in, without going through JS.
    fn container_of(e: &Editor, pos: usize) -> TextRef {
        e.outline.locate(pos).container
    }

    const REGULAR: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Regular.ttf");

    fn editor_with(text: &str) -> Editor {
        let mut e = Editor::new();
        e.register_font(REGULAR.to_vec());
        e.insert(0, text).unwrap();
        e.layout();
        e
    }

    #[test]
    fn lays_out_inserted_text_into_flat_glyph_data() {
        let mut e = Editor::new();
        let font = e.register_font(REGULAR.to_vec());
        e.insert(0, "Hello\nworld").unwrap();
        assert_eq!(e.layout(), 1);

        let data = e.page_glyphs(0);
        assert_eq!(data.len(), 10 * GLYPH_STRIDE);
        let first = &data[..GLYPH_STRIDE];
        assert_eq!(first[0], font as f32);
        assert!((first[3] - 72.0).abs() < 1.0);

        let glyph = first[2] as u32;
        assert!(e.glyph_path(font, 0, glyph).unwrap().starts_with('M'));
        assert_eq!(e.units_per_em(font, 0), 1000);
        assert_eq!(e.page_width(), 612.0);
    }

    #[test]
    fn positions_round_trip_through_layout_positions() {
        // Multi-byte chars in several paragraphs: char index and byte offset differ.
        let text = "héllo wörld\nsecond ünïcode line\n\nlast";
        let e = editor_with(text);
        for pos in 0..=text.chars().count() {
            let tp = e.position(pos, false).unwrap();
            assert_eq!(e.caret_out(tp), vec![pos as u32, 0], "pos {pos} -> {tp:?}");
        }
    }

    #[test]
    fn click_then_type_inserts_at_the_clicked_position() {
        let mut e = editor_with("first\nsecond");
        let line_h = 16.34;
        let hit = e.hit_test(0, 72.0, 72.0 + line_h * 1.5).unwrap();
        assert_eq!(hit, vec![6, 0]);
        let after = e.insert(hit[0] as usize, "the ").unwrap();
        assert_eq!(after, 10);
        e.layout();
        assert_eq!(e.text(), "first\nthe second");
    }

    #[test]
    fn caret_and_selection_queries() {
        let mut e = editor_with("ab\ncd");
        let r = e.caret_rect(3, false).unwrap(); // start of "cd"
        assert_eq!(r[0], 0.0);
        assert!((r[1] - 72.0).abs() < 0.01);
        assert_eq!(e.selection_rects(1, 4).len(), 2 * RECT_STRIDE); // "b" + break, then "c"
        assert_eq!(e.slice(1, 4), "b\nc");
        assert_eq!(
            e.move_vertical(1, false, r[1] + 7.0, true),
            Some(vec![4, 0])
        );
        assert_eq!(e.line_end(3, false), Some(vec![5, 0]));
        e.delete(1, 4).unwrap();
        e.layout();
        assert_eq!(e.text(), "ad");
    }

    #[test]
    fn insert_table_type_in_cells_and_tab_between_them() {
        let mut e = editor_with("before");
        let c00 = e.insert_table(6, 2, 2).unwrap();
        e.layout();
        assert!(matches!(container_of(&e, c00), TextRef::Cell(_)));
        assert_eq!(e.table_info(c00), Some(vec![0, 0, 2, 2]));

        let after = e.insert(c00, "A").unwrap();
        e.layout();
        let c01 = e.next_cell(after).unwrap();
        e.insert(c01, "B").unwrap();
        e.layout();
        assert_eq!(e.text(), "before\nA\nB\n\n\n");
        assert_eq!(e.page_rules(0).len(), 2 * 5 * RULE_STRIDE);

        // The caret drawn in cell (0,1) sits in the right half of the page.
        let r = e.caret_rect(c01, false).unwrap();
        assert!(r[1] > 72.0 + 234.0);
        // Clicking there lands back in that cell.
        let hit = e.hit_test(0, r[1], (r[2] + r[3]) / 2.0).unwrap();
        assert_eq!(hit[0] as usize, c01);
    }

    #[test]
    fn table_rows_and_columns_from_the_caret() {
        let mut e = editor_with("");
        let c00 = e.insert_table(0, 1, 1).unwrap();
        e.layout();
        let below = e.table_insert_row(c00, true).unwrap();
        e.layout();
        assert_eq!(e.table_info(below), Some(vec![1, 0, 2, 1]));
        let right = e.table_insert_column(below, true).unwrap();
        e.layout();
        assert_eq!(e.table_info(right), Some(vec![1, 1, 2, 2]));
        let at = e.table_delete_row(right).unwrap();
        e.layout();
        assert_eq!(e.table_info(at), Some(vec![0, 1, 1, 2]));
        let at = e.table_delete_column(at).unwrap();
        e.layout();
        assert_eq!(e.table_info(at), Some(vec![0, 0, 1, 1]));
        let at = e.table_delete_column(at).unwrap(); // last column: removes the table
        e.layout();
        assert_eq!(e.table_info(at), None);
        assert_eq!(e.len(), 0);
    }

    /// A stand-in server: answers the handshake, keeps edits, relays presence as client 1.
    fn fake_server(server: &Document, msg: &[u8]) -> Option<Vec<u8>> {
        use opendox_model::Version;
        use opendox_sync::Message;
        match Message::decode(msg).unwrap() {
            Message::SyncRequest { version } => Some(
                Message::SyncReply {
                    updates: server
                        .export_updates_since(&Version::decode(&version).unwrap())
                        .unwrap(),
                    version: server.version().encode(),
                }
                .encode(),
            ),
            Message::Update { updates } => {
                server.import(&updates).unwrap();
                Some(msg.to_vec()) // forwarded as-is to the other client
            }
            Message::Presence { payload, .. } => {
                Some(Message::Presence { client: 1, payload }.encode())
            }
            _ => None,
        }
    }

    fn join(e: &mut Editor, server: &Document) {
        let hello = e.sync_connect();
        let reply = fake_server(server, &hello).unwrap();
        e.sync_receive(&reply, 0, 0).unwrap();
        e.layout();
        while let Some(m) = e.sync_flush().unwrap() {
            fake_server(server, &m);
        }
        assert!(e.sync_is_synced());
    }

    #[test]
    fn remote_edits_move_the_local_caret_and_remote_carets_resolve() {
        let server = Document::new();
        let mut a = editor_with("hello world");
        let mut b = editor_with("");
        join(&mut a, &server);
        join(&mut b, &server);
        assert_eq!(b.text(), "hello world");

        // B's caret sits before "world"; A types at the start.
        a.insert(0, ">> ").unwrap();
        a.layout();
        let update = a.sync_flush().unwrap().unwrap();
        let forwarded = fake_server(&server, &update).unwrap();
        let r = b.sync_receive(&forwarded, 6, 6).unwrap();
        assert_eq!(r, vec![SYNC_DOC_CHANGED, 9, 9]);
        b.layout();
        assert_eq!(b.slice(9, 14), "world");
        // Nothing to echo back.
        assert!(b.sync_flush().unwrap().is_none());

        // A's caret, as B sees it.
        let p = a.sync_presence("Ada", "#0f766e", 3, 8, true).unwrap();
        let relayed = fake_server(&server, &p).unwrap();
        assert_eq!(
            b.sync_receive(&relayed, 0, 0).unwrap()[0],
            SYNC_PEERS_CHANGED
        );
        assert_eq!(b.peer_ids(), vec![1]);
        assert_eq!(b.peer_name(1).as_deref(), Some("Ada"));
        assert_eq!(b.peer_selection(1), Some(vec![3, 8]));

        // A local edit in B before it shifts it after relayout.
        b.insert(0, "!").unwrap();
        b.layout();
        assert_eq!(b.peer_selection(1), Some(vec![4, 9]));

        b.sync_disconnected();
        assert!(b.peer_ids().is_empty());
        assert!(!b.sync_is_synced());
    }

    #[test]
    fn caret_in_a_deleted_table_lands_nearby() {
        let server = Document::new();
        let mut a = editor_with("ab");
        let c00 = a.insert_table(2, 2, 2).unwrap();
        a.layout();
        let mut b = editor_with("");
        join(&mut a, &server);
        join(&mut b, &server);
        // B's caret in a cell; A deletes the table.
        a.table_delete(c00).unwrap();
        a.layout();
        let fwd = fake_server(&server, &a.sync_flush().unwrap().unwrap()).unwrap();
        let r = b.sync_receive(&fwd, c00, c00).unwrap();
        b.layout();
        assert!(r[1] as usize <= b.len());
        assert_eq!(b.text(), a.text());
    }

    const BOLD: &[u8] = include_bytes!("../../../fonts/noto/NotoSans-Bold.ttf");

    /// Font ids used by the glyphs on page 0, in order, deduplicated.
    fn page_fonts(e: &Editor) -> Vec<u32> {
        let mut v: Vec<u32> = e
            .page_glyphs(0)
            .chunks(GLYPH_STRIDE)
            .map(|g| g[0] as u32)
            .collect();
        v.dedup();
        v
    }

    #[test]
    fn bold_selection_and_pending_bold_reach_the_bold_font() {
        let mut e = editor_with("plain bold");
        let bold = e.register_font(BOLD.to_vec());
        e.set_bold(6, 10, true).unwrap();
        e.layout();
        assert_eq!(page_fonts(&e), vec![0, bold]);
        assert_eq!(e.char_format(6, 10), vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(e.char_format(0, 10)[0], 0.0, "mixed selection isn't bold");

        // Ctrl+B with nothing selected: nothing changes until text is typed there.
        let before = e.page_glyphs(0);
        e.set_bold(5, 5, false).unwrap(); // caret right after "plain": already not bold
        e.set_bold(0, 0, true).unwrap();
        assert_eq!(e.char_format(0, 0)[0], 1.0);
        e.layout();
        assert_eq!(e.page_glyphs(0), before);
        e.insert(0, "B").unwrap();
        e.layout();
        assert_eq!(page_fonts(&e), vec![bold, 0, bold]);
        // Moving away drops the pending format.
        e.set_italic(3, 3, true).unwrap();
        e.insert(7, "x").unwrap();
        e.layout();
        assert_eq!(e.char_format(7, 8), vec![0.0; 6]);
    }

    #[test]
    fn font_registered_after_layout_reaches_text_already_laid_out() {
        let mut e = editor_with("plain bold");
        e.set_bold(6, 10, true).unwrap();
        e.layout();
        assert_eq!(
            page_fonts(&e),
            vec![0],
            "no bold face yet: falls back to regular"
        );
        let bold = e.register_font(BOLD.to_vec());
        e.layout();
        assert_eq!(page_fonts(&e), vec![0, bold]);
    }

    #[test]
    fn centred_paragraph_moves_its_glyphs_and_caret() {
        let mut e = editor_with("short\nleft");
        let left_x = e.caret_rect(0, false).unwrap()[1];
        e.set_align(0, 0, 1).unwrap();
        e.layout();
        assert_eq!(e.paragraph_format(2)[0], 1.0);
        let x = e.caret_rect(0, false).unwrap()[1];
        assert!(x > left_x + 200.0, "centred caret at {x}");
        assert_eq!(
            e.caret_rect(6, false).unwrap()[1],
            left_x,
            "other paragraph unchanged"
        );
        // A click on the centred text lands in it.
        let r = e.caret_rect(3, false).unwrap();
        assert_eq!(e.hit_test(0, r[1], (r[2] + r[3]) / 2.0).unwrap()[0], 3);
    }

    #[test]
    fn undo_and_redo_restore_text_formats_and_caret() {
        let mut e = editor_with("");
        let mut pos = 0;
        for c in ["h", "e", "y"] {
            pos = e.insert(pos, c).unwrap();
            e.layout();
        }
        e.set_bold(0, 3, true).unwrap();
        e.layout();
        // Moving the caret, then typing, starts a new undo step.
        e.insert(0, ">").unwrap();
        e.layout();

        assert_eq!(e.undo(1, 1).unwrap(), Some(vec![0, 0]));
        e.layout();
        assert_eq!(e.text(), "hey");
        assert_eq!(
            e.undo(0, 0).unwrap(),
            Some(vec![0, 3]),
            "selection before bolding"
        );
        e.layout();
        assert_eq!(e.char_format(0, 3)[0], 0.0);
        assert_eq!(
            e.undo(0, 3).unwrap(),
            Some(vec![0, 0]),
            "the whole burst of typing"
        );
        e.layout();
        assert_eq!(e.text(), "");
        assert_eq!(e.undo(0, 0).unwrap(), None);
        assert!(e.can_redo());

        e.redo(0, 0).unwrap().unwrap();
        e.redo(0, 0).unwrap().unwrap();
        e.layout();
        assert_eq!(e.text(), "hey");
        assert_eq!(e.char_format(0, 3)[0], 1.0);
    }

    #[test]
    fn undo_leaves_a_collaborators_edit_alone() {
        let server = Document::new();
        let mut a = editor_with("");
        let mut b = editor_with("");
        join(&mut a, &server);
        join(&mut b, &server);
        a.insert(0, "mine").unwrap();
        a.layout();
        let fwd = fake_server(&server, &a.sync_flush().unwrap().unwrap()).unwrap();
        b.sync_receive(&fwd, 0, 0).unwrap();
        b.layout();
        b.insert(4, " theirs").unwrap();
        b.layout();
        let fwd = fake_server(&server, &b.sync_flush().unwrap().unwrap()).unwrap();
        a.sync_receive(&fwd, 4, 4).unwrap();
        a.layout();

        a.undo(4, 4).unwrap().unwrap();
        a.layout();
        assert_eq!(a.text(), " theirs");
        // The undo is an ordinary edit and syncs like one.
        let fwd = fake_server(&server, &a.sync_flush().unwrap().unwrap()).unwrap();
        b.sync_receive(&fwd, 0, 0).unwrap();
        b.layout();
        assert_eq!(b.text(), " theirs");
    }

    #[test]
    fn selection_from_body_into_table_covers_the_table() {
        let mut e = editor_with("ab");
        let c00 = e.insert_table(2, 2, 2).unwrap();
        e.layout();
        let (a, b) = e.visible_range(1, c00);
        assert_eq!(a, 1);
        let t = e.outline.tables().next().unwrap();
        assert_eq!(b, t.flat_last);
        // Deleting that selection removes the table and the "b".
        let caret = e.delete(1, c00).unwrap();
        e.layout();
        assert_eq!(caret, 1);
        assert_eq!(e.text(), "a\n");
    }
    fn png() -> Vec<u8> {
        vec![0x89, b'P', b'N', b'G']
    }

    /// The first image on page 0: `[key, layer, x, y, w, h, drawn w, drawn h, rotation]`.
    fn first_object(e: &Editor) -> Vec<f32> {
        e.page_objects(0)[..OBJECT_STRIDE].to_vec()
    }

    #[test]
    fn inserted_image_is_laid_out_in_line_and_drawn() {
        let mut e = editor_with("ab");
        let id = e
            .insert_image(1, png(), "image/png", 40, 20, 80.0, 40.0)
            .unwrap();
        e.layout();
        let o = first_object(&e);
        assert_eq!(e.object_id(o[0] as usize).as_deref(), Some(id.as_str()));
        assert_eq!(o[1], 1.0, "text layer");
        assert_eq!((o[6], o[7]), (80.0, 40.0));
        // After "a", on the first line.
        let a_end = e.caret_rect(1, false).unwrap();
        assert!((o[2] - a_end[1]).abs() < 0.01);
        assert_eq!(e.image_position(&id), Some(1));
        let props = e.image_props(&id).unwrap();
        assert_eq!(props[3], 0.0, "in line");
        assert_eq!((props[10], props[11], props[12]), (40.0, 20.0, 0.0));
        assert_eq!(e.image_data(&id), png());
        assert_eq!(e.image_mime(&id), "image/png");
        // The caret after the image is at its right edge.
        let after = e.caret_rect(2, false).unwrap();
        assert!((after[1] - (o[2] + 80.0)).abs() < 0.01);
    }

    #[test]
    fn switching_wrap_mode_keeps_the_image_where_it_was() {
        let mut e = editor_with("Some words before the picture and some after it.");
        let id = e
            .insert_image(10, png(), "image/png", 10, 10, 60.0, 60.0)
            .unwrap();
        e.layout();
        let inline = first_object(&e);
        for mode in [3, 4, 1] {
            e.set_image_wrap(&id, mode).unwrap();
            e.layout();
            let o = first_object(&e);
            assert!(
                (o[2] - inline[2]).abs() < 0.5 && (o[3] - inline[3]).abs() < 0.5,
                "mode {mode}: {o:?} vs {inline:?}"
            );
            e.set_image_wrap(&id, 0).unwrap();
            e.layout();
        }
    }

    #[test]
    fn dragging_a_positioned_image_moves_it_and_its_anchor() {
        let text = (0..30)
            .map(|i| format!("Paragraph number {i}."))
            .collect::<Vec<_>>()
            .join("\n");
        let mut e = editor_with(&text);
        let id = e
            .insert_image(0, png(), "image/png", 10, 10, 50.0, 50.0)
            .unwrap();
        e.layout();
        e.set_image_wrap(&id, 4).unwrap();
        e.layout();
        e.place_image(&id, 0, 300.0, 400.0).unwrap();
        e.layout();
        let o = first_object(&e);
        assert!(
            (o[2] - 300.0).abs() < 0.01 && (o[3] - 400.0).abs() < 0.01,
            "{o:?}"
        );
        // Anchored to the paragraph beside it, not the first one.
        let pos = e.image_position(&id).unwrap();
        assert!(pos > 100, "anchor moved down: {pos}");
        // One undo step puts it back.
        assert!(e.undo(0, 0).unwrap().is_some());
        e.layout();
        assert_eq!(e.image_position(&id), Some(0));
    }

    #[test]
    fn dragging_an_inline_image_moves_it_through_the_text() {
        let mut e = editor_with("first line\nsecond line");
        let id = e
            .insert_image(0, png(), "image/png", 10, 10, 20.0, 20.0)
            .unwrap();
        e.layout();
        let r = e.caret_rect(19, false).unwrap(); // in "second line"
        e.place_image(&id, 0, r[1], (r[2] + r[3]) / 2.0).unwrap();
        e.layout();
        let at = e.image_position(&id).unwrap();
        assert!(at > 11, "moved into the second paragraph: {at}");
        assert_eq!(e.text().matches('\u{FFFC}').count(), 1);
    }

    #[test]
    fn delete_image_and_undo() {
        let mut e = editor_with("ab");
        let id = e
            .insert_image(1, png(), "image/png", 10, 10, 20.0, 20.0)
            .unwrap();
        e.layout();
        e.set_image_adjustments(&id, 10.0, -20.0, 30.0).unwrap();
        e.set_image_alt(&id, "dots").unwrap();
        e.layout();
        let caret = e.delete_image(&id).unwrap();
        e.layout();
        assert_eq!(caret, 1);
        assert!(e.page_objects(0).is_empty());
        e.undo(1, 1).unwrap();
        e.layout();
        assert_eq!(e.page_objects(0).len(), OBJECT_STRIDE);
        let p = e.image_props(&id).unwrap();
        assert_eq!((p[7], p[8], p[9]), (10.0, -20.0, 30.0));
        assert_eq!(e.image_alt(&id), "dots");
    }

    #[test]
    fn text_over_an_image_behind_is_detected() {
        let mut e = editor_with("Some text on the first line");
        let id = e
            .insert_image(0, png(), "image/png", 10, 10, 300.0, 100.0)
            .unwrap();
        e.layout();
        e.set_image_wrap(&id, 3).unwrap();
        e.layout();
        let r = e.caret_rect(5, false).unwrap();
        assert!(e.is_over_text(0, r[1], (r[2] + r[3]) / 2.0));
        assert!(
            !e.is_over_text(0, 500.0, (r[2] + r[3]) / 2.0),
            "past the line's end"
        );
        assert!(!e.is_over_text(0, 100.0, 150.0), "below the text");
    }

    #[test]
    fn heading_style_sets_size_spacing_and_the_outline() {
        let mut e = editor_with("Intro\nbody text");
        let first_line_top = |e: &Editor, pos: usize| e.caret_rect(pos, false).unwrap()[2];
        let body_before = first_line_top(&e, 6);
        e.set_paragraph_style(6, 6, 3).unwrap(); // heading 1
        e.layout();
        assert_eq!(e.paragraph_format(6)[10], 3.0);
        // 20pt glyphs, and the toolbar reads the style's size for unformatted text.
        let sizes: Vec<f32> = e
            .page_glyphs(0)
            .chunks(GLYPH_STRIDE)
            .map(|g| g[5])
            .collect();
        assert_eq!(sizes.last(), Some(&20.0));
        assert_eq!(e.char_format(7, 7)[2], 20.0);
        // Its 20pt space before pushes it down.
        assert!(first_line_top(&e, 6) >= body_before + 20.0 - 0.01);
        assert_eq!(e.headings(), vec![6, 1]);
        // Enter at its end: the new paragraph is normal text again.
        let end = e.insert(15, "\n").unwrap();
        e.layout();
        assert_eq!(e.paragraph_format(end)[10], 0.0);
    }
}
