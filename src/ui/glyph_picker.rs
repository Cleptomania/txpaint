use egui::{Color32, Image, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};

use crate::document::Document;
use crate::font::FontAtlas;

/// Lazily-uploaded egui texture for the current font atlas, plus the source
/// atlas generation so we can re-upload when the font changes.
pub struct GlyphPickerState {
    texture: Option<TextureHandle>,
    /// Monotonic generation counter matching `Document::resources_generation`.
    atlas_generation: u64,
}

impl Default for GlyphPickerState {
    fn default() -> Self {
        Self {
            texture: None,
            atlas_generation: 0,
        }
    }
}

impl GlyphPickerState {
    fn texture<'a>(
        &'a mut self,
        ctx: &egui::Context,
        atlas: &FontAtlas,
        generation: u64,
    ) -> &'a TextureHandle {
        if self.texture.is_none() || self.atlas_generation != generation {
            let img = atlas_to_color_image(atlas);
            let handle = ctx.load_texture(
                format!("font-atlas-{}", atlas.name),
                img,
                TextureOptions::NEAREST,
            );
            self.texture = Some(handle);
            self.atlas_generation = generation;
        }
        self.texture.as_ref().unwrap()
    }
}

fn atlas_to_color_image(atlas: &FontAtlas) -> egui::ColorImage {
    let w = atlas.atlas_w() as usize;
    let h = atlas.atlas_h() as usize;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for &m in &atlas.mask {
        // White glyph pixel, transparent around it — makes glyphs render legibly
        // over any UI background and tint-able via Image::tint().
        rgba.extend_from_slice(&[255, 255, 255, m]);
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba)
}

/// Draw a 16x16 grid of glyphs. Clicking a cell sets `document.selected_glyph`.
/// When `nav_active` is true, the panel draws an outer border to indicate it
/// is the current target of keyboard (arrow/WASD) navigation.
pub fn show(
    ui: &mut egui::Ui,
    document: &mut Document,
    state: &mut GlyphPickerState,
    tile_px: f32,
    nav_active: bool,
) {
    let tex = state
        .texture(ui.ctx(), &document.font, document.resources_generation)
        .clone();
    let tex_size = tex.size_vec2();

    let grid_size = Vec2::splat(tile_px * 16.0);
    let (rect, response) = ui.allocate_exact_size(grid_size, Sense::click());
    let painter = ui.painter_at(rect);

    // Background so missing/empty glyphs don't look like holes in the panel.
    painter.rect_filled(rect, 0.0, Color32::from_gray(24));

    for gy in 0..16u32 {
        for gx in 0..16u32 {
            let idx = (gy * 16 + gx) as u8;
            let cell_min = rect.min + Vec2::new(gx as f32 * tile_px, gy as f32 * tile_px);
            let cell_rect = Rect::from_min_size(cell_min, Vec2::splat(tile_px));

            let u0 = gx as f32 / 16.0;
            let v0 = gy as f32 / 16.0;
            let u1 = (gx + 1) as f32 / 16.0;
            let v1 = (gy + 1) as f32 / 16.0;
            let uv = Rect::from_min_max([u0, v0].into(), [u1, v1].into());

            Image::from_texture((tex.id(), tex_size))
                .uv(uv)
                .tint(Color32::WHITE)
                .paint_at(ui, cell_rect);

            if idx == document.selected_glyph {
                painter.rect_stroke(
                    cell_rect,
                    0.0,
                    Stroke::new(1.5, Color32::YELLOW),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    if let Some(pos) = response.interact_pointer_pos() {
        if rect.contains(pos) && response.clicked() {
            let rel = pos - rect.min;
            let gx = (rel.x / tile_px).floor().clamp(0.0, 15.0) as u8;
            let gy = (rel.y / tile_px).floor().clamp(0.0, 15.0) as u8;
            document.selected_glyph = gy * 16 + gx;
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
}
