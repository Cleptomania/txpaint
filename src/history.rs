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
    /// The layer at `index` was replaced wholesale — used by the Crop tool
    /// (which changes buffer dimensions and offset together). `before` and
    /// `after` are full layer snapshots so undo/redo just swap them in. The
    /// renderer rebuilds that slot on size change.
    ReplaceLayer {
        index: usize,
        before: Layer,
        after: Layer,
    },
    /// The canvas was resized. Buffers are untouched; every layer's display
    /// offset was shifted by `offset_delta` so content stays put in absolute
    /// coordinates. Redo applies the delta and sets dims to `after`; undo
    /// subtracts the delta and restores `before`.
    ResizeCanvas {
        before: (u32, u32),
        after: (u32, u32),
        offset_delta: (i32, i32),
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
    match cmd {
        Command::Cells(changes) => {
            for c in changes {
                if let Some(layer) = document.layers.get_mut(c.layer) {
                    if layer.in_bounds(c.x, c.y) {
                        layer.set(c.x, c.y, c.after);
                    }
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
                layer.offset = *to;
            }
        }
        Command::ReplaceLayer { index, after, .. } => {
            if *index < document.layers.len() {
                let mut layer = after.clone();
                layer.full_upload = true;
                layer.dirty_cells.clear();
                document.layers[*index] = layer;
                document.bump_resources();
            }
        }
        Command::ResizeCanvas {
            after,
            offset_delta,
            ..
        } => {
            document.width = after.0;
            document.height = after.1;
            for layer in &mut document.layers {
                layer.offset.0 += offset_delta.0;
                layer.offset.1 += offset_delta.1;
            }
            document.bump_resources();
        }
    }
}

fn apply_inverse(cmd: &Command, document: &mut Document) {
    match cmd {
        Command::Cells(changes) => {
            for c in changes {
                if let Some(layer) = document.layers.get_mut(c.layer) {
                    if layer.in_bounds(c.x, c.y) {
                        layer.set(c.x, c.y, c.before);
                    }
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
                layer.offset = *from;
            }
        }
        Command::ReplaceLayer { index, before, .. } => {
            if *index < document.layers.len() {
                let mut layer = before.clone();
                layer.full_upload = true;
                layer.dirty_cells.clear();
                document.layers[*index] = layer;
                document.bump_resources();
            }
        }
        Command::ResizeCanvas {
            before,
            offset_delta,
            ..
        } => {
            document.width = before.0;
            document.height = before.1;
            for layer in &mut document.layers {
                layer.offset.0 -= offset_delta.0;
                layer.offset.1 -= offset_delta.1;
            }
            document.bump_resources();
        }
    }
}
