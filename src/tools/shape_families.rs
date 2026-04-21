//! Glyph families used by shape tools (Rectangle, Dynamic pencil, etc.).
//!
//! Two models coexist here:
//!
//! - `RectFamily`: the simple per-family glyph set used by the Rectangle
//!   tool. Each family has six slots (TL, TR, BL, BR, H, V).
//!
//! - `ConnectionPattern` + the pattern/glyph table: used by the Dynamic
//!   pencil. A cell's connections are represented as a 4-tuple of
//!   `LineStyle` (one per cardinal direction), and a flat table maps
//!   between the pattern and the CP437 glyph. This encoding covers mixed
//!   single/double families out of the box — a cell with `top=Double,
//!   left=Single, right=Single` naturally resolves to ╨ (208), the T-up
//!   with a double stem on a single horizontal line.

// ---------- Rectangle shape ----------

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

pub fn rect_family_for(glyph: u8) -> Option<&'static RectFamily> {
    RECT_FAMILIES.iter().find(|f| f.contains(glyph))
}

// ---------- Connected shape (Dynamic pencil) ----------

/// The family presented on one side of a cell.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum LineStyle {
    #[default]
    None,
    Single,
    Double,
}

/// Cardinal direction, used to address a cell's four connection slots and
/// to step to the corresponding neighbor coordinate.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

    pub fn opposite(self) -> Self {
        match self {
            Side::Top => Side::Bottom,
            Side::Right => Side::Left,
            Side::Bottom => Side::Top,
            Side::Left => Side::Right,
        }
    }

    /// Step `(x, y)` one cell in this direction. Can return negative
    /// coordinates when stepping off the top/left edge — callers bounds-check.
    pub fn step(self, x: u32, y: u32) -> (i32, i32) {
        let (x, y) = (x as i32, y as i32);
        match self {
            Side::Top => (x, y - 1),
            Side::Right => (x + 1, y),
            Side::Bottom => (x, y + 1),
            Side::Left => (x - 1, y),
        }
    }
}

/// A cell's 4-slot connection pattern: which family it presents on each side.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct ConnectionPattern {
    pub top: LineStyle,
    pub right: LineStyle,
    pub bottom: LineStyle,
    pub left: LineStyle,
}

impl ConnectionPattern {
    pub const EMPTY: Self = Self {
        top: LineStyle::None,
        right: LineStyle::None,
        bottom: LineStyle::None,
        left: LineStyle::None,
    };

    pub fn get(self, side: Side) -> LineStyle {
        match side {
            Side::Top => self.top,
            Side::Right => self.right,
            Side::Bottom => self.bottom,
            Side::Left => self.left,
        }
    }

    pub fn with(mut self, side: Side, style: LineStyle) -> Self {
        match side {
            Side::Top => self.top = style,
            Side::Right => self.right = style,
            Side::Bottom => self.bottom = style,
            Side::Left => self.left = style,
        }
        self
    }
}

// Short aliases for table readability — (top, right, bottom, left).
const N: LineStyle = LineStyle::None;
const S: LineStyle = LineStyle::Single;
const D: LineStyle = LineStyle::Double;

#[inline]
const fn pat(t: LineStyle, r: LineStyle, b: LineStyle, l: LineStyle) -> ConnectionPattern {
    ConnectionPattern {
        top: t,
        right: r,
        bottom: b,
        left: l,
    }
}

/// Pattern ↔ glyph table. The canonical entry for each glyph comes first so
/// reverse lookup (glyph → pattern) returns the "full" pattern (e.g. 196 →
/// horizontal with left+right both single). Stub entries later in the table
/// cover single-slot patterns for forward lookup only.
const PATTERN_TABLE: &[(ConnectionPattern, u8)] = &[
    // Straight lines
    (pat(N, S, N, S), 196), // ─
    (pat(N, D, N, D), 205), // ═
    (pat(S, N, S, N), 179), // │
    (pat(D, N, D, N), 186), // ║
    // Corners (same family)
    (pat(N, S, S, N), 218), // ┌
    (pat(N, N, S, S), 191), // ┐
    (pat(S, S, N, N), 192), // └
    (pat(S, N, N, S), 217), // ┘
    (pat(N, D, D, N), 201), // ╔
    (pat(N, N, D, D), 187), // ╗
    (pat(D, D, N, N), 200), // ╚
    (pat(D, N, N, D), 188), // ╝
    // Corners (mixed)
    (pat(N, D, S, N), 213), // ╒ TL vert=S horiz=D
    (pat(N, S, D, N), 214), // ╓ TL vert=D horiz=S
    (pat(N, N, S, D), 184), // ╕ TR vert=S horiz=D
    (pat(N, N, D, S), 183), // ╖ TR vert=D horiz=S
    (pat(S, D, N, N), 212), // ╘ BL vert=S horiz=D
    (pat(D, S, N, N), 211), // ╙ BL vert=D horiz=S
    (pat(S, N, N, D), 190), // ╛ BR vert=S horiz=D
    (pat(D, N, N, S), 189), // ╜ BR vert=D horiz=S
    // T-junctions (same family)
    (pat(S, S, N, S), 193), // ┴
    (pat(D, D, N, D), 202), // ╩
    (pat(N, S, S, S), 194), // ┬
    (pat(N, D, D, D), 203), // ╦
    (pat(S, N, S, S), 180), // ┤
    (pat(D, N, D, D), 185), // ╣
    (pat(S, S, S, N), 195), // ├
    (pat(D, D, D, N), 204), // ╠
    // T-junctions (mixed — leg differs in family from the straight line)
    (pat(D, S, N, S), 208), // ╨ T-up line=S stem=D
    (pat(S, D, N, D), 207), // ╧ T-up line=D stem=S
    (pat(N, S, D, S), 210), // ╥ T-down line=S stem=D
    (pat(N, D, S, D), 209), // ╤ T-down line=D stem=S
    (pat(S, N, S, D), 181), // ╡ T-left line=S stem=D
    (pat(D, N, D, S), 182), // ╢ T-left line=D stem=S
    (pat(S, D, S, N), 198), // ╞ T-right line=S stem=D
    (pat(D, S, D, N), 199), // ╟ T-right line=D stem=S
    // Crosses
    (pat(S, S, S, S), 197), // ┼
    (pat(D, D, D, D), 206), // ╬
    (pat(D, S, D, S), 215), // ╫ vert=D horiz=S
    (pat(S, D, S, D), 216), // ╪ vert=S horiz=D
    // Single-slot stubs (forward lookup only).
    (pat(S, N, N, N), 179),
    (pat(D, N, N, N), 186),
    (pat(N, N, S, N), 179),
    (pat(N, N, D, N), 186),
    (pat(N, S, N, N), 196),
    (pat(N, D, N, N), 205),
    (pat(N, N, N, S), 196),
    (pat(N, N, N, D), 205),
];

pub fn glyph_to_pattern(glyph: u8) -> Option<ConnectionPattern> {
    PATTERN_TABLE
        .iter()
        .find(|(_, g)| *g == glyph)
        .map(|(p, _)| *p)
}

pub fn pattern_to_glyph(pattern: ConnectionPattern) -> Option<u8> {
    PATTERN_TABLE
        .iter()
        .find(|(p, _)| *p == pattern)
        .map(|(_, g)| *g)
}

pub fn is_connected_glyph(glyph: u8) -> bool {
    PATTERN_TABLE.iter().any(|(_, g)| *g == glyph)
}

/// Pick the dominant family of a box-drawing glyph by majority slot count.
/// Used as the coercion family when a derived pattern is unsupported by any
/// CP437 glyph (e.g. `left=Single, right=Double` on a pure horizontal).
/// `LineStyle::None` indicates the glyph isn't a box-drawing character.
pub fn stroke_family(glyph: u8) -> LineStyle {
    let Some(p) = glyph_to_pattern(glyph) else {
        return LineStyle::None;
    };
    let mut s = 0u32;
    let mut d = 0u32;
    for slot in [p.top, p.right, p.bottom, p.left] {
        match slot {
            LineStyle::Single => s += 1,
            LineStyle::Double => d += 1,
            LineStyle::None => {}
        }
    }
    if d > s {
        LineStyle::Double
    } else if s > 0 {
        LineStyle::Single
    } else {
        LineStyle::None
    }
}

/// Horizontally/vertically flip a glyph. Box-drawing glyphs route through
/// the connection pattern (swap left↔right for `flip_h`, top↔bottom for
/// `flip_v`) so corners, T-junctions, and mixed-family glyphs pick up the
/// correct flipped variant. Non-box glyphs pass through unchanged —
/// character glyphs don't have a sensible flipped form.
pub fn flip_glyph(glyph: u8, flip_h: bool, flip_v: bool) -> u8 {
    if !flip_h && !flip_v {
        return glyph;
    }
    let Some(mut pattern) = glyph_to_pattern(glyph) else {
        return glyph;
    };
    if flip_h {
        std::mem::swap(&mut pattern.left, &mut pattern.right);
    }
    if flip_v {
        std::mem::swap(&mut pattern.top, &mut pattern.bottom);
    }
    pattern_to_glyph(pattern).unwrap_or(glyph)
}

/// If `pattern` has mismatched opposite slots (e.g. top=S, bottom=D) force
/// the mismatched axis to `family`, which generally gets us back to a
/// lookup-table entry. No-op for `family == None`.
pub fn coerce_to_family(mut pattern: ConnectionPattern, family: LineStyle) -> ConnectionPattern {
    if matches!(family, LineStyle::None) {
        return pattern;
    }
    let mismatched_vert = !matches!(pattern.top, LineStyle::None)
        && !matches!(pattern.bottom, LineStyle::None)
        && pattern.top != pattern.bottom;
    if mismatched_vert {
        pattern.top = family;
        pattern.bottom = family;
    }
    let mismatched_horiz = !matches!(pattern.left, LineStyle::None)
        && !matches!(pattern.right, LineStyle::None)
        && pattern.left != pattern.right;
    if mismatched_horiz {
        pattern.left = family;
        pattern.right = family;
    }
    pattern
}
