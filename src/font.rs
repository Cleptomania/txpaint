use anyhow::{Context, Result, anyhow};
use image::GenericImageView;

pub const BUNDLED_FONTS: &[BundledFont] = &[
    BundledFont {
        name: "cp437_8x8",
        bytes: include_bytes!("../fonts/cp437_8x8.png"),
    },
    BundledFont {
        name: "cp437_10x10",
        bytes: include_bytes!("../fonts/cp437_10x10.png"),
    },
    BundledFont {
        name: "cp437_12x12",
        bytes: include_bytes!("../fonts/cp437_12x12.png"),
    },
];

pub struct BundledFont {
    pub name: &'static str,
    pub bytes: &'static [u8],
}

/// A 16x16-glyph CP437 font atlas.
///
/// `mask` is a single-channel (0..=255) image with dimensions `(cell_w*16, cell_h*16)`.
pub struct FontAtlas {
    pub name: String,
    pub cell_w: u32,
    pub cell_h: u32,
    pub mask: Vec<u8>,
}

impl FontAtlas {
    pub fn atlas_w(&self) -> u32 {
        self.cell_w * 16
    }
    pub fn atlas_h(&self) -> u32 {
        self.cell_h * 16
    }

    /// Decode a PNG assumed to be a 16x16 grid of glyphs. Any non-black pixel is
    /// treated as glyph-foreground; we derive a luminance/alpha-based mask.
    pub fn from_png_bytes(name: impl Into<String>, bytes: &[u8]) -> Result<Self> {
        let img = image::load_from_memory(bytes).context("decode PNG")?;
        let (w, h) = img.dimensions();
        if w % 16 != 0 || h % 16 != 0 {
            return Err(anyhow!(
                "font image {name}: size {w}x{h} is not divisible by 16",
                name = "atlas"
            ));
        }
        let rgba = img.to_rgba8();
        let mut mask = Vec::with_capacity((w * h) as usize);
        for p in rgba.pixels() {
            let [r, g, b, a] = p.0;
            // Prefer alpha when the image has real transparency; otherwise use luminance.
            let m = if a < 255 {
                a
            } else {
                let lum = (r as u32 * 30 + g as u32 * 59 + b as u32 * 11) / 100;
                lum.min(255) as u8
            };
            mask.push(m);
        }
        Ok(Self {
            name: name.into(),
            cell_w: w / 16,
            cell_h: h / 16,
            mask,
        })
    }

    pub fn from_bundled(index: usize) -> Result<Self> {
        let bf = BUNDLED_FONTS
            .get(index)
            .ok_or_else(|| anyhow!("bundled font index out of range"))?;
        Self::from_png_bytes(bf.name, bf.bytes)
    }

    /// UV rect for glyph index `g` within the atlas, as `(u0, v0, u1, v1)`.
    pub fn glyph_uv(&self, g: u8) -> (f32, f32, f32, f32) {
        let gx = (g % 16) as f32;
        let gy = (g / 16) as f32;
        (gx / 16.0, gy / 16.0, (gx + 1.0) / 16.0, (gy + 1.0) / 16.0)
    }
}
