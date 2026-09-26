//! The game's one typeface: a 5x9 bitmap font, drawn as ordinary sprites.
//!
//! Text is pixels here rather than a TTF for two reasons. The world is drawn
//! at 640x360 and scaled up, so a vector font is either blurred or aliased
//! against pixel art. And the headless renderer ([`crate::render::cpu`]) has
//! to draw exactly what the window draws — a screenshot an agent reads to
//! check a dialogue box is only evidence if its text is the game's text. A
//! bitmap that both backends blit from one atlas is the same glyph in both,
//! with no font rasterizer in either.
//!
//! The glyphs are authored below as text, sixteen to a block, one `#` per lit
//! pixel. Caps and digits are 7 rows tall with the baseline under row 6;
//! rows 7-8 are for descenders. Widths are proportional: a glyph is as wide
//! as its rightmost lit column, and one column of air separates two glyphs.

use std::sync::OnceLock;

/// Rows in a glyph cell, descenders included.
pub const GLYPH_H: u32 = 9;
/// Columns in a glyph cell. Most glyphs use all five; `i` uses one.
pub const GLYPH_W: u32 = 5;
/// Vertical advance between two lines of text.
pub const LINE_H: f32 = 11.0;
/// Horizontal advance of a space.
const SPACE_W: u32 = 3;
/// Air between two glyphs.
const GAP: u32 = 1;

/// Atlas cell, one pixel bigger than a glyph each way so nearest sampling at a
/// cell edge can never pick up a neighbour.
const CELL_W: u32 = GLYPH_W + 1;
const CELL_H: u32 = GLYPH_H + 1;
const ATLAS_COLS: u32 = 16;

/// Every printable ASCII glyph from `!` to `~`, sixteen to a block. Each row
/// string holds one row of every glyph in the block, five columns each,
/// separated by a space.
#[rustfmt::skip]
const BLOCKS: &[(&str, [&str; GLYPH_H as usize])] = &[
    (
        "!\"#$%&'()*+,-./0",
        [
            "#.... #.#.. .#.#. ..#.. ##... .##.. #.... ..#.. #.... ..... ..... ..... ..... ..... ....# .###.",
            "#.... #.#.. .#.#. .#### ##..# #..#. #.... .#... .#... ..#.. ..#.. ..... ..... ..... ....# #...#",
            "#.... ..... ##### #.#.. ...#. #.#.. ..... #.... ..#.. #.#.# ..#.. ..... ..... ..... ...#. #..##",
            "#.... ..... .#.#. .###. ..#.. .#... ..... #.... ..#.. .###. ##### ..... ####. ..... ..#.. #.#.#",
            "#.... ..... ##### ..#.# .#... #.#.# ..... #.... ..#.. #.#.# ..#.. ..... ..... ..... .#... ##..#",
            "..... ..... .#.#. ####. #..## #..#. ..... .#... .#... ..#.. ..#.. .#... ..... ..... #.... #...#",
            "#.... ..... .#.#. ..#.. ...## .##.# ..... ..#.. #.... ..... ..... .#... ..... #.... #.... .###.",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... #.... ..... ..... ..... .....",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
        ],
    ),
    (
        "123456789:;<=>?@",
        [
            ".#... .###. ####. ...#. ##### ..##. ##### .###. .###. ..... ..... ...#. ..... #.... .###. .###.",
            "##... #...# ....# ..##. #.... .#... ....# #...# #...# ..... ..... ..#.. ..... .#... #...# #...#",
            ".#... ....# ....# .#.#. ####. #.... ...#. #...# #...# #.... .#... .#... ####. ..#.. ....# #.###",
            ".#... ...#. .###. #..#. ....# ####. ..#.. .###. .#### ..... ..... #.... ..... ...#. ...#. #.#.#",
            ".#... ..#.. ....# ##### ....# #...# .#... #...# ....# ..... ..... .#... ####. ..#.. ..#.. #.###",
            ".#... .#... ....# ...#. #...# #...# .#... #...# ...#. #.... .#... ..#.. ..... .#... ..... #....",
            "###.. ##### ####. ...#. .###. .###. .#... .###. .##.. ..... #.... ...#. ..... #.... ..#.. .####",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
        ],
    ),
    (
        "ABCDEFGHIJKLMNOP",
        [
            ".###. ####. .###. ####. ##### ##### .###. #...# ###.. ..### #...# #.... #...# #...# .###. ####.",
            "#...# #...# #...# #...# #.... #.... #...# #...# .#... ...#. #..#. #.... ##.## #...# #...# #...#",
            "#...# #...# #.... #...# #.... #.... #.... #...# .#... ...#. #.#.. #.... #.#.# ##..# #...# #...#",
            "##### ####. #.... #...# ####. ####. #.### ##### .#... ...#. ##... #.... #.#.# #.#.# #...# ####.",
            "#...# #...# #.... #...# #.... #.... #...# #...# .#... ...#. #.#.. #.... #...# #..## #...# #....",
            "#...# #...# #...# #...# #.... #.... #...# #...# .#... #..#. #..#. #.... #...# #...# #...# #....",
            "#...# ####. .###. ####. ##### #.... .#### #...# ###.. .##.. #...# ##### #...# #...# .###. #....",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
        ],
    ),
    (
        "QRSTUVWXYZ[\\]^_`",
        [
            ".###. ####. .#### ##### #...# #...# #...# #...# #...# ##### ###.. #.... ###.. ..#.. ..... #....",
            "#...# #...# #.... ..#.. #...# #...# #...# #...# #...# ....# #.... #.... ..#.. .#.#. ..... .#...",
            "#...# #...# #.... ..#.. #...# #...# #...# .#.#. .#.#. ...#. #.... .#... ..#.. #...# ..... .....",
            "#...# ####. .###. ..#.. #...# #...# #.#.# ..#.. ..#.. ..#.. #.... ..#.. ..#.. ..... ..... .....",
            "#.#.# #.#.. ....# ..#.. #...# #...# #.#.# .#.#. ..#.. .#... #.... ...#. ..#.. ..... ..... .....",
            "#..#. #..#. ....# ..#.. #...# .#.#. #.#.# #...# ..#.. #.... #.... ....# ..#.. ..... ..... .....",
            ".##.# #...# ####. ..#.. .###. ..#.. .#.#. #...# ..#.. ##### ###.. ....# ###.. ..... ..... .....",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ##### .....",
            "..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... ..... .....",
        ],
    ),
    (
        "abcdefghijklmnop",
        [
            "..... #.... ..... ....# ..... ..##. ..... #.... #.... ..#.. #.... #.... ..... ..... ..... .....",
            "..... #.... ..... ....# ..... .#... ..... #.... ..... ..... #.... #.... ..... ..... ..... .....",
            ".###. ####. .###. .#### .###. ###.. .#### ####. #.... ..#.. #..#. #.... ##.#. ####. .###. ####.",
            "....# #...# #.... #...# #...# .#... #...# #...# #.... ..#.. #.#.. #.... #.#.# #...# #...# #...#",
            ".#### #...# #.... #...# ##### .#... #...# #...# #.... ..#.. ##... #.... #.#.# #...# #...# #...#",
            "#...# #...# #.... #...# #.... .#... #...# #...# #.... ..#.. #.#.. #.... #.#.# #...# #...# #...#",
            ".#### ####. .###. .#### .###. .#... .#### #...# #.... ..#.. #..#. .#... #.#.# #...# .###. ####.",
            "..... ..... ..... ..... ..... ..... ....# ..... ..... #.#.. ..... ..... ..... ..... ..... #....",
            "..... ..... ..... ..... ..... ..... .###. ..... ..... .#... ..... ..... ..... ..... ..... #....",
        ],
    ),
    (
        "qrstuvwxyz{|}~",
        [
            "..... ..... ..... .#... ..... ..... ..... ..... ..... ..... ..##. #.... .##.. .....",
            "..... ..... ..... .#... ..... ..... ..... ..... ..... ..... .#... #.... ...#. .....",
            ".#### #.##. .#### ###.. #...# #...# #...# #...# #...# ##### .#... #.... ...#. .#..#",
            "#...# ##..# #.... .#... #...# #...# #...# .#.#. #...# ...#. #.... #.... ....# #.##.",
            "#...# #.... .###. .#... #...# #...# #.#.# ..#.. #...# ..#.. .#... #.... ...#. .....",
            "#...# #.... ....# .#... #..## .#.#. #.#.# .#.#. #...# .#... .#... #.... ...#. .....",
            ".#### #.... ####. ..##. .##.# ..#.. .#.#. #...# .#### ##### ..##. #.... .##.. .....",
            "....# ..... ..... ..... ..... ..... ..... ..... ....# ..... ..... #.... ..... .....",
            "....# ..... ..... ..... ..... ..... ..... ..... .###. ..... ..... ..... ..... .....",
        ],
    ),
];

/// One glyph: where it is in the atlas and how wide it draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    /// Top-left of the glyph's cell in [`atlas`], in pixels.
    pub atlas: (u32, u32),
    /// Lit columns, counted from the left of the cell.
    pub width: u32,
}

struct Font {
    /// Indexed by `char as usize - 33`.
    glyphs: Vec<Glyph>,
    atlas: image::RgbaImage,
}

fn font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(build)
}

fn build() -> Font {
    let count = BLOCKS
        .iter()
        .map(|(chars, _)| chars.chars().count())
        .sum::<usize>() as u32;
    let rows = count.div_ceil(ATLAS_COLS);
    let mut atlas = image::RgbaImage::new(ATLAS_COLS * CELL_W, rows * CELL_H);
    let mut glyphs = Vec::with_capacity(count as usize);

    for (chars, lines) in BLOCKS {
        for (slot, _) in chars.chars().enumerate() {
            let index = glyphs.len() as u32;
            let origin = ((index % ATLAS_COLS) * CELL_W, (index / ATLAS_COLS) * CELL_H);
            let mut width = 0;
            for (y, line) in lines.iter().enumerate() {
                let cells: Vec<char> = line.chars().collect();
                for x in 0..GLYPH_W as usize {
                    if cells[slot * (GLYPH_W as usize + 1) + x] == '#' {
                        atlas.put_pixel(
                            origin.0 + x as u32,
                            origin.1 + y as u32,
                            image::Rgba([255, 255, 255, 255]),
                        );
                        width = width.max(x as u32 + 1);
                    }
                }
            }
            glyphs.push(Glyph {
                atlas: origin,
                width: width.max(1),
            });
        }
    }
    Font { glyphs, atlas }
}

/// The glyph atlas: white on transparent, so a draw tints it any colour.
pub fn atlas() -> &'static image::RgbaImage {
    &font().atlas
}

/// The glyph for `ch`, or `None` for a space (which draws nothing).
///
/// A handful of typographic characters that turn up in hand-written dialogue
/// are folded onto their ASCII cousins, and anything else unknown draws as `?`
/// — a visible placeholder rather than a silent gap in a line of text.
pub fn glyph(ch: char) -> Option<Glyph> {
    let ch = match ch {
        ' ' | '\t' | '\n' => return None,
        '\u{2014}' | '\u{2013}' => '-',
        '\u{2018}' | '\u{2019}' => '\'',
        '\u{201c}' | '\u{201d}' => '"',
        '!'..='~' => ch,
        _ => '?',
    };
    font().glyphs.get(ch as usize - 33).copied()
}

/// How far a character moves the pen.
fn advance(ch: char) -> u32 {
    match glyph(ch) {
        Some(g) => g.width + GAP,
        None => SPACE_W,
    }
}

/// How wide `text` draws, in pixels, on one line: from the pen's start to the
/// right edge of the last lit glyph, so trailing spaces and the gap after the
/// last letter are not width.
pub fn width(text: &str) -> f32 {
    let (mut pen, mut end) = (0u32, 0u32);
    for ch in text.chars() {
        if let Some(g) = glyph(ch) {
            end = pen + g.width;
        }
        pen += advance(ch);
    }
    end as f32
}

/// Every glyph in `text` with the x offset it is drawn at.
pub fn layout(text: &str) -> impl Iterator<Item = (Glyph, f32)> + '_ {
    let mut pen = 0u32;
    text.chars().filter_map(move |ch| {
        let at = pen;
        pen += advance(ch);
        glyph(ch).map(|g| (g, at as f32))
    })
}

/// Break `text` into lines no wider than `max_width` pixels, on word
/// boundaries.
///
/// Greedy, and a word longer than a whole line is cut rather than allowed to
/// run off the edge — an id or a long name in a line of dialogue is exactly
/// that. Pure and total, so it is tested without a window; "text longer than
/// the box wraps rather than overflowing" is the whole of what this is for.
/// A `\n` is kept as a line break, for text that should break somewhere in
/// particular.
pub fn wrap(text: &str, max_width: f32) -> Vec<String> {
    if text.contains('\n') {
        return text
            .lines()
            .flat_map(|line| wrap(line, max_width))
            .collect();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let mut word = word.to_string();
        // A word that cannot fit on a line of its own is cut, not hidden.
        while width(&word) > max_width && word.chars().count() > 1 {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let mut cut = 1;
            while cut < word.chars().count() {
                let head: String = word.chars().take(cut + 1).collect();
                if width(&head) > max_width {
                    break;
                }
                cut += 1;
            }
            lines.push(word.chars().take(cut).collect());
            word = word.chars().skip(cut).collect();
        }

        let candidate = if current.is_empty() {
            word.clone()
        } else {
            format!("{current} {word}")
        };
        if width(&candidate) > max_width && !current.is_empty() {
            lines.push(std::mem::replace(&mut current, word));
        } else {
            current = candidate;
        }
    }

    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_printable_ascii_character_has_a_glyph() {
        for ch in '!'..='~' {
            assert!(glyph(ch).is_some(), "no glyph for {ch:?}");
        }
        assert_eq!(font().glyphs.len(), 94);
    }

    /// Proportional: an `i` is narrower than an `m`, and a space advances the
    /// pen without drawing.
    #[test]
    fn widths_are_proportional() {
        assert!(width("i") < width("m"));
        assert_eq!(width("ab"), width("a") + 1.0 + width("b"));
        assert_eq!(width(" "), 0.0, "trailing space is not width");
        assert_eq!(layout("a b").count(), 2);
    }

    #[test]
    fn unknown_characters_draw_as_a_question_mark() {
        assert_eq!(glyph('\u{2603}'), glyph('?'));
        assert_eq!(glyph('\u{2014}'), glyph('-'), "em dash folds to a hyphen");
    }

    #[test]
    fn wrapping_keeps_every_line_inside_the_width() {
        let text = "Take this, then. Bring back what you do not drink - glass is dear here.";
        let lines = wrap(text, 120.0);
        assert!(lines.len() > 1);
        for line in &lines {
            assert!(width(line) <= 120.0, "{line:?} is {} wide", width(line));
        }
        assert_eq!(lines.join(" "), text, "wrapping loses no words");
    }

    #[test]
    fn a_word_wider_than_the_line_is_cut_rather_than_overflowing() {
        let lines = wrap("abcdefghijklmnopqrstuvwxyz", 40.0);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| width(l) <= 40.0), "{lines:?}");
        assert_eq!(lines.concat(), "abcdefghijklmnopqrstuvwxyz");
    }

    #[test]
    fn a_line_break_is_kept() {
        assert_eq!(wrap("one two\nthree", 400.0), vec!["one two", "three"]);
    }

    #[test]
    fn empty_text_is_one_empty_line() {
        assert_eq!(wrap("", 100.0), vec![String::new()]);
    }
}
