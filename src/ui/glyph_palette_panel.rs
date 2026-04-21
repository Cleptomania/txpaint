use egui::{Color32, Image, PointerButton, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};

use crate::document::Document;
use crate::font::FontAtlas;
use crate::glyph_palette::GlyphPalette;
use crate::ui::async_dialog::PendingFile;

enum PendingOp {
    Save(PendingFile),
    Load(PendingFile),
}

pub struct GlyphPalettePanelState {
    /// True → clicks write the selected glyph into palette slots; right-click
    /// clears. False → clicks pick a palette slot's glyph as the active glyph.
    pub edit_mode: bool,
    /// Cursor position for keyboard navigation when this panel is the nav
    /// target. Clamped to the active palette's bounds on read.
    pub cursor: (u32, u32),
    /// Pending size shown in the DragValue widgets. Diverges from the palette
    /// size while the user is mid-edit (typing or dragging), then commits on
    /// lost_focus / drag_stopped.
    pending_w: u32,
    pending_h: u32,
    /// Last save/load error, shown inline until the user dismisses it.
    pub last_error: Option<String>,
    /// In-flight save/load file-dialog promise — None while nothing is open.
    pending: Option<PendingOp>,
    texture: Option<TextureHandle>,
    atlas_generation: u64,
}

impl Default for GlyphPalettePanelState {
    fn default() -> Self {
        Self {
            edit_mode: false,
            cursor: (0, 0),
            pending_w: 0,
            pending_h: 0,
            last_error: None,
            pending: None,
            texture: None,
            atlas_generation: 0,
        }
    }
}

impl GlyphPalettePanelState {
    pub fn clamp_cursor(&mut self, w: u32, h: u32) {
        self.cursor.0 = self.cursor.0.min(w.saturating_sub(1));
        self.cursor.1 = self.cursor.1.min(h.saturating_sub(1));
    }

    /// Move the cursor by `(dx, dy)` clamped to the palette bounds, then sync
    /// `document.selected_glyph` if the new slot has a glyph.
    pub fn nav(&mut self, dx: i32, dy: i32, document: &mut Document) {
        let (pw, ph) = {
            let p = document.active_glyph_palette();
            (p.w, p.h)
        };
        self.clamp_cursor(pw, ph);
        let nx = (self.cursor.0 as i32 + dx).clamp(0, pw as i32 - 1) as u32;
        let ny = (self.cursor.1 as i32 + dy).clamp(0, ph as i32 - 1) as u32;
        self.cursor = (nx, ny);
        if let Some(g) = document.active_glyph_palette().get(nx, ny) {
            document.selected_glyph = g;
        }
    }
}

pub fn show(
    ui: &mut egui::Ui,
    document: &mut Document,
    state: &mut GlyphPalettePanelState,
    nav_active: bool,
) {
    let (pal_w, pal_h) = {
        let p = document.active_glyph_palette();
        (p.w, p.h)
    };
    state.clamp_cursor(pal_w, pal_h);
    if state.pending_w == 0 {
        state.pending_w = pal_w;
    }
    if state.pending_h == 0 {
        state.pending_h = pal_h;
    }
    let tile_px = 20.0;

    // --- Row 1: palette selector + rename + add / delete ---
    ui.horizontal(|ui| {
        ui.label("Palette:");
        let selected_name = document.active_glyph_palette().name.clone();
        egui::ComboBox::from_id_salt("active_glyph_palette")
            .selected_text(selected_name)
            .show_ui(ui, |ui| {
                for i in 0..document.glyph_palettes.len() {
                    let selected = i == document.active_glyph_palette;
                    let name = document.glyph_palettes[i].name.clone();
                    if ui.selectable_label(selected, name).clicked() && !selected {
                        document.active_glyph_palette = i;
                        state.pending_w = document.active_glyph_palette().w;
                        state.pending_h = document.active_glyph_palette().h;
                        state.cursor = (0, 0);
                    }
                }
            });

        ui.add(
            egui::TextEdit::singleline(&mut document.active_glyph_palette_mut().name)
                .desired_width(140.0),
        )
        .on_hover_text("Rename the active palette");

        if ui.small_button("+ New").clicked() {
            let n = document.glyph_palettes.len() + 1;
            document
                .glyph_palettes
                .push(GlyphPalette::new(format!("Palette {n}"), 8, 4));
            document.active_glyph_palette = document.glyph_palettes.len() - 1;
            state.pending_w = 8;
            state.pending_h = 4;
            state.cursor = (0, 0);
        }

        let can_delete = document.glyph_palettes.len() > 1;
        if ui
            .add_enabled(can_delete, egui::Button::new("✖ Delete"))
            .on_hover_text("Remove the active palette (kept in the current session only)")
            .clicked()
        {
            let i = document.active_glyph_palette;
            document.glyph_palettes.remove(i);
            if document.active_glyph_palette >= document.glyph_palettes.len() {
                document.active_glyph_palette = document.glyph_palettes.len() - 1;
            }
            state.pending_w = document.active_glyph_palette().w;
            state.pending_h = document.active_glyph_palette().h;
            state.cursor = (0, 0);
        }
    });

    // --- Row 2: mode toggle, size, clear, save/load ---
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.edit_mode, false, "Use")
            .on_hover_text("Click a slot to set the active glyph");
        ui.selectable_value(&mut state.edit_mode, true, "Edit")
            .on_hover_text("Left-click: set slot to active glyph · Right-click: clear");

        ui.separator();

        let w_resp = ui.add(
            egui::DragValue::new(&mut state.pending_w)
                .range(1..=32)
                .prefix("W: "),
        );
        let h_resp = ui.add(
            egui::DragValue::new(&mut state.pending_h)
                .range(1..=32)
                .prefix("H: "),
        );
        let editing = w_resp.has_focus()
            || w_resp.dragged()
            || h_resp.has_focus()
            || h_resp.dragged();
        let commit = w_resp.drag_stopped()
            || w_resp.lost_focus()
            || h_resp.drag_stopped()
            || h_resp.lost_focus();
        if commit
            && (state.pending_w != document.active_glyph_palette().w
                || state.pending_h != document.active_glyph_palette().h)
        {
            document
                .active_glyph_palette_mut()
                .resize(state.pending_w, state.pending_h);
        }
        if !editing
            && (state.pending_w != document.active_glyph_palette().w
                || state.pending_h != document.active_glyph_palette().h)
        {
            state.pending_w = document.active_glyph_palette().w;
            state.pending_h = document.active_glyph_palette().h;
        }

        if state.edit_mode && ui.button("Clear All").clicked() {
            document.active_glyph_palette_mut().clear();
        }

        ui.separator();

        let dialog_open = state.pending.is_some();
        ui.add_enabled_ui(!dialog_open, |ui| {
            if ui.small_button("Save…").clicked() {
                let default_name = default_filename(&document.active_glyph_palette().name);
                state.pending = Some(PendingOp::Save(PendingFile::save(
                    "Glyph Palette",
                    "gpal",
                    &default_name,
                )));
            }
            if ui.small_button("Load…").clicked() {
                state.pending = Some(PendingOp::Load(PendingFile::load(
                    "Glyph Palette",
                    &["gpal"],
                )));
            }
        });
    });

    // Poll pending dialog each frame; keep repainting so the poll runs.
    if let Some(op) = &state.pending {
        ui.ctx().request_repaint();
        let file = match op {
            PendingOp::Save(f) | PendingOp::Load(f) => f,
        };
        if let Some(result) = file.poll() {
            match state.pending.take().unwrap() {
                PendingOp::Save(_) => {
                    if let Some(path) = result {
                        if let Err(e) = crate::io::glyph_palette::save_to_path(
                            &path,
                            document.active_glyph_palette(),
                        ) {
                            state.last_error = Some(format!("Save failed: {e:#}"));
                        }
                    }
                }
                PendingOp::Load(_) => {
                    if let Some(path) = result {
                        match crate::io::glyph_palette::load_from_path(&path) {
                            Ok(mut pal) => {
                                if pal.name.trim().is_empty() {
                                    pal.name = path
                                        .file_stem()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or("Loaded")
                                        .to_owned();
                                }
                                document.glyph_palettes.push(pal);
                                document.active_glyph_palette =
                                    document.glyph_palettes.len() - 1;
                                state.pending_w = document.active_glyph_palette().w;
                                state.pending_h = document.active_glyph_palette().h;
                                state.cursor = (0, 0);
                            }
                            Err(e) => state.last_error = Some(format!("Load failed: {e:#}")),
                        }
                    }
                }
            }
        }
    }

    if let Some(err) = state.last_error.clone() {
        ui.horizontal(|ui| {
            ui.colored_label(Color32::from_rgb(240, 120, 120), err);
            if ui.small_button("Dismiss").clicked() {
                state.last_error = None;
            }
        });
    }

    let tex = ensure_texture(
        ui.ctx(),
        state,
        &document.font,
        document.resources_generation,
    );
    let tex_size = tex.size_vec2();
    let pal_w = document.active_glyph_palette().w;
    let pal_h = document.active_glyph_palette().h;

    let grid_size = Vec2::new(pal_w as f32 * tile_px, pal_h as f32 * tile_px);
    let (rect, response) = ui.allocate_exact_size(grid_size, Sense::click());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0, Color32::from_gray(18));

    let hovered_cell = if state.edit_mode {
        ui.input(|i| i.pointer.hover_pos()).and_then(|p| {
            if !rect.contains(p) {
                return None;
            }
            let rel = p - rect.min;
            let hx = (rel.x / tile_px).floor().clamp(0.0, pal_w as f32 - 1.0) as u32;
            let hy = (rel.y / tile_px).floor().clamp(0.0, pal_h as f32 - 1.0) as u32;
            Some((hx, hy))
        })
    } else {
        None
    };

    for py in 0..pal_h {
        for px in 0..pal_w {
            let min = rect.min + Vec2::new(px as f32 * tile_px, py as f32 * tile_px);
            let cell_rect = Rect::from_min_size(min, Vec2::splat(tile_px));

            let slot_glyph = document.active_glyph_palette().get(px, py);
            match slot_glyph {
                Some(g) => {
                    let gx = (g % 16) as f32;
                    let gy = (g / 16) as f32;
                    let uv = Rect::from_min_max(
                        [gx / 16.0, gy / 16.0].into(),
                        [(gx + 1.0) / 16.0, (gy + 1.0) / 16.0].into(),
                    );
                    Image::from_texture((tex.id(), tex_size))
                        .uv(uv)
                        .tint(Color32::WHITE)
                        .paint_at(ui, cell_rect);
                }
                None => {
                    painter.rect_filled(cell_rect, 0.0, Color32::from_gray(28));
                }
            }

            if hovered_cell == Some((px, py)) {
                let g = document.selected_glyph;
                let gx = (g % 16) as f32;
                let gy = (g / 16) as f32;
                let uv = Rect::from_min_max(
                    [gx / 16.0, gy / 16.0].into(),
                    [(gx + 1.0) / 16.0, (gy + 1.0) / 16.0].into(),
                );
                painter.rect_filled(
                    cell_rect,
                    0.0,
                    Color32::from_rgba_unmultiplied(0, 0, 0, 140),
                );
                Image::from_texture((tex.id(), tex_size))
                    .uv(uv)
                    .tint(Color32::from_rgba_unmultiplied(255, 255, 255, 130))
                    .paint_at(ui, cell_rect);
            }

            painter.rect_stroke(
                cell_rect,
                0.0,
                Stroke::new(0.5, Color32::from_gray(60)),
                egui::StrokeKind::Inside,
            );

            if nav_active && (px, py) == state.cursor {
                painter.rect_stroke(
                    cell_rect,
                    0.0,
                    Stroke::new(1.5, Color32::from_rgb(100, 170, 255)),
                    egui::StrokeKind::Inside,
                );
            }

            if slot_glyph == Some(document.selected_glyph) {
                painter.rect_stroke(
                    cell_rect,
                    0.0,
                    Stroke::new(1.5, Color32::YELLOW),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    if nav_active {
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(2.0, Color32::from_rgb(100, 170, 255)),
            egui::StrokeKind::Inside,
        );
    }

    if let Some(pos) = response.interact_pointer_pos() {
        if rect.contains(pos) {
            let rel = pos - rect.min;
            let px = (rel.x / tile_px).floor().clamp(0.0, pal_w as f32 - 1.0) as u32;
            let py = (rel.y / tile_px).floor().clamp(0.0, pal_h as f32 - 1.0) as u32;

            if response.clicked_by(PointerButton::Primary) {
                state.cursor = (px, py);
                if state.edit_mode {
                    let g = document.selected_glyph;
                    document.active_glyph_palette_mut().set(px, py, Some(g));
                } else if let Some(g) = document.active_glyph_palette().get(px, py) {
                    document.selected_glyph = g;
                }
            }
            if response.clicked_by(PointerButton::Secondary) && state.edit_mode {
                document.active_glyph_palette_mut().set(px, py, None);
            }
        }
    }
}

fn default_filename(name: &str) -> String {
    let trimmed: String = name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    if trimmed.is_empty() {
        "palette.gpal".to_owned()
    } else {
        format!("{trimmed}.gpal")
    }
}

fn ensure_texture(
    ctx: &egui::Context,
    state: &mut GlyphPalettePanelState,
    atlas: &FontAtlas,
    generation: u64,
) -> TextureHandle {
    if state.texture.is_none() || state.atlas_generation != generation {
        let img = atlas_to_color_image(atlas);
        let handle = ctx.load_texture(
            format!("custom-palette-atlas-{}", atlas.name),
            img,
            TextureOptions::NEAREST,
        );
        state.texture = Some(handle);
        state.atlas_generation = generation;
    }
    state.texture.clone().unwrap()
}

fn atlas_to_color_image(atlas: &FontAtlas) -> egui::ColorImage {
    let w = atlas.atlas_w() as usize;
    let h = atlas.atlas_h() as usize;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for &m in &atlas.mask {
        rgba.extend_from_slice(&[255, 255, 255, m]);
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba)
}
