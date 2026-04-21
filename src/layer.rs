use std::collections::HashSet;

use crate::tile::{Tile, TRANSPARENT_BG};

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    /// Buffer dimensions. Independent of the canvas — a layer can be larger or
    /// smaller than `document.width × document.height`. Cells outside the
    /// canvas viewport are still stored here and become visible if the layer
    /// is moved so they fall inside the canvas.
    pub width: u32,
    pub height: u32,
    pub tiles: Vec<Tile>,
    /// Display shift: canvas cell (cx, cy) shows buffer cell
    /// (cx - offset.0, cy - offset.1). Content whose buffer coord falls
    /// outside the canvas viewport is still stored and re-appears when the
    /// layer is moved back.
    pub offset: (i32, i32),
    pub dirty_cells: HashSet<(u32, u32)>,
    /// If true, the renderer should do a full re-upload before drawing this layer.
    pub full_upload: bool,
}

impl Layer {
    pub fn new(name: impl Into<String>, width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let tiles = vec![Tile::default(); (width * height) as usize];
        Self {
            name: name.into(),
            visible: true,
            width,
            height,
            tiles,
            offset: (0, 0),
            dirty_cells: HashSet::new(),
            full_upload: true,
        }
    }

    pub fn idx(&self, x: u32, y: u32) -> usize {
        (y * self.width + x) as usize
    }

    pub fn in_bounds(&self, x: u32, y: u32) -> bool {
        x < self.width && y < self.height
    }

    pub fn get(&self, x: u32, y: u32) -> Tile {
        self.tiles[self.idx(x, y)]
    }

    pub fn set(&mut self, x: u32, y: u32, tile: Tile) {
        let i = self.idx(x, y);
        if self.tiles[i] != tile {
            self.tiles[i] = tile;
            self.dirty_cells.insert((x, y));
        }
    }

    /// Merge `above` into `self` with top-layer precedence. Iterates `above`'s
    /// own buffer, resolves each tile's canvas-space position via
    /// `above.offset`, then translates into `self`'s buffer via `self.offset`.
    /// Tiles of `above` that don't overlap `self`'s buffer are dropped.
    pub fn merge_from_above(&mut self, above: &Layer) {
        for ty in 0..above.height {
            for tx in 0..above.width {
                let top = above.get(tx, ty);
                if is_empty_tile(top) {
                    continue;
                }
                let cx = tx as i32 + above.offset.0;
                let cy = ty as i32 + above.offset.1;
                let bx = cx - self.offset.0;
                let by = cy - self.offset.1;
                if bx < 0 || by < 0 || bx >= self.width as i32 || by >= self.height as i32 {
                    continue;
                }
                let (bx, by) = (bx as u32, by as u32);
                let bot = self.get(bx, by);
                self.set(bx, by, composite_over(top, bot));
            }
        }
    }
}

fn is_empty_tile(t: Tile) -> bool {
    t.bg == TRANSPARENT_BG && t.glyph == 0
}

/// Flatten two tiles to one with `top` taking precedence. A single tile
/// stores only `glyph+fg+bg`, so the per-pixel glyph×glyph overlap case is
/// approximated: when top has a visible glyph on a transparent bg, we keep
/// top's glyph/fg but borrow bottom's bg so the glyph still reads against
/// the layer beneath. Bottom's glyph is lost in that case.
fn composite_over(top: Tile, bot: Tile) -> Tile {
    if top.bg != TRANSPARENT_BG {
        top
    } else if top.glyph == 0 {
        bot
    } else {
        Tile {
            glyph: top.glyph,
            fg: top.fg,
            bg: bot.bg,
        }
    }
}
