//! Load/save custom glyph palettes as standalone `.gpal` files (RON format).
//!
//! Kept independent of the `.xp` canvas format so a user can reuse the same
//! handcrafted palette across multiple projects without duplicating the data.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::glyph_palette::GlyphPalette;

const CURRENT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct GlyphPaletteFile {
    version: u32,
    palette: GlyphPalette,
}

pub fn save_to_path(path: &Path, palette: &GlyphPalette) -> Result<()> {
    let file = GlyphPaletteFile {
        version: CURRENT_VERSION,
        palette: palette.clone(),
    };
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
        .context("serialize palette")?;
    std::fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn load_from_path(path: &Path) -> Result<GlyphPalette> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    let file: GlyphPaletteFile = ron::from_str(&text).context("parse palette")?;
    if file.version > CURRENT_VERSION {
        return Err(anyhow!(
            "palette file version {} is newer than this build supports ({CURRENT_VERSION})",
            file.version
        ));
    }
    let slots_len = (file.palette.w as usize) * (file.palette.h as usize);
    if file.palette.slots.len() != slots_len {
        return Err(anyhow!(
            "palette has {}x{}={} slots but file contains {}",
            file.palette.w,
            file.palette.h,
            slots_len,
            file.palette.slots.len()
        ));
    }
    Ok(file.palette)
}
