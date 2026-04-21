use std::collections::HashSet;

use crate::document::{CellRect, Document};
use crate::history::{CellChange, History};
use crate::tile::{TRANSPARENT_BG, Tile};

pub mod shape_families;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Pencil,
    Select,
    Line,
    Rectangle,
}

impl ToolKind {
    pub const ALL: [ToolKind; 4] = [
        ToolKind::Pencil,
        ToolKind::Select,
        ToolKind::Line,
        ToolKind::Rectangle,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Pencil => "Pencil",
            ToolKind::Select => "Select",
            ToolKind::Line => "Line",
            ToolKind::Rectangle => "Rectangle",
        }
    }

    /// Keyboard shortcut character shown in tooltips; also used by the global
    /// hotkey handler in `app.rs`. Keep the two lists in sync.
    pub fn hotkey(self) -> &'static str {
        match self {
            ToolKind::Pencil => "B",
            ToolKind::Select => "M",
            ToolKind::Line => "L",
            ToolKind::Rectangle => "R",
        }
    }
}

/// Sub-mode for the Pencil tool.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PencilMode {
    /// Writes the selected glyph to each visited cell. Default.
    Simple,
    /// If the selected glyph belongs to a Connected family, each written cell
    /// picks the correct connection glyph based on its neighbors, and
    /// existing family-neighbor cells are re-evaluated too. A non-family
    /// glyph degrades to Simple behavior.
    Dynamic,
}

impl PencilMode {
    pub const ALL: [PencilMode; 2] = [PencilMode::Simple, PencilMode::Dynamic];

    pub fn label(self) -> &'static str {
        match self {
            PencilMode::Simple => "Simple",
            PencilMode::Dynamic => "Dynamic",
        }
    }
}

/// Sub-mode for the Rectangle tool.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RectMode {
    /// Draw only the perimeter. If the selected glyph belongs to a shape
    /// family (see `shape_families`) the four corners, horizontal edges, and
    /// vertical edges use the family's slot glyphs; otherwise every perimeter
    /// cell uses the selected glyph directly.
    Outline,
    /// Fill every cell in the rectangle with the selected glyph.
    Fill,
}

impl RectMode {
    pub const ALL: [RectMode; 2] = [RectMode::Outline, RectMode::Fill];

    pub fn label(self) -> &'static str {
        match self {
            RectMode::Outline => "Outline",
            RectMode::Fill => "Fill",
        }
    }
}

/// Sub-mode for the Select tool — picks which shape the drag describes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SelectMode {
    /// Free-form per-cell paint: each cell touched while dragging toggles into
    /// (or out of) the selection mask live.
    Cell,
    /// Drag-rectangle: bounding box between drag start and release.
    Rect,
    /// Drag-inscribed ellipse: bounding box between drag start and release,
    /// with cells inside the inscribed ellipse committed.
    Oval,
}

impl SelectMode {
    pub const ALL: [SelectMode; 3] = [SelectMode::Cell, SelectMode::Rect, SelectMode::Oval];

    pub fn label(self) -> &'static str {
        match self {
            SelectMode::Cell => "Cell",
            SelectMode::Rect => "Rect",
            SelectMode::Oval => "Oval",
        }
    }
}

/// Per-cell pencil apply. In Simple mode this writes the selected glyph with
/// the active fg/bg. In Dynamic mode, if the selected glyph belongs to a
/// Connected family, this cell's glyph and each of its family-neighbor
/// cells' glyphs are re-evaluated so lines/corners/T-junctions auto-connect.
/// Caller brackets the stroke with `begin_stroke` / `end_stroke`.
pub fn apply_pencil_cell(
    document: &mut Document,
    history: &mut History,
    x: u32,
    y: u32,
    from: Option<(u32, u32)>,
    fresh_cells: &HashSet<(u32, u32)>,
) {
    if x >= document.width || y >= document.height {
        return;
    }
    let w = document.width;
    let layer_index = document.active_layer;
    match document.pencil_mode {
        PencilMode::Simple => {
            let new_tile = Tile {
                glyph: document.selected_glyph,
                fg: document.fg,
                bg: document.bg,
            };
            write_cell(document, history, layer_index, w, x, y, new_tile);
        }
        PencilMode::Dynamic => {
            let selected = document.selected_glyph;
            if shape_families::is_connected_glyph(selected) {
                write_dynamic_cell(document, history, x, y, from, fresh_cells);
            } else {
                // Non-box-drawing glyph — dynamic mode degrades to a literal
                // write so the user still gets their selected glyph placed.
                let new_tile = Tile {
                    glyph: selected,
                    fg: document.fg,
                    bg: document.bg,
                };
                write_cell(document, history, layer_index, w, x, y, new_tile);
            }
        }
    }
}

/// Core of the Dynamic pencil. Derives `(x, y)`'s connection pattern from
/// its four neighbors' glyphs, picks the matching CP437 box-drawing glyph,
/// and then re-evaluates each neighbor's glyph by flipping only the slot
/// that faces the just-written cell (preserving the neighbor's other slots
/// so an unrelated existing line segment isn't rewritten).
fn write_dynamic_cell(
    document: &mut Document,
    history: &mut History,
    x: u32,
    y: u32,
    from: Option<(u32, u32)>,
    fresh_cells: &HashSet<(u32, u32)>,
) {
    use shape_families::{ConnectionPattern, LineStyle, Side};

    let w = document.width;
    let layer_index = document.active_layer;
    let selected = document.selected_glyph;
    let stroke_fam = shape_families::stroke_family(selected);

    // Start from the origin cell's existing canonical pattern so drawing
    // through an existing box cell preserves unrelated connection slots,
    // then override slots where a neighbor actively presents a family on
    // that side. (We don't override with `None` — a box neighbor that
    // doesn't face us shouldn't erase an existing slot.)
    let current_glyph = document.layers[layer_index].get(w, x, y).glyph;
    let mut pattern =
        shape_families::glyph_to_pattern(current_glyph).unwrap_or(ConnectionPattern::EMPTY);
    for side in Side::ALL {
        if let Some(facing) = neighbor_facing_if_family(document, x, y, side) {
            if !matches!(facing, LineStyle::None) {
                pattern = pattern.with(side, facing);
            }
        }
    }

    // Stroke force: every cell in a stroke is implicitly connected to the
    // previous cell the pencil visited. Without this, turning direction
    // mid-stroke (e.g. starting horizontal then going down) wouldn't form
    // a corner — the previous cell's glyph has no connection on the side
    // facing us. Also fills in the "new cell under empty canvas adjacent to
    // first stroke cell" case so a 2-cell stroke actually links up.
    if let Some((fx, fy)) = from {
        if let Some(side) = orthogonal_side_toward(x, y, fx, fy) {
            if !matches!(stroke_fam, LineStyle::None) {
                pattern = pattern.with(side, stroke_fam);
            }
        }
    }

    // Resolve glyph: direct lookup, then coerce mismatched opposites to the
    // stroke's family, then fall back to the selected glyph so an isolated
    // click still places the user's pick.
    let glyph = shape_families::pattern_to_glyph(pattern)
        .or_else(|| shape_families::pattern_to_glyph(shape_families::coerce_to_family(pattern, stroke_fam)))
        .unwrap_or(selected);

    let new_tile = Tile {
        glyph,
        fg: document.fg,
        bg: document.bg,
    };
    write_cell(document, history, layer_index, w, x, y, new_tile);

    // For neighbor refresh we advertise the written glyph's canonical
    // pattern. This is what makes cross-family work: writing 179 above an
    // existing 205 lets the 205 neighbor see `bottom=Single` coming at it
    // and upgrade to 207 (╧). The canonical also carries any implied
    // arms of a stub glyph so adjacent pre-existing lines merge in.
    let our_canonical = shape_families::glyph_to_pattern(glyph).unwrap_or(pattern);

    for side in Side::ALL {
        let (nx, ny) = side.step(x, y);
        if nx < 0 || ny < 0 || nx >= document.width as i32 || ny >= document.height as i32 {
            continue;
        }
        let nx = nx as u32;
        let ny = ny as u32;
        let n_glyph = document.layers[layer_index].get(w, nx, ny).glyph;
        let Some(n_pattern) = shape_families::glyph_to_pattern(n_glyph) else {
            continue;
        };
        // Neighbors placed by this stroke re-derive their pattern entirely
        // from their actual neighbors (drops canonical stubs — so a corner
        // comes out as ┐ instead of ┬). Pre-existing neighbors keep their
        // canonical so drawing through them preserves unrelated arms.
        let updated = if fresh_cells.contains(&(nx, ny)) {
            rederive_pattern(document, nx, ny)
        } else {
            let our_facing: LineStyle = our_canonical.get(side);
            n_pattern.with(side.opposite(), our_facing)
        };
        let n_family = shape_families::stroke_family(n_glyph);
        let new_glyph = shape_families::pattern_to_glyph(updated)
            .or_else(|| {
                shape_families::pattern_to_glyph(shape_families::coerce_to_family(
                    updated, n_family,
                ))
            })
            .unwrap_or(n_glyph);
        if new_glyph == n_glyph {
            continue;
        }
        let existing = document.layers[layer_index].get(w, nx, ny);
        let new_tile = Tile {
            glyph: new_glyph,
            fg: existing.fg,
            bg: existing.bg,
        };
        write_cell(document, history, layer_index, w, nx, ny, new_tile);
    }
}

/// Rebuild a cell's connection pattern from scratch using only what each of
/// its four neighbors actually presents — no preserved canonical slots. Used
/// on refresh targets that were placed by the current stroke.
fn rederive_pattern(document: &Document, x: u32, y: u32) -> shape_families::ConnectionPattern {
    use shape_families::{ConnectionPattern, LineStyle, Side};
    let mut pattern = ConnectionPattern::EMPTY;
    for side in Side::ALL {
        if let Some(facing) = neighbor_facing_if_family(document, x, y, side) {
            if !matches!(facing, LineStyle::None) {
                pattern = pattern.with(side, facing);
            }
        }
    }
    pattern
}

/// Which side of `(x, y)` faces `(fx, fy)`? `None` if the two cells are not
/// orthogonally adjacent.
fn orthogonal_side_toward(x: u32, y: u32, fx: u32, fy: u32) -> Option<shape_families::Side> {
    let dx = fx as i32 - x as i32;
    let dy = fy as i32 - y as i32;
    match (dx, dy) {
        (0, -1) => Some(shape_families::Side::Top),
        (1, 0) => Some(shape_families::Side::Right),
        (0, 1) => Some(shape_families::Side::Bottom),
        (-1, 0) => Some(shape_families::Side::Left),
        _ => None,
    }
}

/// What family does `(x, y)`'s neighbor in `side` present back toward `(x, y)`?
/// `None` if the neighbor is out-of-bounds or isn't a box-drawing glyph —
/// which lets the caller distinguish "no family neighbor here" (keep the
/// origin's existing slot) from "family neighbor presents None on this side".
fn neighbor_facing_if_family(
    document: &Document,
    x: u32,
    y: u32,
    side: shape_families::Side,
) -> Option<shape_families::LineStyle> {
    let (nx, ny) = side.step(x, y);
    if nx < 0 || ny < 0 || nx >= document.width as i32 || ny >= document.height as i32 {
        return None;
    }
    let w = document.width;
    let n_glyph = document.layers[document.active_layer]
        .get(w, nx as u32, ny as u32)
        .glyph;
    let n_pattern = shape_families::glyph_to_pattern(n_glyph)?;
    Some(n_pattern.get(side.opposite()))
}

/// Fill the active layer's selected region with the current (glyph, fg, bg).
/// No-op if there is no selection.
pub fn fill_selection(document: &mut Document, history: &mut History) {
    run_on_selection(document, history, |doc| Tile {
        glyph: doc.selected_glyph,
        fg: doc.fg,
        bg: doc.bg,
    });
}

/// Erase the active layer's selected region: glyph 0 with transparent bg.
pub fn erase_selection(document: &mut Document, history: &mut History) {
    run_on_selection(document, history, |doc| Tile {
        glyph: 0,
        fg: doc.fg,
        bg: TRANSPARENT_BG,
    });
}

fn run_on_selection(
    document: &mut Document,
    history: &mut History,
    make_tile: impl Fn(&Document) -> Tile,
) {
    let Some(mask) = document.selection.as_ref() else {
        return;
    };
    // Collect up front so we can hand a `&mut Document` into write_cell without
    // borrowing the selection mask at the same time.
    let cells: Vec<(u32, u32)> = mask.iter_cells().collect();
    if cells.is_empty() {
        return;
    }
    let new_tile = make_tile(document);
    let w = document.width;
    let layer_index = document.active_layer;
    history.begin_stroke();
    for (x, y) in cells {
        write_cell(document, history, layer_index, w, x, y, new_tile);
    }
    history.end_stroke();
}

/// Rasterize a line from (x0, y0) to (x1, y1) using Bresenham's algorithm,
/// returning the cells in draw order (start to end, inclusive).
pub fn bresenham_cells(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut x = x0;
    let mut y = y0;
    let mut out = Vec::new();
    loop {
        out.push((x, y));
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    out
}

/// Enumerate the cells of a rectangle for the given mode. For `Outline`,
/// degenerate shapes (1-wide, 1-tall, 1x1) collapse to the right edge set:
/// a 1x1 is a single cell; a 1xN or Nx1 is all of those cells.
fn rectangle_cells(rect: CellRect, mode: RectMode) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    if rect.w == 0 || rect.h == 0 {
        return out;
    }
    match mode {
        RectMode::Fill => {
            for y in rect.y..rect.y + rect.h {
                for x in rect.x..rect.x + rect.w {
                    out.push((x, y));
                }
            }
        }
        RectMode::Outline => {
            if rect.w == 1 || rect.h == 1 {
                for y in rect.y..rect.y + rect.h {
                    for x in rect.x..rect.x + rect.w {
                        out.push((x, y));
                    }
                }
            } else {
                // Top edge, bottom edge, then the two vertical edges excluding
                // the corners we already covered.
                for x in rect.x..rect.x + rect.w {
                    out.push((x, rect.y));
                    out.push((x, rect.y + rect.h - 1));
                }
                for y in (rect.y + 1)..(rect.y + rect.h - 1) {
                    out.push((rect.x, y));
                    out.push((rect.x + rect.w - 1, y));
                }
            }
        }
    }
    out
}

/// Resolve the (cell, glyph) pairs that a rectangle stroke would write.
/// This is the single source of truth for Rectangle-tool output — both the
/// canvas preview and the commit path use it so their cells + glyphs agree.
pub fn rectangle_cell_glyphs(
    rect: CellRect,
    mode: RectMode,
    selected: u8,
) -> Vec<(u32, u32, u8)> {
    let cells = rectangle_cells(rect, mode);
    let family = match mode {
        RectMode::Outline => shape_families::rect_family_for(selected),
        RectMode::Fill => None,
    };
    let degenerate = rect.w == 1 && rect.h == 1;
    cells
        .into_iter()
        .map(|(x, y)| {
            let glyph = match (mode, family) {
                (RectMode::Fill, _) => selected,
                (RectMode::Outline, None) => selected,
                // 1x1 is too small for a shape; fall back to the literal
                // glyph so a user-selected glyph isn't silently replaced.
                (RectMode::Outline, Some(_)) if degenerate => selected,
                (RectMode::Outline, Some(f)) => slot_glyph(f, rect, x, y),
            };
            (x, y, glyph)
        })
        .collect()
}

/// Commit a rectangle stroke over `rect` using the active (glyph, fg, bg).
/// Caller brackets with `begin_stroke` / `end_stroke` for one-shot undo.
pub fn commit_rectangle(
    document: &mut Document,
    history: &mut History,
    start: (u32, u32),
    end: (u32, u32),
    mode: RectMode,
) {
    let cw = document.width;
    let ch = document.height;
    let rect = CellRect::from_corners(start.0, start.1, end.0, end.1).clamped(cw, ch);
    if rect.w == 0 || rect.h == 0 {
        return;
    }
    let layer_index = document.active_layer;
    let fg = document.fg;
    let bg = document.bg;
    let selected = document.selected_glyph;
    for (x, y, glyph) in rectangle_cell_glyphs(rect, mode, selected) {
        let new_tile = Tile { glyph, fg, bg };
        write_cell(document, history, layer_index, cw, x, y, new_tile);
    }
}

/// Resolve the family slot glyph for cell `(x, y)` on the perimeter of
/// `rect`. Falls back to the horizontal/vertical glyph for degenerate rects.
fn slot_glyph(f: &shape_families::RectFamily, rect: CellRect, x: u32, y: u32) -> u8 {
    if rect.w == 1 {
        return f.v;
    }
    if rect.h == 1 {
        return f.h;
    }
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.w - 1;
    let y1 = rect.y + rect.h - 1;
    match (x, y) {
        (cx, cy) if cx == x0 && cy == y0 => f.tl,
        (cx, cy) if cx == x1 && cy == y0 => f.tr,
        (cx, cy) if cx == x0 && cy == y1 => f.bl,
        (cx, cy) if cx == x1 && cy == y1 => f.br,
        (_, cy) if cy == y0 || cy == y1 => f.h,
        _ => f.v,
    }
}

/// Commit a line stroke between two canvas cells using the active
/// (glyph, fg, bg). Caller is responsible for bracketing with
/// `begin_stroke` / `end_stroke` so the line lands as a single undo step.
pub fn commit_line(
    document: &mut Document,
    history: &mut History,
    start: (u32, u32),
    end: (u32, u32),
) {
    let w = document.width;
    let h = document.height;
    let layer_index = document.active_layer;
    let new_tile = Tile {
        glyph: document.selected_glyph,
        fg: document.fg,
        bg: document.bg,
    };
    for (cx, cy) in bresenham_cells(start.0 as i32, start.1 as i32, end.0 as i32, end.1 as i32) {
        if cx < 0 || cy < 0 || cx >= w as i32 || cy >= h as i32 {
            continue;
        }
        write_cell(document, history, layer_index, w, cx as u32, cy as u32, new_tile);
    }
}

fn write_cell(
    document: &mut Document,
    history: &mut History,
    layer_index: usize,
    w: u32,
    x: u32,
    y: u32,
    new_tile: Tile,
) {
    let Some(layer) = document.layers.get_mut(layer_index) else {
        return;
    };
    let before = layer.get(w, x, y);
    if before == new_tile {
        return;
    }
    layer.set(w, x, y, new_tile);
    history.record(CellChange {
        layer: layer_index,
        x,
        y,
        before,
        after: new_tile,
    });
}
