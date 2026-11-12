//! The game's own interface, drawn from the game's own art.
//!
//! The layout is written here; the pixels come out of the player's jar. Vanilla picks a whole
//! number for the interface scale so the 16-pixel art never lands between screen pixels, and
//! every placement below is in those interface units rather than screen pixels — which is why
//! the hotbar sits in the same place at any window size.

use ninefold_assets::{AssetSource, Texture};
use ninefold_render::Glyph;

/// Both sheets the HUD needs, side by side in one texture.
pub struct Sheets {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Where each sheet starts inside [`Sheets`].
const WIDGETS_AT: (u32, u32) = (0, 0);
const ICONS_AT: (u32, u32) = (256, 0);
const SHEET: u32 = 256;

/// A rectangle of one of the sheets, in its own pixels.
#[derive(Debug, Clone, Copy)]
pub struct Sprite {
    at: (u32, u32),
    x: u32,
    y: u32,
    w: u32,
    h: u32,
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
}

/// The pieces of the 1.8 HUD, at the coordinates they live at in the sheets.
pub const HOTBAR: Sprite = Sprite::widgets(0, 0, 182, 22);
pub const SELECTED: Sprite = Sprite::widgets(0, 22, 24, 24);
pub const CROSSHAIR: Sprite = Sprite::icons(0, 0, 16, 16);
pub const HEART_SLOT: Sprite = Sprite::icons(16, 0, 9, 9);
pub const HEART_FULL: Sprite = Sprite::icons(52, 0, 9, 9);
pub const HEART_HALF: Sprite = Sprite::icons(61, 0, 9, 9);
pub const FOOD_SLOT: Sprite = Sprite::icons(16, 27, 9, 9);
pub const FOOD_FULL: Sprite = Sprite::icons(52, 27, 9, 9);
pub const FOOD_HALF: Sprite = Sprite::icons(61, 27, 9, 9);

/// Put the two sheets into one texture, so the HUD is a single draw.
#[must_use]
pub fn sheets(assets: &dyn AssetSource) -> Sheets {
    let width = SHEET * 2;
    let height = SHEET;
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for (name, (ox, oy)) in [("gui/widgets", WIDGETS_AT), ("gui/icons", ICONS_AT)] {
        let Some(sheet) = assets.texture(name) else {
            continue;
        };
        paste(&mut rgba, width, ox, oy, &sheet);
    }
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

/// Lays sprites out in interface units and turns them into screen-space quads.
pub struct Hud {
    /// How many screen pixels one interface pixel takes.
    pub scale: f32,
    screen: (f32, f32),
    quads: Vec<Glyph>,
}

impl Hud {
    /// Vanilla picks the largest whole scale that still leaves a 320x240 interface fitting.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let fits = (width / 320).min(height / 240).clamp(1, 4);
        Self {
            scale: fits as f32,
            screen: (width as f32, height as f32),
            quads: Vec::new(),
        }
    }

    /// The interface is this many units across and down.
    #[must_use]
    pub fn size(&self) -> (f32, f32) {
        (self.screen.0 / self.scale, self.screen.1 / self.scale)
    }

    /// Place one sprite, in interface units from the top left.
    pub fn draw(&mut self, sprite: Sprite, x: f32, y: f32) {
        self.draw_scaled(sprite, x, y, sprite.w as f32, sprite.h as f32);
    }

    /// Place one sprite at a size of its own.
    pub fn draw_scaled(&mut self, sprite: Sprite, x: f32, y: f32, w: f32, h: f32) {
        let (sw, sh) = self.screen;
        let to_clip = |px: f32, py: f32| {
            [
                px * self.scale / sw * 2.0 - 1.0,
                1.0 - py * self.scale / sh * 2.0,
                0.0,
            ]
        };
        // The sheets sit side by side in a texture twice as wide as one sheet.
        let (tw, th) = ((SHEET * 2) as f32, SHEET as f32);
        let u0 = (sprite.at.0 + sprite.x) as f32 / tw;
        let v0 = (sprite.at.1 + sprite.y) as f32 / th;
        let u1 = (sprite.at.0 + sprite.x + sprite.w) as f32 / tw;
        let v1 = (sprite.at.1 + sprite.y + sprite.h) as f32 / th;
        let colour = [1.0, 1.0, 1.0, 1.0];
        let corners = [
            ([x, y], [u0, v0]),
            ([x + w, y], [u1, v0]),
            ([x + w, y + h], [u1, v1]),
            ([x, y], [u0, v0]),
            ([x + w, y + h], [u1, v1]),
            ([x, y + h], [u0, v1]),
        ];
        for ([px, py], uv) in corners {
            self.quads.push(Glyph {
                position: to_clip(px, py),
                uv,
                colour,
            });
        }
    }

    /// The whole 1.8 HUD: hotbar, selection, crosshair, hearts and food.
    pub fn lay_out(&mut self, selected: u8, health: f32, food: u8) {
        let (w, h) = self.size();
        let middle = w / 2.0;

        // Hotbar, centred along the bottom.
        let bar_x = middle - HOTBAR.w as f32 / 2.0;
        let bar_y = h - HOTBAR.h as f32;
        self.draw(HOTBAR, bar_x, bar_y);
        // The selection box is two pixels bigger than a slot on every side.
        let slot = f32::from(selected.min(8)) * 20.0;
        self.draw(SELECTED, bar_x - 1.0 + slot, bar_y - 1.0);

        // Crosshair, dead centre.
        self.draw(
            CROSSHAIR,
            middle - CROSSHAIR.w as f32 / 2.0,
            h / 2.0 - CROSSHAIR.h as f32 / 2.0,
        );

        // Ten hearts and ten drumsticks, above the hotbar, halves included.
        let row = bar_y - 10.0;
        for i in 0u8..10 {
            let x = bar_x + f32::from(i) * 8.0;
            self.draw(HEART_SLOT, x, row);
            let left = health - f32::from(i) * 2.0;
            if left >= 2.0 {
                self.draw(HEART_FULL, x, row);
            } else if left >= 1.0 {
                self.draw(HEART_HALF, x, row);
            }
        }
        for i in 0u8..10 {
            // Food fills from the right, which is why it is counted backwards.
            let x = bar_x + HOTBAR.w as f32 - 9.0 - f32::from(i) * 8.0;
            self.draw(FOOD_SLOT, x, row);
            let left = f32::from(food) - f32::from(i) * 2.0;
            if left >= 2.0 {
                self.draw(FOOD_FULL, x, row);
            } else if left >= 1.0 {
                self.draw(FOOD_HALF, x, row);
            }
        }
    }

    #[must_use]
    pub fn finish(self) -> Vec<Glyph> {
        self.quads
    }
}
