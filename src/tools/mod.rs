use crate::document::Document;
use crate::history::{CellChange, History};
use crate::tile::{TRANSPARENT_BG, Tile};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Pencil,
    Eraser,
    Eyedropper,
    RectSelect,
}

impl ToolKind {
    pub const ALL: [ToolKind; 4] = [
        ToolKind::Pencil,
        ToolKind::Eraser,
        ToolKind::Eyedropper,
        ToolKind::RectSelect,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Pencil => "Pencil",
            ToolKind::Eraser => "Eraser",
            ToolKind::Eyedropper => "Eyedropper",
            ToolKind::RectSelect => "Rect Select",
        }
    }

    /// Keyboard shortcut character shown in tooltips; also used by the global
    /// hotkey handler in `app.rs`. Keep the two lists in sync.
    pub fn hotkey(self) -> &'static str {
        match self {
            ToolKind::Pencil => "B",
            ToolKind::Eraser => "E",
            ToolKind::Eyedropper => "I",
            ToolKind::RectSelect => "M",
        }
    }
}

pub fn apply(document: &mut Document, history: &mut History, x: u32, y: u32) {
    if x >= document.width || y >= document.height {
        return;
    }
    let w = document.width;
    let layer_index = document.active_layer;
    match document.active_tool {
        ToolKind::RectSelect => {
            // Selection is managed by canvas_view via drag events, not by the
            // per-cell tool-apply path.
        }
        ToolKind::Pencil => {
            let new_tile = Tile {
                glyph: document.selected_glyph,
                fg: document.fg,
                bg: document.bg,
            };
            write_cell(document, history, layer_index, w, x, y, new_tile);
        }
        ToolKind::Eraser => {
            let new_tile = Tile {
                glyph: 0,
                fg: document.fg,
                bg: TRANSPARENT_BG,
            };
            write_cell(document, history, layer_index, w, x, y, new_tile);
        }
        ToolKind::Eyedropper => {
            let mut picked: Option<Tile> = None;
            for layer in document.layers.iter().rev() {
                if !layer.visible {
                    continue;
                }
                let t = layer.get(w, x, y);
                if t.bg != TRANSPARENT_BG || t.glyph != 0 {
                    picked = Some(t);
                    break;
                }
            }
            if let Some(t) = picked {
                document.selected_glyph = t.glyph;
                document.fg = t.fg;
                if t.bg != TRANSPARENT_BG {
                    document.bg = t.bg;
                }
            }
        }
    }
}

/// Fill the active layer's selected region with the current (glyph, fg, bg).
/// No-op if there is no selection.
pub fn fill_selection(document: &mut Document, history: &mut History) {
    run_on_selection(document, history, |doc| Tile {
        glyph: doc.selected_glyph,
        fg: doc.fg,
        bg: doc.bg,
    });
}

/// Erase the active layer's selected region: glyph 0 with transparent bg.
pub fn erase_selection(document: &mut Document, history: &mut History) {
    run_on_selection(document, history, |doc| Tile {
        glyph: 0,
        fg: doc.fg,
        bg: TRANSPARENT_BG,
    });
}

fn run_on_selection(
    document: &mut Document,
    history: &mut History,
    make_tile: impl Fn(&Document) -> Tile,
) {
    let Some(mask) = document.selection.as_ref() else {
        return;
    };
    // Collect up front so we can hand a `&mut Document` into write_cell without
    // borrowing the selection mask at the same time.
    let cells: Vec<(u32, u32)> = mask.iter_cells().collect();
    if cells.is_empty() {
        return;
    }
    let new_tile = make_tile(document);
    let w = document.width;
    let layer_index = document.active_layer;
    history.begin_stroke();
    for (x, y) in cells {
        write_cell(document, history, layer_index, w, x, y, new_tile);
    }
    history.end_stroke();
}

fn write_cell(
    document: &mut Document,
    history: &mut History,
    layer_index: usize,
    w: u32,
    x: u32,
    y: u32,
    new_tile: Tile,
) {
    let Some(layer) = document.layers.get_mut(layer_index) else {
        return;
    };
    let before = layer.get(w, x, y);
    if before == new_tile {
        return;
    }
    layer.set(w, x, y, new_tile);
    history.record(CellChange {
        layer: layer_index,
        x,
        y,
        before,
        after: new_tile,
    });
}
