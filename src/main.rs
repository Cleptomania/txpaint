mod app;
mod document;
mod font;
mod glyph_palette;
mod history;
mod io;
mod layer;
mod palette;
mod renderer;
mod tile;
mod tools;
mod ui;

use app::TxPaintApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 480.0])
            .with_title("txpaint"),
        ..Default::default()
    };

    eframe::run_native(
        "txpaint",
        options,
        Box::new(|cc| Ok(Box::new(TxPaintApp::new(cc)))),
    )
}
