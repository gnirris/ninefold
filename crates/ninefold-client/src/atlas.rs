//! One texture holding every block texture, and where each one sits in it.
//!
//! A draw call can only sample one texture, so every block face has to come out of the same
//! image. The tiles are 16×16 because that is what 1.8 ships, laid out in a square grid, and
//! each face of each block gets the rectangle it should sample.
//!
//! Two things are not simply copied in:
//!
//! - **grass_top** and the leaves ship greyscale and take their colour from the biome. That
//!   colour is not baked in here: it goes on the vertices, because one atlas tile has to serve a
//!   grass block in a desert and the same block in a swamp.
//! - **grass_side** is dirt with a dull fringe, and `grass_side_overlay` is the green part of
//!   that fringe as a separate cut-out. Vanilla draws the side plain and then lays the tinted
//!   overlay over it, so both get a tile and the mesher emits two quads.
//!
//! The biome colours themselves come out of `colormap/grass.png` and `colormap/foliage.png` —
//! the same 256×256 lookups the game uses, indexed by the biome's temperature and rainfall.

use std::collections::HashMap;

use ninefold_assets::{AssetSource, Texture};
use ninefold_mesh::{Face, Shape};

/// Plains colours, for when the colour maps cannot be read.
const FALLBACK_GRASS: [f32; 3] = [
    0x91 as f32 / 255.0,
    0xBD as f32 / 255.0,
    0x59 as f32 / 255.0,
];
const FALLBACK_FOLIAGE: [f32; 3] = [
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
pub enum Tint {
    Grass,
    Foliage,
}

impl Tint {
    const fn fallback(self) -> [f32; 3] {
        match self {
            Tint::Grass => FALLBACK_GRASS,
            Tint::Foliage => FALLBACK_FOLIAGE,
        }
    }

    const fn map(self) -> &'static str {
        match self {
            Tint::Grass => "colormap/grass",
            Tint::Foliage => "colormap/foliage",
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
        Block {
            id: 6,
            meta: None,
            top: Recipe::Plain("blocks/sapling_oak"),
            bottom: Recipe::Plain("blocks/sapling_oak"),
            side: Recipe::Plain("blocks/sapling_oak"),
        },
        Block {
            id: 6,
            meta: Some(1),
            top: Recipe::Plain("blocks/sapling_spruce"),
            bottom: Recipe::Plain("blocks/sapling_spruce"),
            side: Recipe::Plain("blocks/sapling_spruce"),
        },
        Block {
            id: 6,
            meta: Some(2),
            top: Recipe::Plain("blocks/sapling_birch"),
            bottom: Recipe::Plain("blocks/sapling_birch"),
            side: Recipe::Plain("blocks/sapling_birch"),
        },
        Block {
            id: 6,
            meta: Some(3),
            top: Recipe::Plain("blocks/sapling_jungle"),
            bottom: Recipe::Plain("blocks/sapling_jungle"),
            side: Recipe::Plain("blocks/sapling_jungle"),
        },
        Block {
            id: 6,
            meta: Some(4),
            top: Recipe::Plain("blocks/sapling_acacia"),
            bottom: Recipe::Plain("blocks/sapling_acacia"),
            side: Recipe::Plain("blocks/sapling_acacia"),
        },
        Block {
            id: 6,
            meta: Some(5),
            top: Recipe::Plain("blocks/sapling_roofed_oak"),
            bottom: Recipe::Plain("blocks/sapling_roofed_oak"),
            side: Recipe::Plain("blocks/sapling_roofed_oak"),
        },
        Block {
            id: 31,
            meta: None,
            top: Recipe::Plain("blocks/deadbush"),
            bottom: Recipe::Plain("blocks/deadbush"),
            side: Recipe::Plain("blocks/deadbush"),
        },
        Block {
            id: 31,
            meta: Some(1),
            top: Recipe::Tinted("blocks/tallgrass", Tint::Grass),
            bottom: Recipe::Tinted("blocks/tallgrass", Tint::Grass),
            side: Recipe::Tinted("blocks/tallgrass", Tint::Grass),
        },
        Block {
            id: 31,
            meta: Some(2),
            top: Recipe::Tinted("blocks/fern", Tint::Grass),
            bottom: Recipe::Tinted("blocks/fern", Tint::Grass),
            side: Recipe::Tinted("blocks/fern", Tint::Grass),
        },
        Block {
            id: 32,
            meta: None,
            top: Recipe::Plain("blocks/deadbush"),
            bottom: Recipe::Plain("blocks/deadbush"),
            side: Recipe::Plain("blocks/deadbush"),
        },
        Block {
            id: 37,
            meta: None,
            top: Recipe::Plain("blocks/flower_dandelion"),
            bottom: Recipe::Plain("blocks/flower_dandelion"),
            side: Recipe::Plain("blocks/flower_dandelion"),
        },
        Block {
            id: 38,
            meta: None,
            top: Recipe::Plain("blocks/flower_rose"),
            bottom: Recipe::Plain("blocks/flower_rose"),
            side: Recipe::Plain("blocks/flower_rose"),
        },
        Block {
            id: 38,
            meta: Some(1),
            top: Recipe::Plain("blocks/flower_blue_orchid"),
            bottom: Recipe::Plain("blocks/flower_blue_orchid"),
            side: Recipe::Plain("blocks/flower_blue_orchid"),
        },
        Block {
            id: 38,
            meta: Some(2),
            top: Recipe::Plain("blocks/flower_allium"),
            bottom: Recipe::Plain("blocks/flower_allium"),
            side: Recipe::Plain("blocks/flower_allium"),
        },
        Block {
            id: 38,
            meta: Some(3),
            top: Recipe::Plain("blocks/flower_houstonia"),
            bottom: Recipe::Plain("blocks/flower_houstonia"),
            side: Recipe::Plain("blocks/flower_houstonia"),
        },
        Block {
            id: 38,
            meta: Some(4),
            top: Recipe::Plain("blocks/flower_tulip_red"),
            bottom: Recipe::Plain("blocks/flower_tulip_red"),
            side: Recipe::Plain("blocks/flower_tulip_red"),
        },
        Block {
            id: 38,
            meta: Some(5),
            top: Recipe::Plain("blocks/flower_tulip_orange"),
            bottom: Recipe::Plain("blocks/flower_tulip_orange"),
            side: Recipe::Plain("blocks/flower_tulip_orange"),
        },
        Block {
            id: 38,
            meta: Some(6),
            top: Recipe::Plain("blocks/flower_tulip_white"),
            bottom: Recipe::Plain("blocks/flower_tulip_white"),
            side: Recipe::Plain("blocks/flower_tulip_white"),
        },
        Block {
            id: 38,
            meta: Some(7),
            top: Recipe::Plain("blocks/flower_tulip_pink"),
            bottom: Recipe::Plain("blocks/flower_tulip_pink"),
            side: Recipe::Plain("blocks/flower_tulip_pink"),
        },
        Block {
            id: 38,
            meta: Some(8),
            top: Recipe::Plain("blocks/flower_oxeye_daisy"),
            bottom: Recipe::Plain("blocks/flower_oxeye_daisy"),
            side: Recipe::Plain("blocks/flower_oxeye_daisy"),
        },
        Block {
            id: 39,
            meta: None,
            top: Recipe::Plain("blocks/mushroom_brown"),
            bottom: Recipe::Plain("blocks/mushroom_brown"),
            side: Recipe::Plain("blocks/mushroom_brown"),
        },
        Block {
            id: 40,
            meta: None,
            top: Recipe::Plain("blocks/mushroom_red"),
            bottom: Recipe::Plain("blocks/mushroom_red"),
            side: Recipe::Plain("blocks/mushroom_red"),
        },
        Block {
            id: 50,
            meta: None,
            top: Recipe::Plain("blocks/torch_on"),
            bottom: Recipe::Plain("blocks/torch_on"),
            side: Recipe::Plain("blocks/torch_on"),
        },
        Block {
            id: 59,
            meta: None,
            top: Recipe::Plain("blocks/wheat_stage_0"),
            bottom: Recipe::Plain("blocks/wheat_stage_0"),
            side: Recipe::Plain("blocks/wheat_stage_0"),
        },
        Block {
            id: 59,
            meta: Some(1),
            top: Recipe::Plain("blocks/wheat_stage_1"),
            bottom: Recipe::Plain("blocks/wheat_stage_1"),
            side: Recipe::Plain("blocks/wheat_stage_1"),
        },
        Block {
            id: 59,
            meta: Some(2),
            top: Recipe::Plain("blocks/wheat_stage_2"),
            bottom: Recipe::Plain("blocks/wheat_stage_2"),
            side: Recipe::Plain("blocks/wheat_stage_2"),
        },
        Block {
            id: 59,
            meta: Some(3),
            top: Recipe::Plain("blocks/wheat_stage_3"),
            bottom: Recipe::Plain("blocks/wheat_stage_3"),
            side: Recipe::Plain("blocks/wheat_stage_3"),
        },
        Block {
            id: 59,
            meta: Some(4),
            top: Recipe::Plain("blocks/wheat_stage_4"),
            bottom: Recipe::Plain("blocks/wheat_stage_4"),
            side: Recipe::Plain("blocks/wheat_stage_4"),
        },
        Block {
            id: 59,
            meta: Some(5),
            top: Recipe::Plain("blocks/wheat_stage_5"),
            bottom: Recipe::Plain("blocks/wheat_stage_5"),
            side: Recipe::Plain("blocks/wheat_stage_5"),
        },
        Block {
            id: 59,
            meta: Some(6),
            top: Recipe::Plain("blocks/wheat_stage_6"),
            bottom: Recipe::Plain("blocks/wheat_stage_6"),
            side: Recipe::Plain("blocks/wheat_stage_6"),
        },
        Block {
            id: 59,
            meta: Some(7),
            top: Recipe::Plain("blocks/wheat_stage_7"),
            bottom: Recipe::Plain("blocks/wheat_stage_7"),
            side: Recipe::Plain("blocks/wheat_stage_7"),
        },
        Block {
            id: 83,
            meta: None,
            top: Recipe::Tinted("blocks/reeds", Tint::Grass),
            bottom: Recipe::Tinted("blocks/reeds", Tint::Grass),
            side: Recipe::Tinted("blocks/reeds", Tint::Grass),
        },
        Block {
            id: 104,
            meta: None,
            top: Recipe::Plain("blocks/pumpkin_stem_disconnected"),
            bottom: Recipe::Plain("blocks/pumpkin_stem_disconnected"),
            side: Recipe::Plain("blocks/pumpkin_stem_disconnected"),
        },
        Block {
            id: 105,
            meta: None,
            top: Recipe::Plain("blocks/melon_stem_disconnected"),
            bottom: Recipe::Plain("blocks/melon_stem_disconnected"),
            side: Recipe::Plain("blocks/melon_stem_disconnected"),
        },
        Block {
            id: 115,
            meta: None,
            top: Recipe::Plain("blocks/nether_wart_stage_2"),
            bottom: Recipe::Plain("blocks/nether_wart_stage_2"),
            side: Recipe::Plain("blocks/nether_wart_stage_2"),
        },
        Block {
            id: 141,
            meta: None,
            top: Recipe::Plain("blocks/carrots_stage_3"),
            bottom: Recipe::Plain("blocks/carrots_stage_3"),
            side: Recipe::Plain("blocks/carrots_stage_3"),
        },
        Block {
            id: 142,
            meta: None,
            top: Recipe::Plain("blocks/potatoes_stage_3"),
            bottom: Recipe::Plain("blocks/potatoes_stage_3"),
            side: Recipe::Plain("blocks/potatoes_stage_3"),
        },
        Block {
            id: 175,
            meta: None,
            top: Recipe::Tinted("blocks/double_plant_grass_top", Tint::Grass),
            bottom: Recipe::Tinted("blocks/double_plant_grass_top", Tint::Grass),
            side: Recipe::Tinted("blocks/double_plant_grass_top", Tint::Grass),
        },
        Block {
            id: 175,
            meta: Some(1),
            top: Recipe::Plain("blocks/double_plant_syringa_top"),
            bottom: Recipe::Plain("blocks/double_plant_syringa_top"),
            side: Recipe::Plain("blocks/double_plant_syringa_top"),
        },
        Block {
            id: 175,
            meta: Some(4),
            top: Recipe::Plain("blocks/double_plant_rose_top"),
            bottom: Recipe::Plain("blocks/double_plant_rose_top"),
            side: Recipe::Plain("blocks/double_plant_rose_top"),
        },
        Block {
            id: 175,
            meta: Some(5),
            top: Recipe::Plain("blocks/double_plant_paeonia_top"),
            bottom: Recipe::Plain("blocks/double_plant_paeonia_top"),
            side: Recipe::Plain("blocks/double_plant_paeonia_top"),
        },
        Block {
            id: 19,
            meta: None,
            top: Recipe::Plain("blocks/sponge"),
            bottom: Recipe::Plain("blocks/sponge"),
            side: Recipe::Plain("blocks/sponge"),
        },
        Block {
            id: 23,
            meta: None,
            top: Recipe::Plain("blocks/furnace_side"),
            bottom: Recipe::Plain("blocks/furnace_side"),
            side: Recipe::Plain("blocks/furnace_side"),
        },
        Block {
            id: 25,
            meta: None,
            top: Recipe::Plain("blocks/noteblock"),
            bottom: Recipe::Plain("blocks/noteblock"),
            side: Recipe::Plain("blocks/noteblock"),
        },
        Block {
            id: 43,
            meta: None,
            top: Recipe::Plain("blocks/stone_slab_top"),
            bottom: Recipe::Plain("blocks/stone_slab_top"),
            side: Recipe::Plain("blocks/stone_slab_top"),
        },
        Block {
            id: 52,
            meta: None,
            top: Recipe::Plain("blocks/mob_spawner"),
            bottom: Recipe::Plain("blocks/mob_spawner"),
            side: Recipe::Plain("blocks/mob_spawner"),
        },
        Block {
            id: 58,
            meta: None,
            top: Recipe::Plain("blocks/crafting_table_top"),
            bottom: Recipe::Plain("blocks/crafting_table_top"),
            side: Recipe::Plain("blocks/crafting_table_top"),
        },
        Block {
            id: 61,
            meta: None,
            top: Recipe::Plain("blocks/furnace_front_off"),
            bottom: Recipe::Plain("blocks/furnace_front_off"),
            side: Recipe::Plain("blocks/furnace_front_off"),
        },
        Block {
            id: 84,
            meta: None,
            top: Recipe::Plain("blocks/jukebox_side"),
            bottom: Recipe::Plain("blocks/jukebox_side"),
            side: Recipe::Plain("blocks/jukebox_side"),
        },
        Block {
            id: 97,
            meta: None,
            top: Recipe::Plain("blocks/stone"),
            bottom: Recipe::Plain("blocks/stone"),
            side: Recipe::Plain("blocks/stone"),
        },
        Block {
            id: 103,
            meta: None,
            top: Recipe::Plain("blocks/melon_side"),
            bottom: Recipe::Plain("blocks/melon_side"),
            side: Recipe::Plain("blocks/melon_side"),
        },
        Block {
            id: 110,
            meta: None,
            top: Recipe::Plain("blocks/mycelium_top"),
            bottom: Recipe::Plain("blocks/mycelium_top"),
            side: Recipe::Plain("blocks/mycelium_top"),
        },
        Block {
            id: 123,
            meta: None,
            top: Recipe::Plain("blocks/redstone_lamp_off"),
            bottom: Recipe::Plain("blocks/redstone_lamp_off"),
            side: Recipe::Plain("blocks/redstone_lamp_off"),
        },
        Block {
            id: 133,
            meta: None,
            top: Recipe::Plain("blocks/emerald_block"),
            bottom: Recipe::Plain("blocks/emerald_block"),
            side: Recipe::Plain("blocks/emerald_block"),
        },
        Block {
            id: 152,
            meta: None,
            top: Recipe::Plain("blocks/redstone_block"),
            bottom: Recipe::Plain("blocks/redstone_block"),
            side: Recipe::Plain("blocks/redstone_block"),
        },
        Block {
            id: 153,
            meta: None,
            top: Recipe::Plain("blocks/quartz_ore"),
            bottom: Recipe::Plain("blocks/quartz_ore"),
            side: Recipe::Plain("blocks/quartz_ore"),
        },
        Block {
            id: 165,
            meta: None,
            top: Recipe::Plain("blocks/slime"),
            bottom: Recipe::Plain("blocks/slime"),
            side: Recipe::Plain("blocks/slime"),
        },
        Block {
            id: 168,
            meta: None,
            top: Recipe::Plain("blocks/prismarine_rough"),
            bottom: Recipe::Plain("blocks/prismarine_rough"),
            side: Recipe::Plain("blocks/prismarine_rough"),
        },
        Block {
            id: 169,
            meta: None,
            top: Recipe::Plain("blocks/sea_lantern"),
            bottom: Recipe::Plain("blocks/sea_lantern"),
            side: Recipe::Plain("blocks/sea_lantern"),
        },
        Block {
            id: 170,
            meta: None,
            top: Recipe::Plain("blocks/hay_block_side"),
            bottom: Recipe::Plain("blocks/hay_block_side"),
            side: Recipe::Plain("blocks/hay_block_side"),
        },
        Block {
            id: 173,
            meta: None,
            top: Recipe::Plain("blocks/coal_block"),
            bottom: Recipe::Plain("blocks/coal_block"),
            side: Recipe::Plain("blocks/coal_block"),
        },
        Block {
            id: 179,
            meta: None,
            top: Recipe::Plain("blocks/red_sandstone_normal"),
            bottom: Recipe::Plain("blocks/red_sandstone_normal"),
            side: Recipe::Plain("blocks/red_sandstone_normal"),
        },
    ]
}

/// Blocks that fill their cube and hide whatever is behind them.
///
/// Stated this way round on purpose. The list of things that *are* a solid cube is short and
/// known; the list of things that are not — every stair, slab, fence, pane, plant, torch, rail,
/// door, carpet and piece of redstone — is long, and anything left off it would wrongly hide the
/// faces of its neighbours. An unknown id erring towards see-through costs a few extra faces; the
/// other way round punches holes in the world.
const OPAQUE: &[u16] = &[
    1, 2, 3, 4, 5, 7, 12, 13, 14, 15, 16, 17, 19, 21, 22, 23, 24, 25, 29, 33, 35, 41, 42, 43, 45,
    46, 47, 48, 49, 52, 56, 57, 58, 60, 61, 62, 73, 74, 79, 80, 82, 84, 86, 87, 88, 89, 91, 97,
    98, 99, 100, 103, 110, 112, 121, 123, 124, 125, 129, 133, 137, 152, 153, 155, 158, 159, 162,
    168, 169, 170, 172, 173, 174, 179, 181, 201, 202, 206, 213, 214, 215, 216,
];

/// Blocks drawn as two crossed planes rather than a cube: everything that grows.
const CROSSED: &[u16] = &[
    6, 31, 32, 37, 38, 39, 40, 50, 59, 83, 104, 105, 115, 141, 142, 175,
];

/// Blocks that hide the faces they share with another of themselves.
///
/// Water is the one that matters: an ocean is thousands of blocks of it, and without this every
/// face inside it is meshed, lit and drawn for nobody. Leaves are deliberately *not* here —
/// hiding the faces between two leaf blocks is what turns a canopy into a checkerboard.
const SELF_HIDING: &[u16] = &[8, 9, 10, 11, 20, 79, 95, 102, 160, 174];

/// Does this block hide the face of whatever is next to it?
#[must_use]
pub fn opaque(block: u16) -> bool {
    OPAQUE.contains(&(block >> 4))
}

/// Does `here` hide the face of `against` that looks at it?
#[must_use]
pub fn covers(here: u16, against: u16) -> bool {
    if opaque(here) {
        return true;
    }
    // Two of the same pane of glass, or two of the same water: one surface, not two.
    here >> 4 == against >> 4 && SELF_HIDING.contains(&(here >> 4))
}

/// Should this block be drawn at all?
#[must_use]
pub fn drawn(block: u16) -> bool {
    !matches!(block >> 4, 0 | 34 | 36)
}

/// What shape to draw it as.
#[must_use]
pub fn shape_of(block: u16) -> Shape {
    if !drawn(block) {
        Shape::Invisible
    } else if CROSSED.contains(&(block >> 4)) {
        Shape::Cross
    } else {
        Shape::Cube
    }
}

/// Every 1.8 biome's temperature and rainfall, which is all the colour maps are indexed by.
///
/// These are the game's own numbers. The mutated variants of a biome — the "M" ones, ids 128
/// higher — share their parent's climate, which is why anything above 128 looks itself up again.
/// Two biomes get colours that are not from the map at all in vanilla: swampland mixes in a
/// noise-driven murk, and roofed forest darkens what the map gives it. Neither is done here, so a
/// swamp comes out ordinary green.
fn climate(biome: u8) -> (f32, f32) {
    match biome {
        1 | 16 => (0.8, 0.4),                 // plains, beach
        2 | 17 | 37 | 38 | 39 => (2.0, 0.0),  // desert and mesa
        3 | 20 | 25 | 34 => (0.2, 0.3),       // extreme hills
        4 | 18 | 29 => (0.7, 0.8),            // forest, roofed forest
        5 | 19 => (0.25, 0.8),                // taiga
        6 => (0.8, 0.9),                      // swampland
        8 => (2.0, 0.0),                      // the nether
        10..=13 => (0.0, 0.5),                // everything frozen
        14 | 15 => (0.9, 1.0),                // mushroom island
        21 | 22 => (0.95, 0.9),               // jungle
        23 => (0.95, 0.8),                    // jungle edge
        26 => (0.05, 0.3),                    // cold beach
        27 | 28 => (0.6, 0.6),                // birch forest
        30 | 31 => (-0.5, 0.4),               // cold taiga
        32 | 33 => (0.3, 0.8),                // mega taiga
        35 => (1.2, 0.0),                     // savanna
        36 => (1.0, 0.0),                     // savanna plateau
        // A mutated biome has its parent's weather.
        other if other >= 128 => climate(other - 128),
        // Ocean, river, and anything this does not know.
        _ => (0.5, 0.5),
    }
}

/// Read one of the game's colour maps into a colour per biome.
///
/// Vanilla's own lookup: clamp the biome's temperature and rainfall to 0..1, multiply the
/// rainfall by the temperature, and read the pixel at `(255 - temperature, 255 - rainfall)`.
fn biome_colours(assets: &dyn AssetSource, tint: Tint) -> [[f32; 3]; 256] {
    let mut out = [tint.fallback(); 256];
    let Some(map) = assets.texture(tint.map()) else {
        return out;
    };
    if map.width < 256 || map.height < 256 {
        return out;
    }
    for (biome, colour) in out.iter_mut().enumerate() {
        let (temperature, rainfall) = climate(biome as u8);
        let temperature = temperature.clamp(0.0, 1.0);
        let rainfall = rainfall.clamp(0.0, 1.0) * temperature;
        let x = ((1.0 - temperature) * 255.0) as u32;
        let y = ((1.0 - rainfall) * 255.0) as u32;
        if let Some(pixel) = map.pixel(x.min(255), y.min(255)) {
            *colour = [
                f32::from(pixel[0]) / 255.0,
                f32::from(pixel[1]) / 255.0,
                f32::from(pixel[2]) / 255.0,
            ];
        }
    }
    out
}

/// The finished atlas, and where everything is in it.
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Left, top, width, height in 0..1, by block id and face.
    rects: HashMap<(u16, Option<u8>, bool, bool), [f32; 4]>,
    /// Which blocks take a biome colour, and which of the two maps they take it from.
    tints: HashMap<(u16, Option<u8>), Tint>,
    /// The tinted cut-out laid over a block's sides: grass, and only grass, in 1.8.
    overlays: HashMap<(u16, Option<u8>), ([f32; 4], Tint)>,
    grass: [[f32; 3]; 256],
    foliage: [[f32; 3]; 256],
    missing: [f32; 4],
}

impl Atlas {
    /// Build it from whatever the asset source can provide.
    #[must_use]
    pub fn build(assets: &dyn AssetSource) -> Self {
        let blocks = blocks();
        // One tile per distinct recipe, plus one for anything unknown. An overlaid face needs
        // two tiles, not one: the plain side and the cut-out that goes over it.
        let mut order: Vec<Recipe> = Vec::new();
        let want = |recipe: Recipe, order: &mut Vec<Recipe>| {
            if !order.contains(&recipe) {
                order.push(recipe);
            }
        };
        for block in &blocks {
            for recipe in [block.top, block.bottom, block.side] {
                match recipe {
                    Recipe::Overlay(base, over, _) => {
                        want(Recipe::Plain(base), &mut order);
                        want(Recipe::Plain(over), &mut order);
                    }
                    Recipe::Plain(name) => want(Recipe::Plain(name), &mut order),
                    Recipe::Tinted(name, tint) => want(Recipe::Tinted(name, tint), &mut order),
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
        let mut tints = HashMap::new();
        let mut overlays = HashMap::new();
        for block in &blocks {
            for (recipe, top, bottom) in [
                (block.top, true, false),
                (block.bottom, false, true),
                (block.side, false, false),
            ] {
                let key = (block.id, block.meta, top, bottom);
                match recipe {
                    Recipe::Plain(_) => {
                        if let Some(r) = at.get(&recipe) {
                            rects.insert(key, *r);
                        }
                    }
                    Recipe::Tinted(_, tint) => {
                        if let Some(r) = at.get(&recipe) {
                            rects.insert(key, *r);
                        }
                        tints.insert((block.id, block.meta), tint);
                    }
                    Recipe::Overlay(base, over, tint) => {
                        if let Some(r) = at.get(&Recipe::Plain(base)) {
                            rects.insert(key, *r);
                        }
                        if let Some(r) = at.get(&Recipe::Plain(over)) {
                            overlays.insert((block.id, block.meta), (*r, tint));
                        }
                    }
                }
            }
        }
        Self {
            width,
            height,
            rgba,
            rects,
            tints,
            overlays,
            grass: biome_colours(assets, Tint::Grass),
            foliage: biome_colours(assets, Tint::Foliage),
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

    /// Map a corner onto an arbitrary tile, for the overlay quads.
    #[must_use]
    pub fn uv_in(tile: [f32; 4], uv: [f32; 2]) -> [f32; 2] {
        let [x, y, w, h] = tile;
        [x + uv[0] * w, y + uv[1] * h]
    }

    /// Which biome colour this block takes, if any.
    #[must_use]
    pub fn tint_of(&self, block: u16) -> Option<Tint> {
        let (id, meta) = (block >> 4, (block & 0xF) as u8);
        self.tints
            .get(&(id, Some(meta)))
            .or_else(|| self.tints.get(&(id, None)))
            .copied()
    }

    /// The tinted cut-out that goes over this block's sides, if it has one.
    #[must_use]
    pub fn overlay_of(&self, block: u16) -> Option<([f32; 4], Tint)> {
        let (id, meta) = (block >> 4, (block & 0xF) as u8);
        self.overlays
            .get(&(id, Some(meta)))
            .or_else(|| self.overlays.get(&(id, None)))
            .copied()
    }

    /// What colour a tint is in a given biome.
    #[must_use]
    pub fn biome_colour(&self, tint: Tint, biome: u8) -> [f32; 3] {
        match tint {
            Tint::Grass => self.grass[biome as usize],
            Tint::Foliage => self.foliage[biome as usize],
        }
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
///
/// The tint is deliberately not applied here. A tile is shared by every copy of that block in
/// the world, and two of them can be in different biomes; the colour belongs on the vertices.
fn bake(assets: &dyn AssetSource, recipe: Recipe) -> Texture {
    let name = match recipe {
        Recipe::Plain(name) | Recipe::Tinted(name, _) | Recipe::Overlay(name, _, _) => name,
    };
    assets
        .texture(name)
        .map_or_else(|| Texture::flat(TILE, TILE, [0xCC, 0x44, 0xCC, 0xFF]), crop)
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

/// How long a block takes to break by hand, in seconds, on a 1.8 server.
///
/// Vanilla works it out from the block's hardness: without a tool that helps, breaking takes
/// `hardness * 5` seconds, and `hardness * 1.5` when the tool is right for the job. These are
/// the bare-hand times, which is what this client has until it has an inventory.
///
/// It matters because the server times the dig itself and throws away a "finished" that arrives
/// too early — which is exactly why sending start and finish together broke nothing at all.
#[must_use]
pub fn break_seconds(block: u16) -> f32 {
    let hardness = match block >> 4 {
        // Instant.
        0 | 31 | 32 | 37 | 38 | 39 | 40 | 59 | 175 => 0.0,
        78 => 0.1,                                     // snow layer
        18 | 161 => 0.2,                               // leaves
        2 | 3 | 12 | 13 | 60 => 0.6,                   // grass, dirt, sand, gravel, farmland
        79 | 174 => 0.5,                               // ice
        17 | 162 | 5 | 47 | 85 => 2.0,                 // wood
        1 | 4 | 48 | 98 | 24 | 45 => 2.0,              // stone and its kin
        14 | 15 | 16 | 21 | 56 | 73 | 74 | 129 => 3.0, // ores
        41 | 42 | 57 | 22 => 5.0,                      // metal and gem blocks
        49 => 50.0,                                    // obsidian
        7 => f32::INFINITY,                            // bedrock
        _ => 1.0,
    };
    if hardness == 0.0 { 0.0 } else { hardness * 5.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat;
    impl AssetSource for Flat {
        fn texture(&self, id: &str) -> Option<Texture> {
            // The colour maps are big; everything else is a tile.
            if id.starts_with("colormap/") {
                let mut map = Texture::flat(256, 256, [0, 0, 0, 255]);
                // A gradient, so two different climates cannot come out the same colour.
                for y in 0..256u32 {
                    for x in 0..256u32 {
                        let at = ((y * 256 + x) * 4) as usize;
                        map.rgba[at] = x as u8;
                        map.rgba[at + 1] = y as u8;
                        map.rgba[at + 2] = 0x40;
                    }
                }
                return Some(map);
            }
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
    fn the_blocks_that_need_a_biome_colour_ask_for_one() {
        let atlas = Atlas::build(&Flat);
        assert_eq!(atlas.tint_of(2 << 4), Some(Tint::Grass), "grass block top");
        assert_eq!(atlas.tint_of(18 << 4), Some(Tint::Foliage), "oak leaves");
        assert_eq!(
            atlas.tint_of((31 << 4) | 1),
            Some(Tint::Grass),
            "tall grass"
        );
        assert_eq!(atlas.tint_of(1 << 4), None, "stone is just stone");
        assert_eq!(atlas.tint_of(5 << 4), None, "so are planks");
    }

    #[test]
    fn a_grass_block_has_a_tinted_fringe_on_its_sides() {
        let atlas = Atlas::build(&Flat);
        let (tile, tint) = atlas.overlay_of(2 << 4).expect("grass side overlay");
        assert_eq!(tint, Tint::Grass);
        assert_ne!(
            tile,
            atlas.rect(2 << 4, Face::North),
            "the overlay is its own tile, not the side itself"
        );
        assert!(atlas.overlay_of(1 << 4).is_none(), "stone has no fringe");
    }

    #[test]
    fn the_colour_map_is_read_and_differs_by_biome() {
        let atlas = Atlas::build(&Flat);
        let plains = atlas.biome_colour(Tint::Grass, 1);
        let desert = atlas.biome_colour(Tint::Grass, 2);
        let taiga = atlas.biome_colour(Tint::Grass, 5);
        assert_ne!(plains, desert, "a desert is not a meadow");
        assert_ne!(plains, taiga);
        // A mutated biome is its parent's weather, so it is its parent's colour.
        assert_eq!(
            atlas.biome_colour(Tint::Grass, 1 + 128),
            plains,
            "sunflower plains is plains"
        );
    }

    #[test]
    fn without_a_colour_map_the_plains_colours_are_used() {
        struct NoMaps;
        impl AssetSource for NoMaps {
            fn texture(&self, id: &str) -> Option<Texture> {
                if id.starts_with("colormap/") {
                    return None;
                }
                Some(Texture::flat(TILE, TILE, [128, 128, 128, 255]))
            }
            fn describe(&self) -> String {
                "test".into()
            }
        }
        let atlas = Atlas::build(&NoMaps);
        assert_eq!(atlas.biome_colour(Tint::Grass, 4), FALLBACK_GRASS);
        assert_eq!(atlas.biome_colour(Tint::Foliage, 4), FALLBACK_FOLIAGE);
    }

    #[test]
    fn plants_are_crosses_and_stone_is_a_cube() {
        assert_eq!(shape_of(1 << 4), Shape::Cube);
        assert_eq!(shape_of(37 << 4), Shape::Cross, "a dandelion");
        assert_eq!(shape_of((31 << 4) | 1), Shape::Cross, "tall grass");
        assert_eq!(shape_of(0), Shape::Invisible, "air");
    }

    #[test]
    fn only_full_cubes_hide_their_neighbours() {
        assert!(opaque(1 << 4), "stone");
        assert!(!opaque(0), "air");
        assert!(!opaque(44 << 4), "a slab fills half its cube");
        assert!(!opaque(85 << 4), "a fence");
        assert!(!opaque(18 << 4), "leaves have holes in them");
        assert!(!opaque(9999 << 4), "something unknown errs towards visible");
    }

    #[test]
    fn water_hides_water_and_leaves_do_not_hide_leaves() {
        let water = 9 << 4;
        let leaves = 18 << 4;
        assert!(covers(water, water), "an ocean is one surface");
        assert!(
            !covers(leaves, leaves),
            "hiding these is what makes a canopy a checkerboard"
        );
        assert!(covers(1 << 4, water), "stone hides anything");
        assert!(!covers(water, 1 << 4), "water does not hide stone");
        // Flowing water and still water are the same surface as far as this is concerned.
        assert!(covers((9 << 4) | 3, (9 << 4) | 1));
    }

    #[test]
    fn breaking_times_are_the_bare_handed_ones() {
        assert_eq!(break_seconds(0), 0.0, "air");
        assert_eq!(break_seconds(2 << 4), 3.0, "a grass block by hand");
        assert!(break_seconds(7 << 4).is_infinite(), "bedrock");
        assert!(break_seconds(49 << 4) > 200.0, "obsidian, pointlessly");
    }
}
