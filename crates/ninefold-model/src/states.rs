//! Which block state a block id and its four metadata bits mean.
//!
//! This is the one part the jar cannot answer. 1.8's files are keyed by name and property —
//! `granite`, `facing=east,half=bottom,shape=straight` — while the wire speaks in `id << 4 |
//! metadata`, and the mapping between them lived in the Java code rather than in the assets. So
//! it is written out here: block by block, which blockstate file and which variant key.
//!
//! Everything downstream of this comes out of the player's own copy. Get the name right and the
//! geometry, the textures and the rotation are the game's own.
//!
//! Two things are deliberately not here yet and are noted where they bite: a stair's
//! `shape` (whether it turns a corner) and a vine's sides both depend on more of the
//! surroundings than the four horizontal neighbours this is given.

/// Which neighbours a connecting block is joined to — a fence, a pane, a wall, a set of bars.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Links {
    pub north: bool,
    pub east: bool,
    pub south: bool,
    pub west: bool,
}

impl Links {
    /// The variant key 1.8 writes these under, which is alphabetical rather than clockwise.
    #[must_use]
    pub fn key(self) -> String {
        format!(
            "east={},north={},south={},west={}",
            self.east, self.north, self.south, self.west
        )
    }

    /// Pack into four bits, so a shape cache can be keyed on it.
    #[must_use]
    pub const fn bits(self) -> u8 {
        (self.north as u8) | (self.east as u8) << 1 | (self.south as u8) << 2 | (self.west as u8) << 3
    }

    /// Unpack again.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self {
            north: bits & 1 != 0,
            east: bits & 2 != 0,
            south: bits & 4 != 0,
            west: bits & 8 != 0,
        }
    }
}

/// The sixteen dye colours, in the order their metadata runs.
const COLOURS: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "silver",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// The six woods, likewise. Acacia and dark oak are ids of their own, but their metadata
/// continues the same order.
const WOODS: [&str; 6] = ["oak", "spruce", "birch", "jungle", "acacia", "dark_oak"];

/// What the eight kinds of stone slab are called.
const SLABS: [&str; 8] = [
    "stone",
    "sandstone",
    "wood_old",
    "cobblestone",
    "brick",
    "stone_brick",
    "nether_brick",
    "quartz",
];

/// Facing, as most blocks store it: 2 is north and it goes round from there.
fn facing_nswe(meta: u8) -> &'static str {
    match meta & 7 {
        2 => "north",
        3 => "south",
        4 => "west",
        _ => "east",
    }
}

/// Facing, as stairs store it.
fn facing_stairs(meta: u8) -> &'static str {
    match meta & 3 {
        0 => "east",
        1 => "west",
        2 => "south",
        _ => "north",
    }
}

/// Which blockstate file and variant a block is, given what it is joined to.
///
/// `None` means nothing to draw — air, or something drawn another way entirely, like a liquid
/// or a sign.
#[must_use]
pub fn state_of(block: u16, links: Links) -> Option<(String, String)> {
    let id = block >> 4;
    let meta = (block & 0xF) as u8;
    let normal = |name: &str| Some((name.to_owned(), "normal".to_owned()));
    let variant = |name: &str, key: String| Some((name.to_owned(), key));
    let wood = |suffix: &str| WOODS.get((meta & 7) as usize).map(|w| format!("{w}_{suffix}"));

    match id {
        // Liquids are not in the blockstates at all: the real game draws them with a renderer of
        // its own, and so does this one.
        0 | 8..=11 => None,
        1 => normal(match meta {
            1 => "granite",
            2 => "smooth_granite",
            3 => "diorite",
            4 => "smooth_diorite",
            5 => "andesite",
            6 => "smooth_andesite",
            _ => "stone",
        }),
        2 => variant("grass", "snowy=false".to_owned()),
        3 => normal(match meta {
            1 => "coarse_dirt",
            2 => "podzol",
            _ => "dirt",
        }),
        4 => normal("cobblestone"),
        5 => normal(&wood("planks")?),
        6 => variant(&wood("sapling")?, "stage=0".to_owned()),
        7 => normal("bedrock"),
        12 => normal(if meta == 1 { "red_sand" } else { "sand" }),
        13 => normal("gravel"),
        14 => normal("gold_ore"),
        15 => normal("iron_ore"),
        16 => normal("coal_ore"),
        // A log's metadata carries both which wood and which way it lies.
        17 => {
            let axis = match meta >> 2 {
                1 => "x",
                2 => "z",
                3 => "none",
                _ => "y",
            };
            variant(
                &format!("{}_log", WOODS.get((meta & 3) as usize)?),
                format!("axis={axis}"),
            )
        }
        18 => normal(&format!("{}_leaves", WOODS.get((meta & 3) as usize)?)),
        19 => normal("sponge"),
        20 => normal("glass"),
        21 => normal("lapis_ore"),
        22 => normal("lapis_block"),
        23 => variant("dispenser", format!("facing={}", facing_nswe(meta))),
        24 => normal(match meta {
            1 => "chiseled_sandstone",
            2 => "smooth_sandstone",
            _ => "sandstone",
        }),
        25 => normal("noteblock"),
        27 => normal("golden_rail"),
        28 => normal("detector_rail"),
        29 => variant("sticky_piston", format!("facing={}", facing_nswe(meta))),
        30 => normal("web"),
        31 => normal(match meta {
            1 => "tall_grass",
            2 => "fern",
            _ => "dead_bush",
        }),
        32 => normal("dead_bush"),
        33 => variant("piston", format!("facing={}", facing_nswe(meta))),
        35 => normal(&format!("{}_wool", COLOURS.get(meta as usize)?)),
        37 => normal("dandelion"),
        38 => normal(match meta {
            1 => "blue_orchid",
            2 => "allium",
            3 => "houstonia",
            4 => "red_tulip",
            5 => "orange_tulip",
            6 => "white_tulip",
            7 => "pink_tulip",
            8 => "oxeye_daisy",
            _ => "poppy",
        }),
        39 => normal("brown_mushroom"),
        40 => normal("red_mushroom"),
        41 => normal("gold_block"),
        42 => normal("iron_block"),
        43 => normal(&format!("{}_double_slab", SLABS.get((meta & 7) as usize)?)),
        44 => variant(
            &format!("{}_slab", SLABS.get((meta & 7) as usize)?),
            format!("half={}", if meta & 8 != 0 { "top" } else { "bottom" }),
        ),
        45 => normal("brick_block"),
        46 => normal("tnt"),
        47 => normal("bookshelf"),
        48 => normal("mossy_cobblestone"),
        49 => normal("obsidian"),
        50 | 75 | 76 => {
            let name = match id {
                75 => "unlit_redstone_torch",
                76 => "redstone_torch",
                _ => "torch",
            };
            let facing = match meta & 7 {
                1 => "east",
                2 => "west",
                3 => "south",
                4 => "north",
                _ => "up",
            };
            variant(name, format!("facing={facing}"))
        }
        51 => normal("fire"),
        52 => normal("mob_spawner"),
        56 => normal("diamond_ore"),
        57 => normal("diamond_block"),
        58 => normal("crafting_table"),
        59 => variant("wheat", format!("age={}", meta & 7)),
        60 => variant("farmland", format!("moisture={}", meta & 7)),
        61 => variant("furnace", format!("facing={}", facing_nswe(meta))),
        62 => variant("lit_furnace", format!("facing={}", facing_nswe(meta))),
        65 => variant("ladder", format!("facing={}", facing_nswe(meta))),
        66 => normal("rail"),
        69 => normal("lever"),
        70 => variant(
            "stone_pressure_plate",
            format!("powered={}", meta & 1 != 0),
        ),
        72 => variant(
            "wooden_pressure_plate",
            format!("powered={}", meta & 1 != 0),
        ),
        73 => normal("redstone_ore"),
        74 => normal("lit_redstone_ore"),
        77 => normal("stone_button"),
        // A snow layer is one to eight eighths of a block tall, which is why walking onto one
        // works and walking into a cube of snow does not.
        78 => variant("snow_layer", format!("layers={}", (meta & 7) + 1)),
        79 => normal("ice"),
        80 => normal("snow"),
        81 => normal("cactus"),
        82 => normal("clay"),
        83 => normal("reeds"),
        84 => normal("jukebox"),
        86 => variant("pumpkin", format!("facing={}", facing_nswe(meta))),
        87 => normal("netherrack"),
        88 => normal("soul_sand"),
        89 => normal("glowstone"),
        91 => variant("lit_pumpkin", format!("facing={}", facing_nswe(meta))),
        92 => variant("cake", format!("bites={}", meta & 7)),
        93 => variant(
            "unpowered_repeater",
            format!("delay={},facing={}", (meta >> 2) + 1, facing_nswe(meta + 2)),
        ),
        94 => variant(
            "powered_repeater",
            format!("delay={},facing={}", (meta >> 2) + 1, facing_nswe(meta + 2)),
        ),
        95 => normal(&format!("{}_stained_glass", COLOURS.get(meta as usize)?)),
        96 => normal("trapdoor"),
        98 => normal(match meta {
            1 => "mossy_stonebrick",
            2 => "cracked_stonebrick",
            3 => "chiseled_stonebrick",
            _ => "stonebrick",
        }),
        99 => normal("brown_mushroom_block"),
        100 => normal("red_mushroom_block"),
        101 => variant("iron_bars", links.key()),
        102 => variant("glass_pane", links.key()),
        103 => normal("melon_block"),
        104 => variant("pumpkin_stem", format!("age={}", meta & 7)),
        105 => variant("melon_stem", format!("age={}", meta & 7)),
        // A vine's faces come from its metadata, which this does not carry through yet: it is
        // drawn on the side it is attached to, and only that.
        106 => normal("vine"),
        110 => variant("mycelium", "snowy=false".to_owned()),
        111 => normal("waterlily"),
        112 => normal("nether_brick"),
        113 => variant("nether_brick_fence", links.key()),
        115 => variant("nether_wart", format!("age={}", meta & 3)),
        116 => normal("enchanting_table"),
        117 => normal("brewing_stand"),
        118 => variant("cauldron", format!("level={}", meta & 3)),
        120 => normal("end_portal_frame"),
        121 => normal("end_stone"),
        122 => normal("dragon_egg"),
        123 => normal("redstone_lamp"),
        124 => normal("lit_redstone_lamp"),
        125 => normal(&format!(
            "{}_double_slab",
            WOODS.get((meta & 7) as usize).copied().unwrap_or("oak")
        )),
        126 => variant(
            &format!(
                "{}_slab",
                WOODS.get((meta & 7) as usize).copied().unwrap_or("oak")
            ),
            format!("half={}", if meta & 8 != 0 { "top" } else { "bottom" }),
        ),
        129 => normal("emerald_ore"),
        133 => normal("emerald_block"),
        137 => normal("command_block"),
        138 => normal("beacon"),
        139 => variant(
            if meta == 1 {
                "mossy_cobblestone_wall"
            } else {
                "cobblestone_wall"
            },
            format!("{},up=true", links.key()),
        ),
        140 => normal("flower_pot"),
        141 => variant("carrots", format!("age={}", meta & 7)),
        142 => variant("potatoes", format!("age={}", meta & 7)),
        143 => normal("wooden_button"),
        145 => variant("anvil", format!("damage={}", meta >> 2)),
        147 => variant(
            "light_weighted_pressure_plate",
            format!("power={}", meta & 15),
        ),
        148 => variant(
            "heavy_weighted_pressure_plate",
            format!("power={}", meta & 15),
        ),
        151 => normal("daylight_detector"),
        152 => normal("redstone_block"),
        153 => normal("quartz_ore"),
        154 => variant("hopper", format!("facing={}", facing_nswe(meta))),
        155 => match meta {
            1 => normal("chiseled_quartz_block"),
            2 => variant("quartz_column", "axis=y".to_owned()),
            3 => variant("quartz_column", "axis=x".to_owned()),
            4 => variant("quartz_column", "axis=z".to_owned()),
            _ => normal("quartz_block"),
        },
        157 => normal("activator_rail"),
        158 => variant("dropper", format!("facing={}", facing_nswe(meta))),
        159 => normal(&format!(
            "{}_stained_hardened_clay",
            COLOURS.get(meta as usize)?
        )),
        160 => variant(
            &format!("{}_stained_glass_pane", COLOURS.get(meta as usize)?),
            links.key(),
        ),
        161 => normal(&format!(
            "{}_leaves",
            if meta & 1 == 1 { "dark_oak" } else { "acacia" }
        )),
        162 => {
            let axis = match meta >> 2 {
                1 => "x",
                2 => "z",
                3 => "none",
                _ => "y",
            };
            variant(
                if meta & 1 == 1 {
                    "dark_oak_log"
                } else {
                    "acacia_log"
                },
                format!("axis={axis}"),
            )
        }
        165 => normal("slime"),
        167 => normal("iron_trapdoor"),
        168 => normal(match meta {
            1 => "prismarine_bricks",
            2 => "dark_prismarine",
            _ => "prismarine",
        }),
        169 => normal("sea_lantern"),
        170 => variant("hay_block", "axis=y".to_owned()),
        171 => normal(&format!("{}_carpet", COLOURS.get(meta as usize)?)),
        172 => normal("hardened_clay"),
        173 => normal("coal_block"),
        174 => normal("packed_ice"),
        175 => variant(
            match meta & 7 {
                1 => "syringa",
                2 => "double_grass",
                3 => "double_fern",
                4 => "double_rose",
                5 => "paeonia",
                _ => "sunflower",
            },
            "half=lower".to_owned(),
        ),
        178 => normal("daylight_detector_inverted"),
        179 => normal(match meta {
            1 => "chiseled_red_sandstone",
            2 => "smooth_red_sandstone",
            _ => "red_sandstone",
        }),
        181 => normal("red_sandstone_double_slab"),
        182 => variant(
            "red_sandstone_slab",
            format!("half={}", if meta & 8 != 0 { "top" } else { "bottom" }),
        ),
        // Stairs. The `shape` property says whether a stair turns a corner, which depends on its
        // neighbours' facings rather than on anything here, so every stair is straight for now —
        // visibly wrong only where two stairs meet.
        53 | 67 | 108 | 109 | 114 | 128 | 134 | 135 | 136 | 156 | 163 | 164 | 180 => {
            let name = match id {
                53 => "oak_stairs",
                67 => "stone_stairs",
                108 => "brick_stairs",
                109 => "stone_brick_stairs",
                114 => "nether_brick_stairs",
                128 => "sandstone_stairs",
                134 => "spruce_stairs",
                135 => "birch_stairs",
                136 => "jungle_stairs",
                156 => "quartz_stairs",
                163 => "acacia_stairs",
                164 => "dark_oak_stairs",
                _ => "red_sandstone_stairs",
            };
            variant(
                name,
                format!(
                    "facing={},half={},shape=straight",
                    facing_stairs(meta),
                    if meta & 4 != 0 { "top" } else { "bottom" }
                ),
            )
        }
        // Fences and gates, by wood.
        85 | 188..=192 => {
            let wood = match id {
                188 => "spruce",
                189 => "birch",
                190 => "jungle",
                191 => "dark_oak",
                192 => "acacia",
                _ => "",
            };
            let name = if wood.is_empty() {
                "fence".to_owned()
            } else {
                format!("{wood}_fence")
            };
            variant(&name, links.key())
        }
        107 | 183..=187 => {
            let wood = match id {
                183 => "spruce",
                184 => "birch",
                185 => "jungle",
                186 => "dark_oak",
                187 => "acacia",
                _ => "",
            };
            let name = if wood.is_empty() {
                "fence_gate".to_owned()
            } else {
                format!("{wood}_fence_gate")
            };
            let facing = match meta & 3 {
                0 => "south",
                1 => "west",
                2 => "north",
                _ => "east",
            };
            variant(
                &name,
                format!("facing={facing},in_wall=false,open={}", meta & 4 != 0),
            )
        }
        // Doors. The lower half carries the facing and whether it is open; the upper half
        // carries which side the hinge is on, and neither half knows the other's bits — so a
        // door drawn from one half alone is a door with a guess in it.
        64 | 71 | 193..=197 => {
            let name = match id {
                71 => "iron_door",
                193 => "spruce_door",
                194 => "birch_door",
                195 => "jungle_door",
                196 => "acacia_door",
                197 => "dark_oak_door",
                _ => "wooden_door",
            };
            let upper = meta & 8 != 0;
            let facing = match meta & 3 {
                0 => "east",
                1 => "south",
                2 => "west",
                _ => "north",
            };
            variant(
                name,
                format!(
                    "facing={facing},half={},hinge=left,open={}",
                    if upper { "upper" } else { "lower" },
                    meta & 4 != 0
                ),
            )
        }
        // Everything else: no model here, and the caller falls back to a plain cube so that an
        // unknown block is visible rather than missing.
        _ => None,
    }
}

/// Does this block join up with fences, panes and walls next to it?
#[must_use]
pub fn connects(block: u16) -> bool {
    matches!(
        block >> 4,
        85 | 101 | 102 | 113 | 139 | 160 | 188..=192 | 107 | 183..=187
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(id: u16, meta: u8) -> Option<(String, String)> {
        state_of((id << 4) | u16::from(meta), Links::default())
    }

    #[test]
    fn the_plain_blocks_are_themselves() {
        assert_eq!(
            state(1, 0),
            Some(("stone".to_owned(), "normal".to_owned()))
        );
        assert_eq!(
            state(4, 0),
            Some(("cobblestone".to_owned(), "normal".to_owned()))
        );
        assert_eq!(state(0, 0), None, "air is nothing");
    }

    #[test]
    fn metadata_picks_the_variant_blocks_apart() {
        assert_eq!(state(1, 1).unwrap().0, "granite");
        assert_eq!(state(1, 5).unwrap().0, "andesite");
        assert_eq!(state(5, 2).unwrap().0, "birch_planks");
        assert_eq!(state(35, 14).unwrap().0, "red_wool");
        assert_eq!(state(35, 0).unwrap().0, "white_wool");
        assert_eq!(state(98, 1).unwrap().0, "mossy_stonebrick");
        assert_eq!(state(38, 4).unwrap().0, "red_tulip");
    }

    #[test]
    fn a_log_carries_both_its_wood_and_which_way_it_lies() {
        assert_eq!(
            state(17, 0),
            Some(("oak_log".to_owned(), "axis=y".to_owned()))
        );
        // Metadata 1 is spruce; adding four lays it along x.
        assert_eq!(
            state(17, 1 | 4),
            Some(("spruce_log".to_owned(), "axis=x".to_owned()))
        );
        assert_eq!(state(17, 12).unwrap().1, "axis=none", "a bark-all-over log");
    }

    #[test]
    fn a_slab_knows_which_half_of_its_block_it_is() {
        assert_eq!(
            state(44, 0),
            Some(("stone_slab".to_owned(), "half=bottom".to_owned()))
        );
        assert_eq!(state(44, 8).unwrap().1, "half=top");
        assert_eq!(state(44, 3).unwrap().0, "cobblestone_slab");
        assert_eq!(state(43, 0).unwrap().0, "stone_double_slab");
        assert_eq!(state(126, 1).unwrap().0, "spruce_slab");
    }

    #[test]
    fn stairs_face_the_way_their_metadata_says() {
        let (name, key) = state(53, 0).unwrap();
        assert_eq!(name, "oak_stairs");
        assert_eq!(key, "facing=east,half=bottom,shape=straight");
        assert!(state(53, 1).unwrap().1.contains("facing=west"));
        assert!(state(53, 2).unwrap().1.contains("facing=south"));
        assert!(state(53, 3).unwrap().1.contains("facing=north"));
        // The top four metadata values are the same stairs upside down.
        assert!(state(53, 4).unwrap().1.contains("half=top"));
        assert_eq!(state(67, 0).unwrap().0, "stone_stairs");
    }

    #[test]
    fn a_snow_layer_is_as_deep_as_its_metadata() {
        assert_eq!(state(78, 0).unwrap().1, "layers=1");
        assert_eq!(state(78, 7).unwrap().1, "layers=8");
    }

    #[test]
    fn a_torch_knows_which_wall_it_is_on() {
        assert_eq!(state(50, 5).unwrap().1, "facing=up");
        assert_eq!(state(50, 1).unwrap().1, "facing=east");
        assert_eq!(state(50, 4).unwrap().1, "facing=north");
        assert_eq!(state(76, 5).unwrap().0, "redstone_torch");
    }

    #[test]
    fn a_fence_takes_its_variant_from_what_it_is_joined_to() {
        let alone = state_of(85 << 4, Links::default()).unwrap();
        assert_eq!(alone.1, "east=false,north=false,south=false,west=false");
        let joined = state_of(
            85 << 4,
            Links {
                north: true,
                south: true,
                ..Links::default()
            },
        )
        .unwrap();
        assert_eq!(joined.1, "east=false,north=true,south=true,west=false");
        assert_eq!(joined.0, "fence");
        assert_eq!(state(188, 0).unwrap().0, "spruce_fence");
    }

    #[test]
    fn links_survive_being_packed_into_four_bits() {
        for bits in 0..16u8 {
            assert_eq!(Links::from_bits(bits).bits(), bits);
        }
        let links = Links {
            north: true,
            west: true,
            ..Links::default()
        };
        assert_eq!(Links::from_bits(links.bits()), links);
    }

    #[test]
    fn only_the_blocks_that_join_up_say_they_do() {
        assert!(connects(85 << 4), "a fence");
        assert!(connects(102 << 4), "a glass pane");
        assert!(connects(139 << 4), "a wall");
        assert!(!connects(1 << 4), "stone does not join a fence");
        assert!(!connects(0), "nor does air");
    }

    #[test]
    fn liquids_are_left_to_be_drawn_another_way() {
        for id in [8, 9, 10, 11] {
            assert_eq!(state(id, 0), None, "block {id} is a liquid");
        }
    }

    #[test]
    fn a_door_reports_which_half_it_is() {
        assert!(state(64, 0).unwrap().1.contains("half=lower"));
        assert!(state(64, 8).unwrap().1.contains("half=upper"));
        assert!(state(64, 4).unwrap().1.contains("open=true"));
        assert_eq!(state(193, 0).unwrap().0, "spruce_door");
    }

    #[test]
    fn an_unknown_block_says_so_rather_than_guessing() {
        assert_eq!(state(250, 0), None);
    }
}
