//! Images: a placeholder character in a paragraph, plus an image record holding the picture
//! and how it's placed.
//!
//! The paragraph holds one U+FFFC (OBJECT REPLACEMENT CHARACTER) per image, marked with the
//! `image` key whose value is the image id, the same way tables are kept (D9). The character is
//! the image's anchor: an in-line image is drawn in its place, and an image positioned on the
//! page (wrapped, breaking text, behind or in front of text) moves with that paragraph. The
//! record lives in the `images` map under the id:
//!
//! ```text
//! images[id] = {
//!     data: bytes, mime: "image/png" | "image/jpeg",
//!     px_w, px_h,            // pixel size of `data`
//!     width, height,         // drawn size in points (before rotation)
//!     rotation,              // degrees clockwise
//!     wrap,                  // "inline" | "wrap" | "break" | "behind" | "front"
//!     x, y,                  // offset in points (see ImageProps)
//!     recolor,               // "none" | "grayscale" | "sepia" | "negative"
//!     brightness, contrast,  // -100..=100
//!     transparency,          // 0..=100
//!     alt,
//! }
//! ```
//!
//! Every property is its own key, so two people changing different properties of one image at
//! the same time both keep their change; the same property is last-writer-wins. The picture
//! bytes never change after insertion, so replicas can cache decoded images by id. Deleting
//! the placeholder leaves the record, so undoing the delete brings the image back.

use loro::{Container, LoroMap, LoroValue, ValueOrContainer};

use crate::document::Document;
use crate::error::ModelError;
use crate::outline::{Outline, TextRef};
use crate::table::TABLE_PLACEHOLDER;

/// The character standing in for an image in paragraph text (the same one tables use).
pub const IMAGE_PLACEHOLDER: char = TABLE_PLACEHOLDER;

/// Mark key on a placeholder; its value is the image id.
pub(crate) const IMAGE_MARK: &str = "image";

pub(crate) const IMAGES: &str = "images";

/// How an image sits among the text, as in Google Docs' image toolbar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Wrap {
    /// Drawn in the line like a large character; the line grows to fit it.
    #[default]
    Inline,
    /// Positioned on the page; text flows beside it.
    Wrap,
    /// Positioned on the page on lines of its own; text continues above and below it.
    Break,
    /// Positioned on the page under the text, which ignores it.
    Behind,
    /// Positioned on the page over the text, which ignores it.
    Front,
}

impl Wrap {
    fn as_str(self) -> &'static str {
        match self {
            Wrap::Inline => "inline",
            Wrap::Wrap => "wrap",
            Wrap::Break => "break",
            Wrap::Behind => "behind",
            Wrap::Front => "front",
        }
    }

    fn parse(s: &str) -> Wrap {
        match s {
            "wrap" => Wrap::Wrap,
            "break" => Wrap::Break,
            "behind" => Wrap::Behind,
            "front" => Wrap::Front,
            _ => Wrap::Inline,
        }
    }
}

/// A colour treatment applied to the whole picture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Recolor {
    #[default]
    None,
    Grayscale,
    Sepia,
    Negative,
}

impl Recolor {
    fn as_str(self) -> &'static str {
        match self {
            Recolor::None => "none",
            Recolor::Grayscale => "grayscale",
            Recolor::Sepia => "sepia",
            Recolor::Negative => "negative",
        }
    }

    fn parse(s: &str) -> Recolor {
        match s {
            "grayscale" => Recolor::Grayscale,
            "sepia" => Recolor::Sepia,
            "negative" => Recolor::Negative,
            _ => Recolor::None,
        }
    }
}

/// An image's placement and look. Lengths in points.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageProps {
    /// Drawn width and height, before rotation.
    pub width: f32,
    pub height: f32,
    /// Degrees clockwise, `0..360`.
    pub rotation: f32,
    pub wrap: Wrap,
    /// For images placed on the page: offset of the image's box (its rotated bounding box)
    /// from the left page margin and from the top of the anchor paragraph. For `Break`, only
    /// `x` is used. Ignored for in-line images.
    pub x: f32,
    pub y: f32,
    pub recolor: Recolor,
    /// `-100..=100`; 0 leaves the picture unchanged.
    pub brightness: f32,
    pub contrast: f32,
    /// `0..=100`: 0 opaque, 100 invisible.
    pub transparency: f32,
    /// Description for people who can't see the image.
    pub alt: String,
}

impl Default for ImageProps {
    fn default() -> Self {
        Self {
            width: 72.0,
            height: 72.0,
            rotation: 0.0,
            wrap: Wrap::Inline,
            x: 0.0,
            y: 0.0,
            recolor: Recolor::None,
            brightness: 0.0,
            contrast: 0.0,
            transparency: 0.0,
            alt: String::new(),
        }
    }
}

/// One change to an image's properties.
#[derive(Debug, Clone, PartialEq)]
pub enum ImageChange {
    /// Width and height in points; both are kept at least 1pt.
    Size(f32, f32),
    /// Degrees clockwise; normalised into `0..360`.
    Rotation(f32),
    Wrap(Wrap),
    /// Offset in points (see [`ImageProps::x`]).
    Offset(f32, f32),
    Recolor(Recolor),
    Brightness(f32),
    Contrast(f32),
    Transparency(f32),
    Alt(String),
}

/// The picture of an image: encoded bytes, their type and pixel size.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageData {
    pub bytes: Vec<u8>,
    /// `image/png` or `image/jpeg`.
    pub mime: String,
    pub px_width: u32,
    pub px_height: u32,
}

/// An image anchored in a paragraph: char offset of its placeholder within the paragraph
/// ([`crate::Slot`] text) and its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    pub offset: usize,
    pub id: String,
}

const DATA: &str = "data";
const MIME: &str = "mime";
const PX_W: &str = "px_w";
const PX_H: &str = "px_h";
const WIDTH: &str = "width";
const HEIGHT: &str = "height";
const ROTATION: &str = "rotation";
const WRAP: &str = "wrap";
const X: &str = "x";
const Y: &str = "y";
const RECOLOR: &str = "recolor";
const BRIGHTNESS: &str = "brightness";
const CONTRAST: &str = "contrast";
const TRANSPARENCY: &str = "transparency";
const ALT: &str = "alt";

/// Images by flat position (see [`Outline`]). `outline` must describe the current state.
impl Document {
    /// Inserts an image at flat position `pos` and returns its id. It starts in line, at
    /// `width` × `height` points.
    pub fn insert_image(
        &self,
        outline: &Outline,
        pos: usize,
        data: &ImageData,
        width: f32,
        height: f32,
    ) -> Result<String, ModelError> {
        let id = self.new_id("i");
        let map = self
            .images_map()
            .insert_container(&id, LoroMap::new())
            .map_err(crdt)?;
        map.insert(DATA, LoroValue::from(data.bytes.clone()))
            .map_err(crdt)?;
        map.insert(MIME, data.mime.as_str()).map_err(crdt)?;
        map.insert(PX_W, i64::from(data.px_width)).map_err(crdt)?;
        map.insert(PX_H, i64::from(data.px_height)).map_err(crdt)?;
        let props = ImageProps {
            width: width.max(1.0),
            height: height.max(1.0),
            ..ImageProps::default()
        };
        write_props(&map, &props)?;
        let loc = outline.locate(outline.snap(pos));
        self.raw_insert_placeholder(&loc.container, loc.offset, &id)?;
        self.doc.commit();
        Ok(id)
    }

    /// An image's placement and look, or `None` if there's no such image.
    pub fn image_props(&self, id: &str) -> Option<ImageProps> {
        let map = self.image(id)?;
        let num = |k: &str, d: f32| get_f32(&map, k).unwrap_or(d);
        let d = ImageProps::default();
        Some(ImageProps {
            width: num(WIDTH, d.width),
            height: num(HEIGHT, d.height),
            rotation: num(ROTATION, d.rotation),
            wrap: get_str(&map, WRAP).map_or(d.wrap, |s| Wrap::parse(&s)),
            x: num(X, d.x),
            y: num(Y, d.y),
            recolor: get_str(&map, RECOLOR).map_or(d.recolor, |s| Recolor::parse(&s)),
            brightness: num(BRIGHTNESS, d.brightness),
            contrast: num(CONTRAST, d.contrast),
            transparency: num(TRANSPARENCY, d.transparency),
            alt: get_str(&map, ALT).unwrap_or_default(),
        })
    }

    /// An image's picture, or `None` if there's no such image.
    pub fn image_data(&self, id: &str) -> Option<ImageData> {
        let map = self.image(id)?;
        let bytes = match map.get(DATA)? {
            ValueOrContainer::Value(LoroValue::Binary(b)) => b.to_vec(),
            _ => return None,
        };
        Some(ImageData {
            bytes,
            mime: get_str(&map, MIME).unwrap_or_default(),
            px_width: get_f32(&map, PX_W).unwrap_or(0.0) as u32,
            px_height: get_f32(&map, PX_H).unwrap_or(0.0) as u32,
        })
    }

    /// Changes one property of an image.
    pub fn set_image(&self, id: &str, change: ImageChange) -> Result<(), ModelError> {
        self.set_image_many(id, vec![change])
    }

    /// Several changes as one edit (one undo step), e.g. a resize from a corner that also
    /// moves the image.
    pub fn set_image_many(&self, id: &str, changes: Vec<ImageChange>) -> Result<(), ModelError> {
        self.raw_set_image(id, changes)?;
        self.doc.commit();
        Ok(())
    }

    /// Flat position of an image's placeholder, or `None` if it isn't in the document.
    pub fn image_position(&self, outline: &Outline, id: &str) -> Option<usize> {
        let formats = self.image_refs(outline);
        outline.slots.iter().zip(&formats).find_map(|(slot, refs)| {
            refs.iter()
                .find(|r| r.id == id)
                .map(|r| slot.flat + r.offset)
        })
    }

    /// Moves an image's placeholder to flat position `pos` (where it's given in the old
    /// outline), optionally changing properties too, as one edit.
    pub fn move_image(
        &self,
        outline: &Outline,
        id: &str,
        pos: usize,
        changes: Vec<ImageChange>,
    ) -> Result<(), ModelError> {
        let from = self
            .image_position(outline, id)
            .ok_or(ModelError::NoSuchImage)?;
        let to = outline.snap(pos);
        if to != from && to != from + 1 {
            let old = outline.locate(from);
            let new = outline.locate(to);
            let text = self.container(&old.container)?;
            text.delete(old.offset, 1).map_err(crdt)?;
            // Removing the old placeholder shifts later positions in the same container.
            let offset = if new.container == old.container && new.offset > old.offset {
                new.offset - 1
            } else {
                new.offset
            };
            self.raw_insert_placeholder(&new.container, offset, id)?;
        }
        self.raw_set_image(id, changes)?;
        self.doc.commit();
        Ok(())
    }

    /// Removes an image from the document. The record stays, so undo can bring it back.
    pub fn delete_image(&self, outline: &Outline, id: &str) -> Result<(), ModelError> {
        let at = self
            .image_position(outline, id)
            .ok_or(ModelError::NoSuchImage)?;
        let loc = outline.locate(at);
        self.container(&loc.container)?
            .delete(loc.offset, 1)
            .map_err(crdt)?;
        self.doc.commit();
        Ok(())
    }

    /// The images anchored in each paragraph of `outline.slots`, in the same order.
    pub fn image_refs(&self, outline: &Outline) -> Vec<Vec<ImageRef>> {
        self.formats(outline)
            .into_iter()
            .map(|f| f.images)
            .collect()
    }

    fn images_map(&self) -> LoroMap {
        self.doc.get_map(IMAGES)
    }

    pub(crate) fn image(&self, id: &str) -> Option<LoroMap> {
        match self.images_map().get(id)? {
            ValueOrContainer::Container(Container::Map(map)) => Some(map),
            _ => None,
        }
    }

    /// Applies changes without committing. Writes only keys whose value changed, so a
    /// concurrent edit of another property survives.
    fn raw_set_image(&self, id: &str, changes: Vec<ImageChange>) -> Result<(), ModelError> {
        let map = self.image(id).ok_or(ModelError::NoSuchImage)?;
        let before = self.image_props(id).ok_or(ModelError::NoSuchImage)?;
        let mut after = before.clone();
        for change in changes {
            apply(&mut after, change);
        }
        for ((k, old), (_, new)) in entries(&before).into_iter().zip(entries(&after)) {
            if old != new {
                map.insert(k, new).map_err(crdt)?;
            }
        }
        Ok(())
    }

    /// Inserts a marked placeholder for image `id` without committing.
    fn raw_insert_placeholder(
        &self,
        container: &TextRef,
        offset: usize,
        id: &str,
    ) -> Result<(), ModelError> {
        let text = self.container(container)?;
        text.insert(offset, &IMAGE_PLACEHOLDER.to_string())
            .map_err(crdt)?;
        text.mark(offset..offset + 1, IMAGE_MARK, id)
            .map_err(crdt)?;
        Ok(())
    }
}

fn apply(props: &mut ImageProps, change: ImageChange) {
    match change {
        ImageChange::Size(w, h) => {
            props.width = w.max(1.0);
            props.height = h.max(1.0);
        }
        ImageChange::Rotation(r) => props.rotation = r.rem_euclid(360.0),
        ImageChange::Wrap(w) => props.wrap = w,
        ImageChange::Offset(x, y) => {
            props.x = x;
            props.y = y;
        }
        ImageChange::Recolor(r) => props.recolor = r,
        ImageChange::Brightness(v) => props.brightness = v.clamp(-100.0, 100.0),
        ImageChange::Contrast(v) => props.contrast = v.clamp(-100.0, 100.0),
        ImageChange::Transparency(v) => props.transparency = v.clamp(0.0, 100.0),
        ImageChange::Alt(s) => props.alt = s,
    }
}

fn entries(p: &ImageProps) -> [(&'static str, LoroValue); 11] {
    let n = |v: f32| LoroValue::from(f64::from(v));
    [
        (WIDTH, n(p.width)),
        (HEIGHT, n(p.height)),
        (ROTATION, n(p.rotation)),
        (WRAP, LoroValue::from(p.wrap.as_str())),
        (X, n(p.x)),
        (Y, n(p.y)),
        (RECOLOR, LoroValue::from(p.recolor.as_str())),
        (BRIGHTNESS, n(p.brightness)),
        (CONTRAST, n(p.contrast)),
        (TRANSPARENCY, n(p.transparency)),
        (ALT, LoroValue::from(p.alt.as_str())),
    ]
}

fn write_props(map: &LoroMap, p: &ImageProps) -> Result<(), ModelError> {
    for (k, v) in entries(p) {
        map.insert(k, v).map_err(crdt)?;
    }
    Ok(())
}

fn get_f32(map: &LoroMap, k: &str) -> Option<f32> {
    match map.get(k)? {
        ValueOrContainer::Value(LoroValue::Double(d)) => Some(d as f32),
        ValueOrContainer::Value(LoroValue::I64(i)) => Some(i as f32),
        _ => None,
    }
}

fn get_str(map: &LoroMap, k: &str) -> Option<String> {
    match map.get(k)? {
        ValueOrContainer::Value(v) => v.as_string().map(|s| s.to_string()),
        ValueOrContainer::Container(_) => None,
    }
}

fn crdt(e: impl std::fmt::Display) -> ModelError {
    ModelError::Crdt(e.to_string())
}
