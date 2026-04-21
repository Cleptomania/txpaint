use crate::font::FontAtlas;
use crate::glyph_palette::GlyphPalette;
use crate::layer::Layer;
use crate::palette::{Color, Palette};
use crate::tools::{PencilMode, RectMode, SelectMode, ToolKind};

pub struct Document {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
    pub active_layer: usize,

    pub palettes: Vec<Palette>,
    pub active_palette: usize,

    pub fg: Color,
    pub bg: Color,
    pub selected_glyph: u8,

    /// One or more custom glyph palettes; only `active_glyph_palette` drives
    /// the picker / keyboard nav. Not serialized to `.xp`.
    pub glyph_palettes: Vec<GlyphPalette>,
    pub active_glyph_palette: usize,

    pub font: FontAtlas,
    pub bundled_font_index: usize,

    pub active_tool: ToolKind,
    /// Sub-mode for the Pencil tool (Simple / Dynamic).
    pub pencil_mode: PencilMode,
    /// Sub-mode for the Select tool (Cell / Rect / Oval). Preserved across
    /// tool switches so returning to Select uses the last-chosen shape.
    pub select_mode: SelectMode,
    /// Sub-mode for the Rectangle tool (Outline / Fill).
    pub rect_mode: RectMode,

    /// Current selection as a per-cell mask. Cleared to `None` when no cells
    /// are selected (so callers can use `is_some` for "active selection").
    pub selection: Option<SelectionMask>,

    /// Generation counter; the renderer compares this against its cached copy to
    /// decide when to rebuild GPU resources (e.g. on resize or font swap).
    pub resources_generation: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CellRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl CellRect {
    pub fn from_corners(ax: u32, ay: u32, bx: u32, by: u32) -> Self {
        let x0 = ax.min(bx);
        let y0 = ay.min(by);
        let x1 = ax.max(bx);
        let y1 = ay.max(by);
        Self {
            x: x0,
            y: y0,
            w: x1 - x0 + 1,
            h: y1 - y0 + 1,
        }
    }

    pub fn clamped(&self, cw: u32, ch: u32) -> Self {
        let x = self.x.min(cw.saturating_sub(1));
        let y = self.y.min(ch.saturating_sub(1));
        let w = self.w.min(cw.saturating_sub(x));
        let h = self.h.min(ch.saturating_sub(y));
        Self { x, y, w, h }
    }
}

/// Per-cell selection bitmap.
#[derive(Clone, Debug)]
pub struct SelectionMask {
    pub w: u32,
    pub h: u32,
    cells: Vec<bool>,
}

impl SelectionMask {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            cells: vec![false; (w as usize) * (h as usize)],
        }
    }

    pub fn from_rect(w: u32, h: u32, rect: CellRect) -> Option<Self> {
        let mut m = Self::new(w, h);
        m.add_rect(rect);
        if m.is_empty() { None } else { Some(m) }
    }

    #[inline]
    fn idx(&self, x: u32, y: u32) -> usize {
        (y as usize) * (self.w as usize) + (x as usize)
    }

    pub fn contains(&self, x: u32, y: u32) -> bool {
        if x >= self.w || y >= self.h {
            return false;
        }
        self.cells[self.idx(x, y)]
    }

    pub fn set(&mut self, x: u32, y: u32, v: bool) {
        if x >= self.w || y >= self.h {
            return;
        }
        let i = self.idx(x, y);
        self.cells[i] = v;
    }

    pub fn add_rect(&mut self, rect: CellRect) {
        let r = rect.clamped(self.w, self.h);
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                self.set(x, y, true);
            }
        }
    }

    pub fn subtract_rect(&mut self, rect: CellRect) {
        let r = rect.clamped(self.w, self.h);
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                self.set(x, y, false);
            }
        }
    }

    pub fn from_oval(w: u32, h: u32, bounds: CellRect) -> Option<Self> {
        let mut m = Self::new(w, h);
        m.add_oval(bounds);
        if m.is_empty() { None } else { Some(m) }
    }

    pub fn add_oval(&mut self, bounds: CellRect) {
        for (x, y) in oval_cells(bounds.clamped(self.w, self.h)) {
            self.set(x, y, true);
        }
    }

    pub fn subtract_oval(&mut self, bounds: CellRect) {
        for (x, y) in oval_cells(bounds.clamped(self.w, self.h)) {
            self.set(x, y, false);
        }
    }

    pub fn fill_all(&mut self) {
        for c in &mut self.cells {
            *c = true;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|c| !c)
    }

    pub fn iter_cells(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        let w = self.w;
        self.cells.iter().enumerate().filter_map(move |(i, &on)| {
            if on {
                Some(((i as u32) % w, (i as u32) / w))
            } else {
                None
            }
        })
    }
}

impl Document {
    pub fn new_default() -> Self {
        Self::new_with_size(80, 25)
    }

    pub fn new_with_size(width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let font =
            FontAtlas::from_bundled(1).expect("bundled cp437_10x10 font must decode cleanly");
        let layers = vec![Layer::new("Background", width, height)];

        Self {
            width,
            height,
            layers,
            active_layer: 0,
            palettes: vec![Palette::default_dos()],
            active_palette: 0,
            fg: Color::rgb(255, 255, 255),
            bg: Color::rgb(0, 0, 0),
            selected_glyph: b'A',
            glyph_palettes: vec![GlyphPalette::default()],
            active_glyph_palette: 0,
            font,
            bundled_font_index: 1,
            active_tool: ToolKind::Pencil,
            pencil_mode: PencilMode::Simple,
            select_mode: SelectMode::Rect,
            rect_mode: RectMode::Outline,
            selection: None,
            resources_generation: 1,
        }
    }

    pub fn active_layer_mut(&mut self) -> &mut Layer {
        &mut self.layers[self.active_layer]
    }

    pub fn active_glyph_palette(&self) -> &GlyphPalette {
        &self.glyph_palettes[self.active_glyph_palette]
    }

    pub fn active_glyph_palette_mut(&mut self) -> &mut GlyphPalette {
        &mut self.glyph_palettes[self.active_glyph_palette]
    }

    /// Bump the generation counter and mark every layer as needing a full
    /// GPU re-upload. Call this after swapping fonts or resizing the canvas.
    pub fn bump_resources(&mut self) {
        self.resources_generation += 1;
        for layer in &mut self.layers {
            layer.full_upload = true;
        }
    }
}

/// Iterate the cells of the ellipse inscribed in `bounds` (cell centers
/// tested against the continuous ellipse equation). Assumes `bounds` is
/// already clamped to a valid region — empty rects yield nothing.
pub fn oval_cells(bounds: CellRect) -> impl Iterator<Item = (u32, u32)> {
    let x0 = bounds.x;
    let y0 = bounds.y;
    let w = bounds.w;
    let h = bounds.h;
    let rx = w as f32 * 0.5;
    let ry = h as f32 * 0.5;
    let cx = x0 as f32 + rx;
    let cy = y0 as f32 + ry;
    // rx or ry can be zero only when bounds is empty; guard against div-by-0.
    let rx2 = (rx * rx).max(f32::EPSILON);
    let ry2 = (ry * ry).max(f32::EPSILON);
    (0..h).flat_map(move |dy| {
        (0..w).filter_map(move |dx| {
            let px = (x0 + dx) as f32 + 0.5 - cx;
            let py = (y0 + dy) as f32 + 0.5 - cy;
            if (px * px) / rx2 + (py * py) / ry2 <= 1.0 {
                Some((x0 + dx, y0 + dy))
            } else {
                None
            }
        })
    })
}

