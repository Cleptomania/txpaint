use std::collections::HashSet;

use crate::tile::{Tile, TRANSPARENT_BG};

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub tiles: Vec<Tile>,
    /// Display shift: canvas cell (cx, cy) shows tile at buffer (cx - offset.0,
    /// cy - offset.1). Content whose buffer coordinate falls outside the canvas
    /// is preserved in `tiles` but not rendered, so scrolling it back into view
    /// restores it.
    pub offset: (i32, i32),
    pub dirty_cells: HashSet<(u32, u32)>,
    /// If true, the renderer should do a full re-upload before drawing this layer.
    pub full_upload: bool,
}

impl Layer {
    pub fn new(name: impl Into<String>, width: u32, height: u32) -> Self {
        let tiles = vec![Tile::default(); (width * height) as usize];
        Self {
            name: name.into(),
            visible: true,
            tiles,
            offset: (0, 0),
            dirty_cells: HashSet::new(),
            full_upload: true,
        }
    }

    pub fn idx(&self, width: u32, x: u32, y: u32) -> usize {
        (y * width + x) as usize
    }

    pub fn get(&self, width: u32, x: u32, y: u32) -> Tile {
        self.tiles[self.idx(width, x, y)]
    }

    pub fn set(&mut self, width: u32, x: u32, y: u32, tile: Tile) {
        let i = self.idx(width, x, y);
        if self.tiles[i] != tile {
            self.tiles[i] = tile;
            self.dirty_cells.insert((x, y));
        }
    }

    /// Merge `above` into `self` with top-layer precedence. Iterates `above`'s
    /// buffer, resolves each tile's canvas position via the layers' `offset`
    /// fields, and writes the composited result into `self`'s buffer. Tiles
    /// from `above` that fall outside the canvas viewport or outside `self`'s
    /// buffer range are dropped.
    pub fn merge_from_above(&mut self, above: &Layer, width: u32, height: u32) {
        for ty in 0..height {
            for tx in 0..width {
                let top = above.get(width, tx, ty);
                if is_empty_tile(top) {
                    continue;
                }
                let cx = tx as i32 + above.offset.0;
                let cy = ty as i32 + above.offset.1;
                let bx = cx - self.offset.0;
                let by = cy - self.offset.1;
                if bx < 0 || by < 0 || bx >= width as i32 || by >= height as i32 {
                    continue;
                }
                let (bx, by) = (bx as u32, by as u32);
                let bot = self.get(width, bx, by);
                self.set(width, bx, by, composite_over(top, bot));
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
