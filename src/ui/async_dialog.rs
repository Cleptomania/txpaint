//! Non-blocking file-dialog helper.
//!
//! `rfd::FileDialog::{pick_file, save_file}` blocks the calling thread until
//! the dialog is dismissed. On the UI thread that freezes egui's event loop —
//! the OS flags the window as "not responding". Instead we spawn a worker
//! thread, run the dialog there, and let the UI poll for the result each
//! frame while continuing to paint normally.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

pub struct PendingFile {
    rx: Receiver<Option<PathBuf>>,
}

impl PendingFile {
    /// Produces a PendingFile whose result is already available — useful for
    /// the "Save to known path" path where we still want the same drain logic
    /// as the picker-driven flow but no dialog to show.
    pub fn immediate(result: Option<PathBuf>) -> Self {
        let (tx, rx) = channel();
        let _ = tx.send(result);
        Self { rx }
    }

    pub fn save(filter_name: &str, ext: &str, default_name: &str) -> Self {
        let (tx, rx) = channel();
        let filter_name = filter_name.to_owned();
        let ext = ext.to_owned();
        let default_name = default_name.to_owned();
        std::thread::spawn(move || {
            let path = rfd::FileDialog::new()
                .add_filter(&filter_name, &[&ext])
                .set_file_name(&default_name)
                .save_file();
            let _ = tx.send(path);
        });
        Self { rx }
    }

    pub fn load(filter_name: &str, exts: &[&str]) -> Self {
        let (tx, rx) = channel();
        let filter_name = filter_name.to_owned();
        let exts: Vec<String> = exts.iter().map(|e| (*e).to_owned()).collect();
        std::thread::spawn(move || {
            let ext_refs: Vec<&str> = exts.iter().map(String::as_str).collect();
            let path = rfd::FileDialog::new()
                .add_filter(&filter_name, &ext_refs)
                .pick_file();
            let _ = tx.send(path);
        });
        Self { rx }
    }

    /// `Some(Some(path))` = file chosen; `Some(None)` = dialog cancelled or
    /// dropped; `None` = still waiting (dialog is open).
    pub fn poll(&self) -> Option<Option<PathBuf>> {
        match self.rx.try_recv() {
            Ok(p) => Some(p),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(None),
        }
    }
}
