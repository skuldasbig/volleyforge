//! The original's 24x24 gradient font.
//!
//! The font sheet in `graph.pak` holds 51 characters (10 digits, 26 letters
//! and 15 symbols) plus 4 decorative 24x24 elements in a separate 8 bpp
//! group. The original stores them in one array of 55 slots at `+0x9589E8`
//! and draws them with a fixed advance of 20 pixels.
//!
//! The letters are *strokes on black*: the original's visibility test is
//! `(pixel & 0xE0E0E0E0) != 0`, so the black body of a letter stays
//! transparent and only the coloured gradient strokes are painted.

use crate::canvas::Canvas;
use crate::layout::{GLYPH_ADVANCE, GLYPH_SIZE};

/// How a piece of text is painted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextStyle {
    /// Normal coloured strokes.
    Normal,
    /// The selected menu entry: `c = min(255, c*2 + 0x1E)`.
    Highlight,
    /// An entry that cannot be used: turned into grey (`FUN_0044724C`).
    Disabled,
}

/// The font sheet.
#[derive(Clone, Debug)]
pub struct Font {
    /// 55 slots of 24x24 RGBA, already masked for visibility.
    glyphs: Vec<Vec<u8>>,
    /// Number of frames per glyph.
    stride: usize,
}

impl Font {
    /// Build the font from the two sprite groups of `graph.pak`.
    ///
    /// `letters` are the 24 bpp 24x24 glyphs (51 of them), `decorations` the
    /// 8 bpp 24x24 elements that occupy the slots behind them.
    pub fn from_groups(letters: &[u8], decorations: &[u8]) -> Font {
        let stride = GLYPH_SIZE as usize * GLYPH_SIZE as usize * 4;
        let mut glyphs: Vec<Vec<u8>> = Vec::with_capacity(55);

        for chunk in letters.chunks(stride) {
            if chunk.len() < stride {
                break;
            }
            glyphs.push(mask_font_glyph(chunk));
        }
        for chunk in decorations.chunks(stride) {
            if chunk.len() < stride {
                break;
            }
            glyphs.push(mask_font_glyph(chunk));
        }
        Font { glyphs, stride }
    }

    /// Number of glyph slots.
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// Width of a text in pixels - the width of its **widest line**.
    pub fn width(&self, text: &str) -> i32 {
        text.lines()
            .map(|l| l.chars().count() as i32 * GLYPH_ADVANCE)
            .max()
            .unwrap_or(0)
    }

    /// Draw a string with its **left edge** at `x` and its **top edge** at `y`.
    ///
    /// The original draws with the lower edge in a bottom-up surface; the
    /// screens convert that with [`crate::layout::to_top_down`] beforehand.
    ///
    /// A text with several lines (the original's texts use a line break
    /// inside the string) is drawn as a block: the lines follow each other
    /// downwards and the block stays centred on `y`.
    pub fn draw(&self, canvas: &mut Canvas, text: &str, x: i32, y: i32, style: TextStyle) {
        let lines: Vec<&str> = text.lines().collect();
        let top = y - ((lines.len() as i32 - 1) / 2) * crate::layout::LINE_HEIGHT;
        for (i, line) in lines.iter().enumerate() {
            self.draw_line(canvas, line, x, top + i as i32 * crate::layout::LINE_HEIGHT, style);
        }
    }

    /// Draw a text **centred** on `center_x`.
    ///
    /// Every line of a multi-line text is centred on its own, the block
    /// stays centred on `y` - the German quit question is three lines long
    /// ("Spiel / wirklich / beenden?"), the English one only a single line.
    pub fn draw_centered(
        &self,
        canvas: &mut Canvas,
        text: &str,
        center_x: i32,
        y: i32,
        style: TextStyle,
    ) {
        let lines: Vec<&str> = text.lines().collect();
        let top = y - ((lines.len() as i32 - 1) / 2) * crate::layout::LINE_HEIGHT;
        for (i, line) in lines.iter().enumerate() {
            let x = center_x - self.width(line) / 2;
            self.draw_line(canvas, line, x, top + i as i32 * crate::layout::LINE_HEIGHT, style);
        }
    }

    /// Draw a single line with its **left edge** at `x`.
    fn draw_line(&self, canvas: &mut Canvas, text: &str, x: i32, y: i32, style: TextStyle) {
        let mut cursor = x;
        for ch in text.chars() {
            let Some((idx, slanted)) = glyph_index(ch) else {
                cursor += GLYPH_ADVANCE;
                continue;
            };
            if idx < self.glyphs.len() {
                let g = &self.glyphs[idx];
                if slanted {
                    // The original's lower case letters reuse the upper case
                    // shapes and are drawn with a slant: row `r` is offset by
                    // `(r * 3) / 4` instead of `r`.
                    for row in 0..GLYPH_SIZE as usize {
                        let dy = y + (row as i32 * 3) / 4;
                        self.draw_row(canvas, g, row, cursor, dy, style);
                    }
                } else {
                    match style {
                        TextStyle::Normal => {
                            canvas.blit(g, GLYPH_SIZE as usize, GLYPH_SIZE as usize, cursor, y)
                        }
                        TextStyle::Highlight => canvas.blit_highlighted(
                            g,
                            GLYPH_SIZE as usize,
                            GLYPH_SIZE as usize,
                            cursor,
                            y,
                        ),
                        TextStyle::Disabled => canvas.blit_grey(
                            g,
                            GLYPH_SIZE as usize,
                            GLYPH_SIZE as usize,
                            cursor,
                            y,
                            false,
                        ),
                    }
                }
            }
            cursor += GLYPH_ADVANCE;
        }
    }

    /// Draw a string **tinted** with a team colour.
    ///
    /// The original tints the two player icons of the settings screen (the
    /// font slots 51 and 52) with the colour of the team: the text tinting
    /// `FUN_0044A6F0` multiplies every channel with the colour and adds a
    /// small lift.
    pub fn draw_tinted(&self, canvas: &mut Canvas, text: &str, x: i32, y: i32, color: u32) {
        let mut cursor = x;
        for ch in text.chars() {
            let Some((idx, slanted)) = glyph_index(ch) else {
                cursor += GLYPH_ADVANCE;
                continue;
            };
            if idx < self.glyphs.len() {
                let g = &self.glyphs[idx];
                for row in 0..GLYPH_SIZE as usize {
                    let dy = y + if slanted { (row as i32 * 3) / 4 } else { row as i32 };
                    canvas.blit_row_with(g, GLYPH_SIZE as usize, row, cursor, dy, |r, g, b, _| {
                        Some(crate::layout::tint_rgb(color, r, g, b))
                    });
                }
            }
            cursor += GLYPH_ADVANCE;
        }
    }

    /// Draw a single row of a glyph with the given style.
    fn draw_row(
        &self,
        canvas: &mut Canvas,
        glyph: &[u8],
        row: usize,
        x: i32,
        y: i32,
        style: TextStyle,
    ) {
        let w = GLYPH_SIZE as usize;
        match style {
            TextStyle::Normal => canvas.blit_row(glyph, w, row, x, y),
            TextStyle::Highlight => canvas.blit_row_with(glyph, w, row, x, y, |r, g, b, _| {
                Some((
                    crate::layout::highlight_channel(r),
                    crate::layout::highlight_channel(g),
                    crate::layout::highlight_channel(b),
                ))
            }),
            TextStyle::Disabled => canvas.blit_row_with(glyph, w, row, x, y, |r, g, b, _| {
                Some(crate::layout::grey_rgb(r, g, b, false))
            }),
        }
    }
}

/// Apply the original's font visibility rule: a pixel counts as ink when one
/// of its channels has one of the top three bits set.
fn mask_font_glyph(src: &[u8]) -> Vec<u8> {
    let mut out = src.to_vec();
    for p in (0..out.len()).step_by(4) {
        if p + 3 >= out.len() {
            break;
        }
        let visible = (out[p] & 0xE0) != 0
            || (out[p + 1] & 0xE0) != 0
            || (out[p + 2] & 0xE0) != 0;
        out[p + 3] = if visible { 255 } else { 0 };
    }
    out
}

/// Index of a character in the font sheet, and whether it is drawn slanted.
///
/// This is the original's own lookup table (`FUN_0044708C`):
///
/// ```text
///  0 -  9  '0' .. '9'
/// 10 - 35  'A' .. 'Z'
/// 36       '.'
/// 37       '!'
/// 38       '('
/// 39       ')'
/// 40       'ß'
/// 41       'Ä'
/// 42       'Ö'
/// 43       'Ü'
/// 44       '\''
/// 45       ':'
/// 46       ';'
/// 47       '?'
/// 48       ','
/// 49       '/'
/// 50       '-'
/// 51       '<'      (a small player icon, not a character)
/// 52       '=' '>'  (a small player icon, not a character)
/// ```
///
/// Lower case letters reuse the upper case shapes and carry the `0x8000`
/// flag, which makes the original draw them slanted.
pub fn glyph_index(c: char) -> Option<(usize, bool)> {
    let (idx, slanted) = match c {
        '0'..='9' => (c as usize - '0' as usize, false),
        'A'..='Z' => (c as usize - 'A' as usize + 10, false),
        'a'..='z' => (c as usize - 'a' as usize + 10, true),
        '.' => (36, false),
        '!' => (37, false),
        '(' => (38, false),
        ')' => (39, false),
        'ß' => (40, false),
        'Ä' => (41, false),
        'Ö' => (42, false),
        'Ü' => (43, false),
        '\'' => (44, false),
        ':' => (45, false),
        ';' => (46, false),
        '?' => (47, false),
        ',' => (48, false),
        '/' => (49, false),
        '-' => (50, false),
        '<' => (51, false),
        '>' => (52, false),
        // The two remaining decoration slots are the "²" and "³" of the
        // statistics table (the original's lookup table maps the bytes
        // 0xB2 and 0xB3 to the slots 53 and 54).
        '\u{b2}' => (53, false),
        '\u{b3}' => (54, false),
        // The lower case umlauts reuse the upper case shapes, slanted -
        // the original maps the bytes 0xE4, 0xF6 and 0xFC to the slots 41,
        // 42 and 43 with the slant flag.
        '\u{e4}' => (41, true),
        '\u{f6}' => (42, true),
        '\u{fc}' => (43, true),
        _ => return None,
    };
    Some((idx, slanted))
}
