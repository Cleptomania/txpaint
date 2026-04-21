use std::collections::VecDeque;

use crate::document::Document;
use crate::layer::Layer;
use crate::tile::Tile;

pub const MAX_HISTORY: usize = 200;

#[derive(Debug, Clone)]
pub struct CellChange {
    pub layer: usize,
    pub x: u32,
    pub y: u32,
    pub before: Tile,
    pub after: Tile,
}

#[derive(Debug, Clone)]
pub enum Command {
    Cells(Vec<CellChange>),
    /// A new layer was inserted at `index`. Redo re-inserts the snapshot;
    /// undo removes the layer at that index. Used by paste-to-new-layer.
    AddLayer { index: usize, layer: Layer },
    /// The layer at `index` had its display offset moved from `from` to
    /// `to`. Redo sets offset to `to`; undo sets it back to `from`.
    MoveLayer {
        index: usize,
        from: (i32, i32),
        to: (i32, i32),
    },
}

pub struct History {
    past: VecDeque<Command>,
    future: Vec<Command>,
    stroke: Option<Vec<CellChange>>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            past: VecDeque::new(),
            future: Vec::new(),
            stroke: None,
        }
    }
}

impl History {
    pub fn begin_stroke(&mut self) {
        if self.stroke.is_none() {
            self.stroke = Some(Vec::new());
        }
    }

    pub fn record(&mut self, change: CellChange) {
        if change.before == change.after {
            return;
        }
        if let Some(s) = self.stroke.as_mut() {
            // Coalesce repeat visits to the same cell: keep the first `before`
            // and the latest `after`.
            if let Some(existing) = s
                .iter_mut()
                .rfind(|c| c.layer == change.layer && c.x == change.x && c.y == change.y)
            {
                existing.after = change.after;
            } else {
                s.push(change);
            }
        } else {
            // Ungrouped change — push as its own command.
            self.push(Command::Cells(vec![change]));
        }
    }

    pub fn end_stroke(&mut self) {
        if let Some(stroke) = self.stroke.take() {
            if !stroke.is_empty() {
                self.push(Command::Cells(stroke));
            }
        }
    }

    pub fn push(&mut self, cmd: Command) {
        self.future.clear();
        self.past.push_back(cmd);
        while self.past.len() > MAX_HISTORY {
            self.past.pop_front();
        }
    }

    pub fn undo(&mut self, document: &mut Document) -> bool {
        // Don't undo mid-stroke — finalize first so the user gets predictable behavior.
        self.end_stroke();
        let Some(cmd) = self.past.pop_back() else {
            return false;
        };
        apply_inverse(&cmd, document);
        self.future.push(cmd);
        true
    }

    pub fn redo(&mut self, document: &mut Document) -> bool {
        self.end_stroke();
        let Some(cmd) = self.future.pop() else {
            return false;
        };
        apply_forward(&cmd, document);
        self.past.push_back(cmd);
        true
    }
}

fn apply_forward(cmd: &Command, document: &mut Document) {
    let w = document.width;
    match cmd {
        Command::Cells(changes) => {
            for c in changes {
                if let Some(layer) = document.layers.get_mut(c.layer) {
                    layer.set(w, c.x, c.y, c.after);
                }
            }
        }
        Command::AddLayer { index, layer } => {
            let i = (*index).min(document.layers.len());
            document.layers.insert(i, layer.clone());
            document.active_layer = i;
            document.bump_resources();
        }
        Command::MoveLayer { index, to, .. } => {
            if let Some(layer) = document.layers.get_mut(*index) {
                if layer.offset != *to {
                    layer.offset = *to;
                    layer.full_upload = true;
                }
            }
        }
    }
}

fn apply_inverse(cmd: &Command, document: &mut Document) {
    let w = document.width;
    match cmd {
        Command::Cells(changes) => {
            for c in changes {
                if let Some(layer) = document.layers.get_mut(c.layer) {
                    layer.set(w, c.x, c.y, c.before);
                }
            }
        }
        Command::AddLayer { index, .. } => {
            // Document invariant: always keep at least one layer. If the
            // paste produced the sole layer, leave it — matches the
            // delete-button guard in `layers_panel.rs`.
            if document.layers.len() > 1 && *index < document.layers.len() {
                document.layers.remove(*index);
                if document.active_layer >= document.layers.len() {
                    document.active_layer = document.layers.len() - 1;
                }
                document.bump_resources();
            }
        }
        Command::MoveLayer { index, from, .. } => {
            if let Some(layer) = document.layers.get_mut(*index) {
                if layer.offset != *from {
                    layer.offset = *from;
                    layer.full_upload = true;
                }
            }
        }
    }
}
