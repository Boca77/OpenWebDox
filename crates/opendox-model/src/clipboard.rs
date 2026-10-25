//! Copy and paste with formatting.
//!
//! A [`Clip`] is a run of paragraphs with their character and paragraph formats: what a copy
//! takes out of a document ([`Document::copy_at`]) and what a paste puts in
//! ([`Document::paste_at`], one change, so one undo step). It travels as JSON
//! ([`Clip::to_json`]), and as HTML ([`Clip::to_html`]) that other editors understand and that
//! also carries the JSON, so pasting between opendox documents loses nothing.
//!
//! Tables and images aren't carried: a table's cells come out as paragraphs and images are
//! left out.

use std::collections::HashMap;
use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::error::ModelError;
use crate::format::{Align, CharFormat, ListFormat, ListKind, ParagraphFormat, Script};
use crate::image::IMAGE_PLACEHOLDER;
use crate::outline::Outline;
use crate::style::StyleName;

/// Copied content: paragraphs in order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub paragraphs: Vec<ClipParagraph>,
}

/// One paragraph of a [`Clip`]. Absent fields are the defaults.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipParagraph {
    pub text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<ClipRun>,
    /// `title`, `subtitle`, `h1` … `h6`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// `center`, `right`, `justify`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_spacing: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_before: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_after: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indent_start: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<ClipList>,
}

/// A list item's place: `bullet`, `number` or `check`, its level, and which list (items
/// sharing an id stay one list after pasting).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipList {
    pub kind: String,
    pub level: u8,
    pub id: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub checked: bool,
}

/// A formatted stretch of a paragraph: `start..end` in chars.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipRun {
    pub start: usize,
    pub end: usize,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub strike: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
    /// `super` or `sub`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

impl ClipRun {
    fn format(&self) -> CharFormat {
        CharFormat {
            bold: self.bold,
            italic: self.italic,
            size: self.size.filter(|s| s.is_finite() && *s > 0.0),
            font: self.font.clone().filter(|f| !f.is_empty()),
            underline: self.underline,
            strike: self.strike,
            color: self.color.as_deref().and_then(hex_color),
            highlight: self.highlight.as_deref().and_then(hex_color),
            script: match self.script.as_deref() {
                Some("super") => Script::Super,
                Some("sub") => Script::Sub,
                _ => Script::Normal,
            },
            link: self.link.clone().filter(|l| !l.is_empty()),
        }
    }

    fn of(start: usize, end: usize, f: &CharFormat) -> ClipRun {
        ClipRun {
            start,
            end,
            bold: f.bold,
            italic: f.italic,
            underline: f.underline,
            strike: f.strike,
            size: f.size,
            font: f.font.clone(),
            color: f.color.clone(),
            highlight: f.highlight.clone(),
            script: match f.script {
                Script::Normal => None,
                Script::Super => Some("super".into()),
                Script::Sub => Some("sub".into()),
            },
            link: f.link.clone(),
        }
    }
}

/// `#rrggbb` (or `rrggbb`), lower-cased; anything else is `None`.
fn hex_color(s: &str) -> Option<String> {
    let h = s.trim().trim_start_matches('#');
    (h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("#{}", h.to_ascii_lowercase()))
}

impl ClipParagraph {
    /// The paragraph format to give the pasted paragraph; `lists` maps copied list ids to
    /// lists made for this paste.
    fn format(&self, lists: &mut HashMap<String, u64>, doc: &Document) -> ParagraphFormat {
        let d = ParagraphFormat::default();
        let ok =
            |v: Option<f32>, lo: f32, hi: f32| v.filter(|v| v.is_finite() && (lo..=hi).contains(v));
        ParagraphFormat {
            align: match self.align.as_deref() {
                Some("center") => Align::Center,
                Some("right") => Align::Right,
                Some("justify") => Align::Justify,
                _ => Align::Left,
            },
            line_spacing: ok(self.line_spacing, 0.5, 5.0).unwrap_or(d.line_spacing),
            space_before: ok(self.space_before, 0.0, 200.0).unwrap_or(0.0),
            space_after: ok(self.space_after, 0.0, 200.0).unwrap_or(0.0),
            indent_start: ok(self.indent_start, 0.0, 400.0).unwrap_or(0.0),
            list: self.list.as_ref().and_then(|l| {
                let kind = match l.kind.as_str() {
                    "bullet" => ListKind::Bullet,
                    "number" => ListKind::Number,
                    "check" => ListKind::Check,
                    _ => return None,
                };
                let id = *lists
                    .entry(format!("{}:{}", l.kind, l.id))
                    .or_insert_with(|| doc.new_list_id());
                Some(ListFormat {
                    kind,
                    level: l.level.min(crate::format::MAX_LIST_LEVEL),
                    id,
                    checked: kind == ListKind::Check && l.checked,
                })
            }),
            style: self
                .style
                .as_deref()
                .and_then(StyleName::parse)
                .unwrap_or_default(),
            ..d
        }
    }

    fn of(text: String, runs: Vec<ClipRun>, f: &ParagraphFormat) -> ClipParagraph {
        let d = ParagraphFormat::default();
        let some = |v: f32, default: f32| (v != default).then_some(v);
        ClipParagraph {
            text,
            runs,
            style: (f.style != StyleName::Normal).then(|| f.style.key().to_owned()),
            align: match f.align {
                Align::Left => None,
                Align::Center => Some("center".into()),
                Align::Right => Some("right".into()),
                Align::Justify => Some("justify".into()),
            },
            line_spacing: some(f.line_spacing, d.line_spacing),
            space_before: some(f.space_before, 0.0),
            space_after: some(f.space_after, 0.0),
            indent_start: some(f.indent_start, 0.0),
            list: f.list.map(|l| ClipList {
                kind: match l.kind {
                    ListKind::Bullet => "bullet",
                    ListKind::Number => "number",
                    ListKind::Check => "check",
                }
                .into(),
                level: l.level,
                id: format!("{:x}", l.id),
                checked: l.checked,
            }),
        }
    }
}

impl Clip {
    /// The clip as JSON.
    pub fn to_json(&self) -> String {
        // Serializing plain strings and numbers can't fail.
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Reads a clip from JSON (from [`Clip::to_json`], or built by an editor from pasted HTML).
    pub fn from_json(json: &str) -> Result<Clip, ModelError> {
        serde_json::from_str(json).map_err(|e| ModelError::BadClip(e.to_string()))
    }

    /// The text, paragraphs separated by `\n`.
    pub fn plain_text(&self) -> String {
        let texts: Vec<&str> = self.paragraphs.iter().map(|p| p.text.as_str()).collect();
        texts.join("\n")
    }

    /// HTML for other editors (headings, lists, bold, colours, links …), carrying the clip's
    /// JSON in a `data-opendox-clip` attribute for lossless pasting back into opendox.
    pub fn to_html(&self) -> String {
        let mut out = String::from(r#"<meta charset="utf-8">"#);
        let _ = write!(
            out,
            r#"<div data-opendox-clip="{}">"#,
            escape(&self.to_json())
        );
        // Open list tags, innermost last.
        let mut open: Vec<&str> = Vec::new();
        for p in &self.paragraphs {
            let list = p.list.as_ref();
            let depth = list.map_or(0, |l| l.level as usize + 1);
            let tag_here = list.map(list_tag);
            // Close deeper lists, and a list of the other kind at this depth.
            while open.len() > depth
                || (open.len() == depth && depth > 0 && open.last().copied() != tag_here)
            {
                if let Some(tag) = open.pop() {
                    let _ = write!(out, "</{tag}>");
                }
            }
            if let Some(tag) = tag_here {
                while open.len() < depth {
                    let _ = write!(out, "<{tag}>");
                    open.push(tag);
                }
            }
            let (tag, style) = match (list, p.style.as_deref()) {
                (Some(_), _) => ("li", String::new()),
                (None, Some(h @ ("h1" | "h2" | "h3" | "h4" | "h5" | "h6"))) => (h, String::new()),
                (None, Some("title")) => ("p", "font-size:26pt;".into()),
                (None, Some("subtitle")) => ("p", "font-size:15pt;color:#666666;".into()),
                _ => ("p", String::new()),
            };
            let align = p
                .align
                .as_deref()
                .map(|a| format!("text-align:{a};"))
                .unwrap_or_default();
            let style = format!("{style}{align}");
            if style.is_empty() {
                let _ = write!(out, "<{tag}>");
            } else {
                let _ = write!(out, r#"<{tag} style="{style}">"#);
            }
            if list.is_some_and(|l| l.kind == "check") {
                out.push_str(if list.is_some_and(|l| l.checked) {
                    "☑ "
                } else {
                    "☐ "
                });
            }
            write_runs(&mut out, p);
            let _ = write!(out, "</{tag}>");
        }
        while let Some(tag) = open.pop() {
            let _ = write!(out, "</{tag}>");
        }
        out.push_str("</div>");
        out
    }
}

fn list_tag(l: &ClipList) -> &'static str {
    if l.kind == "number" {
        "ol"
    } else {
        "ul"
    }
}

/// A paragraph's text as HTML, each formatted run wrapped in its tags and styles.
fn write_runs(out: &mut String, p: &ClipParagraph) {
    let chars: Vec<char> = p.text.chars().collect();
    let mut at = 0;
    let piece = |out: &mut String, a: usize, b: usize, r: Option<&ClipRun>| {
        let text: String = chars[a.min(chars.len())..b.min(chars.len())]
            .iter()
            .collect();
        if text.is_empty() {
            return;
        }
        let Some(r) = r else {
            out.push_str(&escape(&text));
            return;
        };
        let mut css = String::new();
        if let Some(s) = r.size {
            let _ = write!(css, "font-size:{s}pt;");
        }
        if let Some(f) = &r.font {
            let _ = write!(css, "font-family:'{}';", f.replace('\'', ""));
        }
        if let Some(c) = &r.color {
            let _ = write!(css, "color:{c};");
        }
        if let Some(c) = &r.highlight {
            let _ = write!(css, "background-color:{c};");
        }
        let mut close = Vec::new();
        if let Some(url) = &r.link {
            let _ = write!(out, r#"<a href="{}">"#, escape(url));
            close.push("</a>");
        }
        for (on, open, end) in [
            (r.bold, "<b>", "</b>"),
            (r.italic, "<i>", "</i>"),
            (r.underline, "<u>", "</u>"),
            (r.strike, "<s>", "</s>"),
            (r.script.as_deref() == Some("super"), "<sup>", "</sup>"),
            (r.script.as_deref() == Some("sub"), "<sub>", "</sub>"),
        ] {
            if on {
                out.push_str(open);
                close.push(end);
            }
        }
        if !css.is_empty() {
            let _ = write!(out, r#"<span style="{css}">"#);
            close.push("</span>");
        }
        out.push_str(&escape(&text));
        for c in close.iter().rev() {
            out.push_str(c);
        }
    };
    for r in &p.runs {
        piece(out, at, r.start, None);
        piece(out, r.start, r.end, Some(r));
        at = r.end;
    }
    piece(out, at, chars.len(), None);
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

impl Document {
    /// What a copy of the range between two flat positions takes: each paragraph (or table
    /// cell paragraph) it touches, cut to the range, with its formats.
    pub fn copy_at(&self, outline: &Outline, a: usize, b: usize) -> Clip {
        let (lo, hi) = (a.min(b), a.max(b));
        let formats = self.formats(outline);
        let mut clip = Clip::default();
        for (slot, f) in outline.slots.iter().zip(&formats) {
            let (from, to) = (lo.max(slot.flat), hi.min(slot.flat_end()));
            if from > to || (from == to && slot.flat_end() > slot.flat && lo != hi) {
                continue;
            }
            let (s, e) = (from - slot.flat, to - slot.flat);
            // Objects (images, fields) don't travel.
            let kept: Vec<(usize, char)> = slot
                .text
                .chars()
                .enumerate()
                .skip(s)
                .take(e - s)
                .filter(|(_, c)| *c != IMAGE_PLACEHOLDER)
                .collect();
            let text: String = kept.iter().map(|(_, c)| c).collect();
            // Char index in the slot → index in the copied text.
            let at = |i: usize| kept.iter().take_while(|(k, _)| *k < i).count();
            let runs = f
                .runs
                .iter()
                .filter(|r| r.range.end > s && r.range.start < e)
                .map(|r| ClipRun::of(at(r.range.start.max(s)), at(r.range.end.min(e)), &r.format))
                .filter(|r| r.start < r.end)
                .collect();
            clip.paragraphs
                .push(ClipParagraph::of(text, runs, &f.paragraph));
        }
        clip
    }

    /// Pastes `clip` at flat position `pos` as one change; returns the caret after it.
    ///
    /// The first pasted paragraph joins the paragraph at the caret and the last one the text
    /// after it, keeping that paragraph's format; paragraphs in between keep their own. A
    /// paragraph the paste fills entirely (pasted at its start, or ending at its end) takes the
    /// pasted format too. Lists in the clip become new lists here.
    pub fn paste_at(
        &self,
        outline: &Outline,
        pos: usize,
        clip: &Clip,
    ) -> Result<usize, ModelError> {
        let paras: Vec<&ClipParagraph> = clip.paragraphs.iter().collect();
        if paras.is_empty() {
            return Ok(pos);
        }
        // Placeholders and line breaks inside a paragraph's text would break the structure.
        let clean = |t: &str| -> String {
            t.chars()
                .filter(|c| *c != IMAGE_PLACEHOLDER && *c != '\n')
                .collect()
        };
        let texts: Vec<String> = paras.iter().map(|p| clean(&p.text)).collect();
        let text = texts.join("\n");
        let loc = outline.locate(pos);
        let container = loc.container.clone();
        let body = self.container(&container)?;
        let before = body.to_string();
        let prev = loc
            .offset
            .checked_sub(1)
            .and_then(|p| before.chars().nth(p));
        let next = before.chars().nth(loc.offset);
        let at_start = prev.is_none_or(|c| c == '\n');
        let at_end = next.is_none_or(|c| c == '\n');

        self.raw_insert(&container, loc.offset, &text, Some(&CharFormat::default()))?;
        let mut lists = HashMap::new();
        let mut start = loc.offset;
        let last = paras.len() - 1;
        for (k, (p, t)) in paras.iter().zip(&texts).enumerate() {
            let n = t.chars().count();
            for r in &p.runs {
                // Runs index the clip's text, which may have had characters dropped.
                let (a, b) = (r.start.min(n), r.end.min(n));
                if a < b {
                    self.set_char_format(&body, start + a..start + b, &r.format())?;
                }
            }
            let whole = match (k == 0, k == last) {
                (true, true) => at_start && at_end,
                (true, false) => at_start,
                (false, true) => at_end,
                (false, false) => true,
            };
            if whole {
                let f = p.format(&mut lists, self);
                self.write_paragraph_format(&container, start, f)?;
            }
            start += n + 1;
        }
        self.doc.commit();
        Ok(pos + text.chars().count())
    }
}
