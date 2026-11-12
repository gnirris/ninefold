//! Minecraft's own font, at Minecraft's own widths.
//!
//! `ascii.png` is a 16×16 grid of glyphs and nothing else — no metrics, no kerning table. The
//! game works out how wide each character is by looking at the picture: scan a glyph's cell from
//! the right until a column has a pixel in it, and that is where the character ends. An `i` comes
//! out two pixels wide and an `m` six, which is why Minecraft text is not monospaced even though
//! its font sheet is.
//!
//! Doing the same here matters for more than looks: the chat box, the button labels and the
//! item-count numbers are all positioned by measuring a string, and a fixed-width guess puts
//! every one of them in the wrong place.

use ninefold_assets::Texture;

/// Cells across and down the sheet.
const GRID: u32 = 16;
/// How tall a line is, in interface units, including the gap vanilla leaves.
pub const LINE: f32 = 9.0;

/// The sixteen colours a `§` code can select, in code order.
const PALETTE: [[f32; 3]; 16] = [
    [0.000, 0.000, 0.000], // 0 black
    [0.000, 0.000, 0.667], // 1 dark blue
    [0.000, 0.667, 0.000], // 2 dark green
    [0.000, 0.667, 0.667], // 3 dark aqua
    [0.667, 0.000, 0.000], // 4 dark red
    [0.667, 0.000, 0.667], // 5 dark purple
    [1.000, 0.667, 0.000], // 6 gold
    [0.667, 0.667, 0.667], // 7 grey
    [0.333, 0.333, 0.333], // 8 dark grey
    [0.333, 0.333, 1.000], // 9 blue
    [0.333, 1.000, 0.333], // a green
    [0.333, 1.000, 1.000], // b aqua
    [1.000, 0.333, 0.333], // c red
    [1.000, 0.333, 1.000], // d light purple
    [1.000, 1.000, 0.333], // e yellow
    [1.000, 1.000, 1.000], // f white
];

/// The advance of every character, measured off the sheet.
pub struct Font {
    widths: [f32; 256],
}

impl Font {
    /// Measure a font sheet. Anything that is not a 16×16 grid falls back to fixed width.
    #[must_use]
    pub fn measure(sheet: Option<&Texture>) -> Self {
        let mut widths = [6.0f32; 256];
        if let Some(sheet) = sheet
            && sheet.width >= GRID
            && sheet.height >= GRID
        {
            let cell = sheet.width / GRID;
            // How many sheet pixels make one of the eight the glyph is drawn at.
            let per_unit = 8.0 / cell as f32;
            for code in 0..256u32 {
                let (cx, cy) = ((code % GRID) * cell, (code / GRID) * cell);
                let mut last = 0u32;
                for x in 0..cell {
                    let inked = (0..cell).any(|y| {
                        sheet
                            .pixel(cx + x, cy + y)
                            .is_some_and(|pixel| pixel[3] > 0)
                    });
                    if inked {
                        last = x + 1;
                    }
                }
                // The game rounds to whole units and leaves one blank column between glyphs.
                widths[code as usize] = (last as f32 * per_unit).round() + 1.0;
            }
        }
        // A space has no pixels at all, so it has to be told how wide it is.
        widths[32] = 4.0;
        Self { widths }
    }

    /// How wide one character is, in interface units.
    #[must_use]
    pub fn advance(&self, character: char) -> f32 {
        let code = if character.is_ascii() {
            character as usize
        } else {
            b'?' as usize
        };
        self.widths.get(code).copied().unwrap_or(6.0)
    }

    /// How wide a string is, ignoring the colour codes in it.
    #[must_use]
    pub fn width(&self, text: &str) -> f32 {
        let mut total = 0.0;
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '§' {
                chars.next();
                continue;
            }
            total += self.advance(c);
        }
        total
    }

    /// Cut a string to fit a width, the way the chat box does.
    #[must_use]
    pub fn trim(&self, text: &str, limit: f32) -> String {
        let mut out = String::new();
        let mut total = 0.0;
        for c in text.chars() {
            total += self.advance(c);
            if total > limit {
                break;
            }
            out.push(c);
        }
        out
    }
}

/// What a `§` code does to the colour, if anything.
#[must_use]
pub fn colour_code(code: char) -> Option<[f32; 3]> {
    code.to_digit(16).map(|n| PALETTE[n as usize])
}

/// Where one character sits in the sheet: left, top, width, height in 0..1.
#[must_use]
pub fn cell(character: char) -> [f32; 4] {
    let code = if character.is_ascii() {
        character as u32
    } else {
        b'?' as u32
    };
    let step = 1.0 / GRID as f32;
    [
        (code % GRID) as f32 * step,
        (code / GRID) as f32 * step,
        step,
        step,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sheet where every glyph is a solid block, so each is as wide as its cell.
    fn solid() -> Texture {
        Texture::flat(128, 128, [255, 255, 255, 255])
    }

    #[test]
    fn a_solid_sheet_measures_a_full_cell_plus_the_gap() {
        let font = Font::measure(Some(&solid()));
        // Eight-pixel cells, every column inked: eight units wide, plus one of space.
        assert_eq!(font.advance('A'), 9.0);
    }

    #[test]
    fn a_space_is_four_units_even_though_it_has_no_pixels() {
        let blank = Texture::flat(128, 128, [255, 255, 255, 0]);
        let font = Font::measure(Some(&blank));
        assert_eq!(font.advance(' '), 4.0);
        assert_eq!(font.advance('A'), 1.0, "nothing inked is one unit of gap");
    }

    #[test]
    fn no_sheet_at_all_still_measures_something() {
        let font = Font::measure(None);
        assert!(font.width("hello") > 0.0);
    }

    #[test]
    fn colour_codes_do_not_count_towards_the_width() {
        let font = Font::measure(Some(&solid()));
        assert_eq!(font.width("§cred"), font.width("red"));
    }

    #[test]
    fn trimming_stops_before_it_overflows() {
        let font = Font::measure(Some(&solid()));
        // Nine units a character, so three fit in thirty.
        assert_eq!(font.trim("abcdef", 30.0), "abc");
        assert_eq!(font.trim("abc", 1000.0), "abc");
    }

    #[test]
    fn every_character_lands_in_its_own_cell() {
        let [x, y, w, h] = cell('A');
        assert!((x - (65 % 16) as f32 / 16.0).abs() < 1e-6);
        assert!((y - (65 / 16) as f32 / 16.0).abs() < 1e-6);
        assert_eq!((w, h), (1.0 / 16.0, 1.0 / 16.0));
    }

    #[test]
    fn the_palette_covers_every_code() {
        assert_eq!(colour_code('0'), Some([0.0, 0.0, 0.0]));
        assert_eq!(colour_code('f'), Some([1.0, 1.0, 1.0]));
        assert_eq!(colour_code('z'), None, "a formatting code is not a colour");
    }
}
