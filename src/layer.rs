use std::collections::HashSet;

use crate::tile::Tile;

pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub tiles: Vec<Tile>,
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
}
