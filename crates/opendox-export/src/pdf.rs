//! PDF export: draws exactly what layout computed, glyph by glyph, so pages, line breaks and
//! positions match the editor.
//!
//! Each font used is embedded as a subset (only the glyphs drawn) in a CID-keyed font whose
//! character codes are the subset's glyph ids. A ToUnicode map from those ids back to the text
//! they draw makes the PDF searchable and copyable. Where that map can't be right, because a
//! character is drawn with several glyphs or with the font's missing-glyph box (which one
//! glyph id would share between different characters), the glyphs are wrapped in an
//! `ActualText` span carrying the real text.

use std::collections::BTreeMap;

use opendox_layout::{FontId, FontLibrary, Paginator};
use pdf_writer::types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use skrifa::instance::{LocationRef, Size};
use skrifa::raw::TableProvider;
use skrifa::string::StringId;
use skrifa::{FontRef, GlyphId, MetadataProvider};
use subsetter::GlyphRemapper;

use crate::ExportError;

/// Text and table-border colour, the same ink the editor draws with (#1d2129).
const INK: [f32; 3] = [29.0 / 255.0, 33.0 / 255.0, 41.0 / 255.0];
/// Table border width in points, as in the editor.
const RULE_WIDTH: f32 = 0.75;

/// Document metadata for [`pdf`].
#[derive(Debug, Clone, Default)]
pub struct PdfInfo<'a> {
    pub title: Option<&'a str>,
}

/// Writes the pages from `paginator`'s last layout as a PDF.
///
/// `paragraphs` holds the text of every paragraph in the paginator's numbering (body and cell
/// paragraphs in reading order), used to map glyphs back to text.
pub fn pdf(
    paginator: &Paginator,
    fonts: &FontLibrary,
    paragraphs: &[&str],
    info: &PdfInfo,
) -> Result<Vec<u8>, ExportError> {
    let geometry = paginator.geometry();
    let height = geometry.height;
    let mut used: BTreeMap<(u32, u32), UsedFont> = BTreeMap::new();
    let mut contents: Vec<Vec<u8>> = Vec::new();

    for (index, page) in paginator.pages().iter().enumerate() {
        let mut c = Content::new();
        c.set_fill_rgb(INK[0], INK[1], INK[2]);
        // The same walk as `Paginator::placed_glyphs`, keeping each run's font and text.
        for f in &page.fragments {
            let Some(para) = paginator.paragraph(f.paragraph) else {
                continue;
            };
            let Some(first) = para.lines.get(f.lines.start) else {
                continue;
            };
            let dy = f.top - first.top;
            let text = paragraphs.get(f.paragraph).copied().unwrap_or("");
            for line in &para.lines[f.lines.clone()] {
                for run in &line.runs {
                    let n = used.len();
                    let font = used
                        .entry((run.font.0, run.face_index))
                        .or_insert_with(|| UsedFont::new(n));
                    c.begin_text();
                    c.set_font(Name(font.name.as_bytes()), run.font_size);
                    let pairs: Vec<_> = run.glyphs.iter().zip(&run.glyph_text).collect();
                    // One group per character (cluster): glyphs sharing their text.
                    for group in pairs.chunk_by(|a, b| a.1 == b.1) {
                        let s = text.get(group[0].1.clone()).unwrap_or("");
                        let spanned = !s.is_empty()
                            && (group.len() > 1 || group.iter().any(|(g, _)| g.id == 0));
                        if spanned {
                            c.begin_marked_content_with_properties(Name(b"Span"))
                                .properties()
                                .actual_text(TextStr(s));
                        }
                        for (g, _) in group {
                            let gid = font.glyph(g.id, (!spanned).then_some(s));
                            c.set_text_matrix([
                                1.0,
                                0.0,
                                0.0,
                                1.0,
                                f.left + g.x,
                                height - (dy + g.y),
                            ]);
                            c.show(Str(&gid.to_be_bytes()));
                        }
                        if spanned {
                            c.end_marked_content();
                        }
                    }
                    c.end_text();
                }
            }
        }
        let rules = &page.rules;
        if !rules.is_empty() {
            c.set_stroke_rgb(INK[0], INK[1], INK[2]);
            c.set_line_width(RULE_WIDTH);
            for r in rules {
                c.move_to(r.x0, height - r.y0);
                c.line_to(r.x1, height - r.y1);
            }
            c.stroke();
        }
        let _ = index;
        contents.push(c.finish().into_vec());
    }

    let mut next = 1;
    let mut alloc = || {
        let r = Ref::new(next);
        next += 1;
        r
    };
    let mut pdf = Pdf::new();
    let catalog = alloc();
    let tree = alloc();
    let info_id = alloc();
    let page_ids: Vec<(Ref, Ref)> = contents.iter().map(|_| (alloc(), alloc())).collect();
    let font_ids: Vec<Ref> = used.values().map(|_| alloc()).collect();

    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree)
        .kids(page_ids.iter().map(|(p, _)| *p))
        .count(page_ids.len() as i32);
    let mut doc_info = pdf.document_info(info_id);
    doc_info.producer(TextStr("opendox"));
    if let Some(t) = info.title {
        doc_info.title(TextStr(t));
    }
    doc_info.finish();

    let media = Rect::new(0.0, 0.0, geometry.width, height);
    for ((page_id, content_id), content) in page_ids.iter().zip(&contents) {
        let mut page = pdf.page(*page_id);
        page.parent(tree).media_box(media).contents(*content_id);
        let mut res = page.resources();
        let mut dict = res.fonts();
        for (font, id) in used.values().zip(&font_ids) {
            dict.pair(Name(font.name.as_bytes()), *id);
        }
        dict.finish();
        res.finish();
        page.finish();
        let packed = deflate(content);
        pdf.stream(*content_id, &packed).filter(Filter::FlateDecode);
    }

    for (((font_id, face), font), type0) in used.iter().map(|(k, v)| (*k, v)).zip(&font_ids) {
        let data = fonts
            .data(FontId(font_id))
            .ok_or(ExportError::MissingFont(font_id))?;
        write_font(&mut pdf, &mut alloc, *type0, data, face, font)?;
    }

    Ok(pdf.finish())
}

/// A font as used in the document: its resource name, the glyphs drawn, and their text.
struct UsedFont {
    name: String,
    glyphs: GlyphRemapper,
    /// Subset glyph id → text it draws; `""` for glyphs only ever drawn inside an
    /// `ActualText` span.
    text: BTreeMap<u16, String>,
}

impl UsedFont {
    fn new(n: usize) -> Self {
        Self {
            name: format!("F{n}"),
            glyphs: GlyphRemapper::new(),
            text: BTreeMap::new(),
        }
    }

    /// The subset id for glyph `id`, recording the text it draws (`None` inside an
    /// `ActualText` span, where readers take the span's text instead).
    fn glyph(&mut self, id: u32, text: Option<&str>) -> u16 {
        // Glyph ids in OpenType are 16-bit; layout only ever produces those.
        let gid = self.glyphs.remap(u16::try_from(id).unwrap_or(0));
        let entry = text.unwrap_or("");
        match self.text.get(&gid) {
            // The first real text wins: one glyph drawing different text in different places
            // can only map to one.
            Some(t) if !t.is_empty() => {}
            _ => {
                self.text.insert(gid, entry.to_owned());
            }
        }
        gid
    }
}

fn write_font(
    pdf: &mut Pdf,
    alloc: &mut impl FnMut() -> Ref,
    type0: Ref,
    data: &[u8],
    face: u32,
    used: &UsedFont,
) -> Result<(), ExportError> {
    let font = FontRef::from_index(data, face).map_err(|e| ExportError::Font(e.to_string()))?;
    let subset = subsetter::subset(data, face, &used.glyphs)
        .map_err(|e| ExportError::Font(format!("{e:?}")))?;
    let cff = font.cff().is_ok() || font.cff2().is_ok();
    let metrics = skrifa::metrics::Metrics::new(&font, Size::unscaled(), LocationRef::default());
    let advances = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let scale = 1000.0 / f32::from(metrics.units_per_em.max(1));

    let ps_name = font
        .localized_strings(StringId::POSTSCRIPT_NAME)
        .english_or_first()
        .map(|s| {
            s.chars()
                .filter(|c| c.is_ascii_graphic())
                .collect::<String>()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Font".into());
    let base = format!("{}+{ps_name}", subset_tag(&used.glyphs));

    let cid = alloc();
    let descriptor = alloc();
    let file = alloc();
    let cmap = alloc();

    pdf.type0_font(type0)
        .base_font(Name(base.as_bytes()))
        .encoding_predefined(Name(b"Identity-H"))
        .descendant_font(cid)
        .to_unicode(cmap);

    let mut c = pdf.cid_font(cid);
    c.subtype(if cff {
        CidFontType::Type0
    } else {
        CidFontType::Type2
    })
    .base_font(Name(base.as_bytes()))
    .system_info(SystemInfo {
        registry: Str(b"Adobe"),
        ordering: Str(b"Identity"),
        supplement: 0,
    })
    .font_descriptor(descriptor);
    if !cff {
        c.cid_to_gid_map_predefined(Name(b"Identity"));
    }
    let widths: Vec<f32> = used
        .glyphs
        .remapped_gids()
        .map(|old| {
            advances
                .advance_width(GlyphId::new(u32::from(old)))
                .unwrap_or(0.0)
                * scale
        })
        .collect();
    c.widths().consecutive(0, widths);
    c.finish();

    let mut flags = FontFlags::SYMBOLIC;
    if metrics.italic_angle != 0.0 {
        flags |= FontFlags::ITALIC;
    }
    let bbox = metrics.bounds.map_or(Rect::new(0.0, 0.0, 0.0, 0.0), |b| {
        Rect::new(
            b.x_min * scale,
            b.y_min * scale,
            b.x_max * scale,
            b.y_max * scale,
        )
    });
    let mut d = pdf.font_descriptor(descriptor);
    d.name(Name(base.as_bytes()))
        .flags(flags)
        .bbox(bbox)
        .italic_angle(metrics.italic_angle)
        .ascent(metrics.ascent * scale)
        .descent(metrics.descent * scale)
        .cap_height(metrics.cap_height.unwrap_or(metrics.ascent) * scale)
        // Not used for rendering embedded fonts; a typical value for regular text.
        .stem_v(80.0);
    if cff {
        d.font_file3(file);
    } else {
        d.font_file2(file);
    }
    d.finish();

    let packed = deflate(&subset);
    let mut stream = pdf.stream(file, &packed);
    stream.filter(Filter::FlateDecode);
    if cff {
        stream.pair(Name(b"Subtype"), Name(b"OpenType"));
    }
    stream.finish();

    let mut map = UnicodeCmap::new(
        Name(b"Custom"),
        SystemInfo {
            registry: Str(b"Adobe"),
            ordering: Str(b"UCS"),
            supplement: 0,
        },
    );
    for (gid, text) in &used.text {
        map.pair_with_multiple(*gid, text.chars());
    }
    let cmap_data = map.finish();
    pdf.cmap(cmap, &cmap_data);
    Ok(())
}

/// Six capital letters naming a font subset, derived from the glyphs it contains so the same
/// subset always gets the same tag.
fn subset_tag(glyphs: &GlyphRemapper) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a
    for g in glyphs.remapped_gids() {
        for b in g.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    (0..6)
        .map(|i| char::from(b'A' + ((h >> (i * 8)) % 26) as u8))
        .collect()
}

fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}
