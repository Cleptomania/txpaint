use std::collections::HashSet;

use crate::document::{CellRect, Document};
use crate::history::{CellChange, Command, History};
use crate::layer::Layer;
use crate::tile::{TRANSPARENT_BG, Tile};

pub mod shape_families;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Pencil,
    Select,
    Line,
    Rectangle,
    Move,
    Crop,
    Resize,
    Text,
}

impl ToolKind {
    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Pencil => "Pencil",
            ToolKind::Select => "Select",
            ToolKind::Line => "Line",
            ToolKind::Rectangle => "Rectangle",
            ToolKind::Move => "Move",
            ToolKind::Crop => "Crop",
            ToolKind::Resize => "Resize",
            ToolKind::Text => "Text",
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
            ToolKind::Move => "V",
            ToolKind::Crop => "C",
            ToolKind::Resize => "N",
            ToolKind::Text => "T",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            ToolKind::Pencil => "Paint one cell at a time. Drag to stroke.",
            ToolKind::Select => {
                "Mark a region of cells to fill, erase, copy, or paste. \
                 Hold Shift to add to the current selection; Ctrl to subtract. \
                 While placing a paste (Ctrl+V), press H to flip the paste \
                 horizontally or J to flip it vertically — box-drawing glyphs \
                 swap to their mirror variant so corners and T-junctions stay \
                 connected."
            }
            ToolKind::Line => "Drag to stroke a straight line of the selected glyph.",
            ToolKind::Rectangle => "Drag to stroke a rectangle of the selected glyph.",
            ToolKind::Move => {
                "Drag the active layer to reposition it. Cells that scroll \
                 off-canvas stay in the layer buffer and return when dragged back."
            }
            ToolKind::Crop => {
                "Drag a rectangle to resize the active layer's buffer to just \
                 that region. Useful for trimming a layer imported larger \
                 than the canvas, or shrinking a layer down to its drawn \
                 extent. Undoable."
            }
            ToolKind::Resize => {
                "Drag a rectangle to set the new canvas extent. The rectangle \
                 can extend past the current canvas into the viewport's empty \
                 margins to grow it, or sit inside the canvas to shrink it. \
                 Layer buffers are untouched; their offsets shift so existing \
                 content stays in the same absolute position. Undoable."
            }
            ToolKind::Text => {
                "Click a cell to place a caret, then type to write text. \
                 Enter starts a new line at the caret's origin x; \
                 Backspace erases; Escape exits text entry."
            }
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

    pub fn tooltip(self) -> &'static str {
        match self {
            PencilMode::Simple => "Stamps the selected glyph on every painted cell.",
            PencilMode::Dynamic => {
                "Box-drawing glyphs auto-connect to adjacent box-drawing cells, \
                 picking corners and T-junctions to match neighbors. \
                 Non-box glyphs behave like Simple."
            }
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

    pub fn tooltip(self) -> &'static str {
        match self {
            RectMode::Outline => {
                "Draw only the perimeter. Box-drawing glyphs pick matching \
                 corners and edges from the shape's family."
            }
            RectMode::Fill => "Fill every cell inside the rectangle with the selected glyph.",
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

    pub fn tooltip(self) -> &'static str {
        match self {
            SelectMode::Cell => {
                "Paint cells freehand — each cell touched toggles in or out \
                 of the selection live."
            }
            SelectMode::Rect => "Drag a bounding rectangle; release to commit the selection.",
            SelectMode::Oval => {
                "Drag a bounding rectangle; the inscribed ellipse becomes \
                 the selection."
            }
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
    let layer_index = document.active_layer;
    if !document.layers[layer_index].in_bounds(x, y) {
        return;
    }
    // All three channel toggles off → nothing to write. Skip dynamic-mode
    // neighbor refresh too, since the center cell wouldn't have changed.
    if !document.pencil_write_glyph
        && !document.pencil_write_fg
        && !document.pencil_write_bg
    {
        return;
    }
    match document.pencil_mode {
        PencilMode::Simple => {
            let existing = document.layers[layer_index].get(x, y);
            let new_tile = pencil_tile(document, existing, document.selected_glyph);
            write_cell(document, history, layer_index, x, y, new_tile);
        }
        PencilMode::Dynamic => {
            let selected = document.selected_glyph;
            // Dynamic glyph derivation only makes sense when we're actually
            // writing the glyph channel — otherwise the cell's glyph stays
            // put, so neighbors have nothing new to connect to.
            if document.pencil_write_glyph && shape_families::is_connected_glyph(selected) {
                write_dynamic_cell(document, history, x, y, from, fresh_cells);
            } else {
                // Non-box glyph or glyph channel disabled: degrade to a
                // per-channel-gated literal write of the center cell.
                let existing = document.layers[layer_index].get(x, y);
                let new_tile = pencil_tile(document, existing, selected);
                write_cell(document, history, layer_index, x, y, new_tile);
            }
        }
    }
}

/// Build the tile the pencil should write, honoring the per-channel toggles
/// on `document`. For each channel whose toggle is off, the existing cell's
/// value is preserved.
fn pencil_tile(document: &Document, existing: Tile, glyph: u8) -> Tile {
    Tile {
        glyph: if document.pencil_write_glyph { glyph } else { existing.glyph },
        fg: if document.pencil_write_fg { document.fg } else { existing.fg },
        bg: if document.pencil_write_bg { document.bg } else { existing.bg },
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

    let layer_index = document.active_layer;
    let selected = document.selected_glyph;
    let stroke_fam = shape_families::stroke_family(selected);

    // Start from the origin cell's existing canonical pattern so drawing
    // through an existing box cell preserves unrelated connection slots,
    // then override slots where a neighbor actively presents a family on
    // that side. (We don't override with `None` — a box neighbor that
    // doesn't face us shouldn't erase an existing slot.)
    let current_glyph = document.layers[layer_index].get(x, y).glyph;
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

    let existing = document.layers[layer_index].get(x, y);
    let new_tile = pencil_tile(document, existing, glyph);
    write_cell(document, history, layer_index, x, y, new_tile);

    // For neighbor refresh we advertise the written glyph's canonical
    // pattern. This is what makes cross-family work: writing 179 above an
    // existing 205 lets the 205 neighbor see `bottom=Single` coming at it
    // and upgrade to 207 (╧). The canonical also carries any implied
    // arms of a stub glyph so adjacent pre-existing lines merge in.
    let our_canonical = shape_families::glyph_to_pattern(glyph).unwrap_or(pattern);

    let (lw, lh) = {
        let layer = &document.layers[layer_index];
        (layer.width, layer.height)
    };
    for side in Side::ALL {
        let (nx, ny) = side.step(x, y);
        if nx < 0 || ny < 0 || nx >= lw as i32 || ny >= lh as i32 {
            continue;
        }
        let nx = nx as u32;
        let ny = ny as u32;
        let n_glyph = document.layers[layer_index].get(nx, ny).glyph;
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
        let existing = document.layers[layer_index].get(nx, ny);
        let new_tile = Tile {
            glyph: new_glyph,
            fg: existing.fg,
            bg: existing.bg,
        };
        write_cell(document, history, layer_index, nx, ny, new_tile);
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

/// Active layer's buffer dims — shorthand for bounds checks below.
fn active_layer_wh(document: &Document) -> (u32, u32) {
    let layer = &document.layers[document.active_layer];
    (layer.width, layer.height)
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
    let (lw, lh) = active_layer_wh(document);
    if nx < 0 || ny < 0 || nx >= lw as i32 || ny >= lh as i32 {
        return None;
    }
    let n_glyph = document.layers[document.active_layer]
        .get(nx as u32, ny as u32)
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
    let layer_index = document.active_layer;
    let (dx, dy) = document.layers[layer_index].offset;
    let (lw, lh) = {
        let layer = &document.layers[layer_index];
        (layer.width, layer.height)
    };
    history.begin_stroke();
    for (cx, cy) in cells {
        // Selection is in canvas space; the active layer may be offset and
        // sized independently of the canvas, so translate each cell to the
        // layer's buffer coordinate and bounds-check against the layer dims.
        let lx = cx as i32 - dx;
        let ly = cy as i32 - dy;
        if lx < 0 || ly < 0 || lx >= lw as i32 || ly >= lh as i32 {
            continue;
        }
        write_cell(document, history, layer_index, lx as u32, ly as u32, new_tile);
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

/// Commit a rectangle stroke over `rect` (canvas coords) using the active
/// (glyph, fg, bg). Caller brackets with `begin_stroke` / `end_stroke` for
/// one-shot undo. Cells whose buffer coord (canvas - active-layer-offset)
/// falls outside the layer buffer are skipped.
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
    let (dx, dy) = document.layers[layer_index].offset;
    let (lw, lh) = {
        let layer = &document.layers[layer_index];
        (layer.width, layer.height)
    };
    let fg = document.fg;
    let bg = document.bg;
    let selected = document.selected_glyph;
    for (cx, cy, glyph) in rectangle_cell_glyphs(rect, mode, selected) {
        let lx = cx as i32 - dx;
        let ly = cy as i32 - dy;
        if lx < 0 || ly < 0 || lx >= lw as i32 || ly >= lh as i32 {
            continue;
        }
        let new_tile = Tile { glyph, fg, bg };
        write_cell(document, history, layer_index, lx as u32, ly as u32, new_tile);
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

/// Write a single cell on the active layer with `glyph` and the active
/// fg/bg. Used by the Text tool — the typed character determines the glyph,
/// not `document.selected_glyph`. Coordinates are canvas-space; the active
/// layer's offset is applied. Out-of-buffer coords no-op. Caller brackets
/// with `begin_stroke` / `end_stroke`.
pub fn write_text_glyph(
    document: &mut Document,
    history: &mut History,
    x: u32,
    y: u32,
    glyph: u8,
) {
    let tile = Tile {
        glyph,
        fg: document.fg,
        bg: document.bg,
    };
    write_text_tile(document, history, x, y, tile);
}

/// Clear a single cell on the active layer to glyph 0 + transparent bg —
/// used by the Text tool's Backspace. Caller brackets with begin/end stroke.
pub fn erase_text_cell(document: &mut Document, history: &mut History, x: u32, y: u32) {
    let tile = Tile {
        glyph: 0,
        fg: document.fg,
        bg: TRANSPARENT_BG,
    };
    write_text_tile(document, history, x, y, tile);
}

fn write_text_tile(
    document: &mut Document,
    history: &mut History,
    x: u32,
    y: u32,
    tile: Tile,
) {
    let layer_index = document.active_layer;
    let (dx, dy) = document.layers[layer_index].offset;
    let (lw, lh) = {
        let layer = &document.layers[layer_index];
        (layer.width, layer.height)
    };
    let lx = x as i32 - dx;
    let ly = y as i32 - dy;
    if lx < 0 || ly < 0 || lx >= lw as i32 || ly >= lh as i32 {
        return;
    }
    write_cell(document, history, layer_index, lx as u32, ly as u32, tile);
}

/// Commit a line stroke between two canvas cells using the active
/// (glyph, fg, bg). Caller is responsible for bracketing with
/// `begin_stroke` / `end_stroke` so the line lands as a single undo step.
/// Cells whose buffer coord (canvas − active-layer-offset) falls outside
/// the layer buffer are skipped.
pub fn commit_line(
    document: &mut Document,
    history: &mut History,
    start: (u32, u32),
    end: (u32, u32),
) {
    let layer_index = document.active_layer;
    let (dx, dy) = document.layers[layer_index].offset;
    let (lw, lh) = {
        let layer = &document.layers[layer_index];
        (layer.width, layer.height)
    };
    let new_tile = Tile {
        glyph: document.selected_glyph,
        fg: document.fg,
        bg: document.bg,
    };
    for (cx, cy) in bresenham_cells(start.0 as i32, start.1 as i32, end.0 as i32, end.1 as i32) {
        let lx = cx - dx;
        let ly = cy - dy;
        if lx < 0 || ly < 0 || lx >= lw as i32 || ly >= lh as i32 {
            continue;
        }
        write_cell(document, history, layer_index, lx as u32, ly as u32, new_tile);
    }
}

fn write_cell(
    document: &mut Document,
    history: &mut History,
    layer_index: usize,
    x: u32,
    y: u32,
    new_tile: Tile,
) {
    let Some(layer) = document.layers.get_mut(layer_index) else {
        return;
    };
    if !layer.in_bounds(x, y) {
        return;
    }
    let before = layer.get(x, y);
    if before == new_tile {
        return;
    }
    layer.set(x, y, new_tile);
    history.record(CellChange {
        layer: layer_index,
        x,
        y,
        before,
        after: new_tile,
    });
}

/// A single non-empty cell captured by Ctrl+C. `dx`/`dy` are offsets from
/// the clipboard's bounding-box top-left, so irregular selections (oval,
/// Cell-mode) round-trip exactly.
#[derive(Clone, Debug)]
pub struct ClipCell {
    pub dx: u32,
    pub dy: u32,
    pub tile: Tile,
}

/// Sparse tile clipboard built from a SelectionMask. `w`/`h` are the
/// bounding-box dimensions; only originally-selected cells appear in `cells`.
#[derive(Clone, Debug)]
pub struct Clipboard {
    pub w: u32,
    pub h: u32,
    pub cells: Vec<ClipCell>,
}

impl Clipboard {
    /// Iterate the clipboard's cells with an optional horizontal/vertical
    /// flip applied. Positions are mirrored within the clipboard bbox, and
    /// box-drawing glyphs get swapped for their flipped variant so lines,
    /// corners, and T-junctions keep their connectivity. Used by both the
    /// paste preview and the commit path so they agree.
    pub fn iter_flipped(
        &self,
        flip_h: bool,
        flip_v: bool,
    ) -> impl Iterator<Item = (u32, u32, Tile)> + '_ {
        let w = self.w;
        let h = self.h;
        self.cells.iter().map(move |cc| {
            let dx = if flip_h && w > 0 { w - 1 - cc.dx } else { cc.dx };
            let dy = if flip_v && h > 0 { h - 1 - cc.dy } else { cc.dy };
            let glyph = shape_families::flip_glyph(cc.tile.glyph, flip_h, flip_v);
            let tile = Tile {
                glyph,
                fg: cc.tile.fg,
                bg: cc.tile.bg,
            };
            (dx, dy, tile)
        })
    }
}

/// Capture the cells under `document.selection` on the active layer into a
/// new `Clipboard`. Returns `None` if there is no active selection.
pub fn copy_selection(document: &Document) -> Option<Clipboard> {
    let mask = document.selection.as_ref()?;
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;
    for (x, y) in mask.iter_cells() {
        any = true;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    if !any {
        return None;
    }
    let layer = document.layers.get(document.active_layer)?;
    let (dx, dy) = layer.offset;
    let mut cells = Vec::new();
    for (x, y) in mask.iter_cells() {
        // Selection is canvas-space; translate each cell to the active
        // layer's buffer before sampling. Cells that miss the layer buffer
        // are skipped so the clipboard only carries real tile data.
        let lx = x as i32 - dx;
        let ly = y as i32 - dy;
        if lx < 0 || ly < 0 || lx >= layer.width as i32 || ly >= layer.height as i32 {
            continue;
        }
        cells.push(ClipCell {
            dx: x - min_x,
            dy: y - min_y,
            tile: layer.get(lx as u32, ly as u32),
        });
    }
    Some(Clipboard {
        w: max_x - min_x + 1,
        h: max_y - min_y + 1,
        cells,
    })
}

/// Commit a paste at `(origin_x, origin_y)` (canvas coords) as the
/// clipboard's top-left. When `new_layer` is true the pasted cells go into
/// a freshly created layer (appended on top, made active, undoable as a
/// single AddLayer command). Otherwise the cells land on the active layer
/// as a normal stroke, translated into that layer's buffer by its offset.
/// Cells falling outside the layer buffer are silently clipped.
pub fn commit_paste(
    document: &mut Document,
    history: &mut History,
    clipboard: &Clipboard,
    origin_x: u32,
    origin_y: u32,
    new_layer: bool,
    flip_h: bool,
    flip_v: bool,
) {
    if clipboard.cells.is_empty() {
        return;
    }
    let cw = document.width;
    let ch = document.height;
    if new_layer {
        // New layers are born with offset (0, 0), so canvas coords land
        // directly in the buffer.
        let name = format!("Layer {}", document.layers.len() + 1);
        let mut layer = Layer::new(name, cw, ch);
        for (cdx, cdy, tile) in clipboard.iter_flipped(flip_h, flip_v) {
            let x = origin_x.saturating_add(cdx);
            let y = origin_y.saturating_add(cdy);
            if !layer.in_bounds(x, y) {
                continue;
            }
            layer.set(x, y, tile);
        }
        let index = document.layers.len();
        document.layers.push(layer.clone());
        document.active_layer = index;
        document.bump_resources();
        history.push(Command::AddLayer { index, layer });
    } else {
        let layer_index = document.active_layer;
        let (dx, dy) = document.layers[layer_index].offset;
        let (lw, lh) = {
            let layer = &document.layers[layer_index];
            (layer.width, layer.height)
        };
        history.begin_stroke();
        for (cdx, cdy, tile) in clipboard.iter_flipped(flip_h, flip_v) {
            let cx = origin_x as i32 + cdx as i32;
            let cy = origin_y as i32 + cdy as i32;
            let lx = cx - dx;
            let ly = cy - dy;
            if lx < 0 || ly < 0 || lx >= lw as i32 || ly >= lh as i32 {
                continue;
            }
            write_cell(document, history, layer_index, lx as u32, ly as u32, tile);
        }
        history.end_stroke();
    }
}

/// Resize the canvas so its new extent matches `rect` (expressed in the
/// current canvas-space; the rectangle's origin can be negative and its
/// corners can lie past the old canvas edges). Layer buffers are untouched:
/// each layer's display offset is shifted by `-(rect.x, rect.y)` so content
/// stays put in absolute coordinates while the canvas origin moves to
/// `rect.top_left`. Undoable via `Command::ResizeCanvas`.
pub fn commit_resize(
    document: &mut Document,
    history: &mut History,
    rect_origin: (i32, i32),
    rect_size: (u32, u32),
) {
    let (rx, ry) = rect_origin;
    let (rw, rh) = rect_size;
    if rw == 0 || rh == 0 {
        return;
    }
    let before = (document.width, document.height);
    let after = (rw, rh);
    let offset_delta = (-rx, -ry);
    if before == after && offset_delta == (0, 0) {
        return;
    }
    document.width = rw;
    document.height = rh;
    for layer in &mut document.layers {
        layer.offset.0 += offset_delta.0;
        layer.offset.1 += offset_delta.1;
    }
    // Clear any selection — its coords are canvas-space and the origin has
    // potentially shifted; the cells it covered are no longer necessarily
    // what the user thinks of as selected.
    document.selection = None;
    document.bump_resources();
    history.push(Command::ResizeCanvas {
        before,
        after,
        offset_delta,
    });
}

/// Crop the active layer's buffer to exactly `rect` (canvas-space). The new
/// layer's `offset` becomes `(rect.x, rect.y)` so the cropped content renders
/// where it did before, and its buffer dims become `(rect.w, rect.h)`. Tiles
/// inside the overlap with the old layer are preserved; everything else is
/// blank. A `ReplaceLayer` command is pushed so the crop is undoable.
///
/// `rect.x`/`rect.y` are signed because a user can drag in canvas-space and
/// the canvas itself doesn't extend below (0, 0) — but the rect *can* because
/// `layer.offset` already does, and the crop rect is effectively a new offset.
/// This accepts a signed origin to stay symmetric with `layer.offset`.
pub fn commit_crop(
    document: &mut Document,
    history: &mut History,
    rect_origin: (i32, i32),
    rect_size: (u32, u32),
) {
    let (rx, ry) = rect_origin;
    let (rw, rh) = rect_size;
    if rw == 0 || rh == 0 {
        return;
    }
    let layer_index = document.active_layer;
    let before = document.layers[layer_index].clone();
    let no_op_same_dims = before.width == rw
        && before.height == rh
        && before.offset == (rx, ry);
    if no_op_same_dims {
        return;
    }
    let mut after = Layer::new(before.name.clone(), rw, rh);
    after.visible = before.visible;
    after.offset = (rx, ry);
    // Sample the old layer at each new-buffer cell's canvas coord, translated
    // through the old offset. Cells with no coverage stay as the default
    // blank tile (transparent bg + glyph 0).
    let (ox, oy) = before.offset;
    for ny in 0..rh {
        for nx in 0..rw {
            let cx = rx + nx as i32;
            let cy = ry + ny as i32;
            let bx = cx - ox;
            let by = cy - oy;
            if bx < 0 || by < 0 || bx >= before.width as i32 || by >= before.height as i32 {
                continue;
            }
            let tile = before.get(bx as u32, by as u32);
            after.set(nx, ny, tile);
        }
    }
    after.full_upload = true;
    after.dirty_cells.clear();
    document.layers[layer_index] = after.clone();
    document.bump_resources();
    history.push(Command::ReplaceLayer {
        index: layer_index,
        before,
        after,
    });
}
