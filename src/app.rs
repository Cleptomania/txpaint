use crate::document::Document;
use crate::history::History;
use crate::renderer::CanvasRenderResources;
use crate::tools;
use crate::ui::canvas_view::PastePreview;
use crate::ui::{self, GlyphNavTarget, UiState};

pub struct TxPaintApp {
    pub document: Document,
    pub history: History,
    pub ui_state: UiState,
}

impl TxPaintApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .expect("txpaint requires the wgpu backend");
        let resources =
            CanvasRenderResources::new(&render_state.device, render_state.target_format);
        render_state
            .renderer
            .write()
            .callback_resources
            .insert(resources);

        // Extend the proportional font fallback chain with Hack so geometric
        // shape glyphs (▲/▼ at U+25B2/U+25BC) render in UI labels/buttons.
        // Ubuntu-Light/NotoEmoji/emoji-icon-font don't cover that block.
        let mut fonts = egui::FontDefinitions::default();
        if let Some(chain) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            if !chain.iter().any(|n| n == "Hack") {
                chain.push("Hack".to_owned());
            }
        }
        cc.egui_ctx.set_fonts(fonts);

        Self {
            document: Document::new_default(),
            history: History::default(),
            ui_state: UiState::default(),
        }
    }
}

impl eframe::App for TxPaintApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        handle_global_shortcuts(
            ui.ctx(),
            &mut self.document,
            &mut self.history,
            &mut self.ui_state,
        );
        ui::layout(ui, &mut self.document, &mut self.history, &mut self.ui_state);
    }
}

fn handle_global_shortcuts(
    ctx: &egui::Context,
    document: &mut Document,
    history: &mut History,
    ui_state: &mut UiState,
) {
    use crate::tools::ToolKind;
    // Skip single-letter tool hotkeys when a text field has focus (e.g. layer
    // rename), so typing an "e" into a name field doesn't switch to the eraser.
    let text_has_focus = ctx.wants_keyboard_input();
    ctx.input(|i| {
        let ctrl = i.modifiers.ctrl || i.modifiers.command;
        if ctrl && i.key_pressed(egui::Key::Z) && !i.modifiers.shift {
            history.undo(document);
        }
        if ctrl && i.key_pressed(egui::Key::Y) {
            history.redo(document);
        }
        if ctrl && i.key_pressed(egui::Key::Z) && i.modifiers.shift {
            history.redo(document);
        }
        // egui converts Ctrl+C / Ctrl+V into Copy / Paste events (so they
        // round-trip through the OS clipboard for text), rather than firing
        // `key_pressed(Key::C/V)`. Hook the events to run our selection
        // copy / paste-mode entry. Skipped when a text field owns focus so
        // real text paste still works.
        if !text_has_focus {
            for event in &i.events {
                match event {
                    egui::Event::Copy => {
                        if let Some(clip) = tools::copy_selection(document) {
                            ui_state.canvas_view.clipboard = Some(clip);
                        }
                    }
                    egui::Event::Paste(_) => {
                        if ui_state.canvas_view.clipboard.is_some() {
                            ui_state.canvas_view.select_drag = None;
                            ui_state.canvas_view.line_drag = None;
                            ui_state.canvas_view.rect_drag = None;
                            ui_state.canvas_view.paste_preview =
                                Some(PastePreview { origin: None });
                        }
                    }
                    _ => {}
                }
            }
        }
        if !ctrl && !i.modifiers.alt && !text_has_focus {
            let tool_key = i.key_pressed(egui::Key::B)
                || i.key_pressed(egui::Key::M)
                || i.key_pressed(egui::Key::L)
                || i.key_pressed(egui::Key::R)
                || i.key_pressed(egui::Key::V);
            if tool_key {
                ui_state.canvas_view.paste_preview = None;
            }
            if i.key_pressed(egui::Key::B) {
                document.active_tool = ToolKind::Pencil;
            }
            if i.key_pressed(egui::Key::M) {
                document.active_tool = ToolKind::Select;
            }
            if i.key_pressed(egui::Key::L) {
                document.active_tool = ToolKind::Line;
            }
            if i.key_pressed(egui::Key::R) {
                document.active_tool = ToolKind::Rectangle;
            }
            if i.key_pressed(egui::Key::V) {
                document.active_tool = ToolKind::Move;
            }
            if i.key_pressed(egui::Key::X) {
                std::mem::swap(&mut document.fg, &mut document.bg);
            }
        }
    });

    if text_has_focus {
        return;
    }
    let ctx_input = ctx.input(|i| {
        (
            i.modifiers.ctrl || i.modifiers.command,
            i.modifiers.alt,
            [
                i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::A),
                i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::D),
                i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::W),
                i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::S),
            ],
        )
    });
    let (ctrl, alt, [left, right, up, down]) = ctx_input;
    if ctrl || alt {
        return;
    }
    let dx = (right as i32) - (left as i32);
    let dy = (down as i32) - (up as i32);
    if dx == 0 && dy == 0 {
        return;
    }
    match ui_state.glyph_nav {
        GlyphNavTarget::Standard => {
            // 16×16 grid; wrap so edge-stepping is intuitive.
            let mut g = document.selected_glyph as i32;
            let gx = (g % 16 + dx).rem_euclid(16);
            let gy = (g / 16 + dy).rem_euclid(16);
            g = gy * 16 + gx;
            document.selected_glyph = g as u8;
        }
        GlyphNavTarget::Custom => {
            ui_state.glyph_palette_panel.nav(dx, dy, document);
        }
    }
}
