//! Pictures for export: decoding PNG and JPEG, and the colour treatment (recolor, brightness,
//! contrast) the editor applies, so exported images look as they do on screen.
//!
//! The treatment is defined here and mirrored exactly by the editor's renderer
//! (`packages/editor/src/images.ts`); change both together.

use std::io::Cursor;

use opendox_model::Recolor;

/// An image to export: its file and how it's shown. Lengths aren't needed here; layout (PDF)
/// and the model (.docx) carry them.
#[derive(Debug, Clone, Copy)]
pub struct ExportImage<'a> {
    /// A PNG or JPEG file.
    pub data: &'a [u8],
    /// `image/png` or `image/jpeg`.
    pub mime: &'a str,
    pub recolor: Recolor,
    /// `-100..=100`.
    pub brightness: f32,
    pub contrast: f32,
    /// `0..=100`: 0 opaque, 100 invisible.
    pub transparency: f32,
    pub alt: &'a str,
}

impl ExportImage<'_> {
    /// Whether the colours are shown unchanged.
    pub fn is_unadjusted(&self) -> bool {
        self.recolor == Recolor::None && self.brightness == 0.0 && self.contrast == 0.0
    }

    /// Opacity, `0..=1`.
    pub fn alpha(&self) -> f32 {
        (1.0 - self.transparency / 100.0).clamp(0.0, 1.0)
    }
}

/// An image ready to embed in a PDF.
pub(crate) enum Prepared {
    /// A JPEG file embedded as it is (PDF reads JPEG natively).
    Jpeg {
        data: Vec<u8>,
        width: u32,
        height: u32,
        gray: bool,
    },
    /// Decoded 8-bit RGB pixels, and an alpha channel if any pixel isn't opaque.
    Pixels {
        width: u32,
        height: u32,
        rgb: Vec<u8>,
        alpha: Option<Vec<u8>>,
    },
}

/// Decodes (or, for an unadjusted JPEG, just measures) an image and applies its colour
/// treatment. `None` if the file can't be read.
pub(crate) fn prepare(img: &ExportImage) -> Option<Prepared> {
    if img.mime == "image/jpeg" && img.is_unadjusted() {
        if let Some((width, height, components)) = jpeg_size(img.data) {
            if components == 1 || components == 3 {
                return Some(Prepared::Jpeg {
                    data: img.data.to_vec(),
                    width,
                    height,
                    gray: components == 1,
                });
            }
        }
    }
    let (width, height, mut rgba) = decode_rgba(img.data, img.mime)?;
    if !img.is_unadjusted() {
        for px in rgba.as_chunks_mut::<4>().0.iter_mut() {
            let [r, g, b] = adjust(
                [px[0], px[1], px[2]],
                img.recolor,
                img.brightness,
                img.contrast,
            );
            px[0] = r;
            px[1] = g;
            px[2] = b;
        }
    }
    let rgb: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let alpha = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .any(|p| p[3] < 255)
        .then(|| rgba.as_chunks::<4>().0.iter().map(|p| p[3]).collect());
    Some(Prepared::Pixels {
        width,
        height,
        rgb,
        alpha,
    })
}

/// One pixel's colour treatment: recolor first, then brightness and contrast, each channel
/// `v' = (v - 0.5) * (1 + contrast/100) + 0.5 + brightness/200`, clamped.
pub fn adjust(rgb: [u8; 3], recolor: Recolor, brightness: f32, contrast: f32) -> [u8; 3] {
    let [r, g, b] = rgb.map(|v| f32::from(v) / 255.0);
    let [r, g, b] = match recolor {
        Recolor::None => [r, g, b],
        Recolor::Grayscale => {
            let l = 0.299 * r + 0.587 * g + 0.114 * b;
            [l, l, l]
        }
        Recolor::Sepia => [
            0.393 * r + 0.769 * g + 0.189 * b,
            0.349 * r + 0.686 * g + 0.168 * b,
            0.272 * r + 0.534 * g + 0.131 * b,
        ],
        Recolor::Negative => [1.0 - r, 1.0 - g, 1.0 - b],
    };
    let k = 1.0 + contrast / 100.0;
    let shift = brightness / 200.0;
    [r, g, b].map(|v| ((((v - 0.5) * k + 0.5 + shift).clamp(0.0, 1.0)) * 255.0).round() as u8)
}

/// Width, height and colour components of a JPEG, from its frame header.
fn jpeg_size(data: &[u8]) -> Option<(u32, u32, u8)> {
    if data.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut i = 2;
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            return None;
        }
        let marker = data[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([data[i + 2], data[i + 3]]));
        // Start-of-frame markers: C0-CF except C4 (huffman), C8 (reserved), CC (arithmetic).
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let f = data.get(i + 4..i + 10)?;
            let height = u32::from(u16::from_be_bytes([f[1], f[2]]));
            let width = u32::from(u16::from_be_bytes([f[3], f[4]]));
            return Some((width, height, f[5]));
        }
        i += 2 + len;
    }
    None
}

/// Decodes a PNG or JPEG into 8-bit RGBA.
fn decode_rgba(data: &[u8], mime: &str) -> Option<(u32, u32, Vec<u8>)> {
    if mime == "image/jpeg" {
        use zune_core::colorspace::ColorSpace;
        use zune_core::options::DecoderOptions;
        let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
        let mut d = zune_jpeg::JpegDecoder::new_with_options(Cursor::new(data), options);
        let pixels = d.decode().ok()?;
        let info = d.info()?;
        let (w, h) = (u32::from(info.width), u32::from(info.height));
        return (pixels.len() == (w * h * 4) as usize).then_some((w, h, pixels));
    }
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let pixels = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        // Expanded away by `normalize_to_color8`.
        png::ColorType::Indexed => return None,
    };
    Some((info.width, info.height, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treatment_matches_the_documented_formula() {
        assert_eq!(
            adjust([10, 200, 30], Recolor::None, 0.0, 0.0),
            [10, 200, 30]
        );
        let g = adjust([255, 0, 0], Recolor::Grayscale, 0.0, 0.0);
        assert_eq!(g, [76, 76, 76]);
        assert_eq!(
            adjust([0, 128, 255], Recolor::Negative, 0.0, 0.0),
            [255, 127, 0]
        );
        assert_eq!(
            adjust([100, 100, 100], Recolor::None, 50.0, 0.0),
            [164, 164, 164]
        );
        assert_eq!(
            adjust([200, 50, 128], Recolor::None, 0.0, -100.0),
            [128, 128, 128]
        );
        let s = adjust([100, 100, 100], Recolor::Sepia, 0.0, 0.0);
        assert!(s[0] > s[1] && s[1] > s[2], "{s:?}");
    }

    #[test]
    fn reads_jpeg_frame_size() {
        // SOI, an APP0 segment, then SOF0: 8-bit, 2 x 3 pixels, 3 components.
        let mut j = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        j.extend([0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x03, 0x00, 0x02, 0x03]);
        assert_eq!(jpeg_size(&j), Some((2, 3, 3)));
        assert_eq!(jpeg_size(b"not a jpeg"), None);
    }
}
