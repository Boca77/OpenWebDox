use std::collections::HashMap;
use std::sync::Arc;

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::FontContext;

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
}
