use egui::{Color32, PointerButton, Sense, Stroke, Vec2};

use crate::document::Document;
use crate::palette::{Color, Palette};

pub fn show(ui: &mut egui::Ui, document: &mut Document) {
    ui.heading("Palette");

    ui.horizontal(|ui| {
        let current_name = document
            .palettes
            .get(document.active_palette)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("palette_selector")
            .selected_text(current_name)
            .show_ui(ui, |ui| {
                for (i, p) in document.palettes.iter().enumerate() {
                    let selected = i == document.active_palette;
                    if ui.selectable_label(selected, &p.name).clicked() {
                        document.active_palette = i;
                    }
                }
            });
        if ui.small_button("+").on_hover_text("New palette").clicked() {
            let n = document.palettes.len();
            document
                .palettes
                .push(Palette::default_dos_variant(format!("Palette {}", n + 1)));
            document.active_palette = n;
        }
    });

    ui.label("Left-click: Fg · Right-click: Bg · Right-click menu: Edit/Delete");

    // We defer all mutation until after we've emitted the grid to keep the borrow
    // checker happy — palette cell rows hold a shared reference while we draw.
    let mut fg_pick: Option<Color> = None;
    let mut bg_pick: Option<Color> = None;
    let mut edit: Option<(usize, [u8; 3])> = None;
    let mut delete: Option<usize> = None;
    let mut add_color = false;

    let pal_len;
    {
        let Some(pal) = document.palettes.get(document.active_palette) else {
            return;
        };
        pal_len = pal.colors.len();

        let swatch = 24.0;
        let cols = 8usize;
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                for chunk_start in (0..pal_len).step_by(cols) {
                    ui.horizontal(|ui| {
                        let end = (chunk_start + cols).min(pal_len);
                        for i in chunk_start..end {
                            let c = pal.colors[i];
                            let (rect, resp) =
                                ui.allocate_exact_size(Vec2::splat(swatch), Sense::click());
                            ui.painter().rect_filled(
                                rect,
                                3.0,
                                Color32::from_rgba_unmultiplied(c.0[0], c.0[1], c.0[2], 255),
                            );
                            ui.painter().rect_stroke(
                                rect,
                                3.0,
                                Stroke::new(1.0, Color32::from_gray(40)),
                                egui::StrokeKind::Inside,
                            );
                            if resp.clicked_by(PointerButton::Primary) {
                                fg_pick = Some(c);
                            }
                            if resp.clicked_by(PointerButton::Secondary) {
                                bg_pick = Some(c);
                            }
                            resp.context_menu(|ui| {
                                let mut srgba = [c.0[0], c.0[1], c.0[2]];
                                if ui.color_edit_button_srgb(&mut srgba).changed() {
                                    edit = Some((i, srgba));
                                }
                                if ui.button("Delete").clicked() {
                                    delete = Some(i);
                                    ui.close();
                                }
                            });
                        }
                    });
                }
            });
    }

    ui.horizontal(|ui| {
        if ui.small_button("+ Add").clicked() {
            add_color = true;
        }
    });

    if let Some(c) = fg_pick {
        document.fg = c;
    }
    if let Some(c) = bg_pick {
        document.bg = c;
    }
    if let Some((i, rgb)) = edit {
        if let Some(pal) = document.palettes.get_mut(document.active_palette) {
            if let Some(slot) = pal.colors.get_mut(i) {
                slot.0[0] = rgb[0];
                slot.0[1] = rgb[1];
                slot.0[2] = rgb[2];
            }
        }
    }
    if let Some(i) = delete {
        if let Some(pal) = document.palettes.get_mut(document.active_palette) {
            if i < pal.colors.len() {
                pal.colors.remove(i);
            }
        }
    }
    if add_color {
        if let Some(pal) = document.palettes.get_mut(document.active_palette) {
            pal.colors.push(Color::rgb(128, 128, 128));
        }
    }
}
