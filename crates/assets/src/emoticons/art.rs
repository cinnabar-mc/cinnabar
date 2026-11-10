//! Original pixel art for the built-in emoticons, composed from a few outlines and small
//! letters. Each art row uses one palette symbol per texel; `.` is transparent.

use super::BuiltinEmoticon;

/// Controller and mouse emoticons named by control hints. The code points follow public
/// community documentation of vanilla's built-in page; they are not verified against the
/// game's own list.
pub(super) static BUILTIN: [BuiltinEmoticon; 19] = [
    face("xbox_face_button_down", '\u{e000}', LETTER_A, b'G'),
    face("xbox_face_button_right", '\u{e001}', LETTER_B, b'R'),
    face("xbox_face_button_left", '\u{e002}', LETTER_X, b'B'),
    face("xbox_face_button_up", '\u{e003}', LETTER_Y, b'Y'),
    shoulder("xbox_bumper_left", '\u{e004}', [SMALL_L, SMALL_B], false),
    shoulder("xbox_bumper_right", '\u{e005}', [SMALL_R, SMALL_B], false),
    shoulder("xbox_trigger_left", '\u{e006}', [SMALL_L, SMALL_T], true),
    shoulder("xbox_trigger_right", '\u{e007}', [SMALL_R, SMALL_T], true),
    emoticon("xbox_select", '\u{e008}', Art::Menu(VIEW)),
    emoticon("xbox_start", '\u{e009}', Art::Menu(MENU)),
    emoticon("xbox_stick_left", '\u{e00a}', Art::Stick(SMALL_L)),
    emoticon("xbox_stick_right", '\u{e00b}', Art::Stick(SMALL_R)),
    emoticon("xbox_dpad_up", '\u{e00c}', Art::DPad(b'U')),
    emoticon("xbox_dpad_left", '\u{e00d}', Art::DPad(b'<')),
    emoticon("xbox_dpad_down", '\u{e00e}', Art::DPad(b'D')),
    emoticon("xbox_dpad_right", '\u{e00f}', Art::DPad(b'>')),
    emoticon("mouse_left_button", '\u{e060}', Art::Mouse(b'L')),
    emoticon("mouse_right_button", '\u{e061}', Art::Mouse(b'N')),
    emoticon("mouse_middle_button", '\u{e062}', Art::Mouse(b'M')),
];

/// How one emoticon is drawn.
pub(super) enum Art {
    /// A dark disc with a coloured letter.
    Face { letter: Symbol, ink: u8 },
    /// A bumper or, taller, a trigger, labelled with two small letters.
    Shoulder { letters: [Small; 2], trigger: bool },
    /// A dark disc with a white menu symbol.
    Menu(Symbol),
    /// A light disc with a small dark letter.
    Stick(Small),
    /// A cross with the arm whose template marker is given lit.
    DPad(u8),
    /// A mouse with the button whose template marker is given lit.
    Mouse(u8),
}

/// A 5-by-5 symbol: `w` is ink, `.` is the background.
type Symbol = [&'static str; 5];
/// A 3-by-5 letter: `w` is ink, `.` is the background.
type Small = [&'static str; 5];

const fn emoticon(name: &'static str, codepoint: char, art: Art) -> BuiltinEmoticon {
    BuiltinEmoticon {
        name,
        codepoint,
        art,
    }
}

const fn face(name: &'static str, codepoint: char, letter: Symbol, ink: u8) -> BuiltinEmoticon {
    emoticon(name, codepoint, Art::Face { letter, ink })
}

const fn shoulder(
    name: &'static str,
    codepoint: char,
    letters: [Small; 2],
    trigger: bool,
) -> BuiltinEmoticon {
    emoticon(name, codepoint, Art::Shoulder { letters, trigger })
}

const LETTER_A: Symbol = [".www.", "w...w", "wwwww", "w...w", "w...w"];
const LETTER_B: Symbol = ["wwww.", "w...w", "wwww.", "w...w", "wwww."];
const LETTER_X: Symbol = ["w...w", ".w.w.", "..w..", ".w.w.", "w...w"];
const LETTER_Y: Symbol = ["w...w", ".w.w.", "..w..", "..w..", "..w.."];
const VIEW: Symbol = ["www..", "w.www", "www.w", "..w.w", "..www"];
const MENU: Symbol = ["wwwww", ".....", "wwwww", ".....", "wwwww"];
const SMALL_L: Small = ["w..", "w..", "w..", "w..", "www"];
const SMALL_R: Small = ["ww.", "w.w", "ww.", "w.w", "w.w"];
const SMALL_B: Small = ["ww.", "w.w", "ww.", "w.w", "ww."];
const SMALL_T: Small = ["www", ".w.", ".w.", ".w.", ".w."];

/// A disc; `#` is its fill.
const DISC: [&str; 11] = [
    "...kkkkk...",
    ".kk#####kk.",
    ".k#######k.",
    "k#########k",
    "k#########k",
    "k#########k",
    "k#########k",
    "k#########k",
    ".k#######k.",
    ".kk#####kk.",
    "...kkkkk...",
];
const BUMPER: [&str; 7] = [
    ".kkkkkkkkk.",
    "k#########k",
    "k#########k",
    "k#########k",
    "k#########k",
    "k#########k",
    ".kkkkkkkkk.",
];
const TRIGGER: [&str; 9] = [
    "..kkkkkkk..",
    ".k#######k.",
    "k#########k",
    "k#########k",
    "k#########k",
    "k#########k",
    "k#########k",
    "k#########k",
    "kkkkkkkkkkk",
];
/// Arms are marked `U`, `D`, `<` and `>`; the centre is `g`.
const DPAD: [&str; 11] = [
    "...kkkkk...",
    "...kUUUk...",
    "...kUUUk...",
    "kkkkUUUkkkk",
    "k<<<ggg>>>k",
    "k<<<ggg>>>k",
    "k<<<ggg>>>k",
    "kkkkDDDkkkk",
    "...kDDDk...",
    "...kDDDk...",
    "...kkkkk...",
];
/// Buttons are marked `L` (left), `M` (wheel) and `N` (right).
const MOUSE: [&str; 12] = [
    "..kkkkk..",
    ".kLLMNNk.",
    "kLLLMNNNk",
    "kLLLMNNNk",
    "kLLLMNNNk",
    "kkkkkkkkk",
    "klllllllk",
    "klllllllk",
    "klllllllk",
    "klllllllk",
    ".klllllk.",
    "..kkkkk..",
];

impl Art {
    /// The art's rows, one palette symbol per texel.
    pub(super) fn rows(&self) -> Vec<String> {
        match self {
            Self::Face { letter, ink } => disc(b'd', letter, *ink, 3),
            Self::Menu(symbol) => disc(b'd', symbol, b'w', 3),
            Self::Stick(letter) => disc(b'l', letter, b'd', 4),
            Self::Shoulder { letters, trigger } => {
                let (outline, top) = if *trigger {
                    (&TRIGGER[..], 2)
                } else {
                    (&BUMPER[..], 1)
                };
                let mut rows = filled(outline, b'd');
                for (letter, left) in letters.iter().zip([2, 6]) {
                    stamp(&mut rows, letter, [left, top], b'w');
                }
                finish(rows)
            }
            Self::DPad(lit) => marked(&DPAD, *lit, b'w', b'g'),
            Self::Mouse(lit) => {
                // An unlit wheel is the outline dividing the two buttons.
                let divider = if *lit == b'M' { b'M' } else { b'k' };
                let template: Vec<String> = MOUSE
                    .iter()
                    .map(|row| row.replace('M', &char::from(divider).to_string()))
                    .collect();
                marked(&template, *lit, b'R', b'l')
            }
        }
    }
}

/// RGBA for a palette symbol, or `None` for transparent and unknown symbols.
pub(super) fn color(symbol: u8) -> Option<[u8; 4]> {
    Some(match symbol {
        b'k' => [0x1c, 0x1c, 0x1c, 0xff],
        b'd' => [0x48, 0x48, 0x48, 0xff],
        b'g' => [0x8b, 0x8b, 0x8b, 0xff],
        b'l' => [0xd2, 0xd2, 0xd2, 0xff],
        b'w' => [0xff, 0xff, 0xff, 0xff],
        b'G' => [0x5c, 0xc8, 0x3c, 0xff],
        b'R' => [0xe8, 0x4a, 0x3c, 0xff],
        b'B' => [0x4a, 0x9a, 0xf0, 0xff],
        b'Y' => [0xf2, 0xc8, 0x2e, 0xff],
        _ => return None,
    })
}

/// A disc filled with `fill`, with `symbol` stamped in `ink` from column `left`, row 3.
fn disc(fill: u8, symbol: &[&str; 5], ink: u8, left: usize) -> Vec<String> {
    let mut rows = filled(&DISC, fill);
    stamp(&mut rows, symbol, [left, 3], ink);
    finish(rows)
}

/// The template's rows with its `#` fill replaced by `fill`.
fn filled(template: &[&str], fill: u8) -> Vec<Vec<u8>> {
    template
        .iter()
        .map(|row| {
            row.bytes()
                .map(|symbol| if symbol == b'#' { fill } else { symbol })
                .collect()
        })
        .collect()
}

/// Paints the `w` texels of `symbol` in `ink`, with its top-left texel at `[left, top]`.
fn stamp(rows: &mut [Vec<u8>], symbol: &[&str; 5], [left, top]: [usize; 2], ink: u8) {
    for (y, line) in symbol.iter().enumerate() {
        for (x, texel) in line.bytes().enumerate() {
            if texel == b'w' {
                rows[top + y][left + x] = ink;
            }
        }
    }
}

/// The template with marker `lit` in `ink` and every other marker in `unlit`.
fn marked(template: &[impl AsRef<str>], lit: u8, ink: u8, unlit: u8) -> Vec<String> {
    let rows = template
        .iter()
        .map(|row| {
            row.as_ref()
                .bytes()
                .map(|symbol| match symbol {
                    _ if symbol == lit => ink,
                    b'U' | b'D' | b'<' | b'>' | b'L' | b'M' | b'N' => unlit,
                    _ => symbol,
                })
                .collect()
        })
        .collect();
    finish(rows)
}

fn finish(rows: Vec<Vec<u8>>) -> Vec<String> {
    rows.into_iter()
        .map(|row| String::from_utf8(row).expect("palette symbols are ASCII"))
        .collect()
}
