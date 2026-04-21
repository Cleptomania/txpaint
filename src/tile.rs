use crate::palette::Color;

pub const TRANSPARENT_BG: Color = Color([255, 0, 255, 255]);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub glyph: u8,
    pub fg: Color,
    pub bg: Color,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            glyph: 0,
            fg: Color([255, 255, 255, 255]),
            bg: TRANSPARENT_BG,
        }
    }
}
