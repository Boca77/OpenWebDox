//! PDF export: draws exactly what layout computed, glyph by glyph, so pages, line breaks and
//! positions match the editor.
//!
//! Each font used is embedded as a subset (only the glyphs drawn) in a CID-keyed font whose
//! character codes are the subset's glyph ids. A ToUnicode map from those ids back to the text
//! they draw makes the PDF searchable and copyable. Where that map can't be right, because a
//! character is drawn with several glyphs or with the font's missing-glyph box (which one
//! glyph id would share between different characters), the glyphs are wrapped in an
//! `ActualText` span carrying the real text.
//!
//! Text colours, highlights, underlines and strikethroughs are drawn as layout placed them.
//! Links become link annotations over their text, so they open in PDF readers. Titles and
//! headings become the PDF's bookmarks (its outline), nested by level.

use std::collections::BTreeMap;
use std::ops::Range;

use opendox_layout::{
    DecorationKind, FontId, FontLibrary, GlyphRun, Layer, Paginator, PlacedObject, TextPosition,
};
use pdf_writer::types::{
    ActionType, AnnotationType, CidFontType, FontFlags, SystemInfo, UnicodeCmap,
};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use skrifa::instance::{LocationRef, Size};
use skrifa::raw::TableProvider;
use skrifa::string::StringId;
use skrifa::{FontRef, GlyphId, MetadataProvider};
use subsetter::GlyphRemapper;

use crate::image::{prepare, ExportImage, Prepared};
use crate::ExportError;

/// Text and table-border colour, the same ink the editor draws with (#1d2129).
const INK: [f32; 3] = [29.0 / 255.0, 33.0 / 255.0, 41.0 / 255.0];
/// Table border width in points, as in the editor.
const RULE_WIDTH: f32 = 0.75;

/// Document metadata and pictures for [`pdf`].
#[derive(Debug, Clone, Default)]
pub struct PdfInfo<'a> {
    pub title: Option<&'a str>,
    /// The picture for each object layout placed, indexed by [`PlacedObject::key`]. An object
    /// without one (or whose file can't be decoded) is left out.
    pub images: &'a [Option<ExportImage<'a>>],
    /// Text that links somewhere: made clickable in the PDF.
    pub links: &'a [PdfLink<'a>],
    /// Titles and headings in reading order, for the bookmarks.
    pub headings: &'a [PdfHeading<'a>],
}

/// A bookmark: a title or heading paragraph and its level (0 title, 1–6 headings; deeper
/// levels nest under shallower ones).
#[derive(Debug, Clone)]
pub struct PdfHeading<'a> {
    /// Paragraph index, as layout numbers them.
    pub paragraph: usize,
    pub level: u8,
    pub title: &'a str,
}

/// A link: some of a paragraph's text and where it goes.
#[derive(Debug, Clone)]
pub struct PdfLink<'a> {
    /// Paragraph index, as layout numbers them.
    pub paragraph: usize,
    /// UTF-8 byte range in the paragraph's text.
    pub bytes: Range<usize>,
    pub url: &'a str,
}

/// `0xRRGGBB` (or the ink for `None`) as PDF RGB components.
fn rgb(color: Option<u32>) -> [f32; 3] {
    match color {
        None => INK,
        Some(c) => [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff].map(|v| v as f32 / 255.0),
    }
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

    // Every picture drawn, decoded once however often it appears.
    let mut pictures: BTreeMap<usize, (Prepared, f32)> = BTreeMap::new();
    for o in paginator.pages().iter().flat_map(|p| &p.objects) {
        if pictures.contains_key(&o.key) {
            continue;
        }
        let Some(Some(img)) = info.images.get(o.key) else {
            continue;
        };
        if let Some(prepared) = prepare(img) {
            pictures.insert(o.key, (prepared, img.alpha()));
        }
    }

    for (index, page) in paginator.pages().iter().enumerate() {
        let mut c = Content::new();
        // Behind the text first, then those among it (they never overlap it).
        for o in page.objects.iter().filter(|o| o.layer != Layer::Front) {
            draw_picture(&mut c, o, height, &pictures);
        }
        let decorations = paginator.placed_decorations(index);
        let fill = |c: &mut Content, color: Option<u32>, r: &opendox_layout::Rect| {
            let [red, green, blue] = rgb(color);
            c.set_fill_rgb(red, green, blue);
            c.rect(r.x, height - r.y - r.height, r.width, r.height);
            c.fill_nonzero();
        };
        // Highlights behind the text.
        for d in decorations
            .iter()
            .filter(|d| d.kind == DecorationKind::Highlight)
        {
            fill(&mut c, d.color, &d.rect);
        }
        let mut ink: Option<Option<u32>> = None;
        // The same walk as `Paginator::placed_glyphs`, keeping each run's font and text, and
        // where its glyph positions are measured from.
        let mut runs: Vec<(&GlyphRun, &str, f32, f32)> = Vec::new();
        for f in &page.fragments {
            let Some(para) = paginator.paragraph(f.paragraph) else {
                continue;
            };
            let Some(first) = para.lines.get(f.lines.start) else {
                continue;
            };
            let dy = f.top - first.top;
            let text = paragraphs.get(f.paragraph).copied().unwrap_or("");
            // A list marker first, so it extracts before its item's text; its runs index
            // into the marker's own text.
            if let Some(m) = para.marker.as_ref().filter(|_| f.lines.start == 0) {
                runs.extend(m.runs.iter().map(|r| (r, m.text.as_str(), f.left, dy)));
            }
            for line in &para.lines[f.lines.clone()] {
                // Header and footer fields where they sit in the line, so text extracts in
                // reading order; their runs are in page coordinates and index their own text.
                let mut items: Vec<(usize, &GlyphRun, &str, f32, f32)> = line
                    .runs
                    .iter()
                    .map(|r| (r.byte_range.start, r, text, f.left, dy))
                    .collect();
                for l in page
                    .labels
                    .iter()
                    .filter(|l| l.paragraph == f.paragraph && line.byte_range.contains(&l.byte))
                {
                    items.extend(
                        l.runs
                            .iter()
                            .map(|r| (l.byte, r, l.text.as_str(), 0.0, 0.0)),
                    );
                }
                // Only lines holding a field are reordered: runs come in visual order, which
                // right-to-left text keeps.
                if items.len() > line.runs.len() {
                    items.sort_by_key(|i| i.0);
                }
                runs.extend(items.into_iter().map(|(_, r, t, x, y)| (r, t, x, y)));
            }
        }
        for (run, text, dx, dy) in runs {
            let n = used.len();
            let font = used
                .entry((run.font.0, run.face_index))
                .or_insert_with(|| UsedFont::new(n));
            c.begin_text();
            if ink != Some(run.color) {
                let [red, green, blue] = rgb(run.color);
                c.set_fill_rgb(red, green, blue);
                ink = Some(run.color);
            }
            c.set_font(Name(font.name.as_bytes()), run.font_size);
            let pairs: Vec<_> = run.glyphs.iter().zip(&run.glyph_text).collect();
            // One group per character (cluster): glyphs sharing their text.
            for group in pairs.chunk_by(|a, b| a.1 == b.1) {
                let s = text.get(group[0].1.clone()).unwrap_or("");
                let spanned =
                    !s.is_empty() && (group.len() > 1 || group.iter().any(|(g, _)| g.id == 0));
                if spanned {
                    c.begin_marked_content_with_properties(Name(b"Span"))
                        .properties()
                        .actual_text(TextStr(s));
                }
                for (g, _) in group {
                    let gid = font.glyph(g.id, (!spanned).then_some(s));
                    c.set_text_matrix([1.0, 0.0, 0.0, 1.0, dx + g.x, height - (dy + g.y)]);
                    c.show(Str(&gid.to_be_bytes()));
                }
                if spanned {
                    c.end_marked_content();
                }
            }
            c.end_text();
        }
        // Underlines and strikethroughs over it.
        for d in decorations
            .iter()
            .filter(|d| d.kind != DecorationKind::Highlight)
        {
            fill(&mut c, d.color, &d.rect);
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
        for o in page.objects.iter().filter(|o| o.layer == Layer::Front) {
            draw_picture(&mut c, o, height, &pictures);
        }
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
    // Each link's areas, per page: one per line it covers.
    let mut annotations: Vec<Vec<(Ref, Rect, &str)>> = vec![Vec::new(); contents.len()];
    for link in info.links {
        let at = |byte: usize| TextPosition {
            paragraph: link.paragraph,
            byte,
            upstream: false,
        };
        let rects = paginator.selection_rects(at(link.bytes.start), at(link.bytes.end));
        for r in rects {
            if let Some(list) = annotations.get_mut(r.page) {
                let area = Rect::new(r.x, height - r.y - r.height, r.x + r.width, height - r.y);
                list.push((alloc(), area, link.url));
            }
        }
    }
    let font_ids: Vec<Ref> = used.values().map(|_| alloc()).collect();
    // Per picture: the image, its alpha mask if it has one, a graphics state if see-through.
    let picture_ids: BTreeMap<usize, (Ref, Ref, Ref)> = pictures
        .keys()
        .map(|k| (*k, (alloc(), alloc(), alloc())))
        .collect();

    // Bookmarks: each heading's parent is the nearest earlier heading of a shallower level.
    let outline_root = alloc();
    let mut marks: Vec<Bookmark> = Vec::new();
    for h in info.headings {
        let Some(line) = paginator
            .placed_lines()
            .iter()
            .find(|l| l.paragraph == h.paragraph && l.line == 0)
        else {
            continue;
        };
        let parent = marks.iter().rposition(|m| m.level < h.level);
        let id = alloc();
        marks.push(Bookmark {
            id,
            level: h.level,
            title: h.title.trim(),
            page: line.page,
            top: height - line.top,
            parent,
        });
    }
    let mut catalog_writer = pdf.catalog(catalog);
    catalog_writer.pages(tree);
    if !marks.is_empty() {
        catalog_writer.outlines(outline_root);
    }
    catalog_writer.finish();
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
    for (n, ((page_id, content_id), content)) in page_ids.iter().zip(&contents).enumerate() {
        let mut page = pdf.page(*page_id);
        page.parent(tree).media_box(media).contents(*content_id);
        let links = &annotations[n];
        if !links.is_empty() {
            page.annotations(links.iter().map(|(id, _, _)| *id));
        }
        let mut res = page.resources();
        let mut dict = res.fonts();
        for (font, id) in used.values().zip(&font_ids) {
            dict.pair(Name(font.name.as_bytes()), *id);
        }
        dict.finish();
        if !pictures.is_empty() {
            let mut x = res.x_objects();
            for (key, (image, _, _)) in &picture_ids {
                x.pair(Name(picture_name(*key).as_bytes()), *image);
            }
            x.finish();
            let mut g = res.ext_g_states();
            for (key, (_, _, state)) in &picture_ids {
                g.pair(Name(alpha_name(*key).as_bytes()), *state);
            }
            g.finish();
        }
        res.finish();
        page.finish();
        let packed = deflate(content);
        pdf.stream(*content_id, &packed).filter(Filter::FlateDecode);
    }

    if !marks.is_empty() {
        write_outline(&mut pdf, outline_root, &marks, &page_ids);
    }

    for (id, area, url) in annotations.iter().flatten() {
        let mut a = pdf.annotation(*id);
        a.subtype(AnnotationType::Link)
            .rect(*area)
            .border(0.0, 0.0, 0.0, None);
        a.action()
            .action_type(ActionType::Uri)
            .uri(Str(url.as_bytes()));
    }

    for (((font_id, face), font), type0) in used.iter().map(|(k, v)| (*k, v)).zip(&font_ids) {
        let data = fonts
            .data(FontId(font_id))
            .ok_or(ExportError::MissingFont(font_id))?;
        write_font(&mut pdf, &mut alloc, *type0, data, face, font)?;
    }

    for (key, (prepared, alpha)) in &pictures {
        let Some(&(image, mask, state)) = picture_ids.get(key) else {
            continue;
        };
        write_picture(&mut pdf, prepared, image, mask);
        pdf.ext_graphics(state)
            .non_stroking_alpha(*alpha)
            .stroking_alpha(*alpha);
    }

    Ok(pdf.finish())
}

/// A bookmark to write: where its heading starts (page, y from the page's bottom) and its
/// parent bookmark, as an index into the list.
struct Bookmark<'a> {
    id: Ref,
    level: u8,
    title: &'a str,
    page: usize,
    top: f32,
    parent: Option<usize>,
}

/// The outline dictionary and its items, every item open.
fn write_outline(pdf: &mut Pdf, root: Ref, marks: &[Bookmark], pages: &[(Ref, Ref)]) {
    let children = |parent: Option<usize>| -> Vec<usize> {
        (0..marks.len())
            .filter(|&i| marks[i].parent == parent)
            .collect()
    };
    // Items below each one, at any depth (what an open item's /Count holds).
    let mut below = vec![0i32; marks.len()];
    for i in (0..marks.len()).rev() {
        if let Some(p) = marks[i].parent {
            below[p] += 1 + below[i];
        }
    }
    let top = children(None);
    let mut o = pdf.outline(root);
    if let (Some(&first), Some(&last)) = (top.first(), top.last()) {
        o.first(marks[first].id).last(marks[last].id);
    }
    o.count(marks.len() as i32);
    o.finish();
    for (i, m) in marks.iter().enumerate() {
        let siblings = children(m.parent);
        let at = siblings.iter().position(|&s| s == i).unwrap_or(0);
        let kids = children(Some(i));
        let mut item = pdf.outline_item(m.id);
        item.title(TextStr(if m.title.is_empty() {
            "Untitled"
        } else {
            m.title
        }));
        item.parent(m.parent.map_or(root, |p| marks[p].id));
        if let Some(&prev) = at.checked_sub(1).and_then(|k| siblings.get(k)) {
            item.prev(marks[prev].id);
        }
        if let Some(&next) = siblings.get(at + 1) {
            item.next(marks[next].id);
        }
        if let (Some(&first), Some(&last)) = (kids.first(), kids.last()) {
            item.first(marks[first].id).last(marks[last].id);
            item.count(below[i]);
        }
        if let Some((page, _)) = pages.get(m.page) {
            item.dest().page(*page).xyz(0.0, m.top, None);
        }
    }
}

fn picture_name(key: usize) -> String {
    format!("Im{key}")
}

fn alpha_name(key: usize) -> String {
    format!("Ga{key}")
}

/// Draws a placed picture: its unit square scaled to the drawn size, rotated about the centre
/// of its bounding box. PDF's y axis points up, so a clockwise turn is a negative angle.
fn draw_picture(
    c: &mut Content,
    o: &PlacedObject,
    page_height: f32,
    pictures: &BTreeMap<usize, (Prepared, f32)>,
) {
    let Some((_, alpha)) = pictures.get(&o.key) else {
        return;
    };
    let b = o.bounds;
    let (cx, cy) = (b.x + b.width / 2.0, page_height - (b.y + b.height / 2.0));
    let t = -o.rotation.to_radians();
    let (sin, cos) = t.sin_cos();
    let (a, bb, cc, d) = (
        o.width * cos,
        o.width * sin,
        -o.height * sin,
        o.height * cos,
    );
    c.save_state();
    if *alpha < 1.0 {
        c.set_parameters(Name(alpha_name(o.key).as_bytes()));
    }
    c.transform([a, bb, cc, d, cx - 0.5 * (a + cc), cy - 0.5 * (bb + d)]);
    c.x_object(Name(picture_name(o.key).as_bytes()));
    c.restore_state();
}

/// Writes a picture as an image XObject (with an alpha mask if it has transparent pixels).
fn write_picture(pdf: &mut Pdf, prepared: &Prepared, image: Ref, mask: Ref) {
    match prepared {
        Prepared::Jpeg {
            data,
            width,
            height,
            gray,
        } => {
            let mut x = pdf.image_xobject(image, data);
            x.filter(Filter::DctDecode);
            x.width(*width as i32);
            x.height(*height as i32);
            if *gray {
                x.color_space().device_gray();
            } else {
                x.color_space().device_rgb();
            }
            x.bits_per_component(8);
        }
        Prepared::Pixels {
            width,
            height,
            rgb,
            alpha,
        } => {
            if let Some(alpha) = alpha {
                let packed = deflate(alpha);
                let mut m = pdf.image_xobject(mask, &packed);
                m.filter(Filter::FlateDecode);
                m.width(*width as i32);
                m.height(*height as i32);
                m.color_space().device_gray();
                m.bits_per_component(8);
            }
            let packed = deflate(rgb);
            let mut x = pdf.image_xobject(image, &packed);
            x.filter(Filter::FlateDecode);
            x.width(*width as i32);
            x.height(*height as i32);
            x.color_space().device_rgb();
            x.bits_per_component(8);
            if alpha.is_some() {
                x.s_mask(mask);
            }
        }
    }
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
