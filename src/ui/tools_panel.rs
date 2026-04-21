use crate::document::Document;
use crate::history::History;
use crate::tools::{self, PencilMode, RectMode, SelectMode, ToolKind};

pub fn show(ui: &mut egui::Ui, document: &mut Document, history: &mut History) {
    ui.vertical(|ui| {
        const CATEGORIES: &[(&str, &[ToolKind])] = &[
            (
                "Draw Tools",
                &[
                    ToolKind::Pencil,
                    ToolKind::Line,
                    ToolKind::Rectangle,
                    ToolKind::Text,
                ],
            ),
            ("Selection Tools", &[ToolKind::Select]),
            ("Layer Tools", &[ToolKind::Move]),
        ];
        for (title, tools) in CATEGORIES {
            ui.heading(*title);
            for &tool in *tools {
                tool_row(ui, document, history, tool);
            }
        }
        ui.separator();
        ui.heading("Colors");
        ui.horizontal(|ui| {
            ui.label("Fg:");
            color_edit(ui, &mut document.fg);
        });
        ui.horizontal(|ui| {
            ui.label("Bg:");
            color_edit(ui, &mut document.bg);
        });
        if ui
            .button("Swap")
            .on_hover_text("Swap foreground and background (X)")
            .clicked()
        {
            std::mem::swap(&mut document.fg, &mut document.bg);
        }
    });
}

fn tool_row(ui: &mut egui::Ui, document: &mut Document, history: &mut History, tool: ToolKind) {
    let selected = document.active_tool == tool;
    let label = tool.label();
    let hotkey = tool.hotkey();
    let response = ui
        .selectable_label(selected, label)
        .on_hover_text(format!("{label} ({hotkey}) — {}", tool.tooltip()));
    if response.clicked() {
        document.active_tool = tool;
    }
    if !selected {
        return;
    }
    match tool {
        ToolKind::Pencil => {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                for mode in PencilMode::ALL {
                    let sel = document.pencil_mode == mode;
                    if ui
                        .selectable_label(sel, mode.label())
                        .on_hover_text(mode.tooltip())
                        .clicked()
                    {
                        document.pencil_mode = mode;
                    }
                }
            });
        }
        ToolKind::Select => {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                for mode in SelectMode::ALL {
                    let sel = document.select_mode == mode;
                    if ui
                        .selectable_label(sel, mode.label())
                        .on_hover_text(mode.tooltip())
                        .clicked()
                    {
                        document.select_mode = mode;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.add_enabled_ui(document.selection.is_some(), |ui| {
                    if ui.button("Fill").clicked() {
                        tools::fill_selection(document, history);
                    }
                    if ui.button("Erase").clicked() {
                        tools::erase_selection(document, history);
                    }
                    if ui.button("Deselect").clicked() {
                        document.selection = None;
                    }
                });
            });
        }
        ToolKind::Rectangle => {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                for mode in RectMode::ALL {
                    let sel = document.rect_mode == mode;
                    if ui
                        .selectable_label(sel, mode.label())
                        .on_hover_text(mode.tooltip())
                        .clicked()
                    {
                        document.rect_mode = mode;
                    }
                }
            });
        }
        _ => {}
    }
}

fn color_edit(ui: &mut egui::Ui, color: &mut crate::palette::Color) {
    let mut srgba = [color.0[0], color.0[1], color.0[2]];
    if ui.color_edit_button_srgb(&mut srgba).changed() {
        color.0[0] = srgba[0];
        color.0[1] = srgba[1];
        color.0[2] = srgba[2];
    }
}
