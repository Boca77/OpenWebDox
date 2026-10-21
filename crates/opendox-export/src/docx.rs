//! .docx export: the document's structure and formatting as WordprocessingML, for editing in
//! Word or Google Docs. Word lays the text out again with its own engine, so line and page
//! breaks can differ from the editor's; use PDF for print-exact output.

use std::fmt::Write;

use opendox_layout::PageGeometry;
use opendox_model::{Align, Block, CharFormat, Document, Outline, ParagraphFormat, SlotFormat};

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

/// Writes the document as a .docx file.
pub fn docx(doc: &Document, options: &DocxOptions) -> Result<Vec<u8>, ExportError> {
    let outline = doc.outline();
    let formats = doc.formats(&outline);
    let fonts = Fonts::new(options.default_font);

    let mut body = String::new();
    let mut previous_was_table = false;
    for block in &outline.blocks {
        match block {
            Block::Paragraph(i) => {
                paragraph(&mut body, &outline, &formats, *i, &fonts);
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
                            paragraph(&mut body, &outline, &formats, i, &fonts);
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
        r#"{XML}<w:document xmlns:w="{W}" xmlns:r="{R}"><w:body>{body}<w:sectPr><w:pgSz w:w="{}" w:h="{}"/><w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>"#,
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
    zip.add("word/_rels/document.xml.rels", DOCUMENT_RELS.as_bytes());
    Ok(zip.finish())
}

/// One paragraph (`outline.slots[i]`) as `<w:p>`.
fn paragraph(out: &mut String, outline: &Outline, formats: &[SlotFormat], i: usize, fonts: &Fonts) {
    let text: Vec<char> = outline.slots[i].text.chars().collect();
    let format = formats.get(i).cloned().unwrap_or_default();
    out.push_str("<w:p>");
    paragraph_properties(out, &format.paragraph);
    // Formatted runs cover only non-default stretches; the gaps between are plain.
    let mut at = 0;
    let default = CharFormat::default();
    for run in &format.runs {
        if run.range.start > at {
            text_run(out, &text[at..run.range.start], &default, fonts);
        }
        text_run(out, &text[run.range.clone()], &run.format, fonts);
        at = run.range.end;
    }
    if at < text.len() {
        text_run(out, &text[at..], &default, fonts);
    }
    out.push_str("</w:p>");
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

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;

const DOCUMENT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
