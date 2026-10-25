//! Lists: numbering, marker text and the commands that make paragraphs list items.
//!
//! A list item is an ordinary paragraph whose format has a [`ListFormat`] (kind, level, list
//! id). Numbering is never stored: it's counted from the items in reading order every time, so
//! concurrent inserts, deletes and level changes can't leave numbers stale. Items of one list
//! (same id) are numbered together across other paragraphs, as in Google Docs.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::document::Document;
use crate::error::ModelError;
use crate::format::{ListFormat, ListKind, ParagraphChange, SlotFormat, MAX_LIST_LEVEL};
use crate::outline::Outline;

/// How far each list level indents its text, in points (0.5 in, as in Google Docs).
pub const LIST_INDENT_STEP: f32 = 36.0;

/// How far a list marker hangs out to the left of its item's text, in points.
pub const LIST_MARKER_HANG: f32 = 18.0;

/// The indents a list level adds: `(start, first line)`. Text starts `start` points from the
/// margin (on top of the paragraph's own indent); the marker sits at `start + first`.
pub fn list_indent(level: u8) -> (f32, f32) {
    (
        LIST_INDENT_STEP * (f32::from(level) + 1.0),
        -LIST_MARKER_HANG,
    )
}

/// The marker drawn before each paragraph ([`Outline::slots`] order): `None` for paragraphs
/// that aren't list items. Bullets cycle ● ○ ■ and numbers 1. a. i. by level; checklist items
/// get ☐ or ☑. Drawing them needs a font with these characters (e.g. Noto Sans Symbols 2).
pub fn list_markers(formats: &[SlotFormat]) -> Vec<Option<String>> {
    // Per list: the count at each level, reset when a shallower item follows.
    let mut counters: HashMap<u64, [u32; MAX_LIST_LEVEL as usize + 1]> = HashMap::new();
    formats
        .iter()
        .map(|f| {
            let l = f.paragraph.list?;
            let level = usize::from(l.level.min(MAX_LIST_LEVEL));
            let c = counters.entry(l.id).or_default();
            c[level] += 1;
            for deeper in &mut c[level + 1..] {
                *deeper = 0;
            }
            Some(marker_text(l, c[level]))
        })
        .collect()
}

/// The marker of item number `n` (from 1) of a list at `l.level`.
pub fn marker_text(l: ListFormat, n: u32) -> String {
    match l.kind {
        ListKind::Bullet => ["●", "○", "■"][usize::from(l.level) % 3].to_owned(),
        ListKind::Check if l.checked => "☑".to_owned(),
        ListKind::Check => "☐".to_owned(),
        ListKind::Number => match l.level % 3 {
            0 => format!("{n}."),
            1 => format!("{}.", alpha(n)),
            _ => format!("{}.", roman(n)),
        },
    }
}

/// 1 → a, 26 → z, 27 → aa, as Word and Google Docs count.
fn alpha(n: u32) -> String {
    let mut n = n.max(1);
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(b'a' + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Lower-case Roman numerals; past 3999 it falls back to digits.
fn roman(n: u32) -> String {
    if n == 0 || n > 3999 {
        return n.to_string();
    }
    const TABLE: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut n = n;
    let mut out = String::new();
    for (v, s) in TABLE {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

impl Document {
    /// Makes every paragraph touched by the range between two flat positions an item of a
    /// `kind` list, or removes them from their lists with `None`.
    ///
    /// The items join the list just before them if it's the same kind (so applying numbering
    /// right after a numbered list continues it), else start a new list. Items keep their
    /// level; a checklist item that was ticked stays ticked.
    pub fn set_list_at(
        &self,
        outline: &Outline,
        a: usize,
        b: usize,
        kind: Option<ListKind>,
    ) -> Result<(), ModelError> {
        let slots = touched(outline, a, b);
        let Some(kind) = kind else {
            for &i in &slots {
                self.change_slot(outline, i, ParagraphChange::List(None))?;
            }
            self.doc.commit();
            return Ok(());
        };
        let id = slots
            .first()
            .and_then(|&i| self.list_before(outline, i))
            .filter(|l| l.kind == kind)
            .map_or_else(|| self.new_list_id(), |l| l.id);
        for &i in &slots {
            let slot = &outline.slots[i];
            let old = self.paragraph_format_in(&slot.container, slot.offset).list;
            let list = ListFormat {
                kind,
                level: old.map_or(0, |l| l.level),
                id,
                checked: kind == ListKind::Check && old.is_some_and(|l| l.checked),
            };
            self.change_slot(outline, i, ParagraphChange::List(Some(list)))?;
        }
        self.doc.commit();
        Ok(())
    }

    /// Indents (`deeper`) or outdents every paragraph touched by the range: list items move a
    /// level, other paragraphs move their start indent by [`LIST_INDENT_STEP`]. Outdenting a
    /// list item at level 0 takes it out of its list, as in Google Docs.
    pub fn indent_at(
        &self,
        outline: &Outline,
        a: usize,
        b: usize,
        deeper: bool,
    ) -> Result<(), ModelError> {
        for i in touched(outline, a, b) {
            let slot = &outline.slots[i];
            let f = self.paragraph_format_in(&slot.container, slot.offset);
            let change = match f.list {
                Some(l) if deeper => ParagraphChange::List(Some(ListFormat {
                    level: (l.level + 1).min(MAX_LIST_LEVEL),
                    ..l
                })),
                Some(l) if l.level == 0 => ParagraphChange::List(None),
                Some(l) => ParagraphChange::List(Some(ListFormat {
                    level: l.level - 1,
                    ..l
                })),
                None => {
                    let step = if deeper {
                        LIST_INDENT_STEP
                    } else {
                        -LIST_INDENT_STEP
                    };
                    // Snap to the step grid, as Google Docs does with odd indents.
                    let v = ((f.indent_start + step) / LIST_INDENT_STEP).round() * LIST_INDENT_STEP;
                    ParagraphChange::IndentStart(v.max(0.0))
                }
            };
            self.change_slot(outline, i, change)?;
        }
        self.doc.commit();
        Ok(())
    }

    /// The list item format of the paragraph before slot `i` in the same container.
    fn list_before(&self, outline: &Outline, i: usize) -> Option<ListFormat> {
        let prev = outline.slots.get(i.checked_sub(1)?)?;
        if prev.container != outline.slots[i].container {
            return None;
        }
        self.paragraph_format_in(&prev.container, prev.offset).list
    }

    fn change_slot(
        &self,
        outline: &Outline,
        i: usize,
        change: ParagraphChange,
    ) -> Result<(), ModelError> {
        let slot = &outline.slots[i];
        let mut f = self.paragraph_format_in(&slot.container, slot.offset);
        f.apply(change);
        self.write_paragraph_format(&slot.container, slot.offset, f)
    }

    /// A list id no other replica will pick: a hash of an id unique across replicas.
    pub(crate) fn new_list_id(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.new_id("list").hash(&mut h);
        // Zero means "no id" in stored data; never hand it out.
        h.finish().max(1)
    }
}

/// Slot indices of the paragraphs the range between two flat positions touches (the caret's
/// paragraph if they're equal).
pub(crate) fn touched(outline: &Outline, a: usize, b: usize) -> Vec<usize> {
    let (lo, hi) = (outline.snap(a.min(b)), outline.snap(a.max(b)));
    outline
        .slots
        .iter()
        .enumerate()
        .filter(|(_, s)| s.flat <= hi && s.flat_end() >= lo)
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_and_roman_counting() {
        let a: Vec<String> = [1, 2, 26, 27, 28, 52, 53, 702, 703].map(alpha).to_vec();
        assert_eq!(a, ["a", "b", "z", "aa", "ab", "az", "ba", "zz", "aaa"]);
        let r: Vec<String> = [1, 4, 9, 14, 40, 90, 400, 1994, 3999].map(roman).to_vec();
        assert_eq!(
            r,
            [
                "i",
                "iv",
                "ix",
                "xiv",
                "xl",
                "xc",
                "cd",
                "mcmxciv",
                "mmmcmxcix"
            ]
        );
        assert_eq!(roman(4000), "4000");
    }
}
