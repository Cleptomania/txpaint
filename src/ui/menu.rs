use std::path::PathBuf;

use egui::Modifiers;

use crate::document::Document;
use crate::history::History;
use crate::io::{font_import, xp};
use crate::ui::async_dialog::PendingFile;

enum PendingOp {
    Open(PendingFile),
    Save { to: Option<PathBuf>, dialog: PendingFile },
    ImportFont(PendingFile),
}

pub struct MenuState {
    pub current_path: Option<PathBuf>,
    pub last_error: Option<String>,
    pub new_dialog: Option<NewDialogState>,
    /// Shown once on app launch, letting the user choose New vs Open vs Skip
    /// instead of dropping into a default 80×25 canvas silently.
    pub show_welcome: bool,
    pending: Option<PendingOp>,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            current_path: None,
            last_error: None,
            new_dialog: None,
            show_welcome: true,
            pending: None,
        }
    }
}

pub struct NewDialogState {
    pub width: u32,
    pub height: u32,
}

pub fn show(
    ui: &mut egui::Ui,
    document: &mut Document,
    history: &mut History,
    state: &mut MenuState,
) {
    drain_pending(document, history, state);
    if state.pending.is_some() {
        ui.ctx().request_repaint();
    }
    ui.horizontal(|ui| {
        ui.menu_button("File", |ui| {
            if ui.button("New…").clicked() {
                state.new_dialog = Some(NewDialogState {
                    width: document.width,
                    height: document.height,
                });
                ui.close();
            }
            if ui.button("Open .xp…").clicked() {
                state.pending = Some(PendingOp::Open(PendingFile::load("XP", &["xp"])));
                ui.close();
            }
            if ui.button("Save").clicked() {
                start_save(state, state.current_path.clone());
                ui.close();
            }
            if ui.button("Save As…").clicked() {
                start_save(state, None);
                ui.close();
            }
            ui.separator();
            if ui.button("Import Font…").clicked() {
                state.pending = Some(PendingOp::ImportFont(PendingFile::load(
                    "PNG font",
                    &["png"],
                )));
                ui.close();
            }
            ui.separator();
            if ui.button("Exit").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });

        ui.menu_button("Edit", |ui| {
            if ui
                .add(
                    egui::Button::new("Undo").shortcut_text(ui.ctx().format_shortcut(
                        &egui::KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::Z),
                    )),
                )
                .clicked()
            {
                history.undo(document);
                ui.close();
            }
            if ui
                .add(egui::Button::new("Redo").shortcut_text(ui.ctx().format_shortcut(
                    &egui::KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, egui::Key::Z),
                )))
                .clicked()
            {
                history.redo(document);
                ui.close();
            }
        });

        ui.separator();
        if let Some(path) = &state.current_path {
            ui.label(path.display().to_string());
        } else {
            ui.label("[unsaved]");
        }
    });

    if let Some(err) = state.last_error.clone() {
        egui::Window::new("Error")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(err);
                if ui.button("OK").clicked() {
                    state.last_error = None;
                }
            });
    }

    if state.show_welcome {
        let mut chose_new = false;
        let mut chose_open = false;
        let mut chose_skip = false;
        egui::Window::new("Welcome to txpaint")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label("Start with a new canvas, or open an existing XP file.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("New Canvas…").clicked() {
                        chose_new = true;
                    }
                    if ui.button("Open .xp…").clicked() {
                        chose_open = true;
                    }
                    if ui.button("Skip").clicked() {
                        chose_skip = true;
                    }
                });
            });
        if chose_new {
            state.show_welcome = false;
            state.new_dialog = Some(NewDialogState {
                width: document.width,
                height: document.height,
            });
        } else if chose_open {
            state.show_welcome = false;
            state.pending = Some(PendingOp::Open(PendingFile::load("XP", &["xp"])));
        } else if chose_skip {
            state.show_welcome = false;
        }
    }

    if state.new_dialog.is_some() {
        let mut close = false;
        let mut create = false;
        egui::Window::new("New Canvas")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                let dlg = state.new_dialog.as_mut().unwrap();
                egui::Grid::new("new_canvas_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Width (cells)");
                        ui.add(
                            egui::DragValue::new(&mut dlg.width)
                                .range(1..=1024)
                                .speed(1.0),
                        );
                        ui.end_row();
                        ui.label("Height (cells)");
                        ui.add(
                            egui::DragValue::new(&mut dlg.height)
                                .range(1..=1024)
                                .speed(1.0),
                        );
                        ui.end_row();
                    });

                // Enter submits, Escape cancels — works because the Window grabs focus.
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() || enter {
                        create = true;
                    }
                    if ui.button("Cancel").clicked() || escape {
                        close = true;
                    }
                });
            });
        if create {
            if let Some(dlg) = state.new_dialog.take() {
                *document = Document::new_with_size(dlg.width, dlg.height);
                *history = History::default();
                state.current_path = None;
            }
        } else if close {
            state.new_dialog = None;
        }
    }
}

fn start_save(state: &mut MenuState, existing: Option<PathBuf>) {
    // If the document already has a path, skip the native picker and use an
    // immediate PendingFile so `drain_pending` handles the write through the
    // same branch as the dialog-driven flow.
    let dialog = match &existing {
        Some(_) => PendingFile::immediate(existing.clone()),
        None => PendingFile::save("XP", "xp", "untitled.xp"),
    };
    state.pending = Some(PendingOp::Save { to: existing, dialog });
}

fn drain_pending(document: &mut Document, history: &mut History, state: &mut MenuState) {
    let Some(op) = &state.pending else {
        return;
    };
    let file = match op {
        PendingOp::Open(f) | PendingOp::ImportFont(f) => f,
        PendingOp::Save { dialog, .. } => dialog,
    };
    let Some(result) = file.poll() else {
        return;
    };
    let op = state.pending.take().unwrap();
    match op {
        PendingOp::Open(_) => {
            if let Some(path) = result {
                match xp::load_from_path(&path) {
                    Ok(doc) => {
                        *document = doc;
                        *history = History::default();
                        state.current_path = Some(path);
                    }
                    Err(e) => state.last_error = Some(format!("Open failed: {e:#}")),
                }
            }
        }
        PendingOp::Save { to, .. } => {
            let path = to.or(result);
            if let Some(path) = path {
                match xp::save_to_path(&path, document) {
                    Ok(()) => state.current_path = Some(path),
                    Err(e) => state.last_error = Some(format!("Save failed: {e:#}")),
                }
            }
        }
        PendingOp::ImportFont(_) => {
            if let Some(path) = result {
                match font_import::load_from_path(&path) {
                    Ok(atlas) => {
                        document.font = atlas;
                        document.bump_resources();
                    }
                    Err(e) => state.last_error = Some(format!("Font import failed: {e:#}")),
                }
            }
        }
    }
}
