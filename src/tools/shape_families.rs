//! Glyph families used by shape tools (Rectangle, and future shapes).
//!
//! A family bundles the per-slot glyphs that together render a specific shape.
//! When a shape tool draws an outline and the user's selected glyph matches
//! any slot of a family, the tool renders each slot of the shape with the
//! corresponding family glyph instead of filling every slot with the one
//! selected glyph. The family definitions live in
//! `glyph-shape-families.md` at the repo root — keep the two in sync.

/// Glyphs for the Rectangle shape in a single family.
#[derive(Copy, Clone, Debug)]
pub struct RectFamily {
    pub tl: u8,
    pub tr: u8,
    pub bl: u8,
    pub br: u8,
    pub h: u8,
    pub v: u8,
}

impl RectFamily {
    /// True if `glyph` appears in any slot of this family.
    pub fn contains(&self, glyph: u8) -> bool {
        glyph == self.tl
            || glyph == self.tr
            || glyph == self.bl
            || glyph == self.br
            || glyph == self.h
            || glyph == self.v
    }
}

pub const RECT_FAMILIES: &[RectFamily] = &[
    // Single-line (CP437).
    RectFamily {
        tl: 218,
        tr: 191,
        bl: 192,
        br: 217,
        h: 196,
        v: 179,
    },
    // Double-line (CP437).
    RectFamily {
        tl: 201,
        tr: 187,
        bl: 200,
        br: 188,
        h: 205,
        v: 186,
    },
];

/// Return the first Rectangle family whose slots include `glyph`, or `None`
/// if the glyph is not a member of any family.
pub fn rect_family_for(glyph: u8) -> Option<&'static RectFamily> {
    RECT_FAMILIES.iter().find(|f| f.contains(glyph))
}

// ---------- Connected shape (for Dynamic pencil) ----------

/// Four-bit connection mask for a cell, one bit per neighbor direction.
/// Pick values sum cleanly so `(TOP | BOTTOM)` means "connects up and down".
pub const CONN_TOP: u8 = 1 << 0;
pub const CONN_RIGHT: u8 = 1 << 1;
pub const CONN_BOTTOM: u8 = 1 << 2;
pub const CONN_LEFT: u8 = 1 << 3;

/// Glyphs for the Connected shape — enough slots to cover every combination
/// of connections (2^4 = 16 masks, 11 distinct glyphs). The Rectangle slots
/// are duplicated here because most families use the same corner/edge glyphs
/// for both shapes; keeping them local avoids a cross-struct lookup.
#[derive(Copy, Clone, Debug)]
pub struct ConnectedFamily {
    pub h: u8,
    pub v: u8,
    pub tl: u8,
    pub tr: u8,
    pub bl: u8,
    pub br: u8,
    pub t_up: u8,
    pub t_down: u8,
    pub t_left: u8,
    pub t_right: u8,
    pub cross: u8,
}

impl ConnectedFamily {
    pub fn contains(&self, glyph: u8) -> bool {
        glyph == self.h
            || glyph == self.v
            || glyph == self.tl
            || glyph == self.tr
            || glyph == self.bl
            || glyph == self.br
            || glyph == self.t_up
            || glyph == self.t_down
            || glyph == self.t_left
            || glyph == self.t_right
            || glyph == self.cross
    }

    /// Pick the glyph for the given 4-bit connection mask. `None` means the
    /// mask is 0 (isolated cell) — caller decides the fallback. Single-side
    /// "stub" masks fall through to the full horizontal / vertical glyph.
    pub fn glyph_for_mask(&self, mask: u8) -> Option<u8> {
        match mask {
            0 => None,
            // pure horizontal / vertical and their degenerate single-side forms
            m if m == CONN_LEFT || m == CONN_RIGHT || m == (CONN_LEFT | CONN_RIGHT) => Some(self.h),
            m if m == CONN_TOP || m == CONN_BOTTOM || m == (CONN_TOP | CONN_BOTTOM) => {
                Some(self.v)
            }
            // corners
            m if m == (CONN_RIGHT | CONN_BOTTOM) => Some(self.tl),
            m if m == (CONN_LEFT | CONN_BOTTOM) => Some(self.tr),
            m if m == (CONN_TOP | CONN_RIGHT) => Some(self.bl),
            m if m == (CONN_TOP | CONN_LEFT) => Some(self.br),
            // T-junctions — the "leg" direction is the one missing from the mask
            m if m == (CONN_LEFT | CONN_RIGHT | CONN_TOP) => Some(self.t_up),
            m if m == (CONN_LEFT | CONN_RIGHT | CONN_BOTTOM) => Some(self.t_down),
            m if m == (CONN_TOP | CONN_BOTTOM | CONN_LEFT) => Some(self.t_left),
            m if m == (CONN_TOP | CONN_BOTTOM | CONN_RIGHT) => Some(self.t_right),
            // all four
            m if m == (CONN_TOP | CONN_RIGHT | CONN_BOTTOM | CONN_LEFT) => Some(self.cross),
            _ => None,
        }
    }
}

pub const CONNECTED_FAMILIES: &[ConnectedFamily] = &[
    // Single-line. Rectangle slots from the md file copied verbatim plus the
    // T-junctions and cross that the Connected shape adds.
    ConnectedFamily {
        h: 196,
        v: 179,
        tl: 218,
        tr: 191,
        bl: 192,
        br: 217,
        t_up: 193,
        t_down: 194,
        t_left: 180,
        t_right: 195,
        cross: 197,
    },
    // Double-line.
    ConnectedFamily {
        h: 205,
        v: 186,
        tl: 201,
        tr: 187,
        bl: 200,
        br: 188,
        t_up: 202,
        t_down: 203,
        t_left: 185,
        t_right: 204,
        cross: 206,
    },
];

/// Return the first Connected family whose slots include `glyph`.
pub fn connected_family_for(glyph: u8) -> Option<&'static ConnectedFamily> {
    CONNECTED_FAMILIES.iter().find(|f| f.contains(glyph))
}
