use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PawSide {
    Left,
    Right,
}

/// Canonical 65% ANSI physical key identifiers (plus nearest-hand fallbacks for non-65% keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum PhysicalKey {
    // Row 0 (cat's top / viewer's bottom): Esc + 1..0 + - + = + Backspace + Backtick/Del
    Esc = 0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Digit0,
    Minus,
    Equal,
    Backspace,
    Delete,

    // Row 1: Tab + Q..P + [ ] \ + PageUp
    Tab,
    Q,
    W,
    E,
    R,
    T,
    Y,
    U,
    I,
    O,
    P,
    LBracket,
    RBracket,
    Backslash,
    PageUp,

    // Row 2: Caps + A..L + ; ' + Enter + PageDown
    CapsLock,
    A,
    S,
    D,
    F,
    G,
    H,
    J,
    K,
    L,
    Semicolon,
    Quote,
    Enter,
    PageDown,

    // Row 3: LShift + Z..M + , . / + RShift + Up + End
    LShift,
    Z,
    X,
    C,
    V,
    B,
    N,
    M,
    Comma,
    Period,
    Slash,
    RShift,
    Up,
    End,

    // Row 4 (cat's bottom / viewer's top nearest cat): LCtrl, LWin, LAlt, Space, RAlt, Fn, RCtrl, Left, Down, Right
    LCtrl,
    LWin,
    LAlt,
    Space,
    RAlt,
    Fn,
    RCtrl,
    Left,
    Down,
    Right,

    // Fallbacks for non-65% physical keys (F1..F12, Numpad, ISO extra)
    Non65Left,
    Non65Right,
}

impl PhysicalKey {
    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn is_on_board(self) -> bool {
        (self as usize) < KEYCAP_COUNT
    }
}

pub const KEYCAP_COUNT: usize = 68;

/// Static layout metadata for each keycap on the cat-facing 65% board.
///
/// Anatomical cat perspective (matching LittleOne screenshots):
/// - The cat sits behind the keyboard facing the viewer.
/// - Row 4 (Spacebar) sits nearest the cat (`kb_y` negative / upper in viewer space).
/// - Row 0 (Esc + Numbers) sits nearest the viewer (`kb_y` positive / lower in viewer space).
/// - Cat's left hand sits on viewer's RIGHT (`kb_x > 0`); cat's right hand sits on viewer's LEFT (`kb_x < 0`).
#[derive(Debug, Clone, Copy)]
pub struct KeyCapSpec {
    pub key: PhysicalKey,
    pub row: u8,
    pub col_u: f32,
    pub width_u: f32,
    pub is_accent: bool,
    pub paw_side: PawSide,
    /// Hand zone `0..4` for left paw, `4..8` for right paw.
    pub zone: u8,
    pub label: &'static str,
    /// Normalized keyboard coordinates in `[-1.0, 1.0]` in viewer screen orientation.
    pub kb_x: f32,
    pub kb_y: f32,
}

const TOTAL_U: f32 = 16.0;

const fn make_spec(
    key: PhysicalKey,
    row: u8,
    cat_col_u: f32,
    width_u: f32,
    is_accent: bool,
    paw_side: PawSide,
    zone: u8,
    label: &'static str,
) -> KeyCapSpec {
    // Cat looks from behind the keyboard toward the viewer:
    // cat_col_u = 0 (Esc, cat's left) -> viewer RIGHT (+1.0)
    // cat_col_u = 16 (Del, cat's right) -> viewer LEFT (-1.0)
    let center_u = cat_col_u + width_u * 0.5;
    let kb_x = 1.0 - (center_u / TOTAL_U) * 2.0;
    // row 4 (Spacebar, nearest cat) -> top (-0.80); row 0 (Esc/digits, furthest from cat) -> bottom (+0.80)
    let kb_y = 0.80 - (row as f32) * 0.40;
    KeyCapSpec {
        key,
        row,
        col_u: cat_col_u,
        width_u,
        is_accent,
        paw_side,
        zone,
        label,
        kb_x,
        kb_y,
    }
}

use PawSide::{Left, Right};
use PhysicalKey::*;

const KEYCAPS: [KeyCapSpec; KEYCAP_COUNT] = [
    // Row 0 (16u total): Esc(1, accent), 1..0(10), -(1), =(1), Backspace(2), Del(1)
    make_spec(Esc, 0, 0.0, 1.0, true, Left, 0, "ESC"),
    make_spec(Digit1, 0, 1.0, 1.0, false, Left, 0, "1"),
    make_spec(Digit2, 0, 2.0, 1.0, false, Left, 0, "2"),
    make_spec(Digit3, 0, 3.0, 1.0, false, Left, 1, "3"),
    make_spec(Digit4, 0, 4.0, 1.0, false, Left, 1, "4"),
    make_spec(Digit5, 0, 5.0, 1.0, false, Left, 2, "5"),
    make_spec(Digit6, 0, 6.0, 1.0, false, Left, 2, "6"),
    make_spec(Digit7, 0, 7.0, 1.0, false, Right, 4, "7"),
    make_spec(Digit8, 0, 8.0, 1.0, false, Right, 5, "8"),
    make_spec(Digit9, 0, 9.0, 1.0, false, Right, 5, "9"),
    make_spec(Digit0, 0, 10.0, 1.0, false, Right, 6, "0"),
    make_spec(Minus, 0, 11.0, 1.0, false, Right, 6, "-"),
    make_spec(Equal, 0, 12.0, 1.0, false, Right, 7, "="),
    make_spec(Backspace, 0, 13.0, 2.0, false, Right, 7, "BKSP"),
    make_spec(Delete, 0, 15.0, 1.0, false, Right, 7, "DEL"),
    // Row 1 (16u total): Tab(1.5), Q..P(10), [(1), ](1), \(1.5), PgUp(1)
    make_spec(Tab, 1, 0.0, 1.5, false, Left, 0, "TAB"),
    make_spec(Q, 1, 1.5, 1.0, false, Left, 0, "Q"),
    make_spec(W, 1, 2.5, 1.0, false, Left, 1, "W"),
    make_spec(E, 1, 3.5, 1.0, false, Left, 1, "E"),
    make_spec(R, 1, 4.5, 1.0, false, Left, 2, "R"),
    make_spec(T, 1, 5.5, 1.0, false, Left, 2, "T"),
    make_spec(Y, 1, 6.5, 1.0, false, Right, 4, "Y"),
    make_spec(U, 1, 7.5, 1.0, false, Right, 4, "U"),
    make_spec(I, 1, 8.5, 1.0, false, Right, 5, "I"),
    make_spec(O, 1, 9.5, 1.0, false, Right, 6, "O"),
    make_spec(P, 1, 10.5, 1.0, false, Right, 6, "P"),
    make_spec(LBracket, 1, 11.5, 1.0, false, Right, 6, "["),
    make_spec(RBracket, 1, 12.5, 1.0, false, Right, 7, "]"),
    make_spec(Backslash, 1, 13.5, 1.5, false, Right, 7, "\\"),
    make_spec(PageUp, 1, 15.0, 1.0, false, Right, 7, "PGUP"),
    // Row 2 (16u total): Caps(1.75), A..L(9), ;(1), '(1), Enter(2.25, accent!), PgDn(1)
    make_spec(CapsLock, 2, 0.0, 1.75, false, Left, 0, "CAPS"),
    make_spec(A, 2, 1.75, 1.0, false, Left, 0, "A"),
    make_spec(S, 2, 2.75, 1.0, false, Left, 1, "S"),
    make_spec(D, 2, 3.75, 1.0, false, Left, 1, "D"),
    make_spec(F, 2, 4.75, 1.0, false, Left, 2, "F"),
    make_spec(G, 2, 5.75, 1.0, false, Left, 3, "G"),
    make_spec(H, 2, 6.75, 1.0, false, Right, 4, "H"),
    make_spec(J, 2, 7.75, 1.0, false, Right, 4, "J"),
    make_spec(K, 2, 8.75, 1.0, false, Right, 5, "K"),
    make_spec(L, 2, 9.75, 1.0, false, Right, 6, "L"),
    make_spec(Semicolon, 2, 10.75, 1.0, false, Right, 6, ";"),
    make_spec(Quote, 2, 11.75, 1.0, false, Right, 7, "'"),
    make_spec(Enter, 2, 12.75, 2.25, true, Right, 7, "ENT"),
    make_spec(PageDown, 2, 15.0, 1.0, false, Right, 7, "PGDN"),
    // Row 3 (16u total): LShift(2.25), Z..M(7), ,(1), .(1), /(1), RShift(1.75), Up(1), End(1)
    make_spec(LShift, 3, 0.0, 2.25, false, Left, 0, "SHFT"),
    make_spec(Z, 3, 2.25, 1.0, false, Left, 0, "Z"),
    make_spec(X, 3, 3.25, 1.0, false, Left, 1, "X"),
    make_spec(C, 3, 4.25, 1.0, false, Left, 2, "C"),
    make_spec(V, 3, 5.25, 1.0, false, Left, 2, "V"),
    make_spec(B, 3, 6.25, 1.0, false, Left, 3, "B"),
    make_spec(N, 3, 7.25, 1.0, false, Right, 4, "N"),
    make_spec(M, 3, 8.25, 1.0, false, Right, 5, "M"),
    make_spec(Comma, 3, 9.25, 1.0, false, Right, 5, ","),
    make_spec(Period, 3, 10.25, 1.0, false, Right, 6, "."),
    make_spec(Slash, 3, 11.25, 1.0, false, Right, 6, "/"),
    make_spec(RShift, 3, 12.25, 1.75, false, Right, 7, "SHFT"),
    make_spec(Up, 3, 14.0, 1.0, false, Right, 7, "^"),
    make_spec(End, 3, 15.0, 1.0, false, Right, 7, "END"),
    // Row 4 (16u total): LCtrl(1.25), LWin(1.25), LAlt(1.25), Space(6.25), RAlt(1), Fn(1), RCtrl(1), Left(1), Down(1), Right(1)
    make_spec(LCtrl, 4, 0.0, 1.25, false, Left, 0, "CTRL"),
    make_spec(LWin, 4, 1.25, 1.25, false, Left, 1, "WIN"),
    make_spec(LAlt, 4, 2.5, 1.25, false, Left, 2, "ALT"),
    make_spec(Space, 4, 3.75, 6.25, false, Left, 3, "SPACE"),
    make_spec(RAlt, 4, 10.0, 1.0, false, Right, 5, "ALT"),
    make_spec(Fn, 4, 11.0, 1.0, false, Right, 6, "FN"),
    make_spec(RCtrl, 4, 12.0, 1.0, false, Right, 6, "CTRL"),
    make_spec(PhysicalKey::Left, 4, 13.0, 1.0, false, Right, 7, "<"),
    make_spec(PhysicalKey::Down, 4, 14.0, 1.0, false, Right, 7, "v"),
    make_spec(PhysicalKey::Right, 4, 15.0, 1.0, false, Right, 7, ">"),
];

pub fn keycap_specs() -> &'static [KeyCapSpec; KEYCAP_COUNT] {
    &KEYCAPS
}

pub fn lookup_keycap(key: PhysicalKey) -> &'static KeyCapSpec {
    match key {
        Non65Left => &KEYCAPS[D.index()],
        Non65Right => &KEYCAPS[K.index()],
        other => &KEYCAPS[other.index()],
    }
}

/// Representative keycaps for each of the 8 anonymized typing zones (0..4 left paw, 4..8 right paw).
pub fn anonymized_key_for_zone(zone: u8, salt: u32) -> PhysicalKey {
    const ZONE_KEYS: [&[PhysicalKey]; 8] = [
        &[A, Q, Z, Tab],
        &[W, S, E, D, X],
        &[R, F, T, C, V],
        &[G, B, Space, F],
        &[H, J, Y, U, N],
        &[I, K, M, Comma],
        &[O, L, P, Period],
        &[Semicolon, Enter, Backspace, Quote],
    ];
    let bucket = ZONE_KEYS[(zone as usize) % 8];
    bucket[(salt as usize) % bucket.len()]
}

/// Map a Windows PS/2 Set-1 Raw Input `(make_code, e0, e1)` tuple into a canonical `PhysicalKey`.
pub fn from_windows_scancode(make_code: u16, e0: bool, e1: bool) -> Option<PhysicalKey> {
    if e1 {
        // Pause/Break sequence
        return Some(Non65Right);
    }
    if e0 {
        return match make_code {
            0x1C => Some(Enter),     // Numpad Enter
            0x1D => Some(RCtrl),
            0x35 => Some(Slash),     // Numpad /
            0x38 => Some(RAlt),
            0x47 => Some(PageUp),    // Home fallback
            0x48 => Some(Up),
            0x49 => Some(PageUp),
            0x4B => Some(PhysicalKey::Left),
            0x4D => Some(PhysicalKey::Right),
            0x4F => Some(End),
            0x50 => Some(Down),
            0x51 => Some(PageDown),
            0x52 => Some(Delete),    // Insert fallback
            0x53 => Some(Delete),
            0x5B => Some(LWin),
            0x5C => Some(Fn),        // RWin -> Fn
            0x5D => Some(RCtrl),     // Apps key
            _ => Some(Non65Right),
        };
    }

    match make_code {
        0x01 => Some(Esc),
        0x02 => Some(Digit1),
        0x03 => Some(Digit2),
        0x04 => Some(Digit3),
        0x05 => Some(Digit4),
        0x06 => Some(Digit5),
        0x07 => Some(Digit6),
        0x08 => Some(Digit7),
        0x09 => Some(Digit8),
        0x0A => Some(Digit9),
        0x0B => Some(Digit0),
        0x0C => Some(Minus),
        0x0D => Some(Equal),
        0x0E => Some(Backspace),
        0x0F => Some(Tab),
        0x10 => Some(Q),
        0x11 => Some(W),
        0x12 => Some(E),
        0x13 => Some(R),
        0x14 => Some(T),
        0x15 => Some(Y),
        0x16 => Some(U),
        0x17 => Some(I),
        0x18 => Some(O),
        0x19 => Some(P),
        0x1A => Some(LBracket),
        0x1B => Some(RBracket),
        0x1C => Some(Enter),
        0x1D => Some(LCtrl),
        0x1E => Some(A),
        0x1F => Some(S),
        0x20 => Some(D),
        0x21 => Some(F),
        0x22 => Some(G),
        0x23 => Some(H),
        0x24 => Some(J),
        0x25 => Some(K),
        0x26 => Some(L),
        0x27 => Some(Semicolon),
        0x28 => Some(Quote),
        0x29 => Some(Esc), // Backtick -> Esc position on 65%
        0x2A => Some(LShift),
        0x2B => Some(Backslash),
        0x2C => Some(Z),
        0x2D => Some(X),
        0x2E => Some(C),
        0x2F => Some(V),
        0x30 => Some(B),
        0x31 => Some(N),
        0x32 => Some(M),
        0x33 => Some(Comma),
        0x34 => Some(Period),
        0x35 => Some(Slash),
        0x36 => Some(RShift),
        0x37 => Some(Non65Right), // Numpad *
        0x38 => Some(LAlt),
        0x39 => Some(Space),
        0x3A => Some(CapsLock),
        0x3B..=0x40 => Some(Non65Left),  // F1..F6
        0x41..=0x44 => Some(Non65Right), // F7..F10
        0x47..=0x53 => Some(Non65Right), // Numpad cluster
        0x56 => Some(LShift),            // ISO extra key next to LShift
        0x57 | 0x58 => Some(Non65Right), // F11..F12
        0x00 | 0xFF => None,
        _ => Some(Non65Left),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycap_indices_match_enum_order() {
        for (idx, spec) in keycap_specs().iter().enumerate() {
            assert_eq!(spec.key.index(), idx, "mismatch at index {idx}");
            assert!(spec.zone < 8);
        }
    }

    #[test]
    fn anatomical_cat_perspective_matches_screenshots() {
        let esc = lookup_keycap(Esc);
        let enter = lookup_keycap(Enter);
        let space = lookup_keycap(Space);
        // Esc sits at viewer bottom-right (accent)
        assert!(esc.is_accent);
        assert!(esc.kb_x > 0.7 && esc.kb_y > 0.5);
        // Enter sits at viewer middle-left (accent)
        assert!(enter.is_accent);
        assert!(enter.kb_x < -0.5);
        // Spacebar sits along top edge nearest cat
        assert!(space.kb_y < -0.5);
    }

    #[test]
    fn scancode_mapping_handles_extended_and_synthetic() {
        assert_eq!(from_windows_scancode(0x1E, false, false), Some(A));
        assert_eq!(from_windows_scancode(0x48, true, false), Some(Up));
        assert_eq!(from_windows_scancode(0x53, true, false), Some(Delete));
        assert_eq!(from_windows_scancode(0xFF, false, false), None);
    }
}
