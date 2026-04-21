use std::path::Path;

use anyhow::{Context, Result};
use image::GenericImageView;

use crate::layer::Layer;
use crate::palette::Color;
use crate::tile::{TRANSPARENT_BG, Tile};

/// Decode a PNG and convert it to a canvas-sized Layer. Each pixel becomes one
/// cell: alpha selects a shaded glyph (176/177/178/219), RGB becomes the fg,
/// bg is solid black. Alpha==0 pixels use glyph 0 with a transparent (magic
/// magenta) bg so lower layers show through. The image is placed at (0,0) and
/// clipped to the canvas.
pub fn load_as_layer(path: &Path, canvas_w: u32, canvas_h: u32) -> Result<Layer> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let img = image::load_from_memory(&bytes).context("decode PNG")?;
    let (iw, ih) = img.dimensions();
    let rgba = img.to_rgba8();
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported")
        .to_owned();

    let mut layer = Layer::new(name, canvas_w, canvas_h);
    let cw = iw.min(canvas_w);
    let ch = ih.min(canvas_h);
    for y in 0..ch {
        for x in 0..cw {
            let [r, g, b, a] = rgba.get_pixel(x, y).0;
            let tile = if a == 0 {
                Tile {
                    glyph: 0,
                    fg: Color::BLACK,
                    bg: TRANSPARENT_BG,
                }
            } else {
                let n = a as f32 / 255.0;
                let glyph = if n < 0.25 {
                    176
                } else if n < 0.5 {
                    177
                } else if n < 0.75 {
                    178
                } else {
                    219
                };
                Tile {
                    glyph,
                    fg: Color([r, g, b, 255]),
                    bg: Color::BLACK,
                }
            };
            let idx = (y * canvas_w + x) as usize;
            layer.tiles[idx] = tile;
        }
    }
    layer.full_upload = true;
    Ok(layer)
}
