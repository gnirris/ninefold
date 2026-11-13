//! The game's own interface, drawn from the game's own art.
//!
//! The layout is written here; the pixels come out of the player's jar. Vanilla picks a whole
//! number for the interface scale so the 16-pixel art never lands between screen pixels, and
//! every placement below is in those interface units rather than screen pixels — which is why
//! the hotbar sits in the same place at any window size.
//!
//! Four lists come out, not one, because four different pictures are involved and a draw call
//! can only have one texture bound: the widget sheets, the item sheet, the block atlas (for the
//! little cubes that blocks are drawn as in a slot) and the font.

use ninefold_assets::{AssetSource, Texture};
use ninefold_mesh::{Face, Shape};
use ninefold_protocol::item::Slot;
use ninefold_render::Glyph;

use crate::atlas::Atlas;
use crate::font::{self, Font};
use crate::items::Items;

/// Every sheet the interface needs, in one texture.
pub struct Sheets {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Where each sheet starts inside [`Sheets`].
const WIDGETS_AT: (u32, u32) = (0, 0);
const ICONS_AT: (u32, u32) = (256, 0);
const CONTAINER_AT: (u32, u32) = (0, 256);
const MISC_AT: (u32, u32) = (256, 256);
const TITLE_AT: (u32, u32) = (0, 512);
const SHEET_WIDTH: u32 = 512;
const SHEET_HEIGHT: u32 = 768;

/// A rectangle of one of the sheets, in its own pixels.
#[derive(Debug, Clone, Copy)]
pub struct Sprite {
    at: (u32, u32),
    x: u32,
    y: u32,
    pub w: u32,
    pub h: u32,
}

impl Sprite {
    const fn widgets(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self {
            at: WIDGETS_AT,
            x,
            y,
            w,
            h,
        }
    }
    const fn icons(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self {
            at: ICONS_AT,
            x,
            y,
            w,
            h,
        }
    }
    const fn container(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self {
            at: CONTAINER_AT,
            x,
            y,
            w,
            h,
        }
    }
    const fn misc(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self {
            at: MISC_AT,
            x,
            y,
            w,
            h,
        }
    }

    /// Take a piece out of this sprite, for the button halves.
    const fn part(self, dx: u32, w: u32) -> Self {
        Self {
            x: self.x + dx,
            w,
            ..self
        }
    }
}

/// The pieces of the 1.8 interface, at the coordinates they live at in the sheets.
pub const HOTBAR: Sprite = Sprite::widgets(0, 0, 182, 22);
pub const SELECTED: Sprite = Sprite::widgets(0, 22, 24, 24);
pub const BUTTON_OFF: Sprite = Sprite::widgets(0, 46, 200, 20);
pub const BUTTON: Sprite = Sprite::widgets(0, 66, 200, 20);
pub const BUTTON_OVER: Sprite = Sprite::widgets(0, 86, 200, 20);
pub const CROSSHAIR: Sprite = Sprite::icons(0, 0, 16, 16);
pub const HEART_SLOT: Sprite = Sprite::icons(16, 0, 9, 9);
pub const HEART_FULL: Sprite = Sprite::icons(52, 0, 9, 9);
pub const HEART_HALF: Sprite = Sprite::icons(61, 0, 9, 9);
pub const FOOD_SLOT: Sprite = Sprite::icons(16, 27, 9, 9);
pub const FOOD_FULL: Sprite = Sprite::icons(52, 27, 9, 9);
pub const FOOD_HALF: Sprite = Sprite::icons(61, 27, 9, 9);
pub const BUBBLE_FULL: Sprite = Sprite::icons(16, 18, 9, 9);
pub const BUBBLE_POP: Sprite = Sprite::icons(25, 18, 9, 9);
pub const XP_EMPTY: Sprite = Sprite::icons(0, 64, 182, 5);
pub const XP_FULL: Sprite = Sprite::icons(0, 69, 182, 5);
pub const INVENTORY: Sprite = Sprite::container(0, 0, 176, 166);
/// The tiled dirt behind every menu.
pub const DIRT: Sprite = Sprite::misc(0, 0, 16, 16);
/// A patch of pure white, for drawing flat colour. Inset so no neighbour bleeds into it.
pub const WHITE: Sprite = Sprite::misc(17, 1, 2, 2);
/// The logo, which vanilla draws as two pieces side by side.
pub const LOGO_LEFT: Sprite = Sprite {
    at: TITLE_AT,
    x: 0,
    y: 0,
    w: 155,
    h: 44,
};
pub const LOGO_RIGHT: Sprite = Sprite {
    at: TITLE_AT,
    x: 0,
    y: 45,
    w: 155,
    h: 44,
};

/// Put every sheet into one texture, so the interface is a handful of draws.
#[must_use]
pub fn sheets(assets: &dyn AssetSource) -> Sheets {
    let (width, height) = (SHEET_WIDTH, SHEET_HEIGHT);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for (name, (ox, oy)) in [
        ("gui/widgets", WIDGETS_AT),
        ("gui/icons", ICONS_AT),
        ("gui/container/inventory", CONTAINER_AT),
        ("gui/options_background", MISC_AT),
        ("gui/title/minecraft", TITLE_AT),
    ] {
        if let Some(sheet) = assets.texture(name) {
            paste(&mut rgba, width, ox, oy, &sheet);
        }
    }
    // A white patch of our own, which no sheet provides and every flat-coloured panel needs.
    paste(
        &mut rgba,
        width,
        MISC_AT.0 + 16,
        MISC_AT.1,
        &Texture::flat(4, 4, [0xFF, 0xFF, 0xFF, 0xFF]),
    );
    Sheets {
        width,
        height,
        rgba,
    }
}

fn paste(into: &mut [u8], into_width: u32, ox: u32, oy: u32, from: &Texture) {
    for row in 0..from.height {
        let src = (row * from.width * 4) as usize;
        let dst = (((oy + row) * into_width + ox) * 4) as usize;
        let run = (from.width * 4) as usize;
        if src + run <= from.rgba.len() && dst + run <= into.len() {
            into[dst..dst + run].copy_from_slice(&from.rgba[src..src + run]);
        }
    }
}

/// Which picture a quad samples from, and so which list it goes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Art {
    Sheet,
    Items,
    Blocks,
    Font,
}

/// Lays the interface out in interface units and turns it into screen-space quads.
pub struct Ui {
    /// How many screen pixels one interface pixel takes.
    pub scale: f32,
    screen: (f32, f32),
    /// Quads from the widget sheets.
    pub sheet: Vec<Glyph>,
    /// Quads from the item sheet.
    pub items: Vec<Glyph>,
    /// Quads from the block atlas: the cubes drawn in slots.
    pub blocks: Vec<Glyph>,
    /// Quads from the font.
    pub text: Vec<Glyph>,
}

impl Ui {
    /// Vanilla picks the largest whole scale that still leaves a 320x240 interface fitting.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let fits = (width / 320).min(height / 240).clamp(1, 4);
        Self {
            scale: fits as f32,
            screen: (width as f32, height as f32),
            sheet: Vec::new(),
            items: Vec::new(),
            blocks: Vec::new(),
            text: Vec::new(),
        }
    }

    /// The interface is this many units across and down.
    #[must_use]
    pub fn size(&self) -> (f32, f32) {
        (self.screen.0 / self.scale, self.screen.1 / self.scale)
    }

    /// Interface units to clip space. Y is flipped: the interface is laid out downwards.
    fn clip(&self, x: f32, y: f32) -> [f32; 3] {
        [
            x * self.scale / self.screen.0 * 2.0 - 1.0,
            1.0 - y * self.scale / self.screen.1 * 2.0,
            0.0,
        ]
    }

    /// Which screen position a mouse position in window pixels is, in interface units.
    #[must_use]
    pub fn at(&self, x: f64, y: f64) -> (f32, f32) {
        (x as f32 / self.scale, y as f32 / self.scale)
    }

    fn list(&mut self, which: Art) -> &mut Vec<Glyph> {
        match which {
            Art::Sheet => &mut self.sheet,
            Art::Items => &mut self.items,
            Art::Blocks => &mut self.blocks,
            Art::Font => &mut self.text,
        }
    }

    /// One rectangle, with its own corners and its own patch of texture.
    fn quad(&mut self, which: Art, at: [f32; 4], uv: [f32; 4], colour: [f32; 4]) {
        let [x, y, w, h] = at;
        let [u0, v0, u1, v1] = uv;
        let corners = [
            ([x, y], [u0, v0]),
            ([x + w, y], [u1, v0]),
            ([x + w, y + h], [u1, v1]),
            ([x, y], [u0, v0]),
            ([x + w, y + h], [u1, v1]),
            ([x, y + h], [u0, v1]),
        ];
        let quads: Vec<Glyph> = corners
            .iter()
            .map(|([px, py], uv)| Glyph {
                position: self.clip(*px, *py),
                uv: *uv,
                colour,
            })
            .collect();
        self.list(which).extend(quads);
    }

    /// Four corners of our own, for anything that is not an upright rectangle.
    fn shape(&mut self, which: Art, points: [[f32; 2]; 4], uv: [[f32; 2]; 4], colour: [f32; 4]) {
        let order = [0, 1, 2, 0, 2, 3];
        let quads: Vec<Glyph> = order
            .iter()
            .map(|i| Glyph {
                position: self.clip(points[*i][0], points[*i][1]),
                uv: uv[*i],
                colour,
            })
            .collect();
        self.list(which).extend(quads);
    }

    /// Place one sprite at its own size.
    pub fn sprite(&mut self, sprite: Sprite, x: f32, y: f32) {
        self.sprite_scaled(sprite, x, y, sprite.w as f32, sprite.h as f32);
    }

    /// Place one sprite at a size of its own.
    pub fn sprite_scaled(&mut self, sprite: Sprite, x: f32, y: f32, w: f32, h: f32) {
        self.sprite_tinted(sprite, x, y, w, h, [1.0, 1.0, 1.0, 1.0]);
    }

    /// Place one sprite with a colour through it.
    pub fn sprite_tinted(
        &mut self,
        sprite: Sprite,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        colour: [f32; 4],
    ) {
        let (tw, th) = (SHEET_WIDTH as f32, SHEET_HEIGHT as f32);
        let uv = [
            (sprite.at.0 + sprite.x) as f32 / tw,
            (sprite.at.1 + sprite.y) as f32 / th,
            (sprite.at.0 + sprite.x + sprite.w) as f32 / tw,
            (sprite.at.1 + sprite.y + sprite.h) as f32 / th,
        ];
        self.quad(Art::Sheet, [x, y, w, h], uv, colour);
    }

    /// A flat rectangle of colour, which every panel and overlay is made of.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, colour: [f32; 4]) {
        self.sprite_tinted(WHITE, x, y, w, h, colour);
    }

    /// The tiled dirt behind the menus.
    ///
    /// Vanilla darkens it by drawing the tiles themselves at quarter brightness. Laying a dark
    /// rectangle over them instead buries them: the wash is opaque, and all you get is grey.
    pub fn dirt(&mut self) {
        let (w, h) = self.size();
        let step = 32.0;
        let shade = [0.25, 0.25, 0.25, 1.0];
        let mut y = 0.0;
        while y < h {
            let mut x = 0.0;
            while x < w {
                self.sprite_tinted(DIRT, x, y, step, step, shade);
                x += step;
            }
            y += step;
        }
    }

    /// A line of text, with the shadow vanilla draws under it.
    ///
    /// Returns where the text ended, so callers can put something after it.
    pub fn text(&mut self, font: &Font, line: &str, x: f32, y: f32, colour: [f32; 4]) -> f32 {
        // The shadow first, one unit down and right at a quarter brightness.
        let shadow = [colour[0] * 0.25, colour[1] * 0.25, colour[2] * 0.25, colour[3]];
        self.glyphs(font, line, (x + 1.0, y + 1.0), shadow, (true, 1.0));
        self.glyphs(font, line, (x, y), colour, (false, 1.0))
    }

    /// The same without a shadow, for text on a dark panel.
    pub fn text_flat(&mut self, font: &Font, line: &str, x: f32, y: f32, colour: [f32; 4]) -> f32 {
        self.glyphs(font, line, (x, y), colour, (false, 1.0))
    }

    /// Centred on `middle`.
    pub fn text_centred(&mut self, font: &Font, line: &str, middle: f32, y: f32, colour: [f32; 4]) {
        let width = font.width(line);
        self.text(font, line, middle - width / 2.0, y, colour);
    }

    /// Bigger, for the few things vanilla draws large: "You died!", a menu title.
    pub fn text_big(
        &mut self,
        font: &Font,
        line: &str,
        middle: f32,
        y: f32,
        colour: [f32; 4],
        size: f32,
    ) {
        let x = middle - font.width(line) * size / 2.0;
        let shadow = [colour[0] * 0.25, colour[1] * 0.25, colour[2] * 0.25, colour[3]];
        self.glyphs(font, line, (x + size, y + size), shadow, (true, size));
        self.glyphs(font, line, (x, y), colour, (false, size));
    }

    fn glyphs(
        &mut self,
        font: &Font,
        line: &str,
        at: (f32, f32),
        colour: [f32; 4],
        how: (bool, f32),
    ) -> f32 {
        let (x, y) = at;
        let (shadow, size) = how;
        let mut pen = x;
        let mut rgb = [colour[0], colour[1], colour[2]];
        let mut chars = line.chars().peekable();
        while let Some(character) = chars.next() {
            // A colour code eats the character after it and changes everything that follows.
            if character == '§' {
                if let Some(code) = chars.next()
                    && let Some(picked) = font::colour_code(code)
                {
                    rgb = if shadow {
                        [picked[0] * 0.25, picked[1] * 0.25, picked[2] * 0.25]
                    } else {
                        picked
                    };
                }
                continue;
            }
            let [u, v, uw, vh] = font::cell(character);
            if character != ' ' {
                self.quad(
                    Art::Font,
                    [pen, y, 8.0 * size, 8.0 * size],
                    [u, v, u + uw, v + vh],
                    [rgb[0], rgb[1], rgb[2], colour[3]],
                );
            }
            pen += font.advance(character) * size;
        }
        pen
    }

    /// One of vanilla's buttons. `over` is whether the mouse is on it.
    pub fn button(&mut self, font: &Font, label: &str, at: (f32, f32), w: f32, state: Button) {
        let sprite = match state {
            Button::Disabled => BUTTON_OFF,
            Button::Idle => BUTTON,
            Button::Over => BUTTON_OVER,
        };
        let (x, y) = at;
        // Vanilla draws a button as two halves so any width can be made out of a 200-wide
        // picture without stretching its rounded ends.
        let half = (w / 2.0).min(100.0);
        let pixels = half as u32;
        self.sprite_scaled(sprite.part(0, pixels), x, y, half, 20.0);
        self.sprite_scaled(
            sprite.part(200 - pixels, pixels),
            x + w - half,
            y,
            half,
            20.0,
        );
        let colour = match state {
            Button::Disabled => [0.63, 0.63, 0.63, 1.0],
            Button::Idle => [1.0, 1.0, 1.0, 1.0],
            Button::Over => [1.0, 1.0, 0.63, 1.0],
        };
        self.text_centred(font, label, x + w / 2.0, y + 6.0, colour);
    }

    /// One item in a slot: its picture, or a little cube if it is a block, and its count.
    pub fn slot(&mut self, art: SlotArt<'_>, stack: Slot, x: f32, y: f32) {
        let SlotArt {
            atlas,
            items,
            font,
            light,
        } = art;
        if let Some(tile) = items.rect(stack.id, stack.damage) {
            self.quad(
                Art::Items,
                [x, y, 16.0, 16.0],
                [tile[0], tile[1], tile[0] + tile[2], tile[1] + tile[3]],
                [light, light, light, 1.0],
            );
        } else if let Some(block) = stack.as_block() {
            self.block_icon(atlas, block, x, y, light);
        }
        if stack.count > 1 {
            let count = stack.count.to_string();
            let width = font.width(&count);
            self.text(font, &count, x + 17.0 - width, y + 9.0, [1.0, 1.0, 1.0, 1.0]);
        }
    }

    /// A block as it appears in a slot: three faces of a cube, seen from a corner.
    ///
    /// Vanilla really does render the block model in three dimensions here. Three flat
    /// parallelograms with the block's own faces on them is the same picture for a full cube,
    /// which is what almost every placeable block is.
    pub fn block_icon(&mut self, atlas: &Atlas, block: u16, x: f32, y: f32, light: f32) {
        let s = 16.0;
        // Anything that is not a cube is drawn flat, because a cube of it would be a lie.
        if crate::atlas::shape_of(block) != Shape::Cube {
            let tile = atlas.rect(block, Face::North);
            self.quad(
                Art::Blocks,
                [x, y, s, s],
                [tile[0], tile[1], tile[0] + tile[2], tile[1] + tile[3]],
                [light, light, light, 1.0],
            );
            return;
        }
        let (half, quarter) = (s / 2.0, s / 4.0);
        let top = [x + half, y];
        let right = [x + s, y + quarter];
        let middle = [x + half, y + half];
        let left = [x, y + quarter];
        let low_left = [x, y + quarter * 3.0];
        let bottom = [x + half, y + s];
        let low_right = [x + s, y + quarter * 3.0];
        let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        // A face, its corners, and how much of the light it gets: the top is brightest, as it
        // would be with the sun overhead.
        for (face, points, shade) in [
            (Face::Up, [left, top, right, middle], 1.0),
            (Face::North, [left, middle, bottom, low_left], 0.65),
            (Face::East, [middle, right, low_right, bottom], 0.8),
        ] {
            let uv = square.map(|corner| atlas.uv(block, face, corner));
            let value = light * shade;
            self.shape(Art::Blocks, points, uv, [value, value, value, 1.0]);
        }
    }

    /// The whole 1.8 heads-up display.
    pub fn hud(&mut self, art: SlotArt<'_>, player: &Hud) {
        let (w, h) = self.size();
        let middle = w / 2.0;
        let SlotArt {
            atlas, items, font, ..
        } = art;

        // Hotbar, centred along the bottom.
        let bar_x = middle - HOTBAR.w as f32 / 2.0;
        let bar_y = h - HOTBAR.h as f32;
        self.sprite(HOTBAR, bar_x, bar_y);
        // The selection box is one pixel bigger than a slot on every side.
        let slot = f32::from(player.held.min(8)) * 20.0;
        self.sprite(SELECTED, bar_x - 1.0 + slot, bar_y - 1.0);

        // Crosshair, dead centre.
        self.sprite(
            CROSSHAIR,
            middle - CROSSHAIR.w as f32 / 2.0,
            h / 2.0 - CROSSHAIR.h as f32 / 2.0,
        );

        // The experience bar sits right on top of the hotbar, with the level over it.
        self.sprite(XP_EMPTY, bar_x, bar_y - 7.0);
        let filled = (player.experience.clamp(0.0, 1.0) * XP_FULL.w as f32).floor();
        if filled >= 1.0 {
            self.sprite_scaled(
                XP_FULL.part(0, filled as u32),
                bar_x,
                bar_y - 7.0,
                filled,
                XP_FULL.h as f32,
            );
        }
        if player.level > 0 {
            let level = player.level.to_string();
            let width = font.width(&level);
            // Vanilla draws this in green with a black outline; the shadow is close enough.
            self.text(
                font,
                &level,
                middle - width / 2.0,
                bar_y - 14.0,
                [0.5, 1.0, 0.2, 1.0],
            );
        }

        // Ten hearts and ten drumsticks, above the hotbar, halves included.
        let row = bar_y - 16.0;
        for i in 0u8..10 {
            let x = bar_x + f32::from(i) * 8.0;
            self.sprite(HEART_SLOT, x, row);
            let left = player.health - f32::from(i) * 2.0;
            if left >= 2.0 {
                self.sprite(HEART_FULL, x, row);
            } else if left >= 1.0 {
                self.sprite(HEART_HALF, x, row);
            }
        }
        for i in 0u8..10 {
            // Food fills from the right, which is why it is counted backwards.
            let x = bar_x + HOTBAR.w as f32 - 9.0 - f32::from(i) * 8.0;
            self.sprite(FOOD_SLOT, x, row);
            let left = f32::from(player.food) - f32::from(i) * 2.0;
            if left >= 2.0 {
                self.sprite(FOOD_FULL, x, row);
            } else if left >= 1.0 {
                self.sprite(FOOD_HALF, x, row);
            }
        }
        // Air, but only while there is any to lose.
        if player.air < 300 {
            let bubbles = (f32::from(player.air) * 10.0 / 300.0).ceil() as i32;
            for i in 0..10 {
                let x = bar_x + HOTBAR.w as f32 - 9.0 - i as f32 * 8.0;
                let sprite = if i < bubbles {
                    BUBBLE_FULL
                } else {
                    BUBBLE_POP
                };
                self.sprite(sprite, x, row - 10.0);
            }
        }

        // The hotbar's contents, over the bar itself.
        for (index, stack) in player.hotbar.iter().enumerate() {
            if let Some(stack) = stack {
                let x = bar_x + 3.0 + index as f32 * 20.0;
                self.slot(
                    SlotArt {
                        atlas,
                        items,
                        font,
                        light: 1.0,
                    },
                    *stack,
                    x,
                    bar_y + 3.0,
                );
            }
        }
    }
}

/// What a button looks like right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Idle,
    Over,
    Disabled,
}

/// Everything needed to draw an item, bundled so the calls stay readable.
#[derive(Clone, Copy)]
pub struct SlotArt<'a> {
    pub atlas: &'a Atlas,
    pub items: &'a Items,
    pub font: &'a Font,
    /// How bright to draw it: dimmed for a slot that is not really there.
    pub light: f32,
}

/// What the display needs to know about the player.
pub struct Hud {
    pub held: u8,
    pub health: f32,
    pub food: u8,
    /// Out of three hundred, as the server counts it.
    pub air: u16,
    /// How far along the experience bar is, 0..1, and the level over it.
    pub experience: f32,
    pub level: i32,
    pub hotbar: [Option<Slot>; 9],
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat;
    impl AssetSource for Flat {
        fn texture(&self, _id: &str) -> Option<Texture> {
            Some(Texture::flat(256, 256, [128, 128, 128, 255]))
        }
        fn describe(&self) -> String {
            "test".into()
        }
    }

    fn screen() -> Ui {
        Ui::new(1280, 720)
    }

    #[test]
    fn the_scale_is_a_whole_number_as_vanilla_insists() {
        // 720 pixels of height only fits three 240-unit interfaces, so three it is, even though
        // the width would take four.
        assert_eq!(Ui::new(1280, 720).scale, 3.0);
        assert_eq!(Ui::new(1920, 1080).scale, 4.0);
        assert_eq!(Ui::new(640, 480).scale, 2.0);
        assert_eq!(Ui::new(320, 240).scale, 1.0);
        // Never zero, however small the window gets.
        assert_eq!(Ui::new(100, 100).scale, 1.0);
        // And never more than four, however big.
        assert_eq!(Ui::new(5120, 2880).scale, 4.0);
    }

    #[test]
    fn the_middle_of_the_interface_is_the_middle_of_the_screen() {
        let ui = screen();
        let (w, h) = ui.size();
        let clip = ui.clip(w / 2.0, h / 2.0);
        assert!(clip[0].abs() < 1e-6 && clip[1].abs() < 1e-6, "{clip:?}");
        // Top left is the top left.
        assert_eq!(ui.clip(0.0, 0.0), [-1.0, 1.0, 0.0]);
    }

    #[test]
    fn every_sheet_lands_inside_the_texture() {
        let sheets = sheets(&Flat);
        assert_eq!((sheets.width, sheets.height), (SHEET_WIDTH, SHEET_HEIGHT));
        assert_eq!(
            sheets.rgba.len(),
            (SHEET_WIDTH * SHEET_HEIGHT * 4) as usize
        );
        // The white patch is really white, which every flat panel depends on.
        let at = (((MISC_AT.1 + 1) * SHEET_WIDTH + MISC_AT.0 + 17) * 4) as usize;
        assert_eq!(&sheets.rgba[at..at + 4], &[255, 255, 255, 255]);
    }

    #[test]
    fn a_block_in_a_slot_is_three_faces_and_an_item_is_one() {
        let atlas = Atlas::build(&Flat);
        let items = Items::build(&Flat);
        let font = Font::measure(None);
        let art = SlotArt {
            atlas: &atlas,
            items: &items,
            font: &font,
            light: 1.0,
        };
        let mut ui = screen();
        ui.slot(art, Slot::new(4, 1), 0.0, 0.0);
        assert_eq!(ui.blocks.len(), 18, "three faces, two triangles each");
        assert!(ui.items.is_empty());

        let mut ui = screen();
        ui.slot(art, Slot::new(278, 1), 0.0, 0.0);
        assert_eq!(ui.items.len(), 6, "one quad");
        assert!(ui.blocks.is_empty());
    }

    #[test]
    fn a_stack_of_one_has_no_number_on_it() {
        let atlas = Atlas::build(&Flat);
        let items = Items::build(&Flat);
        let font = Font::measure(None);
        let art = SlotArt {
            atlas: &atlas,
            items: &items,
            font: &font,
            light: 1.0,
        };
        let mut ui = screen();
        ui.slot(art, Slot::new(4, 1), 0.0, 0.0);
        assert!(ui.text.is_empty(), "one of something is just the thing");
        let mut ui = screen();
        ui.slot(art, Slot::new(4, 64), 0.0, 0.0);
        assert!(!ui.text.is_empty(), "a stack says how many");
    }

    #[test]
    fn colour_codes_change_the_colour_and_are_not_drawn() {
        let font = Font::measure(None);
        let mut plain = screen();
        plain.text_flat(&font, "ab", 0.0, 0.0, [1.0, 1.0, 1.0, 1.0]);
        let mut coded = screen();
        coded.text_flat(&font, "§cab", 0.0, 0.0, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(
            plain.text.len(),
            coded.text.len(),
            "the code itself is not a glyph"
        );
        assert_ne!(plain.text[0].colour, coded.text[0].colour);
    }

    #[test]
    fn the_hud_puts_the_hotbar_where_the_game_puts_it() {
        let atlas = Atlas::build(&Flat);
        let items = Items::build(&Flat);
        let font = Font::measure(None);
        let mut ui = screen();
        let mut hotbar = [None; 9];
        hotbar[0] = Some(Slot::new(1, 32));
        ui.hud(
            SlotArt {
                atlas: &atlas,
                items: &items,
                font: &font,
                light: 1.0,
            },
            &Hud {
                held: 0,
                health: 20.0,
                food: 20,
                air: 300,
                experience: 0.5,
                level: 7,
                hotbar,
            },
        );
        assert!(!ui.sheet.is_empty(), "the bar, the hearts and the crosshair");
        assert!(!ui.blocks.is_empty(), "the stone in the first slot");
        // Every quad is inside the screen.
        for glyph in ui.sheet.iter().chain(&ui.blocks) {
            assert!(
                glyph.position[0] >= -1.001 && glyph.position[0] <= 1.001,
                "{:?} is off the side",
                glyph.position
            );
            // One unit of overhang is allowed, because vanilla itself has it: the selected-slot
            // box is 24 units tall over a 22-unit bar and starts a unit above it, so its bottom
            // edge really is one unit below the bottom of the screen.
            let slack = 1.5 * ui.scale / ui.size().1 / ui.scale * 2.0;
            assert!(
                glyph.position[1] >= -1.0 - slack && glyph.position[1] <= 1.0 + slack,
                "{:?} is off the top or bottom (screen is {:?})",
                glyph.position,
                ui.size()
            );
        }
    }

    #[test]
    fn a_window_position_maps_back_to_the_interface() {
        let ui = Ui::new(1280, 720);
        let (x, y) = ui.at(640.0, 360.0);
        let (w, h) = ui.size();
        assert!((x - w / 2.0).abs() < 1e-3 && (y - h / 2.0).abs() < 1e-3);
    }
}
