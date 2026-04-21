use egui::{Color32, PointerButton, Sense, Stroke, TextureHandle, TextureOptions, Vec2};

use crate::document::{CellRect, Document, SelectionMask};
use crate::font::FontAtlas;
use crate::history::History;
use crate::renderer::{CanvasCallback, CanvasRenderRequest};
use crate::tools::{self, RectMode, SelectMode, ToolKind};

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
    /// Last cell visited by the Pencil during the current stroke. Used to
    /// Bresenham-interpolate between pointer frames so fast drags don't skip
    /// cells (important for Dynamic mode's connectivity).
    pub pencil_last: Option<(u32, u32)>,
    /// Cached egui-side copy of the active font atlas, used to draw ghosted
    /// glyph previews (Rectangle-tool drag preview, etc.). Rebuilt when the
    /// document's `resources_generation` changes (font swap, etc.).
    atlas_texture: Option<TextureHandle>,
    atlas_generation: u64,
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

impl Default for CanvasViewState {
    fn default() -> Self {
        Self {
            zoom: 0.0,
            pan: Vec2::ZERO,
            snap_accum: 1.0,
            select_drag: None,
            line_drag: None,
            rect_drag: None,
            pencil_last: None,
            atlas_texture: None,
            atlas_generation: 0,
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

    // Tool dispatch: primary click/drag only (middle drags pan).
    let primary_down = ui.input(|i| i.pointer.primary_down());
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
            if response.drag_started_by(PointerButton::Primary)
                || response.clicked_by(PointerButton::Primary)
            {
                history.begin_stroke();
                view.pencil_last = None;
            }
            if (response.dragged_by(PointerButton::Primary) && primary_down)
                || response.clicked_by(PointerButton::Primary)
            {
                if let Some((x, y)) = hover_cell {
                    // Interpolate between frames so a fast drag doesn't skip
                    // cells. Relevant for Dynamic mode (connections rely on
                    // cell adjacency) and nice-to-have for Simple.
                    match view.pencil_last {
                        None => {
                            tools::apply_pencil_cell(document, history, x, y);
                        }
                        Some((px, py)) if (px, py) == (x, y) => {}
                        Some((px, py)) => {
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
                                tools::apply_pencil_cell(
                                    document,
                                    history,
                                    cx as u32,
                                    cy as u32,
                                );
                            }
                        }
                    }
                    view.pencil_last = Some((x, y));
                }
            }
            if response.drag_stopped_by(PointerButton::Primary)
                || (!primary_down && response.clicked_by(PointerButton::Primary))
            {
                history.end_stroke();
                view.pencil_last = None;
            }
        }
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
            document.selection = None;
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
