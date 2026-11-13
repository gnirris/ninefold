//! One texture holding every block texture, and where each one sits in it.
//!
//! A draw call can only sample one texture, so every block face has to come out of the same
//! image. The tiles are 16×16 because that is what 1.8 ships, laid out in a square grid.
//!
//! What goes into it is no longer a hand-written list of blocks. The model files say which
//! textures they need and the atlas is built of exactly those, so a texture no model mentions is
//! never packed and a model can never want a texture that is missing from here.
//!
//! The biome colours are deliberately *not* baked into the tiles. One tile serves a grass block
//! in a desert and the same block in a swamp, so the colour goes on the vertices instead, read
//! out of the game's own `colormap/grass.png` and `colormap/foliage.png` by each biome's
//! temperature and rainfall — the same lookup vanilla does.

use std::collections::HashMap;

use ninefold_assets::{AssetSource, Texture};
use ninefold_protocol::item::Slot;

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

/// Water is tinted too, and by one flat colour rather than a map.
const WATER: [f32; 3] = [
    0x3F as f32 / 255.0,
    0x76 as f32 / 255.0,
    0xE4 as f32 / 255.0,
];

/// Tiles are the size 1.8's textures are.
const TILE: u32 = 16;

/// Which colour map a block's tinted faces take.
///
/// The model files mark *that* a face is tinted, with a `tintindex`, but not what colour — in
/// the real game that comes from the block itself. These are the blocks that have one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    Grass = 1,
    Foliage = 2,
    Water = 3,
}

impl Tint {
    /// The number the mesher carries on each vertex.
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// Back from that number.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Tint::Grass),
            2 => Some(Tint::Foliage),
            3 => Some(Tint::Water),
            _ => None,
        }
    }

    const fn map(self) -> Option<&'static str> {
        match self {
            Tint::Grass => Some("colormap/grass"),
            Tint::Foliage => Some("colormap/foliage"),
            // Water's colour is not in a colour map: it is one value per biome in the game's own
            // data, and almost every biome shares it.
            Tint::Water => None,
        }
    }

    const fn fallback(self) -> [f32; 3] {
        match self {
            Tint::Grass => FALLBACK_GRASS,
            Tint::Foliage => FALLBACK_FOLIAGE,
            Tint::Water => WATER,
        }
    }
}

/// Which colour map, if any, this block's tinted faces use.
#[must_use]
pub fn tint_of(block: u16) -> Option<Tint> {
    match block >> 4 {
        // Grass blocks, tall grass, ferns, sugar cane, vines and the double-height plants.
        2 | 31 | 83 | 106 | 175 => Some(Tint::Grass),
        18 | 161 => Some(Tint::Foliage),
        8 | 9 => Some(Tint::Water),
        _ => None,
    }
}

/// How many pictures there are of a block part-way to broken.
pub const CRACK_STAGES: usize = 10;

/// The ten pictures of a block part-way to broken, in order.
const CRACK_TEXTURES: [&str; CRACK_STAGES] = [
    "blocks/destroy_stage_0",
    "blocks/destroy_stage_1",
    "blocks/destroy_stage_2",
    "blocks/destroy_stage_3",
    "blocks/destroy_stage_4",
    "blocks/destroy_stage_5",
    "blocks/destroy_stage_6",
    "blocks/destroy_stage_7",
    "blocks/destroy_stage_8",
    "blocks/destroy_stage_9",
];

/// Blocks that hide the faces they share with another of themselves.
///
/// Water is the one that matters: an ocean is thousands of blocks of it, and without this every
/// face inside it is meshed, lit and drawn for nobody. Leaves are deliberately *not* here —
/// hiding the faces between two leaf blocks is what turns a canopy into a checkerboard.
const SELF_HIDING: &[u16] = &[8, 9, 10, 11, 20, 79, 95, 102, 160, 174];

/// Does `here` hide the face of `against` that looks at it?
///
/// `solid` is whether the block here fills its cube, which now comes from its own model rather
/// than from a list that had to be kept in step with one.
#[must_use]
pub fn covers(here: u16, against: u16, solid: bool) -> bool {
    if solid {
        return true;
    }
    here >> 4 == against >> 4 && SELF_HIDING.contains(&(here >> 4))
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
    // Water has no colour map: its colour is one value per biome in the game's own data.
    let Some(name) = tint.map() else {
        return out;
    };
    let Some(map) = assets.texture(name) else {
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
    /// Left, top, width, height in 0..1, by texture name.
    rects: HashMap<String, [f32; 4]>,
    grass: [[f32; 3]; 256],
    foliage: [[f32; 3]; 256],
    /// The ten stages of a block coming apart.
    cracks: Vec<[f32; 4]>,
    missing: [f32; 4],
}

impl Atlas {
    /// Pack exactly the textures named, plus the breaking animation, plus one loud tile for
    /// anything that is asked for later and is not there.
    #[must_use]
    pub fn build(assets: &dyn AssetSource, wanted: &[String]) -> Self {
        let mut order: Vec<String> = Vec::new();
        for name in wanted {
            if !order.contains(name) {
                order.push(name.clone());
            }
        }
        for name in CRACK_TEXTURES {
            let name = name.to_owned();
            if !order.contains(&name) {
                order.push(name);
            }
        }
        let count = u32::try_from(order.len() + 1).unwrap_or(1);
        let across = f64::from(count).sqrt().ceil() as u32;
        let across = across.max(1);
        let down = count.div_ceil(across).max(1);
        let (width, height) = (across * TILE, down * TILE);
        let mut rgba = vec![0u8; (width * height * 4) as usize];

        let mut rects = HashMap::new();
        for (index, name) in order.iter().enumerate() {
            let index = u32::try_from(index).unwrap_or(0);
            let (tx, ty) = (index % across, index / across);
            // A texture the pack does not have still gets a tile, so the gap is loud rather
            // than invisible.
            let tile = assets
                .texture(name)
                .map_or_else(|| Texture::flat(TILE, TILE, [0xCC, 0x44, 0xCC, 0xFF]), crop);
            blit(&mut rgba, width, tx * TILE, ty * TILE, &tile);
            rects.insert(name.clone(), rect(tx, ty, width, height));
        }
        let last = u32::try_from(order.len()).unwrap_or(0);
        let (mx, my) = (last % across, last / across);
        blit(
            &mut rgba,
            width,
            mx * TILE,
            my * TILE,
            &Texture::flat(TILE, TILE, [0xCC, 0x44, 0xCC, 0xFF]),
        );

        Self {
            width,
            height,
            cracks: CRACK_TEXTURES
                .iter()
                .filter_map(|name| rects.get(*name).copied())
                .collect(),
            rgba,
            grass: biome_colours(assets, Tint::Grass),
            foliage: biome_colours(assets, Tint::Foliage),
            rects,
            missing: rect(mx, my, width, height),
        }
    }

    /// Where a texture is, or the loud tile if it was never packed.
    #[must_use]
    pub fn rect(&self, texture: &str) -> [f32; 4] {
        self.rects.get(texture).copied().unwrap_or(self.missing)
    }

    /// Whether this texture made it in at all.
    #[must_use]
    pub fn has(&self, texture: &str) -> bool {
        self.rects.contains_key(texture)
    }

    /// Map a corner given in texture pixels onto a tile of the atlas.
    #[must_use]
    pub fn uv_in(tile: [f32; 4], pixels: [f32; 2]) -> [f32; 2] {
        [
            tile[0] + pixels[0] / 16.0 * tile[2],
            tile[1] + pixels[1] / 16.0 * tile[3],
        ]
    }

    /// Where stage `n` of the breaking animation samples from.
    #[must_use]
    pub fn crack(&self, stage: usize) -> Option<[f32; 4]> {
        self.cracks.get(stage).copied()
    }

    /// What colour a tint is in a given biome.
    #[must_use]
    pub fn biome_colour(&self, tint: Tint, biome: u8) -> [f32; 3] {
        match tint {
            Tint::Grass => self.grass[biome as usize],
            Tint::Foliage => self.foliage[biome as usize],
            Tint::Water => WATER,
        }
    }

    /// How many textures are in it.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rects.len()
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

/// What a tool is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Pickaxe,
    Axe,
    Shovel,
    Shears,
    Sword,
}

/// What is in the player's hand, as far as breaking a block is concerned.
///
/// The tier is 1.8's harvest level: wood and gold 0, stone 1, iron 2, diamond 3. Gold is the
/// fastest tool in the game and the worst at actually harvesting anything, which is why speed
/// and tier are separate numbers.
#[must_use]
pub fn tool_of(item: Option<Slot>) -> Option<(Tool, u8, f32)> {
    let id = item?.id;
    let kind = match id {
        256 | 269 | 273 | 277 | 284 => Tool::Shovel,
        257 | 270 | 274 | 278 | 285 => Tool::Pickaxe,
        258 | 271 | 275 | 279 | 286 => Tool::Axe,
        359 => Tool::Shears,
        267 | 268 | 272 | 276 | 283 => Tool::Sword,
        _ => return None,
    };
    // Material, by the id's place in each tool's run: wood, stone, iron, diamond, gold.
    let (tier, speed) = match id {
        268..=271 => (0, 2.0),   // wood
        272..=275 => (1, 4.0),   // stone
        256..=258 | 267 => (2, 6.0), // iron
        276..=279 => (3, 8.0),   // diamond
        283..=286 => (0, 12.0),  // gold: quick, and harvests almost nothing
        359 => (0, 5.0),         // shears
        _ => (0, 1.0),
    };
    Some((kind, tier, speed))
}

/// Which tool speeds a block up, and what tier is needed to get anything out of it.
///
/// `None` means bare hands are enough — which is most of the world. The second number only
/// matters where a tool is required: mining stone with a wooden pickaxe works, mining diamond
/// ore with one gives you nothing and takes just as long.
fn block_tool(block: u16) -> (Option<Tool>, Option<u8>) {
    // Most specific first: a match takes the first arm that fits, and "any pickaxe" would
    // otherwise swallow the ores that need a better one.
    match block >> 4 {
        // Obsidian, and nothing else, wants diamond.
        49 => (Some(Tool::Pickaxe), Some(3)),
        // Gold, diamond, redstone and emerald want iron.
        14 | 41 | 56 | 57 | 73 | 74 | 129 | 133 => (Some(Tool::Pickaxe), Some(2)),
        // Iron and lapis want stone.
        15 | 21 | 22 | 42 => (Some(Tool::Pickaxe), Some(1)),
        // Everything else mineral comes out with any pickaxe at all.
        1 | 4 | 16 | 24 | 43..=45 | 48 | 52 | 67 | 87 | 98 | 108 | 109 | 112 | 114 | 121 | 128
        | 139 | 152 | 153 | 155 | 156 | 168 | 173 | 179 | 180 => (Some(Tool::Pickaxe), Some(0)),
        // Wood: an axe helps, hands still work.
        5 | 17 | 47 | 53 | 54 | 58 | 63..=65 | 72 | 84 | 85 | 96 | 107 | 125 | 126 | 134..=136
        | 146 | 162..=164 => (Some(Tool::Axe), None),
        // Dirt: a shovel helps, hands still work.
        2 | 3 | 12 | 13 | 60 | 78 | 79 | 80 | 82 | 88 | 110 => (Some(Tool::Shovel), None),
        // Wool and leaves come apart fastest with shears.
        18 | 30 | 35 | 161 => (Some(Tool::Shears), None),
        _ => (None, None),
    }
}

/// A block's hardness, as 1.8 lists it. Negative means it cannot be broken at all.
fn hardness(block: u16) -> f32 {
    match block >> 4 {
        0 | 31 | 32 | 37 | 38 | 39 | 40 | 59 | 104 | 105 | 141 | 142 | 175 | 6 => 0.0,
        30 => 4.0,                       // cobweb
        78 => 0.1,                       // snow layer
        18 | 161 => 0.2,                 // leaves
        50 | 76 | 55 | 63 | 68 => 0.0,   // torches, redstone, signs
        35 | 171 => 0.8,                 // wool and carpet
        2 | 110 => 0.6,                  // grass and mycelium
        3 | 12 | 13 | 60 | 80 | 82 => 0.5, // dirt, sand, gravel, clay
        79 | 174 => 0.5,                 // ice
        5 | 47 | 85 | 107 | 53 | 134..=136 | 163 | 164 => 2.0, // planks and what is made of them
        17 | 162 => 2.0,                 // logs
        1 | 4 | 48 | 98 | 24 | 45 | 43 | 44 | 67 | 139 | 155 | 168 | 179 => 2.0,
        14 | 15 | 16 | 21 | 56 | 73 | 74 | 129 | 153 => 3.0, // ores
        41 | 42 | 57 | 22 | 133 | 152 => 5.0,                // metal and gem blocks
        87 => 0.4,                       // netherrack
        88 => 0.5,                       // soul sand
        89 => 0.3,                       // glowstone
        20 | 95 | 102 => 0.3,            // glass
        49 => 50.0,                      // obsidian
        7 => -1.0,                       // bedrock
        _ => 1.0,
    }
}

/// How long this block takes to break, in seconds, with this in hand.
///
/// 1.8's own arithmetic, which is not "hardness times five": a block you can harvest takes
/// `hardness * 30 / speed` ticks, and one you cannot takes `hardness * 100 / speed`. Bare hands
/// are speed 1. Using the wrong rule makes dirt take three seconds instead of nine tenths of
/// one, which does not read as a slow client — it reads as breaking being broken.
#[must_use]
pub fn break_seconds(block: u16, held: Option<Slot>, on_ground: bool) -> f32 {
    let hardness = hardness(block);
    if hardness < 0.0 {
        return f32::INFINITY;
    }
    if hardness == 0.0 {
        return 0.0;
    }
    let (wants, needs_tier) = block_tool(block);
    let holding = tool_of(held);
    let mut speed = match (wants, holding) {
        (Some(wants), Some((kind, _, speed))) if kind == wants => speed,
        _ => 1.0,
    };
    // Swinging in mid-air is five times slower, which is why you land before you mine.
    if !on_ground {
        speed /= 5.0;
    }
    let harvests = match needs_tier {
        None => true,
        Some(needed) => matches!(
            (wants, holding),
            (Some(wants), Some((kind, tier, _))) if kind == wants && tier >= needed
        ),
    };
    let ticks = hardness * if harvests { 30.0 } else { 100.0 } / speed;
    ticks / 20.0
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat;
    impl AssetSource for Flat {
        fn texture(&self, id: &str) -> Option<Texture> {
            if id.starts_with("colormap/") {
                // A gradient, so two different climates cannot come out the same colour.
                let mut map = Texture::flat(256, 256, [0, 0, 0, 255]);
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
            if id == "blocks/nothing" {
                return None;
            }
            Some(Texture::flat(TILE, TILE, [80, 80, 80, 255]))
        }
        fn describe(&self) -> String {
            "test".into()
        }
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn exactly_the_textures_asked_for_are_packed() {
        let atlas = Atlas::build(&Flat, &names(&["blocks/stone", "blocks/dirt"]));
        assert!(atlas.has("blocks/stone"));
        assert!(atlas.has("blocks/dirt"));
        assert!(!atlas.has("blocks/planks_oak"), "nobody asked for planks");
        // The breaking animation comes along regardless, because no model asks for it.
        assert!(atlas.has("blocks/destroy_stage_0"));
        assert!(atlas.crack(9).is_some());
        assert!(atlas.crack(10).is_none());
    }

    #[test]
    fn two_textures_do_not_share_a_tile() {
        let atlas = Atlas::build(&Flat, &names(&["blocks/stone", "blocks/dirt"]));
        assert_ne!(atlas.rect("blocks/stone"), atlas.rect("blocks/dirt"));
    }

    #[test]
    fn a_texture_that_was_never_packed_gets_the_loud_tile() {
        let atlas = Atlas::build(&Flat, &names(&["blocks/stone"]));
        assert_eq!(atlas.rect("blocks/whatever"), atlas.missing);
        assert_ne!(atlas.rect("blocks/stone"), atlas.missing);
    }

    #[test]
    fn a_name_asked_for_twice_is_packed_once() {
        let atlas = Atlas::build(&Flat, &names(&["blocks/stone", "blocks/stone"]));
        assert_eq!(atlas.len(), 1 + CRACK_STAGES);
    }

    #[test]
    fn a_missing_file_still_gets_a_tile_so_the_gap_is_visible() {
        let atlas = Atlas::build(&Flat, &names(&["blocks/nothing"]));
        assert!(atlas.has("blocks/nothing"));
        let [x, y, ..] = atlas.rect("blocks/nothing");
        let px = (y * atlas.height as f32) as u32 * atlas.width + (x * atlas.width as f32) as u32;
        let at = (px * 4) as usize;
        assert_eq!(atlas.rgba[at], 0xCC, "loud magenta, not nothing");
    }

    #[test]
    fn texture_pixels_map_onto_the_tile_they_belong_to() {
        let atlas = Atlas::build(&Flat, &names(&["blocks/stone"]));
        let tile = atlas.rect("blocks/stone");
        assert_eq!(Atlas::uv_in(tile, [0.0, 0.0]), [tile[0], tile[1]]);
        assert_eq!(
            Atlas::uv_in(tile, [16.0, 16.0]),
            [tile[0] + tile[2], tile[1] + tile[3]]
        );
        let middle = Atlas::uv_in(tile, [8.0, 8.0]);
        assert!((middle[0] - (tile[0] + tile[2] / 2.0)).abs() < 1e-6);
    }

    #[test]
    fn the_blocks_that_need_a_biome_colour_ask_for_one() {
        assert_eq!(tint_of(2 << 4), Some(Tint::Grass), "a grass block");
        assert_eq!(tint_of(18 << 4), Some(Tint::Foliage), "oak leaves");
        assert_eq!(tint_of(9 << 4), Some(Tint::Water));
        assert_eq!(tint_of(1 << 4), None, "stone is just stone");
    }

    #[test]
    fn a_tint_survives_the_trip_through_a_vertex() {
        for tint in [Tint::Grass, Tint::Foliage, Tint::Water] {
            assert_eq!(Tint::from_code(tint.code()), Some(tint));
        }
        assert_eq!(Tint::from_code(0), None, "no colouring");
    }

    #[test]
    fn the_colour_map_is_read_and_differs_by_biome() {
        let atlas = Atlas::build(&Flat, &names(&["blocks/stone"]));
        let plains = atlas.biome_colour(Tint::Grass, 1);
        let desert = atlas.biome_colour(Tint::Grass, 2);
        assert_ne!(plains, desert, "a desert is not a meadow");
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
                (!id.starts_with("colormap/"))
                    .then(|| Texture::flat(TILE, TILE, [128, 128, 128, 255]))
            }
            fn describe(&self) -> String {
                "test".into()
            }
        }
        let atlas = Atlas::build(&NoMaps, &names(&["blocks/stone"]));
        assert_eq!(atlas.biome_colour(Tint::Grass, 4), FALLBACK_GRASS);
        assert_eq!(atlas.biome_colour(Tint::Foliage, 4), FALLBACK_FOLIAGE);
    }

    #[test]
    fn water_hides_water_and_leaves_do_not_hide_leaves() {
        let water = 9 << 4;
        let leaves = 18 << 4;
        assert!(covers(water, water, false), "an ocean is one surface");
        assert!(
            !covers(leaves, leaves, false),
            "hiding these is what makes a canopy a checkerboard"
        );
        assert!(covers(1 << 4, water, true), "a solid block hides anything");
        assert!(!covers(water, 1 << 4, false), "water does not hide stone");
        // Flowing water and still water are the same surface as far as this is concerned.
        assert!(covers((9 << 4) | 3, (9 << 4) | 1, false));
    }

    #[test]
    fn breaking_times_match_the_numbers_the_game_is_known_for() {
        let hand = |block| break_seconds(block, None, true);
        assert_eq!(hand(0), 0.0, "air");
        assert!(hand(7 << 4).is_infinite(), "bedrock");
        assert_eq!(hand(37 << 4), 0.0, "a flower comes out instantly");
        assert!((hand(3 << 4) - 0.75).abs() < 0.01, "dirt by hand");
        assert!((hand(1 << 4) - 10.0).abs() < 0.1, "stone by hand");
    }

    #[test]
    fn the_right_tool_is_faster_and_the_wrong_one_is_not() {
        let with = |block, item| break_seconds(block, Some(Slot::new(item, 1)), true);
        let stone = 1 << 4;
        assert!(with(stone, 270) < 2.0, "a wooden pickaxe on stone");
        assert!(with(stone, 278) < with(stone, 270), "diamond beats wood");
        assert!(
            (with(stone, 277) - break_seconds(stone, None, true)).abs() < 0.01,
            "a shovel is no help on stone"
        );
        assert!(
            with(56 << 4, 285) > with(56 << 4, 257),
            "gold is quick and harvests nothing"
        );
    }

    #[test]
    fn swinging_in_mid_air_is_five_times_slower() {
        let ground = break_seconds(3 << 4, None, true);
        let air = break_seconds(3 << 4, None, false);
        assert!((air / ground - 5.0).abs() < 0.01);
    }
}
