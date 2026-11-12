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
    Tinted(&'static str, Tint),
    /// A base with a tinted cut-out laid over it, which is how grass sides work.
    Overlay(&'static str, &'static str, Tint),
}

/// Which biome colour a greyscale texture is multiplied by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Tint {
    Grass,
    Foliage,
}

impl Tint {
    const fn colour(self) -> [f32; 3] {
        match self {
            Tint::Grass => GRASS_TINT,
            Tint::Foliage => FOLIAGE_TINT,
        }
    }
}

/// Which texture each face of a block uses: (top, bottom, side).
struct Block {
    id: u16,
    /// `None` matches any metadata; `Some` wins over it, which is how wool colours, wood types
    /// and stone variants sit on top of a default.
    meta: Option<u8>,
    top: Recipe,
    bottom: Recipe,
    side: Recipe,
}

/// Enough of 1.8 to render what a flat or ordinary world is made of. The rest arrives with a
/// generated block table; this is hand-written so the first textured world can exist.
fn blocks() -> Vec<Block> {
    vec![
        Block {
            id: 1,
            meta: None,
            top: Recipe::Plain("blocks/stone"),
            bottom: Recipe::Plain("blocks/stone"),
            side: Recipe::Plain("blocks/stone"),
        },
        Block {
            id: 1,
            meta: Some(1),
            top: Recipe::Plain("blocks/stone_granite"),
            bottom: Recipe::Plain("blocks/stone_granite"),
            side: Recipe::Plain("blocks/stone_granite"),
        },
        Block {
            id: 1,
            meta: Some(3),
            top: Recipe::Plain("blocks/stone_diorite"),
            bottom: Recipe::Plain("blocks/stone_diorite"),
            side: Recipe::Plain("blocks/stone_diorite"),
        },
        Block {
            id: 1,
            meta: Some(5),
            top: Recipe::Plain("blocks/stone_andesite"),
            bottom: Recipe::Plain("blocks/stone_andesite"),
            side: Recipe::Plain("blocks/stone_andesite"),
        },
        Block {
            id: 2,
            meta: None,
            top: Recipe::Tinted("blocks/grass_top", Tint::Grass),
            bottom: Recipe::Plain("blocks/dirt"),
            side: Recipe::Overlay(
                "blocks/grass_side",
                "blocks/grass_side_overlay",
                Tint::Grass,
            ),
        },
        Block {
            id: 3,
            meta: None,
            top: Recipe::Plain("blocks/dirt"),
            bottom: Recipe::Plain("blocks/dirt"),
            side: Recipe::Plain("blocks/dirt"),
        },
        Block {
            id: 3,
            meta: Some(2),
            top: Recipe::Plain("blocks/dirt_podzol_top"),
            bottom: Recipe::Plain("blocks/dirt"),
            side: Recipe::Plain("blocks/dirt_podzol_side"),
        },
        Block {
            id: 4,
            meta: None,
            top: Recipe::Plain("blocks/cobblestone"),
            bottom: Recipe::Plain("blocks/cobblestone"),
            side: Recipe::Plain("blocks/cobblestone"),
        },
        Block {
            id: 5,
            meta: None,
            top: Recipe::Plain("blocks/planks_oak"),
            bottom: Recipe::Plain("blocks/planks_oak"),
            side: Recipe::Plain("blocks/planks_oak"),
        },
        Block {
            id: 5,
            meta: Some(1),
            top: Recipe::Plain("blocks/planks_spruce"),
            bottom: Recipe::Plain("blocks/planks_spruce"),
            side: Recipe::Plain("blocks/planks_spruce"),
        },
        Block {
            id: 5,
            meta: Some(2),
            top: Recipe::Plain("blocks/planks_birch"),
            bottom: Recipe::Plain("blocks/planks_birch"),
            side: Recipe::Plain("blocks/planks_birch"),
        },
        Block {
            id: 5,
            meta: Some(3),
            top: Recipe::Plain("blocks/planks_jungle"),
            bottom: Recipe::Plain("blocks/planks_jungle"),
            side: Recipe::Plain("blocks/planks_jungle"),
        },
        Block {
            id: 7,
            meta: None,
            top: Recipe::Plain("blocks/bedrock"),
            bottom: Recipe::Plain("blocks/bedrock"),
            side: Recipe::Plain("blocks/bedrock"),
        },
        Block {
            id: 8,
            meta: None,
            top: Recipe::Plain("blocks/water_still"),
            bottom: Recipe::Plain("blocks/water_still"),
            side: Recipe::Plain("blocks/water_still"),
        },
        Block {
            id: 9,
            meta: None,
            top: Recipe::Plain("blocks/water_still"),
            bottom: Recipe::Plain("blocks/water_still"),
            side: Recipe::Plain("blocks/water_still"),
        },
        Block {
            id: 10,
            meta: None,
            top: Recipe::Plain("blocks/lava_still"),
            bottom: Recipe::Plain("blocks/lava_still"),
            side: Recipe::Plain("blocks/lava_still"),
        },
        Block {
            id: 11,
            meta: None,
            top: Recipe::Plain("blocks/lava_still"),
            bottom: Recipe::Plain("blocks/lava_still"),
            side: Recipe::Plain("blocks/lava_still"),
        },
        Block {
            id: 12,
            meta: None,
            top: Recipe::Plain("blocks/sand"),
            bottom: Recipe::Plain("blocks/sand"),
            side: Recipe::Plain("blocks/sand"),
        },
        Block {
            id: 12,
            meta: Some(1),
            top: Recipe::Plain("blocks/red_sand"),
            bottom: Recipe::Plain("blocks/red_sand"),
            side: Recipe::Plain("blocks/red_sand"),
        },
        Block {
            id: 13,
            meta: None,
            top: Recipe::Plain("blocks/gravel"),
            bottom: Recipe::Plain("blocks/gravel"),
            side: Recipe::Plain("blocks/gravel"),
        },
        Block {
            id: 14,
            meta: None,
            top: Recipe::Plain("blocks/gold_ore"),
            bottom: Recipe::Plain("blocks/gold_ore"),
            side: Recipe::Plain("blocks/gold_ore"),
        },
        Block {
            id: 15,
            meta: None,
            top: Recipe::Plain("blocks/iron_ore"),
            bottom: Recipe::Plain("blocks/iron_ore"),
            side: Recipe::Plain("blocks/iron_ore"),
        },
        Block {
            id: 16,
            meta: None,
            top: Recipe::Plain("blocks/coal_ore"),
            bottom: Recipe::Plain("blocks/coal_ore"),
            side: Recipe::Plain("blocks/coal_ore"),
        },
        Block {
            id: 17,
            meta: None,
            top: Recipe::Plain("blocks/log_oak_top"),
            bottom: Recipe::Plain("blocks/log_oak_top"),
            side: Recipe::Plain("blocks/log_oak"),
        },
        Block {
            id: 17,
            meta: Some(1),
            top: Recipe::Plain("blocks/log_spruce_top"),
            bottom: Recipe::Plain("blocks/log_spruce_top"),
            side: Recipe::Plain("blocks/log_spruce"),
        },
        Block {
            id: 17,
            meta: Some(2),
            top: Recipe::Plain("blocks/log_birch_top"),
            bottom: Recipe::Plain("blocks/log_birch_top"),
            side: Recipe::Plain("blocks/log_birch"),
        },
        Block {
            id: 17,
            meta: Some(3),
            top: Recipe::Plain("blocks/log_jungle_top"),
            bottom: Recipe::Plain("blocks/log_jungle_top"),
            side: Recipe::Plain("blocks/log_jungle"),
        },
        Block {
            id: 18,
            meta: None,
            top: Recipe::Tinted("blocks/leaves_oak", Tint::Foliage),
            bottom: Recipe::Tinted("blocks/leaves_oak", Tint::Foliage),
            side: Recipe::Tinted("blocks/leaves_oak", Tint::Foliage),
        },
        Block {
            id: 18,
            meta: Some(1),
            top: Recipe::Tinted("blocks/leaves_spruce", Tint::Foliage),
            bottom: Recipe::Tinted("blocks/leaves_spruce", Tint::Foliage),
            side: Recipe::Tinted("blocks/leaves_spruce", Tint::Foliage),
        },
        Block {
            id: 20,
            meta: None,
            top: Recipe::Plain("blocks/glass"),
            bottom: Recipe::Plain("blocks/glass"),
            side: Recipe::Plain("blocks/glass"),
        },
        Block {
            id: 21,
            meta: None,
            top: Recipe::Plain("blocks/lapis_ore"),
            bottom: Recipe::Plain("blocks/lapis_ore"),
            side: Recipe::Plain("blocks/lapis_ore"),
        },
        Block {
            id: 22,
            meta: None,
            top: Recipe::Plain("blocks/lapis_block"),
            bottom: Recipe::Plain("blocks/lapis_block"),
            side: Recipe::Plain("blocks/lapis_block"),
        },
        Block {
            id: 24,
            meta: None,
            top: Recipe::Plain("blocks/sandstone_top"),
            bottom: Recipe::Plain("blocks/sandstone_bottom"),
            side: Recipe::Plain("blocks/sandstone_normal"),
        },
        Block {
            id: 31,
            meta: None,
            top: Recipe::Tinted("blocks/tallgrass", Tint::Grass),
            bottom: Recipe::Tinted("blocks/tallgrass", Tint::Grass),
            side: Recipe::Tinted("blocks/tallgrass", Tint::Grass),
        },
        Block {
            id: 35,
            meta: None,
            top: Recipe::Plain("blocks/wool_colored_white"),
            bottom: Recipe::Plain("blocks/wool_colored_white"),
            side: Recipe::Plain("blocks/wool_colored_white"),
        },
        Block {
            id: 41,
            meta: None,
            top: Recipe::Plain("blocks/gold_block"),
            bottom: Recipe::Plain("blocks/gold_block"),
            side: Recipe::Plain("blocks/gold_block"),
        },
        Block {
            id: 42,
            meta: None,
            top: Recipe::Plain("blocks/iron_block"),
            bottom: Recipe::Plain("blocks/iron_block"),
            side: Recipe::Plain("blocks/iron_block"),
        },
        Block {
            id: 45,
            meta: None,
            top: Recipe::Plain("blocks/brick"),
            bottom: Recipe::Plain("blocks/brick"),
            side: Recipe::Plain("blocks/brick"),
        },
        Block {
            id: 46,
            meta: None,
            top: Recipe::Plain("blocks/tnt_top"),
            bottom: Recipe::Plain("blocks/tnt_bottom"),
            side: Recipe::Plain("blocks/tnt_side"),
        },
        Block {
            id: 47,
            meta: None,
            top: Recipe::Plain("blocks/planks_oak"),
            bottom: Recipe::Plain("blocks/planks_oak"),
            side: Recipe::Plain("blocks/bookshelf"),
        },
        Block {
            id: 48,
            meta: None,
            top: Recipe::Plain("blocks/cobblestone_mossy"),
            bottom: Recipe::Plain("blocks/cobblestone_mossy"),
            side: Recipe::Plain("blocks/cobblestone_mossy"),
        },
        Block {
            id: 49,
            meta: None,
            top: Recipe::Plain("blocks/obsidian"),
            bottom: Recipe::Plain("blocks/obsidian"),
            side: Recipe::Plain("blocks/obsidian"),
        },
        Block {
            id: 56,
            meta: None,
            top: Recipe::Plain("blocks/diamond_ore"),
            bottom: Recipe::Plain("blocks/diamond_ore"),
            side: Recipe::Plain("blocks/diamond_ore"),
        },
        Block {
            id: 57,
            meta: None,
            top: Recipe::Plain("blocks/diamond_block"),
            bottom: Recipe::Plain("blocks/diamond_block"),
            side: Recipe::Plain("blocks/diamond_block"),
        },
        Block {
            id: 73,
            meta: None,
            top: Recipe::Plain("blocks/redstone_ore"),
            bottom: Recipe::Plain("blocks/redstone_ore"),
            side: Recipe::Plain("blocks/redstone_ore"),
        },
        Block {
            id: 74,
            meta: None,
            top: Recipe::Plain("blocks/redstone_ore"),
            bottom: Recipe::Plain("blocks/redstone_ore"),
            side: Recipe::Plain("blocks/redstone_ore"),
        },
        Block {
            id: 78,
            meta: None,
            top: Recipe::Plain("blocks/snow"),
            bottom: Recipe::Plain("blocks/snow"),
            side: Recipe::Plain("blocks/snow"),
        },
        Block {
            id: 79,
            meta: None,
            top: Recipe::Plain("blocks/ice"),
            bottom: Recipe::Plain("blocks/ice"),
            side: Recipe::Plain("blocks/ice"),
        },
        Block {
            id: 80,
            meta: None,
            top: Recipe::Plain("blocks/snow"),
            bottom: Recipe::Plain("blocks/snow"),
            side: Recipe::Plain("blocks/snow"),
        },
        Block {
            id: 81,
            meta: None,
            top: Recipe::Plain("blocks/cactus_top"),
            bottom: Recipe::Plain("blocks/cactus_bottom"),
            side: Recipe::Plain("blocks/cactus_side"),
        },
        Block {
            id: 82,
            meta: None,
            top: Recipe::Plain("blocks/clay"),
            bottom: Recipe::Plain("blocks/clay"),
            side: Recipe::Plain("blocks/clay"),
        },
        Block {
            id: 86,
            meta: None,
            top: Recipe::Plain("blocks/pumpkin_top"),
            bottom: Recipe::Plain("blocks/pumpkin_top"),
            side: Recipe::Plain("blocks/pumpkin_side"),
        },
        Block {
            id: 87,
            meta: None,
            top: Recipe::Plain("blocks/netherrack"),
            bottom: Recipe::Plain("blocks/netherrack"),
            side: Recipe::Plain("blocks/netherrack"),
        },
        Block {
            id: 88,
            meta: None,
            top: Recipe::Plain("blocks/soul_sand"),
            bottom: Recipe::Plain("blocks/soul_sand"),
            side: Recipe::Plain("blocks/soul_sand"),
        },
        Block {
            id: 89,
            meta: None,
            top: Recipe::Plain("blocks/glowstone"),
            bottom: Recipe::Plain("blocks/glowstone"),
            side: Recipe::Plain("blocks/glowstone"),
        },
        Block {
            id: 98,
            meta: None,
            top: Recipe::Plain("blocks/stonebrick"),
            bottom: Recipe::Plain("blocks/stonebrick"),
            side: Recipe::Plain("blocks/stonebrick"),
        },
        Block {
            id: 112,
            meta: None,
            top: Recipe::Plain("blocks/nether_brick"),
            bottom: Recipe::Plain("blocks/nether_brick"),
            side: Recipe::Plain("blocks/nether_brick"),
        },
        Block {
            id: 121,
            meta: None,
            top: Recipe::Plain("blocks/end_stone"),
            bottom: Recipe::Plain("blocks/end_stone"),
            side: Recipe::Plain("blocks/end_stone"),
        },
        Block {
            id: 129,
            meta: None,
            top: Recipe::Plain("blocks/emerald_ore"),
            bottom: Recipe::Plain("blocks/emerald_ore"),
            side: Recipe::Plain("blocks/emerald_ore"),
        },
        Block {
            id: 155,
            meta: None,
            top: Recipe::Plain("blocks/quartz_block_top"),
            bottom: Recipe::Plain("blocks/quartz_block_bottom"),
            side: Recipe::Plain("blocks/quartz_block_side"),
        },
        Block {
            id: 159,
            meta: None,
            top: Recipe::Plain("blocks/hardened_clay_stained_white"),
            bottom: Recipe::Plain("blocks/hardened_clay_stained_white"),
            side: Recipe::Plain("blocks/hardened_clay_stained_white"),
        },
        Block {
            id: 161,
            meta: None,
            top: Recipe::Tinted("blocks/leaves_acacia", Tint::Foliage),
            bottom: Recipe::Tinted("blocks/leaves_acacia", Tint::Foliage),
            side: Recipe::Tinted("blocks/leaves_acacia", Tint::Foliage),
        },
        Block {
            id: 162,
            meta: None,
            top: Recipe::Plain("blocks/log_acacia_top"),
            bottom: Recipe::Plain("blocks/log_acacia_top"),
            side: Recipe::Plain("blocks/log_acacia"),
        },
        Block {
            id: 172,
            meta: None,
            top: Recipe::Plain("blocks/hardened_clay"),
            bottom: Recipe::Plain("blocks/hardened_clay"),
            side: Recipe::Plain("blocks/hardened_clay"),
        },
        Block {
            id: 174,
            meta: None,
            top: Recipe::Plain("blocks/ice_packed"),
            bottom: Recipe::Plain("blocks/ice_packed"),
            side: Recipe::Plain("blocks/ice_packed"),
        },
    ]
}

/// The finished atlas, and where everything is in it.
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Left, top, width, height in 0..1, by block id and face.
    rects: HashMap<(u16, Option<u8>, bool, bool), [f32; 4]>,
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
                    rects.insert((block.id, block.meta, top, bottom), *r);
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
        let (id, meta) = (block >> 4, (block & 0xF) as u8);
        // An exact metadata match first, then the entry that covers any metadata.
        self.rects
            .get(&(id, Some(meta), top, bottom))
            .or_else(|| self.rects.get(&(id, None, top, bottom)))
            .copied()
            .unwrap_or(self.missing)
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
    match recipe {
        Recipe::Plain(name) => assets
            .texture(name)
            .map_or_else(|| Texture::flat(TILE, TILE, [0xCC, 0x44, 0xCC, 0xFF]), crop),
        Recipe::Tinted(name, tint) => {
            let mut texture = bake(assets, Recipe::Plain(name));
            apply_tint(&mut texture, tint.colour());
            texture
        }
        Recipe::Overlay(base, over, tint) => {
            let mut bottom = bake(assets, Recipe::Plain(base));
            let mut top = bake(assets, Recipe::Plain(over));
            apply_tint(&mut top, tint.colour());
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
