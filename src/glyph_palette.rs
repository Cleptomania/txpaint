use serde::{Deserialize, Serialize};

/// User-defined grid of glyph slots. Each slot either points at a specific
/// CP437 glyph index or is empty. Lets the user arrange a subset of glyphs in
/// whatever layout makes sense for their project (runes, UI pieces, tiles…).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlyphPalette {
    #[serde(default)]
    pub name: String,
    pub w: u32,
    pub h: u32,
    pub slots: Vec<Option<u8>>,
}

impl GlyphPalette {
    pub fn new(name: impl Into<String>, w: u32, h: u32) -> Self {
        let w = w.max(1);
        let h = h.max(1);
        Self {
            name: name.into(),
            w,
            h,
            slots: vec![None; (w as usize) * (h as usize)],
        }
    }

    fn idx(&self, x: u32, y: u32) -> usize {
        (y as usize) * (self.w as usize) + (x as usize)
    }

    pub fn get(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.w || y >= self.h {
            return None;
        }
        self.slots[self.idx(x, y)]
    }

    pub fn set(&mut self, x: u32, y: u32, g: Option<u8>) {
        if x >= self.w || y >= self.h {
            return;
        }
        let i = self.idx(x, y);
        self.slots[i] = g;
    }

    /// Resize preserving overlapping cells from the top-left corner.
    pub fn resize(&mut self, new_w: u32, new_h: u32) {
        let new_w = new_w.max(1);
        let new_h = new_h.max(1);
        if new_w == self.w && new_h == self.h {
            return;
        }
        let mut new_slots = vec![None; (new_w as usize) * (new_h as usize)];
        let copy_w = new_w.min(self.w);
        let copy_h = new_h.min(self.h);
        for y in 0..copy_h {
            for x in 0..copy_w {
                new_slots[(y * new_w + x) as usize] = self.slots[self.idx(x, y)];
            }
        }
        self.w = new_w;
        self.h = new_h;
        self.slots = new_slots;
    }

    pub fn clear(&mut self) {
        for s in &mut self.slots {
            *s = None;
        }
    }
}

impl Default for GlyphPalette {
    fn default() -> Self {
        Self::new("Palette 1", 8, 4)
    }
}
