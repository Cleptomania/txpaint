use egui::{Color32, RichText, Stroke};

use crate::document::Document;
use crate::history::{Command, History};
use crate::layer::Layer;

#[derive(Default)]
pub struct LayersPanelState {
    /// Index of the layer currently being renamed, and the edit buffer.
    pub renaming: Option<RenameState>,
}

pub struct RenameState {
    pub layer: usize,
    pub buffer: String,
    /// True on the first frame of the rename so we can request focus.
    pub just_started: bool,
}

pub fn show(
    ui: &mut egui::Ui,
    document: &mut Document,
    history: &mut History,
    state: &mut LayersPanelState,
) {
    ui.vertical(|ui| {
        ui.heading("Layers");

        let mut action = LayerAction::None;
        let active_idx = document.active_layer;
        let selection_fill = ui.visuals().selection.bg_fill;
        for (i, layer) in document.layers.iter_mut().enumerate().rev() {
            let is_active = i == active_idx;
            let frame = egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(4, 2))
                .corner_radius(3);
            let frame = if is_active {
                frame.fill(selection_fill)
            } else {
                frame
            };
            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    let bullet = if is_active { "▶ " } else { "  " };
                    let renaming_this = state
                        .renaming
                        .as_ref()
                        .map(|r| r.layer == i)
                        .unwrap_or(false);
                    if renaming_this {
                        let rename = state.renaming.as_mut().unwrap();
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut rename.buffer)
                                .desired_width(140.0),
                        );
                        if rename.just_started {
                            response.request_focus();
                            rename.just_started = false;
                        }
                        let commit = response.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        let blurred = response.lost_focus() && !commit && !cancel;
                        if commit || blurred {
                            let new_name = rename.buffer.trim().to_owned();
                            if !new_name.is_empty() {
                                layer.name = new_name;
                            }
                            state.renaming = None;
                        } else if cancel {
                            state.renaming = None;
                        }
                    } else {
                        let response = ui
                            .button(format!("{bullet}{}", layer.name))
                            .on_hover_text("Click to make active · Double-click to rename");
                        if response.double_clicked() {
                            state.renaming = Some(RenameState {
                                layer: i,
                                buffer: layer.name.clone(),
                                just_started: true,
                            });
                        } else if response.clicked() {
                            action = LayerAction::SetActive(i);
                        }
                    }
                    // Green & bold checkmark when the layer is visible. The
                    // default egui checkbox pulls its check_stroke from the
                    // widget-state fg_stroke, so we scope an override here and
                    // leave the surrounding buttons untouched.
                    ui.scope(|ui| {
                        let green = Color32::from_rgb(80, 220, 100);
                        let stroke = Stroke::new(2.5, green);
                        let widgets = &mut ui.visuals_mut().widgets;
                        widgets.inactive.fg_stroke = stroke;
                        widgets.hovered.fg_stroke = stroke;
                        widgets.active.fg_stroke = stroke;
                        ui.checkbox(&mut layer.visible, "");
                    });
                    if ui.small_button("▲").clicked() {
                        action = LayerAction::MoveUp(i);
                    }
                    if ui.small_button("▼").clicked() {
                        action = LayerAction::MoveDown(i);
                    }
                    if ui
                        .small_button("⎘")
                        .on_hover_text("Duplicate layer")
                        .clicked()
                    {
                        action = LayerAction::Duplicate(i);
                    }
                    if i > 0
                        && ui
                            .small_button("⇓")
                            .on_hover_text("Merge into layer below")
                            .clicked()
                    {
                        action = LayerAction::MergeDown(i);
                    }
                    if ui
                        .small_button(RichText::new("✖").color(Color32::LIGHT_RED))
                        .clicked()
                    {
                        action = LayerAction::Delete(i);
                    }
                });
            });
        }

        ui.separator();
        if ui.button("+ New Layer").clicked() {
            action = LayerAction::Add;
        }

        match action {
            LayerAction::None => {}
            LayerAction::SetActive(i) => document.active_layer = i,
            LayerAction::MoveUp(i) => {
                if i + 1 < document.layers.len() {
                    document.layers.swap(i, i + 1);
                    if document.active_layer == i {
                        document.active_layer = i + 1;
                    } else if document.active_layer == i + 1 {
                        document.active_layer = i;
                    }
                    document.bump_resources();
                }
            }
            LayerAction::MoveDown(i) => {
                if i > 0 {
                    document.layers.swap(i, i - 1);
                    if document.active_layer == i {
                        document.active_layer = i - 1;
                    } else if document.active_layer == i - 1 {
                        document.active_layer = i;
                    }
                    document.bump_resources();
                }
            }
            LayerAction::Delete(i) => {
                if document.layers.len() > 1 {
                    document.layers.remove(i);
                    if document.active_layer >= document.layers.len() {
                        document.active_layer = document.layers.len() - 1;
                    }
                    document.bump_resources();
                }
            }
            LayerAction::Add => {
                let name = format!("Layer {}", document.layers.len() + 1);
                document
                    .layers
                    .push(Layer::new(name, document.width, document.height));
                document.active_layer = document.layers.len() - 1;
                document.bump_resources();
            }
            LayerAction::MergeDown(i) => {
                if i > 0 && i < document.layers.len() {
                    let (low, high) = document.layers.split_at_mut(i);
                    let top = &high[0];
                    let bottom = &mut low[i - 1];
                    bottom.merge_from_above(top);
                    document.layers.remove(i);
                    if document.active_layer == i {
                        document.active_layer = i - 1;
                    } else if document.active_layer > i {
                        document.active_layer -= 1;
                    }
                    document.bump_resources();
                }
            }
            LayerAction::Duplicate(i) => {
                if let Some(src) = document.layers.get(i) {
                    let mut copy = src.clone();
                    copy.name = format!("{} copy", src.name);
                    copy.dirty_cells.clear();
                    copy.full_upload = true;
                    let index = i + 1;
                    document.layers.insert(index, copy.clone());
                    document.active_layer = index;
                    document.bump_resources();
                    history.push(Command::AddLayer { index, layer: copy });
                }
            }
        }
    });
}

enum LayerAction {
    None,
    SetActive(usize),
    MoveUp(usize),
    MoveDown(usize),
    Delete(usize),
    Duplicate(usize),
    MergeDown(usize),
    Add,
}
