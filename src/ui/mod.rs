pub mod async_dialog;
pub mod canvas_view;
pub mod glyph_palette_panel;
pub mod glyph_picker;
pub mod layers_panel;
pub mod menu;
pub mod palette_panel;
pub mod tools_panel;

use crate::document::Document;
use crate::font::{BUNDLED_FONTS, FontAtlas};
use crate::history::History;

#[derive(Default)]
pub struct UiState {
    pub glyph_picker: glyph_picker::GlyphPickerState,
    pub glyph_palette_panel: glyph_palette_panel::GlyphPalettePanelState,
    pub canvas_view: canvas_view::CanvasViewState,
    pub menu: menu::MenuState,
    pub layers: layers_panel::LayersPanelState,
    pub glyph_nav: GlyphNavTarget,
}

/// Which glyph grid the arrow/WASD keys navigate.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum GlyphNavTarget {
    #[default]
    Standard,
    Custom,
}

pub fn layout(
    ui: &mut egui::Ui,
    document: &mut Document,
    history: &mut History,
    state: &mut UiState,
) {
    egui::Panel::top("menu_bar").show_inside(ui, |ui| {
        menu::show(ui, document, history, &mut state.menu);
    });

    egui::Panel::top("status_bar").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(format!("{}x{}", document.width, document.height));
            ui.separator();
            font_selector(ui, document);
            ui.separator();
            ui.label(format!("Tool: {}", document.active_tool.label()));
            ui.separator();
            ui.label(format!(
                "Glyph: {} (0x{:02X})",
                document.selected_glyph, document.selected_glyph
            ));
        });
    });

    egui::Panel::left("tools").default_size(180.0).show_inside(ui, |ui| {
        tools_panel::show(ui, document, history, &mut state.canvas_view);
    });

    egui::Panel::right("layers").default_size(260.0).show_inside(ui, |ui| {
        palette_panel::show(ui, document);
        ui.separator();
        layers_panel::show(ui, document, history, &mut state.layers);
    });

    egui::Panel::bottom("glyph_picker").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("Nav target:");
            ui.selectable_value(&mut state.glyph_nav, GlyphNavTarget::Standard, "Standard")
                .on_hover_text("Arrow keys / WASD move within the 16×16 glyph grid");
            ui.selectable_value(&mut state.glyph_nav, GlyphNavTarget::Custom, "Custom")
                .on_hover_text("Arrow keys / WASD move within the custom palette");
        });
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.label("Glyphs:");
                glyph_picker::show(
                    ui,
                    document,
                    &mut state.glyph_picker,
                    16.0,
                    state.glyph_nav == GlyphNavTarget::Standard,
                );
            });
            ui.separator();
            ui.vertical(|ui| {
                glyph_palette_panel::show(
                    ui,
                    document,
                    &mut state.glyph_palette_panel,
                    state.glyph_nav == GlyphNavTarget::Custom,
                );
            });
        });
    });

    egui::CentralPanel::no_frame().show_inside(ui, |ui| {
        canvas_view::show(ui, document, history, &mut state.canvas_view);
    });
}

fn font_selector(ui: &mut egui::Ui, document: &mut Document) {
    let current_name = document.font.name.clone();
    egui::ComboBox::from_id_salt("font_selector")
        .selected_text(current_name)
        .show_ui(ui, |ui| {
            for (i, bf) in BUNDLED_FONTS.iter().enumerate() {
                let selected = i == document.bundled_font_index;
                if ui.selectable_label(selected, bf.name).clicked() && !selected {
                    match FontAtlas::from_bundled(i) {
                        Ok(atlas) => {
                            document.font = atlas;
                            document.bundled_font_index = i;
                            document.bump_resources();
                        }
                        Err(e) => eprintln!("failed to load bundled font {}: {e:?}", bf.name),
                    }
                }
            }
        });
}
