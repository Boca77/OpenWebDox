//! .docx export: the document's structure and formatting as WordprocessingML, for editing in
//! Word or Google Docs. Word lays the text out again with its own engine, so line and page
//! breaks can differ from the editor's; use PDF for print-exact output.
//!
//! Images become Word pictures: in line, or anchored to their paragraph with square
//! ("largest side", as the editor wraps), top-and-bottom, behind-text or in-front-of-text
//! wrapping. Rotation, brightness, contrast, transparency, grayscale and alt text carry over;
//! sepia is approximated with a duotone and negative isn't expressible, so it's left out.

use std::collections::HashMap;
use std::fmt::Write;

use opendox_layout::PageGeometry;
use opendox_model::{
    Align, Block, CharFormat, Document, ImageProps, ImageRef, Outline, ParagraphFormat, Recolor,
    SlotFormat, TextRef, Wrap,
};

use crate::zip::Zip;
use crate::ExportError;

/// Settings for [`docx`].
#[derive(Debug, Clone)]
pub struct DocxOptions<'a> {
    /// Default font as a CSS-style family list, like the editor's: the first family is used
    /// for Latin text, a later "Arabic" family (if any) for complex scripts.
    pub default_font: &'a str,
    /// Default font size in points.
    pub default_size: f32,
    pub page: PageGeometry,
    pub title: Option<&'a str>,
}

/// Table border width in eighths of a point (0.75pt, as in the editor).
const BORDER_EIGHTHS: u32 = 6;
const INK: &str = "1D2129";
/// Cell padding in twips (5pt, as in the editor).
const CELL_PADDING_TWIPS: u32 = 100;
/// Space between wrapped or text-breaking images and text, in EMU (9pt, as in the editor).
const IMAGE_DISTANCE_EMU: i64 = 9 * 12_700;

/// The pictures in the document: file and properties per image id, numbered in first-use
/// order.
struct Media {
    images: HashMap<String, MediaImage>,
    order: Vec<String>,
}

struct MediaImage {
    /// 1-based: `word/media/image{n}.{ext}`, relationship `rIdImg{n}`.
    n: usize,
    ext: &'static str,
    bytes: Vec<u8>,
    props: ImageProps,
}

impl Media {
    fn collect(doc: &Document, formats: &[SlotFormat]) -> Media {
        let mut media = Media {
            images: HashMap::new(),
            order: Vec::new(),
        };
        for r in formats.iter().flat_map(|f| &f.images) {
            if media.images.contains_key(&r.id) {
                continue;
            }
            let (Some(data), Some(props)) = (doc.image_data(&r.id), doc.image_props(&r.id)) else {
                continue;
            };
            let ext = if data.mime == "image/jpeg" {
                "jpeg"
            } else {
                "png"
            };
            media.order.push(r.id.clone());
            media.images.insert(
                r.id.clone(),
                MediaImage {
                    n: media.order.len(),
                    ext,
                    bytes: data.bytes,
                    props,
                },
            );
        }
        media
    }
}

/// Writes the document as a .docx file.
pub fn docx(doc: &Document, options: &DocxOptions) -> Result<Vec<u8>, ExportError> {
    let outline = doc.outline();
    let formats = doc.formats(&outline);
    let fonts = Fonts::new(options.default_font);
    let media = Media::collect(doc, &formats);
    let cx = Cx {
        outline: &outline,
        formats: &formats,
        fonts: &fonts,
        media: &media,
    };

    let mut body = String::new();
    let mut previous_was_table = false;
    for block in &outline.blocks {
        match block {
            Block::Paragraph(i) => {
                paragraph(&mut body, &cx, *i);
                previous_was_table = false;
            }
            Block::Table(t) => {
                // Word joins tables that touch into one; keep them apart as the editor does.
                if previous_was_table {
                    body.push_str("<w:p/>");
                }
                let content = options.page.content_width();
                let col = twips(content / t.columns.max(1) as f32);
                body.push_str("<w:tbl><w:tblPr>");
                let _ = write!(
                    body,
                    r#"<w:tblW w:w="{}" w:type="dxa"/>"#,
                    col * t.columns as u32
                );
                body.push_str("<w:tblBorders>");
                for side in ["top", "left", "bottom", "right", "insideH", "insideV"] {
                    let _ = write!(
                        body,
                        r#"<w:{side} w:val="single" w:sz="{BORDER_EIGHTHS}" w:space="0" w:color="{INK}"/>"#
                    );
                }
                body.push_str(r#"</w:tblBorders><w:tblLayout w:type="fixed"/><w:tblCellMar>"#);
                for side in ["top", "left", "bottom", "right"] {
                    let _ = write!(
                        body,
                        r#"<w:{side} w:w="{CELL_PADDING_TWIPS}" w:type="dxa"/>"#
                    );
                }
                body.push_str("</w:tblCellMar></w:tblPr><w:tblGrid>");
                for _ in 0..t.columns {
                    let _ = write!(body, r#"<w:gridCol w:w="{col}"/>"#);
                }
                body.push_str("</w:tblGrid>");
                for row in &t.cells {
                    body.push_str("<w:tr>");
                    for cell in row {
                        let _ = write!(
                            body,
                            r#"<w:tc><w:tcPr><w:tcW w:w="{col}" w:type="dxa"/></w:tcPr>"#
                        );
                        // Every cell needs at least one paragraph.
                        if cell.is_empty() {
                            body.push_str("<w:p/>");
                        }
                        for i in cell.clone() {
                            paragraph(&mut body, &cx, i);
                        }
                        body.push_str("</w:tc>");
                    }
                    body.push_str("</w:tr>");
                }
                body.push_str("</w:tbl>");
                previous_was_table = true;
            }
        }
    }
    // A document can't end with a table in Word.
    if previous_was_table {
        body.push_str("<w:p/>");
    }

    let g = &options.page;
    let document = format!(
        r#"{XML}<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:wp="{WP}" xmlns:a="{A}" xmlns:pic="{PIC}"><w:body>{body}<w:sectPr><w:pgSz w:w="{}" w:h="{}"/><w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>"#,
        twips(g.width),
        twips(g.height),
        twips(g.margin_top),
        twips(g.margin_right),
        twips(g.margin_bottom),
        twips(g.margin_left),
    );
    let styles = format!(
        r#"{XML}<w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr>{}<w:sz w:val="{size}"/><w:szCs w:val="{size}"/><w:lang w:val="en-US"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before="0" w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style></w:styles>"#,
        fonts.r_fonts(None),
        size = half_points(options.default_size),
    );
    let title = options.title.map(escape).unwrap_or_default();
    let core = format!(
        r#"{XML}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{title}</dc:title><dc:creator>opendox</dc:creator></cp:coreProperties>"#
    );

    let mut zip = Zip::default();
    zip.add("[Content_Types].xml", CONTENT_TYPES.as_bytes());
    zip.add("_rels/.rels", ROOT_RELS.as_bytes());
    zip.add("docProps/core.xml", core.as_bytes());
    zip.add("word/document.xml", document.as_bytes());
    zip.add("word/styles.xml", styles.as_bytes());
    let mut rels = String::from(DOCUMENT_RELS_START);
    for id in &media.order {
        let m = &media.images[id];
        let _ = write!(
            rels,
            r#"<Relationship Id="rIdImg{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image{n}.{ext}"/>"#,
            n = m.n,
            ext = m.ext
        );
    }
    rels.push_str("</Relationships>");
    zip.add("word/_rels/document.xml.rels", rels.as_bytes());
    for id in &media.order {
        let m = &media.images[id];
        zip.add(&format!("word/media/image{}.{}", m.n, m.ext), &m.bytes);
    }
    Ok(zip.finish())
}

/// What writing a paragraph needs.
struct Cx<'a> {
    outline: &'a Outline,
    formats: &'a [SlotFormat],
    fonts: &'a Fonts,
    media: &'a Media,
}

/// One paragraph (`outline.slots[i]`) as `<w:p>`.
fn paragraph(out: &mut String, cx: &Cx, i: usize) {
    let slot = &cx.outline.slots[i];
    let text: Vec<char> = slot.text.chars().collect();
    let format = cx.formats.get(i).cloned().unwrap_or_default();
    let in_cell = matches!(slot.container, TextRef::Cell(_));
    out.push_str("<w:p>");
    paragraph_properties(out, &format.paragraph);
    let piece = |out: &mut String, from: usize, to: usize, f: &CharFormat| {
        // Images split the text: each placeholder becomes a picture.
        let mut at = from;
        for r in format
            .images
            .iter()
            .filter(|r| r.offset >= from && r.offset < to)
        {
            text_run(out, &text[at..r.offset], f, cx.fonts);
            picture(out, cx.media, r, in_cell);
            at = r.offset + 1;
        }
        text_run(out, &text[at..to], f, cx.fonts);
    };
    // Formatted runs cover only non-default stretches; the gaps between are plain.
    let mut at = 0;
    let default = CharFormat::default();
    for run in &format.runs {
        if run.range.start > at {
            piece(out, at, run.range.start, &default);
        }
        piece(out, run.range.start, run.range.end, &run.format);
        at = run.range.end;
    }
    if at < text.len() {
        piece(out, at, text.len(), &default);
    }
    out.push_str("</w:p>");
}

/// An image as a `<w:drawing>` run: in line, or anchored to the paragraph. Images in table
/// cells are always in line, as the editor lays them out.
fn picture(out: &mut String, media: &Media, r: &ImageRef, in_cell: bool) {
    let Some(m) = media.images.get(&r.id) else {
        return;
    };
    let p = &m.props;
    let n = m.n;
    let (w, h) = (emu(p.width), emu(p.height));
    let alt = escape(&p.alt);
    let doc_pr = format!(r#"<wp:docPr id="{n}" name="Picture {n}" descr="{alt}"/>"#);
    let mut effects = String::new();
    match p.recolor {
        Recolor::Grayscale => effects.push_str("<a:grayscl/>"),
        Recolor::Sepia => effects.push_str(
            r#"<a:duotone><a:prstClr val="black"/><a:srgbClr val="D9C3A5"><a:tint val="50000"/><a:satMod val="180000"/></a:srgbClr></a:duotone>"#,
        ),
        Recolor::None | Recolor::Negative => {}
    }
    if p.transparency > 0.0 {
        let amt = ((100.0 - p.transparency) * 1000.0).round() as i64;
        let _ = write!(effects, r#"<a:alphaModFix amt="{amt}"/>"#);
    }
    if p.brightness != 0.0 || p.contrast != 0.0 {
        let _ = write!(
            effects,
            r#"<a:lum bright="{}" contrast="{}"/>"#,
            (p.brightness * 1000.0).round() as i64,
            (p.contrast * 1000.0).round() as i64
        );
    }
    let rot = (p.rotation * 60_000.0).round() as i64;
    let graphic = format!(
        r#"<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri="{PIC}"><pic:pic><pic:nvPicPr><pic:cNvPr id="{n}" name="Picture {n}" descr="{alt}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImg{n}">{effects}</a:blip><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm rot="{rot}"><a:off x="0" y="0"/><a:ext cx="{w}" cy="{h}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>"#
    );
    out.push_str("<w:r><w:drawing>");
    if in_cell || p.wrap == Wrap::Inline {
        // Word sizes an in-line picture's line by its rotated bounds through effectExtent.
        let (fw, fh) = footprint(p);
        let (ex, ey) = (((emu(fw) - w) / 2).max(0), ((emu(fh) - h) / 2).max(0));
        let _ = write!(
            out,
            r#"<wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{w}" cy="{h}"/><wp:effectExtent l="{ex}" t="{ey}" r="{ex}" b="{ey}"/>{doc_pr}{graphic}</wp:inline>"#
        );
    } else {
        // Our offsets place the rotated bounding box; Word places the unrotated frame.
        let (fw, fh) = footprint(p);
        let x = emu(p.x + (fw - p.width) / 2.0);
        let y = if p.wrap == Wrap::Break {
            emu((fh - p.height) / 2.0)
        } else {
            emu(p.y + (fh - p.height) / 2.0)
        };
        let d = IMAGE_DISTANCE_EMU;
        let (behind, dist, wrap) = match p.wrap {
            Wrap::Wrap => (0, d, r#"<wp:wrapSquare wrapText="largest"/>"#),
            Wrap::Break => (0, d, "<wp:wrapTopAndBottom/>"),
            Wrap::Behind => (1, 0, "<wp:wrapNone/>"),
            _ => (0, 0, "<wp:wrapNone/>"),
        };
        let _ = write!(
            out,
            r#"<wp:anchor distT="{dist}" distB="{dist}" distL="{dist}" distR="{dist}" simplePos="0" relativeHeight="{z}" behindDoc="{behind}" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="margin"><wp:posOffset>{x}</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>{y}</wp:posOffset></wp:positionV><wp:extent cx="{w}" cy="{h}"/><wp:effectExtent l="0" t="0" r="0" b="0"/>{wrap}{doc_pr}{graphic}</wp:anchor>"#,
            z = 251_658_240 + n
        );
    }
    out.push_str("</w:drawing></w:r>");
}

/// An image's rotated bounding box, in points.
fn footprint(p: &ImageProps) -> (f32, f32) {
    let r = p.rotation.to_radians();
    let (s, c) = (r.sin().abs(), r.cos().abs());
    (p.width * c + p.height * s, p.width * s + p.height * c)
}

/// Points to English Metric Units (1pt = 12,700 EMU).
fn emu(pt: f32) -> i64 {
    (f64::from(pt) * 12_700.0).round() as i64
}

fn paragraph_properties(out: &mut String, f: &ParagraphFormat) {
    let d = ParagraphFormat::default();
    if *f == d {
        return;
    }
    out.push_str("<w:pPr>");
    let _ = write!(
        out,
        r#"<w:spacing w:before="{}" w:after="{}" w:line="{}" w:lineRule="auto"/>"#,
        twips(f.space_before),
        twips(f.space_after),
        (240.0 * f.line_spacing).round() as u32,
    );
    let jc = match f.align {
        Align::Left => None,
        Align::Center => Some("center"),
        Align::Right => Some("right"),
        Align::Justify => Some("both"),
    };
    if let Some(jc) = jc {
        let _ = write!(out, r#"<w:jc w:val="{jc}"/>"#);
    }
    out.push_str("</w:pPr>");
}

fn text_run(out: &mut String, text: &[char], f: &CharFormat, fonts: &Fonts) {
    if text.is_empty() {
        return;
    }
    out.push_str("<w:r>");
    if *f != CharFormat::default() {
        out.push_str("<w:rPr>");
        if f.font.is_some() {
            out.push_str(&fonts.r_fonts(f.font.as_deref()));
        }
        if f.bold {
            out.push_str("<w:b/><w:bCs/>");
        }
        if f.italic {
            out.push_str("<w:i/><w:iCs/>");
        }
        if let Some(size) = f.size {
            let hp = half_points(size);
            let _ = write!(out, r#"<w:sz w:val="{hp}"/><w:szCs w:val="{hp}"/>"#);
        }
        out.push_str("</w:rPr>");
    }
    // Tabs are elements of their own in WordprocessingML.
    let s: String = text.iter().collect();
    for (k, piece) in s.split('\t').enumerate() {
        if k > 0 {
            out.push_str("<w:tab/>");
        }
        if !piece.is_empty() {
            let _ = write!(out, r#"<w:t xml:space="preserve">{}</w:t>"#, escape(piece));
        }
    }
    out.push_str("</w:r>");
}

/// The editor's default family list, split into Word's font slots.
struct Fonts {
    latin: String,
    complex: String,
}

impl Fonts {
    fn new(list: &str) -> Self {
        let families: Vec<&str> = list
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let latin = families.first().copied().unwrap_or("Noto Sans").to_owned();
        let complex = families
            .iter()
            .find(|f| f.contains("Arabic"))
            .copied()
            .unwrap_or(&latin)
            .to_owned();
        Self { latin, complex }
    }

    /// `<w:rFonts>` with `family` (or the default) for Latin and East Asian text.
    fn r_fonts(&self, family: Option<&str>) -> String {
        let f = escape(family.unwrap_or(&self.latin));
        format!(
            r#"<w:rFonts w:ascii="{f}" w:hAnsi="{f}" w:eastAsia="{f}" w:cs="{}"/>"#,
            escape(&self.complex)
        )
    }
}

/// Points to twentieths of a point.
fn twips(pt: f32) -> u32 {
    (pt * 20.0).round().max(0.0) as u32
}

fn half_points(pt: f32) -> u32 {
    (pt * 2.0).round().max(1.0) as u32
}

/// Escapes text for XML, dropping characters XML 1.0 can't hold at all.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

const XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;

/// The document's relationships up to the images, which follow, then `</Relationships>`.
const DOCUMENT_RELS_START: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;
