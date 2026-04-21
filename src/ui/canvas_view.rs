use egui::{Color32, PointerButton, Sense, Stroke, Vec2};

use crate::document::{CellRect, Document, SelectionMask};
use crate::history::History;
use crate::renderer::{CanvasCallback, CanvasRenderRequest};
use crate::tools::{self, ToolKind};

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
    /// Anchor cell for an in-progress rectangle selection drag plus the op
    /// chosen when the drag started (decided by the modifier state at that
    /// moment so it stays stable while the user holds the mouse down).
    pub rect_select_drag: Option<RectSelectDrag>,
}

#[derive(Copy, Clone, Debug)]
pub struct RectSelectDrag {
    pub start: (u32, u32),
    pub end: (u32, u32),
    pub op: SelectOp,
}

impl Default for CanvasViewState {
    fn default() -> Self {
        Self {
            zoom: 0.0,
            pan: Vec2::ZERO,
            snap_accum: 1.0,
            rect_select_drag: None,
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
        ToolKind::RectSelect => {
            let modifiers = ui.input(|i| i.modifiers);
            if response.drag_started_by(PointerButton::Primary) {
                if let Some(cell) = hover_cell {
                    let op = if modifiers.shift {
                        SelectOp::Add
                    } else if modifiers.ctrl || modifiers.command {
                        SelectOp::Subtract
                    } else {
                        SelectOp::Replace
                    };
                    view.rect_select_drag = Some(RectSelectDrag {
                        start: cell,
                        end: cell,
                        op,
                    });
                }
            }
            if response.dragged_by(PointerButton::Primary) {
                if let (Some(drag), Some(cell)) = (view.rect_select_drag.as_mut(), hover_cell) {
                    drag.end = cell;
                }
            }
            if response.drag_stopped_by(PointerButton::Primary) {
                if let Some(drag) = view.rect_select_drag.take() {
                    apply_rect_select(document, drag);
                }
            }
            // Plain click (no drag) clears the selection only when no modifier
            // is held — with shift/ctrl a click is ambiguous, so we ignore it.
            if response.clicked_by(PointerButton::Primary)
                && view.rect_select_drag.is_none()
                && !modifiers.shift
                && !modifiers.ctrl
                && !modifiers.command
            {
                document.selection = None;
            }
        }
        _ => {
            if response.drag_started_by(PointerButton::Primary)
                || response.clicked_by(PointerButton::Primary)
            {
                history.begin_stroke();
            }
            if (response.dragged_by(PointerButton::Primary) && primary_down)
                || response.clicked_by(PointerButton::Primary)
            {
                if let Some((x, y)) = hover_cell {
                    tools::apply(document, history, x, y);
                }
            }
            if response.drag_stopped_by(PointerButton::Primary)
                || (!primary_down && response.clicked_by(PointerButton::Primary))
            {
                history.end_stroke();
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
    if let Some(drag) = view.rect_select_drag {
        let pending = CellRect::from_corners(drag.start.0, drag.start.1, drag.end.0, drag.end.1);
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
        let rect = egui::Rect::from_min_size(min, size).intersect(draw_rect);
        ui.painter().rect_filled(rect, 0.0, fill);
        ui.painter()
            .rect_stroke(rect, 0.0, Stroke::new(1.5, stroke), egui::StrokeKind::Inside);
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

fn apply_rect_select(document: &mut Document, drag: RectSelectDrag) {
    let rect = CellRect::from_corners(drag.start.0, drag.start.1, drag.end.0, drag.end.1);
    let cw = document.width;
    let ch = document.height;
    let rect = rect.clamped(cw, ch);
    if rect.w == 0 || rect.h == 0 {
        return;
    }
    match drag.op {
        SelectOp::Replace => {
            document.selection = SelectionMask::from_rect(cw, ch, rect);
        }
        SelectOp::Add => {
            let mask = document
                .selection
                .get_or_insert_with(|| SelectionMask::new(cw, ch));
            mask.add_rect(rect);
            if mask.is_empty() {
                document.selection = None;
            }
        }
        SelectOp::Subtract => {
            if let Some(mask) = document.selection.as_mut() {
                mask.subtract_rect(rect);
                if mask.is_empty() {
                    document.selection = None;
                }
            }
        }
    }
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
