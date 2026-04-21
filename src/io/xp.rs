//! `.xp` file format: gzipped binary, little-endian, column-major cells.
//!
//! ```text
//! version    : i32
//! num_layers : i32
//! for each layer:
//!     width  : i32
//!     height : i32
//!     for x in 0..width:
//!         for y in 0..height:       // column-major
//!             glyph     : i32       // low byte = CP437 index
//!             fg_r/g/b  : u8
//!             bg_r/g/b  : u8
//! ```
//!
//! Background `(255, 0, 255)` is the "transparent" magic sentinel.

use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;

use crate::document::Document;
use crate::layer::Layer;
use crate::palette::Color;
use crate::tile::Tile;

const XP_VERSION: i32 = -1;

pub fn save_to_path(path: &Path, document: &Document) -> Result<()> {
    let file = std::fs::File::create(path).with_context(|| format!("create {}", path.display()))?;
    let mut encoder = GzEncoder::new(file, Compression::default());
    write(&mut encoder, document)?;
    encoder.finish().context("finalize gzip stream")?;
    Ok(())
}

pub fn load_from_path(path: &Path) -> Result<Document> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut decoder = GzDecoder::new(file);
    read(&mut decoder)
}

pub fn write<W: Write>(w: &mut W, document: &Document) -> Result<()> {
    w.write_i32::<LittleEndian>(XP_VERSION)?;
    w.write_i32::<LittleEndian>(document.layers.len() as i32)?;

    let cw = document.width;
    let ch = document.height;
    let blank = Tile::default();
    for layer in &document.layers {
        w.write_i32::<LittleEndian>(cw as i32)?;
        w.write_i32::<LittleEndian>(ch as i32)?;
        // Save is WYSIWYG: bake each layer's display offset into its saved
        // cell positions. Content whose buffer coord maps outside the canvas
        // after the shift is simply not included (the .xp format has no
        // per-layer offset concept). The in-memory layer is unchanged.
        let (dx, dy) = layer.offset;
        for x in 0..cw {
            for y in 0..ch {
                let lx = x as i32 - dx;
                let ly = y as i32 - dy;
                let t = if lx >= 0 && ly >= 0 && lx < cw as i32 && ly < ch as i32 {
                    layer.tiles[(ly as u32 * cw + lx as u32) as usize]
                } else {
                    blank
                };
                w.write_i32::<LittleEndian>(t.glyph as i32)?;
                w.write_u8(t.fg.0[0])?;
                w.write_u8(t.fg.0[1])?;
                w.write_u8(t.fg.0[2])?;
                w.write_u8(t.bg.0[0])?;
                w.write_u8(t.bg.0[1])?;
                w.write_u8(t.bg.0[2])?;
            }
        }
    }
    Ok(())
}

pub fn read<R: Read>(r: &mut R) -> Result<Document> {
    let _version = r.read_i32::<LittleEndian>()?;
    let num_layers = r.read_i32::<LittleEndian>()?;
    if num_layers <= 0 || num_layers > 64 {
        return Err(anyhow!("unreasonable num_layers: {num_layers}"));
    }

    let mut width: u32 = 0;
    let mut height: u32 = 0;
    let mut layers: Vec<Layer> = Vec::with_capacity(num_layers as usize);

    for layer_i in 0..num_layers {
        let w = r.read_i32::<LittleEndian>()?;
        let h = r.read_i32::<LittleEndian>()?;
        if w <= 0 || h <= 0 {
            return Err(anyhow!("invalid layer dimensions: {w}x{h}"));
        }
        let w = w as u32;
        let h = h as u32;

        if layer_i == 0 {
            width = w;
            height = h;
        } else if w != width || h != height {
            return Err(anyhow!(
                "layer {layer_i} has inconsistent size {w}x{h} (expected {width}x{height})"
            ));
        }

        let mut tiles = vec![Tile::default(); (w * h) as usize];
        for x in 0..w {
            for y in 0..h {
                let glyph = r.read_i32::<LittleEndian>()?;
                let fg_r = r.read_u8()?;
                let fg_g = r.read_u8()?;
                let fg_b = r.read_u8()?;
                let bg_r = r.read_u8()?;
                let bg_g = r.read_u8()?;
                let bg_b = r.read_u8()?;
                let idx = (y * w + x) as usize;
                tiles[idx] = Tile {
                    glyph: (glyph & 0xFF) as u8,
                    fg: Color([fg_r, fg_g, fg_b, 255]),
                    bg: Color([bg_r, bg_g, bg_b, 255]),
                };
            }
        }
        let mut layer = Layer::new(format!("Layer {}", layer_i + 1), w, h);
        layer.tiles = tiles;
        layer.full_upload = true;
        layer.dirty_cells.clear();
        layers.push(layer);
    }

    let mut document = Document::new_default();
    document.width = width;
    document.height = height;
    document.layers = layers;
    document.active_layer = 0;
    document.bump_resources();
    Ok(document)
}
