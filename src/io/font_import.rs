use std::path::Path;

use anyhow::{Context, Result};

use crate::font::FontAtlas;

pub fn load_from_path(path: &Path) -> Result<FontAtlas> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported")
        .to_owned();
    FontAtlas::from_png_bytes(name, &bytes)
}
