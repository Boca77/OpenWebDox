//! .docx export: the document's structure and formatting as WordprocessingML, for editing in
//! Word or Google Docs. Word lays the text out again with its own engine, so line and page
//! breaks can differ from the editor's; use PDF for print-exact output.
//!
//! Images become Word pictures: in line, or anchored to their paragraph with square
//! ("largest side", as the editor wraps), top-and-bottom, behind-text or in-front-of-text
//! wrapping. Rotation, brightness, contrast, transparency, grayscale and alt text carry over;
//! sepia is approximated with a duotone and negative isn't expressible, so it's left out.
//!
//! Underline, strikethrough, colours, highlight (as run shading, which takes any colour; Word's
//! own highlight has 16) and superscript/subscript carry over; links become hyperlinks.
//!
//! Paragraph styles become Word's own (Title, Subtitle, Heading 1–6, with their outline
//! levels), so Word's navigation pane and table of contents find the headings.
//!
//! The header and footer become Word's (`header1.xml`, `footer1.xml`), their page number and
//! page count fields Word's `PAGE` and `NUMPAGES`.
//!
//! Open comments become Word comments (`comments.xml`, with their range marked in the text);
//! replies follow the comment's text as paragraphs of their own. Resolved comments are left
//! out.
//!
//! Lists become Word numbering (`word/numbering.xml`): one definition per list, so separate
//! lists restart their numbers as in the editor. Word has no checklists; their items become
//! bullets drawn as ☐ or ☑.

use std::collections::HashMap;
use std::fmt::Write;

use opendox_layout::PageGeometry;
use opendox_model::{
    list_indent, marker_text, Align, Block, CharFormat, Comment, Document, FieldKind, ImageProps,
    ImageRef, ListFormat, ListKind, Outline, ParagraphFormat, Recolor, Script, SlotFormat,
    StyleName, TextRef, Wrap, MAX_LIST_LEVEL,
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
    let lists = Lists::collect(&formats);
    // Every link target, in first-use order: relationship `rIdLink{n}` (from 1).
    let mut urls: Vec<String> = Vec::new();
    for url in formats
        .iter()
        .flat_map(|f| &f.runs)
        .filter_map(|r| r.format.link.as_ref())
    {
        if !urls.contains(url) {
            urls.push(url.clone());
        }
    }
    // Open comments with some text left, numbered for Word in reading order.
    let comments: Vec<Comment> = doc
        .comments(&outline)
        .into_iter()
        .filter(|c| !c.resolved && c.start < c.end)
        .collect();
    let cx = Cx {
        comments: &comments,
        urls: &urls,
        outline: &outline,
        formats: &formats,
        fonts: &fonts,
        media: &media,
        lists: &lists,
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

    // The header and footer, as parts of their own, referenced from the section.
    let mut margins: Vec<(&str, String)> = Vec::new();
    let mut refs = String::new();
    for (kind, range) in [("header", &outline.header), ("footer", &outline.footer)] {
        if range.is_empty() {
            continue;
        }
        let mut xml = String::new();
        for i in range.clone() {
            paragraph(&mut xml, &cx, i);
        }
        let root = if kind == "header" { "hdr" } else { "ftr" };
        let _ = write!(
            refs,
            r#"<w:{kind}Reference w:type="default" r:id="rId{kind}"/>"#
        );
        margins.push((
            kind,
            format!(
                r#"{XML}<w:{root} xmlns:w="{W}" xmlns:r="{R}" xmlns:wp="{WP}" xmlns:a="{A}" xmlns:pic="{PIC}">{xml}</w:{root}>"#
            ),
        ));
    }

    let g = &options.page;
    let document = format!(
        r#"{XML}<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:wp="{WP}" xmlns:a="{A}" xmlns:pic="{PIC}"><w:body>{body}<w:sectPr>{refs}<w:pgSz w:w="{}" w:h="{}"/><w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>"#,
        twips(g.width),
        twips(g.height),
        twips(g.margin_top),
        twips(g.margin_right),
        twips(g.margin_bottom),
        twips(g.margin_left),
    );
    let styles = format!(
        r#"{XML}<w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr>{}<w:sz w:val="{size}"/><w:szCs w:val="{size}"/><w:lang w:val="en-US"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before="0" w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>{}</w:styles>"#,
        fonts.r_fonts(None),
        style_definitions(),
        size = half_points(options.default_size),
    );
    let title = options.title.map(escape).unwrap_or_default();
    let core = format!(
        r#"{XML}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{title}</dc:title><dc:creator>opendox</dc:creator></cp:coreProperties>"#
    );

    let mut zip = Zip::default();
    let has_comments = !comments.is_empty();
    let mut content_types = if lists.order.is_empty() {
        CONTENT_TYPES.to_owned()
    } else {
        CONTENT_TYPES.replace("</Types>", NUMBERING_TYPE)
    };
    if has_comments {
        content_types = content_types.replace(
            "</Types>",
            r#"<Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/></Types>"#,
        );
    }
    for (kind, _) in &margins {
        content_types = content_types.replace(
            "</Types>",
            &format!(
                r#"<Override PartName="/word/{kind}1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml"/></Types>"#
            ),
        );
    }
    zip.add("[Content_Types].xml", content_types.as_bytes());
    zip.add("_rels/.rels", ROOT_RELS.as_bytes());
    zip.add("docProps/core.xml", core.as_bytes());
    zip.add("word/document.xml", document.as_bytes());
    zip.add("word/styles.xml", styles.as_bytes());
    let mut rels = String::from(DOCUMENT_RELS_START);
    if has_comments {
        zip.add("word/comments.xml", comments_xml(&comments).as_bytes());
        rels.push_str(r#"<Relationship Id="rIdComments" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/>"#);
    }
    for (kind, xml) in &margins {
        zip.add(&format!("word/{kind}1.xml"), xml.as_bytes());
        let _ = write!(
            rels,
            r#"<Relationship Id="rId{kind}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="{kind}1.xml"/>"#
        );
    }
    if !lists.order.is_empty() {
        zip.add("word/numbering.xml", lists.xml(&fonts).as_bytes());
        rels.push_str(r#"<Relationship Id="rIdNum" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>"#);
    }
    for (n, url) in urls.iter().enumerate() {
        let _ = write!(
            rels,
            r#"<Relationship Id="rIdLink{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{}" TargetMode="External"/>"#,
            n + 1,
            escape(url)
        );
    }
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
    /// Comments to mark; a comment's Word id is its index.
    comments: &'a [Comment],
    /// Link targets; a link's relationship is `rIdLink{index + 1}`.
    urls: &'a [String],
    outline: &'a Outline,
    formats: &'a [SlotFormat],
    fonts: &'a Fonts,
    media: &'a Media,
    lists: &'a Lists,
}

/// The document's lists as Word numberings, in first-use order. A checklist's ticked and
/// unticked items need different bullets, so each gets a numbering of its own.
struct Lists {
    /// `(list id, ticked)` → `numId` (from 1).
    ids: HashMap<(u64, bool), u32>,
    order: Vec<ListFormat>,
}

impl Lists {
    fn collect(formats: &[SlotFormat]) -> Lists {
        let mut lists = Lists {
            ids: HashMap::new(),
            order: Vec::new(),
        };
        for l in formats.iter().filter_map(|f| f.paragraph.list) {
            let key = (l.id, l.checked);
            if !lists.ids.contains_key(&key) {
                lists.order.push(l);
                lists.ids.insert(key, lists.order.len() as u32);
            }
        }
        lists
    }

    fn num_id(&self, l: &ListFormat) -> Option<u32> {
        self.ids.get(&(l.id, l.checked)).copied()
    }

    /// `word/numbering.xml`: an abstract numbering per list (all nine levels), then the
    /// numberings pointing at them.
    fn xml(&self, fonts: &Fonts) -> String {
        let mut out = format!(r#"{XML}<w:numbering xmlns:w="{W}">"#);
        for (k, l) in self.order.iter().enumerate() {
            let _ = write!(
                out,
                r#"<w:abstractNum w:abstractNumId="{k}"><w:multiLevelType w:val="hybridMultilevel"/>"#
            );
            for level in 0..=MAX_LIST_LEVEL {
                let at = ListFormat { level, ..*l };
                let (fmt, text) = match l.kind {
                    ListKind::Number => (
                        ["decimal", "lowerLetter", "lowerRoman"][usize::from(level % 3)],
                        format!("%{}.", level + 1),
                    ),
                    ListKind::Bullet | ListKind::Check => ("bullet", marker_text(at, 1)),
                };
                let (start, first) = list_indent(level);
                let _ = write!(
                    out,
                    r#"<w:lvl w:ilvl="{level}"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/><w:lvlText w:val="{}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{}" w:hanging="{}"/></w:pPr>"#,
                    escape(&text),
                    twips(start),
                    twips(-first),
                );
                if fmt == "bullet" {
                    // Body fonts lack ● ○ ■ ☐ ☑.
                    let _ = write!(out, "<w:rPr>{}</w:rPr>", fonts.r_fonts(Some(SYMBOL_FONT)));
                }
                out.push_str("</w:lvl>");
            }
            out.push_str("</w:abstractNum>");
        }
        for k in 0..self.order.len() {
            let _ = write!(
                out,
                r#"<w:num w:numId="{}"><w:abstractNumId w:val="{k}"/></w:num>"#,
                k + 1
            );
        }
        out.push_str("</w:numbering>");
        out
    }
}

/// The font list bullets are drawn in.
const SYMBOL_FONT: &str = "Noto Sans Symbols 2";

/// One paragraph (`outline.slots[i]`) as `<w:p>`.
fn paragraph(out: &mut String, cx: &Cx, i: usize) {
    let slot = &cx.outline.slots[i];
    let text: Vec<char> = slot.text.chars().collect();
    let format = cx.formats.get(i).cloned().unwrap_or_default();
    let in_cell = matches!(slot.container, TextRef::Cell(_));
    out.push_str("<w:p>");
    let num = format
        .paragraph
        .list
        .and_then(|l| Some((cx.lists.num_id(&l)?, l.level)));
    paragraph_properties(out, &format.paragraph, num);
    // Where comments start and end in this paragraph: markers that take no characters.
    let mut marks: Vec<(usize, String)> = Vec::new();
    for (n, c) in cx.comments.iter().enumerate() {
        let (first, last) = (slot.flat, slot.flat_end());
        if (first..=last).contains(&c.start) {
            marks.push((
                c.start - first,
                format!(r#"<w:commentRangeStart w:id="{n}"/>"#),
            ));
        }
        if (first..=last).contains(&c.end) {
            marks.push((
                c.end - first,
                format!(
                    r#"<w:commentRangeEnd w:id="{n}"/><w:r><w:commentReference w:id="{n}"/></w:r>"#
                ),
            ));
        }
    }
    let piece = |out: &mut String, from: usize, to: usize, f: &CharFormat| {
        // Images, fields and comment marks split the text: each placeholder becomes a picture
        // or a field; a mark takes no character.
        let mut objects: Vec<(usize, Obj)> = format
            .images
            .iter()
            .map(|r| (r.offset, Obj::Image(r)))
            .chain(
                format
                    .fields
                    .iter()
                    .map(|fr| (fr.offset, Obj::Field(fr.kind))),
            )
            .chain(marks.iter().map(|(o, xml)| (*o, Obj::Mark(xml))))
            .filter(|(o, _)| *o >= from && *o < to)
            .collect();
        objects.sort_by_key(|(o, _)| *o);
        let mut at = from;
        for (offset, obj) in objects {
            linked_run(out, cx, &text[at..offset], f);
            at = offset;
            match obj {
                Obj::Image(r) => {
                    picture(out, cx.media, r, in_cell);
                    at += 1;
                }
                Obj::Field(kind) => {
                    // Word fills in the number; the text shown until it does is a placeholder.
                    let instr = match kind {
                        FieldKind::PageNumber => "PAGE",
                        FieldKind::PageCount => "NUMPAGES",
                    };
                    let _ = write!(out, r#"<w:fldSimple w:instr=" {instr} ">"#);
                    text_run(out, &['1'], f, cx.fonts);
                    out.push_str("</w:fldSimple>");
                    at += 1;
                }
                Obj::Mark(xml) => out.push_str(xml),
            }
        }
        linked_run(out, cx, &text[at..to], f);
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
    // Marks at the paragraph's very end.
    for (_, xml) in marks.iter().filter(|(o, _)| *o >= text.len()) {
        out.push_str(xml);
    }
    out.push_str("</w:p>");
}

/// What splits a paragraph's text when writing it.
enum Obj<'a> {
    Image(&'a ImageRef),
    Field(FieldKind),
    /// Comment range markup, taking no character.
    Mark(&'a str),
}

/// `word/comments.xml`: each comment's text, then its replies as paragraphs of their own.
fn comments_xml(comments: &[Comment]) -> String {
    let mut out = format!(r#"{XML}<w:comments xmlns:w="{W}">"#);
    for (n, c) in comments.iter().enumerate() {
        let initials: String = c
            .author
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .collect();
        let _ = write!(
            out,
            r#"<w:comment w:id="{n}" w:author="{}" w:date="{}" w:initials="{}">"#,
            escape(&c.author),
            iso_date(c.created),
            escape(&initials)
        );
        let _ = write!(
            out,
            r#"<w:p><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
            escape(&c.text)
        );
        for r in &c.replies {
            let _ = write!(
                out,
                r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">{}: </w:t></w:r><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
                escape(&r.author),
                escape(&r.text)
            );
        }
        out.push_str("</w:comment>");
    }
    out.push_str("</w:comments>");
    out
}

/// Milliseconds since the Unix epoch as an ISO 8601 UTC timestamp (`2026-10-25T13:45:00Z`).
fn iso_date(ms: f64) -> String {
    let secs = if ms.is_finite() && ms > 0.0 {
        (ms / 1000.0) as i64
    } else {
        0
    };
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
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

/// Word's id and name for a paragraph style (Normal has neither: it's the default).
fn word_style(s: StyleName) -> Option<(String, String)> {
    match s {
        StyleName::Normal => None,
        StyleName::Title => Some(("Title".into(), "Title".into())),
        StyleName::Subtitle => Some(("Subtitle".into(), "Subtitle".into())),
        StyleName::Heading(n) => Some((format!("Heading{n}"), format!("heading {n}"))),
    }
}

/// `<w:style>` for every style but Normal: the editor's look for each, and the outline level
/// Word's navigation pane uses.
fn style_definitions() -> String {
    let mut out = String::new();
    for s in StyleName::ALL {
        let Some((id, name)) = word_style(s) else {
            continue;
        };
        let look = s.look();
        let _ = write!(
            out,
            r#"<w:style w:type="paragraph" w:styleId="{id}"><w:name w:val="{name}"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="{}" w:after="{}"/>"#,
            twips(look.space_before),
            twips(look.space_after),
        );
        if let Some(level) = s
            .outline_level()
            .filter(|_| matches!(s, StyleName::Heading(_)))
        {
            let _ = write!(out, r#"<w:outlineLvl w:val="{}"/>"#, level - 1);
        }
        out.push_str("</w:pPr><w:rPr>");
        if look.italic {
            out.push_str("<w:i/><w:iCs/>");
        }
        if let Some(c) = look.color {
            let _ = write!(out, r#"<w:color w:val="{c:06X}"/>"#);
        }
        if let Some(size) = look.size {
            let hp = half_points(size);
            let _ = write!(out, r#"<w:sz w:val="{hp}"/><w:szCs w:val="{hp}"/>"#);
        }
        out.push_str("</w:rPr></w:style>");
    }
    out
}

/// `<w:pPr>`, children in the schema's order. `num` is the list numbering and level.
fn paragraph_properties(out: &mut String, f: &ParagraphFormat, num: Option<(u32, u8)>) {
    let d = ParagraphFormat::default();
    if *f == d {
        return;
    }
    out.push_str("<w:pPr>");
    if let Some((id, _)) = word_style(f.style) {
        let _ = write!(out, r#"<w:pStyle w:val="{id}"/>"#);
    }
    if f.page_break_before {
        out.push_str("<w:pageBreakBefore/>");
    }
    if let Some((id, level)) = num {
        let _ = write!(
            out,
            r#"<w:numPr><w:ilvl w:val="{level}"/><w:numId w:val="{id}"/></w:numPr>"#
        );
    }
    // The style's spacing where the paragraph sets none, as the editor does.
    let look = f.style.look();
    let or = |own: f32, style: f32| if own == 0.0 { style } else { own };
    let _ = write!(
        out,
        r#"<w:spacing w:before="{}" w:after="{}" w:line="{}" w:lineRule="auto"/>"#,
        twips(or(f.space_before, look.space_before)),
        twips(or(f.space_after, look.space_after)),
        (240.0 * f.line_spacing).round() as u32,
    );
    // The list's indent adds to the paragraph's own, as in the editor.
    let (list_start, list_first) = f.list.map_or((0.0, 0.0), |l| list_indent(l.level));
    let start = f.indent_start + list_start;
    let first = f.indent_first + list_first;
    if start != 0.0 || first != 0.0 || f.indent_end != 0.0 {
        let _ = write!(
            out,
            r#"<w:ind w:left="{}" w:right="{}" {}/>"#,
            twips(start),
            twips(f.indent_end),
            if first < 0.0 {
                format!(r#"w:hanging="{}""#, twips(-first))
            } else {
                format!(r#"w:firstLine="{}""#, twips(first))
            }
        );
    }
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

/// A run, inside a `<w:hyperlink>` if it's linked.
fn linked_run(out: &mut String, cx: &Cx, text: &[char], f: &CharFormat) {
    let rel = f
        .link
        .as_ref()
        .and_then(|url| cx.urls.iter().position(|u| u == url));
    match rel {
        Some(n) if !text.is_empty() => {
            let _ = write!(
                out,
                r#"<w:hyperlink r:id="rIdLink{}" w:history="1">"#,
                n + 1
            );
            text_run(out, text, f, cx.fonts);
            out.push_str("</w:hyperlink>");
        }
        _ => text_run(out, text, f, cx.fonts),
    }
}

/// Link text's look unless it has its own colour, as in Google Docs.
const LINK_COLOR: &str = "1155CC";

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
        if f.strike {
            out.push_str("<w:strike/>");
        }
        let color = f
            .color
            .as_deref()
            .map(|c| c.trim_start_matches('#').to_ascii_uppercase())
            .or_else(|| f.link.is_some().then(|| LINK_COLOR.to_owned()));
        if let Some(c) = color {
            let _ = write!(out, r#"<w:color w:val="{}"/>"#, escape(&c));
        }
        if let Some(size) = f.size {
            let hp = half_points(size);
            let _ = write!(out, r#"<w:sz w:val="{hp}"/><w:szCs w:val="{hp}"/>"#);
        }
        if f.underline || f.link.is_some() {
            out.push_str(r#"<w:u w:val="single"/>"#);
        }
        if let Some(h) = &f.highlight {
            let _ = write!(
                out,
                r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#,
                escape(&h.trim_start_matches('#').to_ascii_uppercase())
            );
        }
        match f.script {
            Script::Normal => {}
            Script::Super => out.push_str(r#"<w:vertAlign w:val="superscript"/>"#),
            Script::Sub => out.push_str(r#"<w:vertAlign w:val="subscript"/>"#),
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

const NUMBERING_TYPE: &str = r#"<Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#;

/// The document's relationships up to the images, which follow, then `</Relationships>`.
const DOCUMENT_RELS_START: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;
