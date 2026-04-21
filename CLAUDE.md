# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

- `cargo check` — fast type-check during iteration.
- `cargo build` — debug build.
- `cargo run` — launch the desktop app.
- `cargo build --release` — release build (canvas throughput is pixel-bound, so release is only needed for stress-testing huge canvases).

No test suite exists yet. Dependency versions are pinned in `Cargo.toml` against `eframe`/`egui`/`egui-wgpu` `0.34.1` and `wgpu` `29.0.1`.

## Architecture

txpaint is a CP437 tile paint program. egui renders all panels (menu, tools, glyph picker, palette, layers); the canvas itself is drawn directly through wgpu via an `egui_wgpu::CallbackTrait` so repaint cost is pixel-bound, not cell-bound.

### Data flow

- `Document` (`src/document.rs`) owns `layers: Vec<Layer>`, the active `FontAtlas`, palettes, fg/bg colors, selected glyph, active tool, and a `resources_generation` counter.
- `Layer` (`src/layer.rs`) holds row-major `tiles: Vec<Tile>` plus a `dirty_cells` set and a `full_upload: bool` flag. Editing calls `layer.set(w, x, y, tile)` which compares and marks the cell dirty.
- `Tile { glyph: u8, fg: Color, bg: Color }` — `bg == TRANSPARENT_BG` (`(255, 0, 255)`) means the cell is transparent.
- Each frame, `CanvasRenderRequest::from_document` drains per-layer dirty cells and snapshots full tile buffers only when `layer.full_upload` is set. Call `Document::bump_resources()` (not `resources_generation += 1` directly) whenever the canvas size, layer count, or font changes — it also forces `full_upload = true` on every layer so the new GPU textures get reseeded.

### Renderer (`src/renderer/`)

- `CanvasRenderResources` is a single persistent struct stored in `egui_wgpu::Renderer::callback_resources` (inserted once in `TxPaintApp::new`). It owns the pipeline, bind group layouts, the font atlas GPU texture, and per-layer `(glyph_tex: R8Uint, fg_tex: Rgba8UnormSrgb, bg_tex: Rgba8UnormSrgb)` textures plus their bind groups.
- `CanvasCallback` is constructed each frame with a `CanvasRenderRequest` snapshot. `prepare` rebuilds font/layer GPU resources if generations changed, applies `full_tiles` uploads or per-cell `dirty_cells` writes, and updates the uniform buffer. `paint` draws one `draw(0..6)` call per visible layer with `BlendState::ALPHA_BLENDING` so transparent cells reveal lower layers.
- The shader (`src/renderer/shader.wgsl`) emits a fixed full-viewport quad `(NDC -1..1)`. egui-wgpu sets the render-pass viewport to the paint-callback rect, so NDC already maps to the draw area — **do not** try to compute a sub-NDC rect for the canvas or sampling will be off-by-offset. UV is y-down (0,0 = top-left).
- Pan/zoom is expressed as `cell_origin + uv * cell_span` in the shader, where `cell_origin` is the canvas cell at the viewport's top-left (can be fractional). Canvas_view computes those from `(draw_rect - unclipped) / cell_pixel_size`. Fragments with `cell_fpos` outside `[0, canvas_wh)` are discarded, which handles panning the viewport past the canvas edge.

### UI (`src/ui/`)

- `app.rs` implements `eframe::App::ui` (not `update` — that's deprecated in egui 0.34). It also extends the Proportional font family with `Hack` so geometric-shape glyphs (`▲`/`▼`, U+25B2/25BC) render in button labels — Ubuntu-Light/NotoEmoji/emoji-icon-font don't cover that block. `✕` (U+2715) is not in any bundled font; use `✖` (U+2716) for an X icon.
- Panels use `egui::Panel::{top,bottom,left,right}` + `.show_inside(ui, ...)`. The older `TopBottomPanel`/`SidePanel` + `.show(ctx, ...)` API is deprecated.
- `canvas_view.rs` handles pointer→cell hit-testing and tool dispatch. Pan = middle-drag; zoom = ctrl+scroll (fractional) or ctrl+shift+scroll (integer snap, threshold-accumulator'd so one real notch = one step). **egui consumes ctrl+scroll as a zoom gesture**: `i.smooth_scroll_delta` is zero in that case — use `i.zoom_delta()` instead. Home resets view.
- The Pencil tool is dispatched per-cell through `tools::apply_pencil_cell(document, history, x, y)` which calls `write_cell` to record before/after tiles into `History` during a stroke (bracketed by `history.begin_stroke()` / `end_stroke()` in `canvas_view.rs` on primary down/up). Drag-gesture tools (Line, Rectangle, Select) track their own state in `CanvasViewState` and commit via `tools::commit_line` / `tools::commit_rectangle` / `apply_shape_select` on mouse-up.

### File I/O (`src/io/`)

- `xp.rs` reads/writes `.xp` (gzipped binary, **column-major** cells: outer loop over x, inner over y, `i32 glyph` then 6× `u8` for fg/bg). `load_from_path` returns a fresh `Document` with all layers marked `full_upload`.
- `font_import.rs` loads a user PNG and builds a `FontAtlas`. Font images must have dimensions divisible by 16 (16×16 glyph grid).

### Bundled fonts

`fonts/cp437_{8x8,10x10,12x12}.png` are included via `include_bytes!` from `src/font.rs::BUNDLED_FONTS`. Adding a new bundled font: drop the PNG in `fonts/` and append a `BundledFont` entry.

## wgpu 29 / egui 0.34 API gotchas

These have changed from older versions and are easy to get wrong:

- `PipelineLayoutDescriptor.bind_group_layouts` takes `&[Option<&BindGroupLayout>]`, not `&[&BindGroupLayout]`.
- `PipelineLayoutDescriptor` uses `immediate_size: u32`, not `push_constant_ranges`.
- `RenderPipelineDescriptor` uses `multiview_mask: Option<NonZeroU32>`, not `multiview`.
- `SamplerDescriptor.mipmap_filter` takes `wgpu::MipmapFilterMode`, not `wgpu::FilterMode`.
- `egui::PaintCallbackInfo` is re-exported from `egui`, not `egui_wgpu`.
- `egui::Context::screen_rect` is deprecated; use `content_rect`.
- `egui::Frame` uses `NONE` (associated const), not `none()`.
- `egui::CentralPanel::no_frame()` / `default_margins()` replace the older single constructor.
