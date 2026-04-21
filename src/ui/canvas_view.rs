use std::collections::HashSet;

use egui::{Color32, PointerButton, Sense, Stroke, TextureHandle, TextureOptions, Vec2};

use crate::document::{CellRect, Document, SelectionMask};
use crate::font::FontAtlas;
use crate::history::History;
use crate::renderer::{CanvasCallback, CanvasRenderRequest};
use crate::tools::{self, Clipboard, RectMode, SelectMode, ToolKind};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SelectOp {
    Replace,
    Add,
    Subtract,
}

/// Persistent per-canvas viewport state (pan/zoom) stored alongside UiState.
pub struct CanvasViewState {
    /// Pixel scale factor in atlas-pixels-per-screen-pixel. `0.0` means auto-fit
    /// to the viewport each frame; any other value is the user's locked zoom.
    pub zoom: f32,
    /// Pan offset in points, from the center of the canvas view.
    pub pan: Vec2,
    /// Accumulator for integer-snap zoom (ctrl+shift+scroll). Resets when the
    /// user releases shift or after each integer step.
    pub snap_accum: f32,
    /// State for an in-progress Select-tool drag. For Rect/Oval the selection
    /// is previewed as a shape and committed on mouse-up; for Cell the mask
    /// is mutated live and this just remembers the paint op for the drag.
    pub select_drag: Option<SelectDrag>,
    /// Endpoints of an in-progress Line-tool drag. Committed on mouse-up.
    pub line_drag: Option<LineDrag>,
    /// Corners of an in-progress Rectangle-tool drag. Committed on mouse-up.
    pub rect_drag: Option<RectDrag>,
    /// State for an in-progress Move-tool drag on the active layer.
    pub move_drag: Option<MoveDrag>,
    /// Last cell visited by the Pencil during the current stroke. Used to
    /// Bresenham-interpolate between pointer frames so fast drags don't skip
    /// cells (important for Dynamic mode's connectivity).
    pub pencil_last: Option<(u32, u32)>,
    /// Cells created (not already box-family) by the current Dynamic-pencil
    /// stroke. These are re-derived entirely from their actual neighbors
    /// during refresh (dropping canonical stub arms) so a stroke that turns
    /// yields a true corner instead of a T-junction with stub arms. Cells
    /// NOT in this set were pre-existing and keep their canonical.
    pub pencil_stroke_fresh: HashSet<(u32, u32)>,
    /// Cached egui-side copy of the active font atlas, used to draw ghosted
    /// glyph previews (Rectangle-tool drag preview, etc.). Rebuilt when the
    /// document's `resources_generation` changes (font swap, etc.).
    atlas_texture: Option<TextureHandle>,
    atlas_generation: u64,
    /// Tile clipboard populated by Ctrl+C on a selection. Survives across
    /// pastes so repeated Ctrl+V uses the same data. Survives document
    /// swaps (canvas coordinates are absolute).
    pub clipboard: Option<Clipboard>,
    /// When `Some`, the user is in paste mode: a ghost overlay follows the
    /// mouse, normal tool dispatch is suspended, and a primary click commits
    /// the paste (Shift = into a new layer).
    pub paste_preview: Option<PastePreview>,
    /// When `Some`, an active Text-tool caret is on the canvas. Subsequent
    /// text input writes glyphs at `(x, y)` and advances the caret; Enter
    /// returns to `origin_x` on the next row. Cleared on Escape, tool switch,
    /// or placing a new caret.
    pub text_caret: Option<TextCaret>,
}

/// Ephemeral state for an in-progress Text-tool typing session.
#[derive(Clone, Debug)]
pub struct TextCaret {
    /// Column Enter returns to — the X of the first click that started the
    /// session. Preserved across Enter and Backspace so indented text keeps
    /// its left edge.
    pub origin_x: u32,
    /// Current cursor position in canvas-cell coords. Writes land here,
    /// then x advances by 1 (or wraps via Enter).
    pub x: u32,
    pub y: u32,
    /// End-X of each finished line, in order (oldest first). An entry is
    /// pushed whenever Enter actually advances to a new row, storing the
    /// caret's x at the moment of the newline. Backspace at the left margin
    /// pops the last entry and restores the caret to that (x, y-1) so the
    /// user can rub out back across a newline into the previous line.
    pub line_ends: Vec<u32>,
}

/// Ephemeral state for an in-progress paste placement. `origin` is the
/// canvas-space cell where the clipboard's top-left should land; it tracks
/// hover_cell each frame. `None` when the cursor isn't over the canvas yet.
#[derive(Copy, Clone, Debug)]
pub struct PastePreview {
    pub origin: Option<(u32, u32)>,
}

#[derive(Copy, Clone, Debug)]
pub struct SelectDrag {
    pub start: (u32, u32),
    pub end: (u32, u32),
    pub op: SelectOp,
    pub mode: SelectMode,
}

#[derive(Copy, Clone, Debug)]
pub struct LineDrag {
    pub start: (u32, u32),
    pub end: (u32, u32),
}

#[derive(Copy, Clone, Debug)]
pub struct RectDrag {
    pub start: (u32, u32),
    pub end: (u32, u32),
    pub mode: RectMode,
}

#[derive(Copy, Clone, Debug)]
pub struct MoveDrag {
    /// Layer being moved (captured at drag start so a mid-drag active-layer
    /// change doesn't redirect the move to a different layer).
    pub layer_index: usize,
    /// Layer offset at the start of the drag — used to compute the new
    /// offset from the drag delta and as the `from` of the undo command.
    pub from: (i32, i32),
    /// Cursor position at drag start (in egui points); per-frame offset is
    /// derived from the current cursor position minus this.
    pub initial_pos: egui::Pos2,
}

impl Default for CanvasViewState {
    fn default() -> Self {
        Self {
            zoom: 0.0,
            pan: Vec2::ZERO,
            snap_accum: 1.0,
            select_drag: None,
            line_drag: None,
            rect_drag: None,
            move_drag: None,
            pencil_last: None,
            pencil_stroke_fresh: HashSet::new(),
            atlas_texture: None,
            atlas_generation: 0,
            clipboard: None,
            paste_preview: None,
            text_caret: None,
        }
    }
}

pub fn show(
    ui: &mut egui::Ui,
    document: &mut Document,
    history: &mut History,
    view: &mut CanvasViewState,
) {
    let avail = ui.available_size_before_wrap();
    if avail.x < 16.0 || avail.y < 16.0 {
        return;
    }
    let (rect, response) = ui.allocate_exact_size(avail, Sense::click_and_drag());

    let cell_w = document.font.cell_w.max(1) as f32;
    let cell_h = document.font.cell_h.max(1) as f32;
    let native_w = document.width as f32 * cell_w;
    let native_h = document.height as f32 * cell_h;

    let fit = (rect.width() / native_w)
        .min(rect.height() / native_h)
        .max(0.01);
    let scale_f = if view.zoom <= 0.0 { fit } else { view.zoom };
    let draw_w = native_w * scale_f;
    let draw_h = native_h * scale_f;
    let draw_size = Vec2::new(draw_w, draw_h);
    let center = rect.center() + view.pan;
    let unclipped = egui::Rect::from_center_size(center, draw_size);
    let draw_rect = unclipped.intersect(rect);

    // Pan: middle-click drag. The same drag also works with the hand (space) but
    // keyboard state is more involved; middle-drag is sufficient for M7.
    if response.dragged_by(PointerButton::Middle) {
        view.pan += response.drag_delta();
    }

    // Zoom:
    //   ctrl+scroll       → fractional zoom (continuous, smooth but uneven pixels)
    //   ctrl+shift+scroll → snap to integer scales (one notch = one step)
    // Home resets to auto-fit.
    let (zoom_delta, shift_held) = ui.input(|i| (i.zoom_delta(), i.modifiers.shift));
    if !shift_held {
        view.snap_accum = 1.0;
    }
    if response.hovered() && zoom_delta != 1.0 {
        let new_scale = if shift_held {
            const SNAP_THRESHOLD: f32 = 1.25;
            view.snap_accum *= zoom_delta;
            if view.snap_accum > SNAP_THRESHOLD {
                view.snap_accum = 1.0;
                let next = if scale_f.fract().abs() < 1e-4 {
                    scale_f + 1.0
                } else {
                    scale_f.ceil()
                };
                Some(next.clamp(1.0, 64.0))
            } else if view.snap_accum < 1.0 / SNAP_THRESHOLD {
                view.snap_accum = 1.0;
                let next = if scale_f.fract().abs() < 1e-4 {
                    scale_f - 1.0
                } else {
                    scale_f.floor()
                };
                Some(next.clamp(1.0, 64.0))
            } else {
                None
            }
        } else {
            Some((scale_f * zoom_delta).clamp(0.25, 64.0))
        };
        if let Some(new_scale) = new_scale {
            if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                let scale_ratio = new_scale / scale_f;
                let from_center = p - unclipped.center();
                view.pan = p - rect.center() - from_center * scale_ratio;
            }
            view.zoom = new_scale;
        }
    }

    // Pointer → cell (x, y). Based on the unclipped rect so hits work at edges.
    let cell_pixel_w = cell_w * scale_f;
    let cell_pixel_h = cell_h * scale_f;
    let hover_cell = ui.input(|i| i.pointer.hover_pos()).and_then(|p| {
        if !rect.contains(p) || !unclipped.contains(p) {
            return None;
        }
        let rel = p - unclipped.min;
        let cx = (rel.x / cell_pixel_w).floor() as i32;
        let cy = (rel.y / cell_pixel_h).floor() as i32;
        if cx < 0 || cy < 0 || cx >= document.width as i32 || cy >= document.height as i32 {
            return None;
        }
        Some((cx as u32, cy as u32))
    });

    // Paste mode short-circuits normal tool dispatch. The ghost overlay
    // tracks the mouse; a primary click commits (Shift = into a new layer).
    let primary_down = ui.input(|i| i.pointer.primary_down());
    if view.paste_preview.is_some() {
        if let Some(preview) = view.paste_preview.as_mut() {
            if let Some(cell) = hover_cell {
                preview.origin = Some(cell);
            }
        }
        // Accept either a clean click or a drag-release so the paste commits
        // even if the user wiggled the cursor past egui's drag threshold.
        if response.clicked_by(PointerButton::Primary)
            || response.drag_stopped_by(PointerButton::Primary)
        {
            let shift = ui.input(|i| i.modifiers.shift);
            let origin = view.paste_preview.and_then(|p| p.origin);
            if let (Some(clip), Some((ox, oy))) = (view.clipboard.as_ref(), origin) {
                tools::commit_paste(document, history, clip, ox, oy, shift);
            }
            view.paste_preview = None;
        }
    } else {
    match document.active_tool {
        ToolKind::Select => {
            let modifiers = ui.input(|i| i.modifiers);
            let op_from_mods = || {
                if modifiers.shift {
                    SelectOp::Add
                } else if modifiers.ctrl || modifiers.command {
                    SelectOp::Subtract
                } else {
                    SelectOp::Replace
                }
            };
            let mode = document.select_mode;

            // Drag-start: anchor the drag. For Cell mode, Replace also clears
            // the existing mask immediately so the user sees the old selection
            // vanish the moment they press, and then paints the new one as
            // they drag.
            if response.drag_started_by(PointerButton::Primary) {
                if let Some(cell) = hover_cell {
                    let op = op_from_mods();
                    view.select_drag = Some(SelectDrag {
                        start: cell,
                        end: cell,
                        op,
                        mode,
                    });
                    if mode == SelectMode::Cell {
                        let cw = document.width;
                        let ch = document.height;
                        match op {
                            SelectOp::Replace => {
                                let mut mask = SelectionMask::new(cw, ch);
                                mask.set(cell.0, cell.1, true);
                                document.selection = Some(mask);
                            }
                            SelectOp::Add => {
                                let mask = document
                                    .selection
                                    .get_or_insert_with(|| SelectionMask::new(cw, ch));
                                mask.set(cell.0, cell.1, true);
                            }
                            SelectOp::Subtract => {
                                if let Some(mask) = document.selection.as_mut() {
                                    mask.set(cell.0, cell.1, false);
                                }
                            }
                        }
                    }
                }
            }
            if response.dragged_by(PointerButton::Primary) {
                if let (Some(drag), Some(cell)) = (view.select_drag.as_mut(), hover_cell) {
                    drag.end = cell;
                    if drag.mode == SelectMode::Cell {
                        // Cell mode paints live per frame.
                        match drag.op {
                            SelectOp::Replace | SelectOp::Add => {
                                if let Some(mask) = document.selection.as_mut() {
                                    mask.set(cell.0, cell.1, true);
                                }
                            }
                            SelectOp::Subtract => {
                                if let Some(mask) = document.selection.as_mut() {
                                    mask.set(cell.0, cell.1, false);
                                }
                            }
                        }
                    }
                }
            }
            if response.drag_stopped_by(PointerButton::Primary) {
                if let Some(drag) = view.select_drag.take() {
                    match drag.mode {
                        SelectMode::Cell => {
                            // Live-painted; just tidy an empty mask into None.
                            if document
                                .selection
                                .as_ref()
                                .is_some_and(|m| m.is_empty())
                            {
                                document.selection = None;
                            }
                        }
                        SelectMode::Rect | SelectMode::Oval => {
                            apply_shape_select(document, drag);
                        }
                    }
                }
            }
            // Plain click (no drag) with no modifier:
            //   Rect/Oval → clear the selection (matches the old Rect Select
            //     behavior; click-to-deselect is a common pattern).
            //   Cell     → a bare click with no modifier already acted as a
            //     Replace in the drag_started branch if it fired, but for a
            //     true click-only (no drag threshold crossed) we replace the
            //     mask here with just the clicked cell.
            if response.clicked_by(PointerButton::Primary) && view.select_drag.is_none() {
                let has_mods = modifiers.shift || modifiers.ctrl || modifiers.command;
                match mode {
                    SelectMode::Rect | SelectMode::Oval => {
                        if !has_mods {
                            document.selection = None;
                        }
                    }
                    SelectMode::Cell => {
                        if let Some(cell) = hover_cell {
                            let op = op_from_mods();
                            let cw = document.width;
                            let ch = document.height;
                            match op {
                                SelectOp::Replace => {
                                    let mut mask = SelectionMask::new(cw, ch);
                                    mask.set(cell.0, cell.1, true);
                                    document.selection = Some(mask);
                                }
                                SelectOp::Add => {
                                    let mask = document
                                        .selection
                                        .get_or_insert_with(|| SelectionMask::new(cw, ch));
                                    mask.set(cell.0, cell.1, true);
                                }
                                SelectOp::Subtract => {
                                    if let Some(mask) = document.selection.as_mut() {
                                        mask.set(cell.0, cell.1, false);
                                        if mask.is_empty() {
                                            document.selection = None;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        ToolKind::Line => {
            if response.drag_started_by(PointerButton::Primary)
                || response.clicked_by(PointerButton::Primary)
            {
                if let Some(cell) = hover_cell {
                    view.line_drag = Some(LineDrag {
                        start: cell,
                        end: cell,
                    });
                    history.begin_stroke();
                }
            }
            if response.dragged_by(PointerButton::Primary) {
                if let (Some(drag), Some(cell)) = (view.line_drag.as_mut(), hover_cell) {
                    drag.end = cell;
                }
            }
            if response.drag_stopped_by(PointerButton::Primary)
                || (!primary_down && response.clicked_by(PointerButton::Primary))
            {
                if let Some(drag) = view.line_drag.take() {
                    tools::commit_line(document, history, drag.start, drag.end);
                    history.end_stroke();
                }
            }
        }
        ToolKind::Rectangle => {
            if response.drag_started_by(PointerButton::Primary)
                || response.clicked_by(PointerButton::Primary)
            {
                if let Some(cell) = hover_cell {
                    view.rect_drag = Some(RectDrag {
                        start: cell,
                        end: cell,
                        mode: document.rect_mode,
                    });
                    history.begin_stroke();
                }
            }
            if response.dragged_by(PointerButton::Primary) {
                if let (Some(drag), Some(cell)) = (view.rect_drag.as_mut(), hover_cell) {
                    drag.end = cell;
                }
            }
            if response.drag_stopped_by(PointerButton::Primary)
                || (!primary_down && response.clicked_by(PointerButton::Primary))
            {
                if let Some(drag) = view.rect_drag.take() {
                    tools::commit_rectangle(document, history, drag.start, drag.end, drag.mode);
                    history.end_stroke();
                }
            }
        }
        ToolKind::Pencil => {
            // Pencil operates in layer-buffer coord space. Translate the
            // canvas hover cell by the active layer's offset; if it maps
            // outside the buffer, treat it like no-hover.
            let layer_cell = hover_cell.and_then(|(cx, cy)| {
                let (dx, dy) = document.layers[document.active_layer].offset;
                let lx = cx as i32 - dx;
                let ly = cy as i32 - dy;
                if lx < 0
                    || ly < 0
                    || lx >= document.width as i32
                    || ly >= document.height as i32
                {
                    None
                } else {
                    Some((lx as u32, ly as u32))
                }
            });
            if response.drag_started_by(PointerButton::Primary)
                || response.clicked_by(PointerButton::Primary)
            {
                history.begin_stroke();
                view.pencil_last = None;
                view.pencil_stroke_fresh.clear();
            }
            if (response.dragged_by(PointerButton::Primary) && primary_down)
                || response.clicked_by(PointerButton::Primary)
            {
                if let Some((x, y)) = layer_cell {
                    // Interpolate between frames so a fast drag doesn't skip
                    // cells. Relevant for Dynamic mode (connections rely on
                    // cell adjacency) and nice-to-have for Simple.
                    match view.pencil_last {
                        None => {
                            apply_pencil(document, history, view, x, y, None);
                            view.pencil_last = Some((x, y));
                        }
                        Some((px, py)) if (px, py) == (x, y) => {}
                        Some((px, py)) => {
                            let mut prev = (px, py);
                            for (cx, cy) in
                                tools::bresenham_cells(px as i32, py as i32, x as i32, y as i32)
                            {
                                if cx < 0
                                    || cy < 0
                                    || cx >= document.width as i32
                                    || cy >= document.height as i32
                                {
                                    continue;
                                }
                                // Skip the first cell — it's where we
                                // stopped last frame and is already written.
                                if (cx as u32, cy as u32) == (px, py) {
                                    continue;
                                }
                                let cell = (cx as u32, cy as u32);
                                apply_pencil(document, history, view, cell.0, cell.1, Some(prev));
                                prev = cell;
                            }
                            view.pencil_last = Some(prev);
                        }
                    }
                }
            }
            if response.drag_stopped_by(PointerButton::Primary)
                || (!primary_down && response.clicked_by(PointerButton::Primary))
            {
                history.end_stroke();
                view.pencil_last = None;
                view.pencil_stroke_fresh.clear();
            }
        }
        ToolKind::Text => {
            // Primary click places (or moves) the caret, flushing any prior
            // session into a single undo step. Accept drag-release too so a
            // stray wiggle past egui's drag threshold still places the caret.
            if response.clicked_by(PointerButton::Primary)
                || response.drag_stopped_by(PointerButton::Primary)
            {
                if let Some((cx, cy)) = hover_cell {
                    history.end_stroke();
                    view.text_caret = Some(TextCaret {
                        origin_x: cx,
                        x: cx,
                        y: cy,
                        line_ends: Vec::new(),
                    });
                }
            }
        }
        ToolKind::Move => {
            // Drag the active layer around. `offset` is stored on Layer and
            // applied at render time; cells that scroll off-canvas stay in
            // the layer buffer and reappear when scrolled back.
            if response.drag_started_by(PointerButton::Primary) {
                if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                    let layer_index = document.active_layer;
                    let from = document
                        .layers
                        .get(layer_index)
                        .map(|l| l.offset)
                        .unwrap_or((0, 0));
                    view.move_drag = Some(MoveDrag {
                        layer_index,
                        from,
                        initial_pos: p,
                    });
                }
            }
            if response.dragged_by(PointerButton::Primary) {
                if let (Some(drag), Some(p)) =
                    (view.move_drag, ui.input(|i| i.pointer.interact_pos()))
                {
                    let delta_px = p - drag.initial_pos;
                    // Snap to whole cells so the offset is always a clean
                    // integer and the renderer doesn't need sub-cell logic.
                    let delta_x = (delta_px.x / cell_pixel_w).round() as i32;
                    let delta_y = (delta_px.y / cell_pixel_h).round() as i32;
                    let new_offset = (drag.from.0 + delta_x, drag.from.1 + delta_y);
                    if let Some(layer) = document.layers.get_mut(drag.layer_index) {
                        if layer.offset != new_offset {
                            layer.offset = new_offset;
                            layer.full_upload = true;
                        }
                    }
                }
            }
            if response.drag_stopped_by(PointerButton::Primary) {
                if let Some(drag) = view.move_drag.take() {
                    if let Some(layer) = document.layers.get(drag.layer_index) {
                        let to = layer.offset;
                        if to != drag.from {
                            history.push(crate::history::Command::MoveLayer {
                                index: drag.layer_index,
                                from: drag.from,
                                to,
                            });
                        }
                    }
                }
            }
        }
    }
    }

    // Switching away from the Text tool mid-session flushes the stroke so the
    // typed run becomes one undo step and drops the caret. Covers tool-panel
    // clicks, hotkey-driven switches, and loads that change the active tool.
    if document.active_tool != ToolKind::Text && view.text_caret.is_some() {
        history.end_stroke();
        view.text_caret = None;
    }

    // Text-tool keyboard dispatch: runs only once a caret has been placed, so
    // entering Text mode without clicking doesn't swallow global hotkeys.
    // Also defers to any focused egui text field (layer rename, etc.) so
    // typing into that field doesn't double-write to the canvas.
    if document.active_tool == ToolKind::Text
        && view.text_caret.is_some()
        && !ui.ctx().wants_keyboard_input()
    {
        handle_text_input(ui, document, history, view);
    }

    // Visual backdrop.
    ui.painter().rect_filled(rect, 0.0, Color32::from_gray(16));
    ui.painter().rect_filled(draw_rect, 0.0, Color32::BLACK);

    // egui-wgpu sets the render-pass viewport to draw_rect, so the shader's
    // uv (0..1) spans draw_rect. Pass the canvas cell at draw_rect's top-left
    // and the cell span across draw_rect.
    let cell_origin = [
        (draw_rect.min.x - unclipped.min.x) / cell_pixel_w,
        (draw_rect.min.y - unclipped.min.y) / cell_pixel_h,
    ];
    let cell_span = [
        draw_rect.width() / cell_pixel_w,
        draw_rect.height() / cell_pixel_h,
    ];

    let request = CanvasRenderRequest::from_document(document, cell_origin, cell_span);
    let callback = CanvasCallback { request };
    ui.painter()
        .add(egui_wgpu::Callback::new_paint_callback(draw_rect, callback));

    // Selection marquee — draw committed mask (translucent fill + boundary
    // outlines where cells border non-selected cells) and, if a rect-select
    // drag is in progress, an additional rectangle preview whose color depends
    // on the op (green = add, red = subtract, yellow = replace).
    if let Some(mask) = document.selection.as_ref() {
        draw_mask(
            ui,
            mask,
            unclipped.min,
            cell_pixel_w,
            cell_pixel_h,
            draw_rect,
            Color32::from_rgba_unmultiplied(100, 170, 255, 30),
            Color32::from_rgb(120, 190, 255),
        );
    }
    // Shape-mode drag preview. Cell mode paints the mask live, so there's
    // nothing extra to preview there.
    if let Some(drag) = view.select_drag {
        if drag.mode == SelectMode::Rect || drag.mode == SelectMode::Oval {
            let pending =
                CellRect::from_corners(drag.start.0, drag.start.1, drag.end.0, drag.end.1);
            let (fill, stroke) = match drag.op {
                SelectOp::Replace => (
                    Color32::from_rgba_unmultiplied(255, 220, 80, 30),
                    Color32::from_rgb(255, 220, 80),
                ),
                SelectOp::Add => (
                    Color32::from_rgba_unmultiplied(100, 220, 120, 30),
                    Color32::from_rgb(120, 230, 140),
                ),
                SelectOp::Subtract => (
                    Color32::from_rgba_unmultiplied(230, 100, 100, 30),
                    Color32::from_rgb(240, 130, 130),
                ),
            };
            let min = unclipped.min
                + Vec2::new(pending.x as f32 * cell_pixel_w, pending.y as f32 * cell_pixel_h);
            let size = Vec2::new(
                pending.w as f32 * cell_pixel_w,
                pending.h as f32 * cell_pixel_h,
            );
            let shape_rect = egui::Rect::from_min_size(min, size);
            let clipped = shape_rect.intersect(draw_rect);
            match drag.mode {
                SelectMode::Rect => {
                    ui.painter().rect_filled(clipped, 0.0, fill);
                    ui.painter().rect_stroke(
                        clipped,
                        0.0,
                        Stroke::new(1.5, stroke),
                        egui::StrokeKind::Inside,
                    );
                }
                SelectMode::Oval => {
                    draw_oval_preview(ui, shape_rect, draw_rect, fill, stroke);
                }
                SelectMode::Cell => {}
            }
        }
    }

    if let Some(drag) = view.line_drag {
        let painter = ui.painter_at(draw_rect);
        let stroke = Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(255, 255, 0, 180),
        );
        let cells = tools::bresenham_cells(
            drag.start.0 as i32,
            drag.start.1 as i32,
            drag.end.0 as i32,
            drag.end.1 as i32,
        );
        for (cx, cy) in cells {
            if cx < 0 || cy < 0 || cx >= document.width as i32 || cy >= document.height as i32 {
                continue;
            }
            let min = unclipped.min
                + Vec2::new(cx as f32 * cell_pixel_w, cy as f32 * cell_pixel_h);
            let cell_rect = egui::Rect::from_min_size(min, Vec2::new(cell_pixel_w, cell_pixel_h));
            painter.rect_stroke(cell_rect, 0.0, stroke, egui::StrokeKind::Inside);
        }
    }

    if let Some(drag) = view.rect_drag {
        let pending =
            CellRect::from_corners(drag.start.0, drag.start.1, drag.end.0, drag.end.1)
                .clamped(document.width, document.height);
        let texture = ensure_atlas_texture(
            ui.ctx(),
            &mut view.atlas_texture,
            &mut view.atlas_generation,
            &document.font,
            document.resources_generation,
        );
        let tex_id = texture.id();
        let painter = ui.painter_at(draw_rect);
        let fg = document.fg;
        let glyph_tint = Color32::from_rgba_unmultiplied(fg.0[0], fg.0[1], fg.0[2], 200);
        let backdrop = Color32::from_rgba_unmultiplied(0, 0, 0, 140);
        for (cx, cy, glyph) in
            tools::rectangle_cell_glyphs(pending, drag.mode, document.selected_glyph)
        {
            let min = unclipped.min
                + Vec2::new(cx as f32 * cell_pixel_w, cy as f32 * cell_pixel_h);
            let cell_rect = egui::Rect::from_min_size(min, Vec2::new(cell_pixel_w, cell_pixel_h));
            painter.rect_filled(cell_rect, 0.0, backdrop);
            let gx = (glyph % 16) as f32;
            let gy = (glyph / 16) as f32;
            let uv = egui::Rect::from_min_max(
                [gx / 16.0, gy / 16.0].into(),
                [(gx + 1.0) / 16.0, (gy + 1.0) / 16.0].into(),
            );
            painter.add(egui::Shape::image(tex_id, cell_rect, uv, glyph_tint));
        }
        // Thin outline to give the whole shape a crisp edge even on dark glyphs.
        let outline = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 0, 140));
        let shape_min = unclipped.min
            + Vec2::new(
                pending.x as f32 * cell_pixel_w,
                pending.y as f32 * cell_pixel_h,
            );
        let shape_size = Vec2::new(
            pending.w as f32 * cell_pixel_w,
            pending.h as f32 * cell_pixel_h,
        );
        let shape_rect = egui::Rect::from_min_size(shape_min, shape_size);
        painter.rect_stroke(shape_rect, 0.0, outline, egui::StrokeKind::Inside);
    }

    // Paste ghost preview: stamp each clipboard cell at origin+(dx,dy) with
    // a semi-transparent bg backdrop and tinted glyph, plus a yellow bbox
    // outline. Same egui-painter path as the rectangle-tool preview.
    if let (Some(preview), Some(clip)) = (view.paste_preview, view.clipboard.as_ref()) {
        if let Some((ox, oy)) = preview.origin {
            let texture = ensure_atlas_texture(
                ui.ctx(),
                &mut view.atlas_texture,
                &mut view.atlas_generation,
                &document.font,
                document.resources_generation,
            );
            let tex_id = texture.id();
            let painter = ui.painter_at(draw_rect);
            for cc in &clip.cells {
                let cx = ox.saturating_add(cc.dx);
                let cy = oy.saturating_add(cc.dy);
                if cx >= document.width || cy >= document.height {
                    continue;
                }
                let min = unclipped.min
                    + Vec2::new(cx as f32 * cell_pixel_w, cy as f32 * cell_pixel_h);
                let cell_rect =
                    egui::Rect::from_min_size(min, Vec2::new(cell_pixel_w, cell_pixel_h));
                let bg = cc.tile.bg;
                let backdrop = if bg == crate::tile::TRANSPARENT_BG {
                    Color32::from_rgba_unmultiplied(0, 0, 0, 100)
                } else {
                    Color32::from_rgba_unmultiplied(bg.0[0], bg.0[1], bg.0[2], 160)
                };
                painter.rect_filled(cell_rect, 0.0, backdrop);
                let fg = cc.tile.fg;
                let glyph_tint =
                    Color32::from_rgba_unmultiplied(fg.0[0], fg.0[1], fg.0[2], 200);
                let glyph = cc.tile.glyph;
                let gx = (glyph % 16) as f32;
                let gy = (glyph / 16) as f32;
                let uv = egui::Rect::from_min_max(
                    [gx / 16.0, gy / 16.0].into(),
                    [(gx + 1.0) / 16.0, (gy + 1.0) / 16.0].into(),
                );
                painter.add(egui::Shape::image(tex_id, cell_rect, uv, glyph_tint));
            }
            // Yellow bbox outline around the full paste extent (clipped to canvas).
            let bbox_x = ox;
            let bbox_y = oy;
            let bbox_w = clip.w.min(document.width.saturating_sub(bbox_x));
            let bbox_h = clip.h.min(document.height.saturating_sub(bbox_y));
            if bbox_w > 0 && bbox_h > 0 {
                let shape_min = unclipped.min
                    + Vec2::new(
                        bbox_x as f32 * cell_pixel_w,
                        bbox_y as f32 * cell_pixel_h,
                    );
                let shape_size = Vec2::new(
                    bbox_w as f32 * cell_pixel_w,
                    bbox_h as f32 * cell_pixel_h,
                );
                let shape_rect = egui::Rect::from_min_size(shape_min, shape_size);
                let outline =
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 0, 160));
                painter.rect_stroke(shape_rect, 0.0, outline, egui::StrokeKind::Inside);
            }
        }
    }

    // Hover outline — suppressed during paste mode since the ghost overlay
    // already marks the cursor position.
    if view.paste_preview.is_none() {
        if let Some((x, y)) = hover_cell {
            let min = unclipped.min + Vec2::new(x as f32 * cell_pixel_w, y as f32 * cell_pixel_h);
            let cell_rect = egui::Rect::from_min_size(min, Vec2::new(cell_pixel_w, cell_pixel_h));
            ui.painter().rect_stroke(
                cell_rect.intersect(draw_rect),
                0.0,
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 0, 180)),
                egui::StrokeKind::Inside,
            );
        }
    }

    // Text caret: solid cyan outline at the typing cursor. Redrawn every
    // frame (no blink) so it's always visible against any glyph backdrop.
    if document.active_tool == ToolKind::Text {
        if let Some(caret) = view.text_caret.as_ref() {
            if caret.x < document.width && caret.y < document.height {
                let min = unclipped.min
                    + Vec2::new(
                        caret.x as f32 * cell_pixel_w,
                        caret.y as f32 * cell_pixel_h,
                    );
                let cell_rect =
                    egui::Rect::from_min_size(min, Vec2::new(cell_pixel_w, cell_pixel_h));
                ui.painter().rect_stroke(
                    cell_rect.intersect(draw_rect),
                    0.0,
                    Stroke::new(2.0, Color32::from_rgb(80, 220, 255)),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    // Reset-view hotkey: Home.
    if ui.input(|i| i.key_pressed(egui::Key::Home)) {
        view.pan = Vec2::ZERO;
        view.zoom = 0.0;
        view.snap_accum = 1.0;
    }

    // Selection shortcuts (fire only when the canvas is hovered, so typing in a
    // rename field or search box can't accidentally deselect or erase).
    if response.hovered() {
        let (escape, delete_key, ctrl_a) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Delete),
                (i.modifiers.ctrl || i.modifiers.command) && i.key_pressed(egui::Key::A),
            )
        });
        if escape {
            // Escape cancels an in-progress paste first; only clears the
            // selection if no paste is pending.
            if view.paste_preview.is_some() {
                view.paste_preview = None;
            } else {
                document.selection = None;
            }
        }
        if ctrl_a {
            let mut mask = SelectionMask::new(document.width, document.height);
            mask.fill_all();
            document.selection = Some(mask);
        }
        if delete_key && document.selection.is_some() {
            tools::erase_selection(document, history);
        }
    }
}

/// Apply a committed Rect/Oval drag to the document selection. Cell-mode
/// drags are handled live during dragging, not here.
fn apply_shape_select(document: &mut Document, drag: SelectDrag) {
    let rect = CellRect::from_corners(drag.start.0, drag.start.1, drag.end.0, drag.end.1);
    let cw = document.width;
    let ch = document.height;
    let rect = rect.clamped(cw, ch);
    if rect.w == 0 || rect.h == 0 {
        return;
    }
    match (drag.mode, drag.op) {
        (SelectMode::Rect, SelectOp::Replace) => {
            document.selection = SelectionMask::from_rect(cw, ch, rect);
        }
        (SelectMode::Rect, SelectOp::Add) => {
            let mask = document
                .selection
                .get_or_insert_with(|| SelectionMask::new(cw, ch));
            mask.add_rect(rect);
            if mask.is_empty() {
                document.selection = None;
            }
        }
        (SelectMode::Rect, SelectOp::Subtract) => {
            if let Some(mask) = document.selection.as_mut() {
                mask.subtract_rect(rect);
                if mask.is_empty() {
                    document.selection = None;
                }
            }
        }
        (SelectMode::Oval, SelectOp::Replace) => {
            document.selection = SelectionMask::from_oval(cw, ch, rect);
        }
        (SelectMode::Oval, SelectOp::Add) => {
            let mask = document
                .selection
                .get_or_insert_with(|| SelectionMask::new(cw, ch));
            mask.add_oval(rect);
            if mask.is_empty() {
                document.selection = None;
            }
        }
        (SelectMode::Oval, SelectOp::Subtract) => {
            if let Some(mask) = document.selection.as_mut() {
                mask.subtract_oval(rect);
                if mask.is_empty() {
                    document.selection = None;
                }
            }
        }
        (SelectMode::Cell, _) => {
            // Not reached — canvas_view routes Cell drags to a live path.
        }
    }
}

/// Outline the inscribed ellipse inside `bounds` plus a translucent fill
/// (clipped to `clip`). Uses a 64-point approximation that's visually
/// indistinguishable from a true ellipse at typical canvas sizes.
fn draw_oval_preview(
    ui: &egui::Ui,
    bounds: egui::Rect,
    clip: egui::Rect,
    fill: Color32,
    edge: Color32,
) {
    let painter = ui.painter_at(clip);
    let center = bounds.center();
    let rx = bounds.width() * 0.5;
    let ry = bounds.height() * 0.5;
    const STEPS: usize = 64;
    let mut pts = Vec::with_capacity(STEPS);
    for i in 0..STEPS {
        let t = (i as f32) / (STEPS as f32) * std::f32::consts::TAU;
        pts.push(egui::Pos2::new(
            center.x + rx * t.cos(),
            center.y + ry * t.sin(),
        ));
    }
    painter.add(egui::Shape::convex_polygon(
        pts.clone(),
        fill,
        Stroke::new(1.5, edge),
    ));
}

fn draw_mask(
    ui: &egui::Ui,
    mask: &SelectionMask,
    origin: egui::Pos2,
    cell_w: f32,
    cell_h: f32,
    clip: egui::Rect,
    fill: Color32,
    edge: Color32,
) {
    let painter = ui.painter_at(clip);
    let stroke = Stroke::new(1.5, edge);
    for y in 0..mask.h {
        for x in 0..mask.w {
            if !mask.contains(x, y) {
                continue;
            }
            let min = origin + Vec2::new(x as f32 * cell_w, y as f32 * cell_h);
            let cell_rect = egui::Rect::from_min_size(min, Vec2::new(cell_w, cell_h));
            painter.rect_filled(cell_rect, 0.0, fill);

            // Draw an edge segment where the neighbor cell is not selected —
            // produces a clean outline around the composite shape.
            let top = y == 0 || !mask.contains(x, y - 1);
            let bot = y + 1 >= mask.h || !mask.contains(x, y + 1);
            let left = x == 0 || !mask.contains(x - 1, y);
            let right = x + 1 >= mask.w || !mask.contains(x + 1, y);
            if top {
                painter.line_segment([cell_rect.left_top(), cell_rect.right_top()], stroke);
            }
            if bot {
                painter.line_segment(
                    [cell_rect.left_bottom(), cell_rect.right_bottom()],
                    stroke,
                );
            }
            if left {
                painter.line_segment([cell_rect.left_top(), cell_rect.left_bottom()], stroke);
            }
            if right {
                painter.line_segment([cell_rect.right_top(), cell_rect.right_bottom()], stroke);
            }
        }
    }
}

/// Route a single pencil cell through `tools::apply_pencil_cell`, first
/// tagging the cell as "fresh" if it didn't already hold a box-drawing
/// glyph. The fresh set is used by Dynamic mode's refresh step to drop
/// canonical stub arms on cells the stroke itself placed, so turning
/// strokes form true corners instead of T-junctions with dangling arms.
fn apply_pencil(
    document: &mut Document,
    history: &mut History,
    view: &mut CanvasViewState,
    x: u32,
    y: u32,
    from: Option<(u32, u32)>,
) {
    if x >= document.width || y >= document.height {
        return;
    }
    let existing_glyph = document.layers[document.active_layer]
        .get(document.width, x, y)
        .glyph;
    if !tools::shape_families::is_connected_glyph(existing_glyph) {
        view.pencil_stroke_fresh.insert((x, y));
    }
    tools::apply_pencil_cell(document, history, x, y, from, &view.pencil_stroke_fresh);
}

/// Consume keyboard events for the active Text-tool typing session. Text
/// events become glyph writes (one cell per char, caret advances right);
/// Enter returns the caret to `origin_x` on the next row; Backspace moves
/// the caret back one cell (clamped to `origin_x`) and clears that cell;
/// Escape ends the session and flushes the stroke as one undo step.
fn handle_text_input(
    ui: &egui::Ui,
    document: &mut Document,
    history: &mut History,
    view: &mut CanvasViewState,
) {
    let (text_runs, enter, backspace, escape) = ui.input(|i| {
        let mut runs: Vec<String> = Vec::new();
        for event in &i.events {
            if let egui::Event::Text(s) = event {
                runs.push(s.clone());
            }
        }
        (
            runs,
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Backspace),
            i.key_pressed(egui::Key::Escape),
        )
    });

    if escape {
        history.end_stroke();
        view.text_caret = None;
        return;
    }

    let cw = document.width;
    let ch = document.height;

    for run in text_runs {
        for ch_c in run.chars() {
            let Some(glyph) = char_to_cp437_glyph(ch_c) else {
                continue;
            };
            let Some(caret) = view.text_caret.as_mut() else {
                return;
            };
            if caret.x >= cw || caret.y >= ch {
                continue;
            }
            let (wx, wy) = (caret.x, caret.y);
            history.begin_stroke();
            tools::write_text_glyph(document, history, wx, wy, glyph);
            // Advance the caret one cell right. If it reaches the right edge
            // it stays there — no auto-wrap, matching user expectation for a
            // fixed-grid canvas. Re-fetch because write_text_glyph took &mut.
            if let Some(caret) = view.text_caret.as_mut() {
                if caret.x + 1 < cw {
                    caret.x += 1;
                }
            }
        }
    }

    if enter {
        if let Some(caret) = view.text_caret.as_mut() {
            if caret.y + 1 < ch {
                // Remember where this line left off so backspace at the next
                // line's left margin can wrap the caret back here.
                caret.line_ends.push(caret.x);
                caret.y += 1;
                caret.x = caret.origin_x;
            } else {
                // Bottom edge: return to origin but don't advance (and don't
                // record a line-end we can't actually wrap back from).
                caret.x = caret.origin_x;
            }
        }
    }

    if backspace {
        if let Some(caret) = view.text_caret.as_mut() {
            if caret.x > caret.origin_x {
                caret.x -= 1;
                let (ex, ey) = (caret.x, caret.y);
                history.begin_stroke();
                tools::erase_text_cell(document, history, ex, ey);
            } else if let Some(prev_end_x) = caret.line_ends.pop() {
                // At the left margin with a prior line to wrap back into:
                // jump to the end-of-text position of that line. No erase
                // here — the cell was the caret's post-typing slot when
                // Enter fired and stays empty. Subsequent backspaces on the
                // reclaimed line use the normal erase branch above.
                if caret.y > 0 {
                    caret.y -= 1;
                }
                caret.x = prev_end_x;
            }
        }
    }
}

/// Map a typed Unicode scalar to a CP437 glyph index, or None if the
/// character has no CP437 equivalent (keyboards produce a few non-ASCII
/// characters via dead keys / IME that we just drop). ASCII 0x20..=0x7E
/// passes through literally; the extended 0x80..=0xFF range uses the
/// standard CP437 code-page mapping.
fn char_to_cp437_glyph(c: char) -> Option<u8> {
    if (' '..='~').contains(&c) {
        return Some(c as u8);
    }
    // Common extended CP437 entries reachable from typical keyboards.
    match c {
        '\u{00A0}' => Some(0xFF), // nbsp → blank cell glyph
        '¢' => Some(0x9B),
        '£' => Some(0x9C),
        '¥' => Some(0x9D),
        'ƒ' => Some(0x9F),
        '¿' => Some(0xA8),
        '°' => Some(0xF8),
        '·' => Some(0xFA),
        '±' => Some(0xF1),
        '÷' => Some(0xF6),
        '×' => Some(0x9E),
        'ß' => Some(0xE1),
        'ç' => Some(0x87),
        'Ç' => Some(0x80),
        'ñ' => Some(0xA4),
        'Ñ' => Some(0xA5),
        'á' => Some(0xA0),
        'é' => Some(0x82),
        'í' => Some(0xA1),
        'ó' => Some(0xA2),
        'ú' => Some(0xA3),
        'Á' => Some(0xB5),
        'É' => Some(0x90),
        'Í' => Some(0xD6),
        'Ó' => Some(0xE0),
        'Ú' => Some(0xE9),
        'à' => Some(0x85),
        'è' => Some(0x8A),
        'ì' => Some(0x8D),
        'ò' => Some(0x95),
        'ù' => Some(0x97),
        'ä' => Some(0x84),
        'ë' => Some(0x89),
        'ï' => Some(0x8B),
        'ö' => Some(0x94),
        'ü' => Some(0x81),
        'ÿ' => Some(0x98),
        'Ä' => Some(0x8E),
        'Ö' => Some(0x99),
        'Ü' => Some(0x9A),
        'â' => Some(0x83),
        'ê' => Some(0x88),
        'î' => Some(0x8C),
        'ô' => Some(0x93),
        'û' => Some(0x96),
        'å' => Some(0x86),
        'Å' => Some(0x8F),
        'æ' => Some(0x91),
        'Æ' => Some(0x92),
        _ => None,
    }
}

/// Upload (or reuse) an egui-side texture of the font atlas so shape
/// previews can paint semi-transparent ghosted glyphs. The texture is
/// rebuilt whenever `resources_generation` advances (e.g. font swap).
fn ensure_atlas_texture(
    ctx: &egui::Context,
    cache: &mut Option<TextureHandle>,
    cache_generation: &mut u64,
    atlas: &FontAtlas,
    generation: u64,
) -> TextureHandle {
    if cache.is_none() || *cache_generation != generation {
        let w = atlas.atlas_w() as usize;
        let h = atlas.atlas_h() as usize;
        let mut rgba = Vec::with_capacity(w * h * 4);
        for &m in &atlas.mask {
            rgba.extend_from_slice(&[255, 255, 255, m]);
        }
        let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
        let handle = ctx.load_texture(
            format!("canvas-ghost-atlas-{}", atlas.name),
            img,
            TextureOptions::NEAREST,
        );
        *cache = Some(handle);
        *cache_generation = generation;
    }
    cache.clone().unwrap()
}
