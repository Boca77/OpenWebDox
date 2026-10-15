use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::FontContext;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::TableProvider;
use skrifa::{FontRef, GlyphId, MetadataProvider};

/// Identifies a font file registered in a [`FontLibrary`]. The renderer uses it to find the
/// glyph outlines for a [`crate::GlyphRun`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontId(pub u32);

/// The fonts layout may use. Starts empty: system fonts are never consulted.
pub struct FontLibrary {
    pub(crate) cx: FontContext,
    files: Vec<Blob<u8>>,
    by_blob: HashMap<u64, FontId>,
}

impl Default for FontLibrary {
    fn default() -> Self {
        Self::new()
    }
}

impl FontLibrary {
    pub fn new() -> Self {
        let collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        Self {
            cx: FontContext {
                collection,
                source_cache: SourceCache::default(),
            },
            files: Vec::new(),
            by_blob: HashMap::new(),
        }
    }

    /// Registers a font file (TTF, OTF or a collection) and returns its id. Its family names
    /// become usable in [`crate::TextStyle::font_family`].
    pub fn register(&mut self, data: Vec<u8>) -> FontId {
        let blob = Blob::new(Arc::new(data));
        let id = FontId(self.files.len() as u32);
        self.by_blob.insert(blob.id(), id);
        self.cx.collection.register_fonts(blob.clone(), None);
        self.files.push(blob);
        id
    }

    /// The raw bytes of a registered font file.
    pub fn data(&self, id: FontId) -> Option<&[u8]> {
        self.files.get(id.0 as usize).map(|b| b.data())
    }

    pub(crate) fn id_of(&self, blob: &Blob<u8>) -> Option<FontId> {
        self.by_blob.get(&blob.id()).copied()
    }

    /// The outline of one glyph, for renderers that draw glyphs as vector paths.
    ///
    /// Returns `None` for an unknown font or glyph. Glyphs with no ink (spaces) return an empty
    /// path.
    pub fn glyph_outline(&self, font: FontId, face_index: u32, glyph: u32) -> Option<GlyphOutline> {
        let data = self.data(font)?;
        let font = FontRef::from_index(data, face_index).ok()?;
        let units_per_em = font.head().ok()?.units_per_em();
        let outline = font.outline_glyphs().get(GlyphId::new(glyph))?;
        let mut pen = SvgPen(String::new());
        outline
            .draw(
                DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
                &mut pen,
            )
            .ok()?;
        Some(GlyphOutline {
            units_per_em,
            svg_path: pen.0,
        })
    }
}

/// A glyph outline as SVG path data in font units, with y pointing down (so it can be drawn
/// directly on a canvas). Scale by `font_size / units_per_em` to get points.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphOutline {
    pub units_per_em: u16,
    pub svg_path: String,
}

/// Writes outline commands as SVG path data, flipping y from font (up) to screen (down).
struct SvgPen(String);

impl OutlinePen for SvgPen {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.0, "M{x} {}", -y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.0, "L{x} {}", -y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let _ = write!(self.0, "Q{cx} {} {x} {}", -cy, -y);
    }
    fn curve_to(&mut self, c0x: f32, c0y: f32, c1x: f32, c1y: f32, x: f32, y: f32) {
        let _ = write!(self.0, "C{c0x} {} {c1x} {} {x} {}", -c0y, -c1y, -y);
    }
    fn close(&mut self) {
        self.0.push('Z');
    }
}
