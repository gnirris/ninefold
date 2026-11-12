//! One texture holding every block texture, and where each one sits in it.
//!
//! A draw call can only sample one texture, so every block face has to come out of the same
//! image. The tiles are 16×16 because that is what 1.8 ships, laid out in a square grid, and
//! each face of each block gets the rectangle it should sample.
//!
//! Two textures are not simply copied in:
//!
//! - **grass_top** ships greyscale and is tinted by biome at draw time. Untinted, the world is
//!   grey, which looks like a renderer bug and is not one.
//! - **grass_side** is pre-coloured dirt with a transparent-topped overlay, `grass_side_overlay`,
//!   that carries the green. The two are composited here, overlay tinted, which is what gives a
//!   grass block its green edge.

use std::collections::HashMap;

use ninefold_assets::{AssetSource, Texture};
use ninefold_mesh::Face;

/// Plains biome colours. A proper biome lookup reads these out of the chunk's biome array.
const GRASS_TINT: [f32; 3] = [
    0x91 as f32 / 255.0,
    0xBD as f32 / 255.0,
    0x59 as f32 / 255.0,
];
const FOLIAGE_TINT: [f32; 3] = [
    0x77 as f32 / 255.0,
    0xAB as f32 / 255.0,
    0x2F as f32 / 255.0,
];

/// Tiles are the size 1.8's textures are.
const TILE: u32 = 16;

/// How one tile is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Recipe {
    /// Straight out of the jar.
    Plain(&'static str),
    /// Greyscale in the file, coloured at draw time.
    Tinted(&'static str, &'static str),
    /// A base with a tinted cut-out laid over it, which is how grass sides work.
    Overlay(&'static str, &'static str, &'static str),
}

/// Which texture each face of a block uses: (top, bottom, side).
struct Block {
    id: u16,
    top: Recipe,
    bottom: Recipe,
    side: Recipe,
}

/// Enough of 1.8 to render what a flat or ordinary world is made of. The rest arrives with a
/// generated block table; this is hand-written so the first textured world can exist.
fn blocks() -> Vec<Block> {
    let same = |name: &'static str| Block {
        id: 0,
        top: Recipe::Plain(name),
        bottom: Recipe::Plain(name),
        side: Recipe::Plain(name),
    };
    let with = |id: u16, block: Block| Block { id, ..block };
    vec![
        with(1, same("blocks/stone")),
        Block {
            id: 2,
            top: Recipe::Tinted("blocks/grass_top", "grass"),
            bottom: Recipe::Plain("blocks/dirt"),
            side: Recipe::Overlay("blocks/grass_side", "blocks/grass_side_overlay", "grass"),
        },
        with(3, same("blocks/dirt")),
        with(4, same("blocks/cobblestone")),
        with(5, same("blocks/planks_oak")),
        with(7, same("blocks/bedrock")),
        with(12, same("blocks/sand")),
        with(13, same("blocks/gravel")),
        Block {
            id: 17,
            top: Recipe::Plain("blocks/log_oak_top"),
            bottom: Recipe::Plain("blocks/log_oak_top"),
            side: Recipe::Plain("blocks/log_oak"),
        },
        with(18, same("blocks/leaves_oak")),
        with(20, same("blocks/glass")),
        with(24, same("blocks/sandstone_normal")),
        with(35, same("blocks/wool_colored_white")),
        with(45, same("blocks/brick")),
        with(80, same("blocks/snow")),
        with(87, same("blocks/netherrack")),
    ]
}

/// The finished atlas, and where everything is in it.
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Left, top, width, height in 0..1, by block id and face.
    rects: HashMap<(u16, bool, bool), [f32; 4]>,
    missing: [f32; 4],
}

impl Atlas {
    /// Build it from whatever the asset source can provide.
    #[must_use]
    pub fn build(assets: &dyn AssetSource) -> Self {
        let blocks = blocks();
        // One tile per distinct recipe, plus one for anything unknown.
        let mut order: Vec<Recipe> = Vec::new();
        for block in &blocks {
            for recipe in [block.top, block.bottom, block.side] {
                if !order.contains(&recipe) {
                    order.push(recipe);
                }
            }
        }
        let count = order.len() + 1;
        let across = (count as f64).sqrt().ceil() as u32;
        let down = count.div_ceil(across as usize) as u32;
        let (width, height) = (across * TILE, down * TILE);
        let mut rgba = vec![0u8; (width * height * 4) as usize];

        let mut at: HashMap<Recipe, [f32; 4]> = HashMap::new();
        for (index, recipe) in order.iter().enumerate() {
            let tile = bake(assets, *recipe);
            let (tx, ty) = (index as u32 % across, index as u32 / across);
            blit(&mut rgba, width, tx * TILE, ty * TILE, &tile);
            at.insert(*recipe, rect(tx, ty, width, height));
        }
        // The last tile is the "no texture for this" one: loud, so it cannot be mistaken.
        let last = order.len() as u32;
        let (mx, my) = (last % across, last / across);
        blit(
            &mut rgba,
            width,
            mx * TILE,
            my * TILE,
            &Texture::flat(TILE, TILE, [0xCC, 0x44, 0xCC, 0xFF]),
        );

        let mut rects = HashMap::new();
        for block in &blocks {
            for (recipe, top, bottom) in [
                (block.top, true, false),
                (block.bottom, false, true),
                (block.side, false, false),
            ] {
                if let Some(r) = at.get(&recipe) {
                    rects.insert((block.id, top, bottom), *r);
                }
            }
        }
        Self {
            width,
            height,
            rgba,
            rects,
            missing: rect(mx, my, width, height),
        }
    }

    /// Where a face of a block samples from.
    #[must_use]
    pub fn rect(&self, block: u16, face: Face) -> [f32; 4] {
        let (top, bottom) = (face == Face::Up, face == Face::Down);
        *self
            .rects
            .get(&(block >> 4, top, bottom))
            .unwrap_or(&self.missing)
    }

    /// Map a face's 0..1 corner onto its tile.
    #[must_use]
    pub fn uv(&self, block: u16, face: Face, uv: [f32; 2]) -> [f32; 2] {
        let [x, y, w, h] = self.rect(block, face);
        [x + uv[0] * w, y + uv[1] * h]
    }
}

fn rect(tx: u32, ty: u32, width: u32, height: u32) -> [f32; 4] {
    [
        (tx * TILE) as f32 / width as f32,
        (ty * TILE) as f32 / height as f32,
        TILE as f32 / width as f32,
        TILE as f32 / height as f32,
    ]
}

/// Produce one tile's pixels.
fn bake(assets: &dyn AssetSource, recipe: Recipe) -> Texture {
    let tint_of = |name: &str| match name {
        "grass" => GRASS_TINT,
        _ => FOLIAGE_TINT,
    };
    match recipe {
        Recipe::Plain(name) => assets
            .texture(name)
            .map_or_else(|| Texture::flat(TILE, TILE, [0xCC, 0x44, 0xCC, 0xFF]), crop),
        Recipe::Tinted(name, tint) => {
            let mut texture = bake(assets, Recipe::Plain(name));
            apply_tint(&mut texture, tint_of(tint));
            texture
        }
        Recipe::Overlay(base, over, tint) => {
            let mut bottom = bake(assets, Recipe::Plain(base));
            let mut top = bake(assets, Recipe::Plain(over));
            apply_tint(&mut top, tint_of(tint));
            // Straight over: the overlay's transparent pixels keep the base showing through.
            let overlay = top.rgba.as_chunks::<4>().0;
            for (under, above) in bottom.rgba.as_chunks_mut::<4>().0.iter_mut().zip(overlay) {
                if above[3] >= 8 {
                    *under = *above;
                }
            }
            bottom
        }
    }
}

/// Animated textures (water, lava) are a column of frames; take the first one.
fn crop(texture: Texture) -> Texture {
    if texture.height <= TILE && texture.width <= TILE {
        return texture;
    }
    let mut rgba = Vec::with_capacity((TILE * TILE * 4) as usize);
    for y in 0..TILE.min(texture.height) {
        let row = (y * texture.width * 4) as usize;
        rgba.extend_from_slice(&texture.rgba[row..row + (TILE.min(texture.width) * 4) as usize]);
    }
    rgba.resize((TILE * TILE * 4) as usize, 0);
    Texture {
        width: TILE,
        height: TILE,
        rgba,
    }
}

fn apply_tint(texture: &mut Texture, tint: [f32; 3]) {
    for pixel in texture.rgba.as_chunks_mut::<4>().0 {
        for (channel, t) in pixel.iter_mut().take(3).zip(tint) {
            *channel = (f32::from(*channel) * t) as u8;
        }
    }
}

/// Copy a tile into the atlas at a tile position.
fn blit(atlas: &mut [u8], atlas_width: u32, x: u32, y: u32, tile: &Texture) {
    for row in 0..TILE.min(tile.height) {
        let from = (row * tile.width * 4) as usize;
        let to = (((y + row) * atlas_width + x) * 4) as usize;
        let width = (TILE.min(tile.width) * 4) as usize;
        if from + width <= tile.rgba.len() && to + width <= atlas.len() {
            atlas[to..to + width].copy_from_slice(&tile.rgba[from..from + width]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat;
    impl AssetSource for Flat {
        fn texture(&self, id: &str) -> Option<Texture> {
            // grass_top greyscale, so the tint is visible in the result.
            let shade = if id.contains("grass_top") { 150 } else { 80 };
            Some(Texture::flat(TILE, TILE, [shade, shade, shade, 255]))
        }
        fn describe(&self) -> String {
            "test".into()
        }
    }

    #[test]
    fn every_face_of_a_known_block_has_somewhere_to_sample() {
        let atlas = Atlas::build(&Flat);
        for face in Face::ALL {
            let r = atlas.rect(2 << 4, face);
            assert!(r[2] > 0.0 && r[3] > 0.0, "grass {face:?} has no tile");
            assert_ne!(
                r, atlas.missing,
                "grass {face:?} fell back to the missing tile"
            );
        }
    }

    #[test]
    fn an_unknown_block_gets_the_loud_tile_rather_than_nothing() {
        let atlas = Atlas::build(&Flat);
        assert_eq!(atlas.rect(999 << 4, Face::Up), atlas.missing);
    }

    #[test]
    fn grass_uses_a_different_tile_on_top_than_underneath() {
        let atlas = Atlas::build(&Flat);
        let top = atlas.rect(2 << 4, Face::Up);
        let bottom = atlas.rect(2 << 4, Face::Down);
        let side = atlas.rect(2 << 4, Face::North);
        assert_ne!(top, bottom, "grass top is not dirt");
        assert_ne!(top, side);
        // The bottom of a grass block is dirt, so it shares dirt's tile.
        assert_eq!(bottom, atlas.rect(3 << 4, Face::Up), "grass bottom is dirt");
    }

    #[test]
    fn corners_map_inside_the_tile_they_belong_to() {
        let atlas = Atlas::build(&Flat);
        let [x, y, w, h] = atlas.rect(1 << 4, Face::Up);
        assert_eq!(atlas.uv(1 << 4, Face::Up, [0.0, 0.0]), [x, y]);
        assert_eq!(atlas.uv(1 << 4, Face::Up, [1.0, 1.0]), [x + w, y + h]);
    }

    #[test]
    fn the_tint_is_actually_applied() {
        let atlas = Atlas::build(&Flat);
        let [x, y, ..] = atlas.rect(2 << 4, Face::Up);
        let px = (y * atlas.height as f32) as u32 * atlas.width + (x * atlas.width as f32) as u32;
        let at = (px * 4) as usize;
        let (r, g, b) = (atlas.rgba[at], atlas.rgba[at + 1], atlas.rgba[at + 2]);
        assert!(g > r && g > b, "grass should be green, got ({r},{g},{b})");
    }
}
