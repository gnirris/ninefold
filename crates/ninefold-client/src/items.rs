//! What an item looks like in a slot.
//!
//! 1.8 has two kinds of thing in an inventory and draws them differently. An *item* — a sword, an
//! apple, a stick — has a flat picture of its own in `textures/items`. A *block* has no picture at
//! all: the game builds a little three-dimensional cube out of the block's own faces and draws
//! that. Both end up in the hotbar, so both have to work.
//!
//! The split is in the numbering: ids below 256 are blocks and everything from 256 up is an item.
//! That is also what makes placing blocks work — hold id 4, right click, and cobblestone goes
//! down, with no item-to-block mapping anywhere.

use std::collections::HashMap;

use ninefold_assets::{AssetSource, Texture};

/// Tiles are the size 1.8's item pictures are.
const TILE: u32 = 16;
/// Tiles across the sheet.
const ACROSS: u32 = 16;

/// One item's picture. `meta` is the damage value for the items that come in variants —
/// dyes, coal and charcoal — and `None` for everything else.
struct Picture {
    id: i16,
    meta: Option<u16>,
    texture: &'static str,
}

const fn plain(id: i16, texture: &'static str) -> Picture {
    Picture {
        id,
        meta: None,
        texture,
    }
}

const fn variant(id: i16, meta: u16, texture: &'static str) -> Picture {
    Picture {
        id,
        meta: Some(meta),
        texture,
    }
}

/// Enough of 1.8's items to fill a survival inventory.
fn pictures() -> Vec<Picture> {
    let mut all = vec![
        plain(256, "items/iron_shovel"),
        plain(257, "items/iron_pickaxe"),
        plain(258, "items/iron_axe"),
        plain(259, "items/flint_and_steel"),
        plain(260, "items/apple"),
        plain(261, "items/bow_standby"),
        plain(262, "items/arrow"),
        plain(263, "items/coal"),
        plain(264, "items/diamond"),
        plain(265, "items/iron_ingot"),
        plain(266, "items/gold_ingot"),
        plain(267, "items/iron_sword"),
        plain(268, "items/wood_sword"),
        plain(269, "items/wood_shovel"),
        plain(270, "items/wood_pickaxe"),
        plain(271, "items/wood_axe"),
        plain(272, "items/stone_sword"),
        plain(273, "items/stone_shovel"),
        plain(274, "items/stone_pickaxe"),
        plain(275, "items/stone_axe"),
        plain(276, "items/diamond_sword"),
        plain(277, "items/diamond_shovel"),
        plain(278, "items/diamond_pickaxe"),
        plain(279, "items/diamond_axe"),
        plain(280, "items/stick"),
        plain(281, "items/bowl"),
        plain(282, "items/mushroom_stew"),
        plain(283, "items/gold_sword"),
        plain(284, "items/gold_shovel"),
        plain(285, "items/gold_pickaxe"),
        plain(286, "items/gold_axe"),
        plain(287, "items/string"),
        plain(288, "items/feather"),
        plain(289, "items/gunpowder"),
        plain(290, "items/wood_hoe"),
        plain(291, "items/stone_hoe"),
        plain(292, "items/iron_hoe"),
        plain(293, "items/diamond_hoe"),
        plain(294, "items/gold_hoe"),
        plain(295, "items/seeds_wheat"),
        plain(296, "items/wheat"),
        plain(297, "items/bread"),
        plain(298, "items/leather_helmet"),
        plain(299, "items/leather_chestplate"),
        plain(300, "items/leather_leggings"),
        plain(301, "items/leather_boots"),
        plain(302, "items/chainmail_helmet"),
        plain(303, "items/chainmail_chestplate"),
        plain(304, "items/chainmail_leggings"),
        plain(305, "items/chainmail_boots"),
        plain(306, "items/iron_helmet"),
        plain(307, "items/iron_chestplate"),
        plain(308, "items/iron_leggings"),
        plain(309, "items/iron_boots"),
        plain(310, "items/diamond_helmet"),
        plain(311, "items/diamond_chestplate"),
        plain(312, "items/diamond_leggings"),
        plain(313, "items/diamond_boots"),
        plain(314, "items/gold_helmet"),
        plain(315, "items/gold_chestplate"),
        plain(316, "items/gold_leggings"),
        plain(317, "items/gold_boots"),
        plain(318, "items/flint"),
        plain(319, "items/porkchop_raw"),
        plain(320, "items/porkchop_cooked"),
        plain(321, "items/painting"),
        plain(322, "items/apple_golden"),
        plain(323, "items/sign"),
        plain(324, "items/door_wood"),
        plain(325, "items/bucket_empty"),
        plain(326, "items/bucket_water"),
        plain(327, "items/bucket_lava"),
        plain(328, "items/minecart_normal"),
        plain(329, "items/saddle"),
        plain(330, "items/door_iron"),
        plain(331, "items/redstone_dust"),
        plain(332, "items/snowball"),
        plain(333, "items/boat"),
        plain(334, "items/leather"),
        plain(335, "items/bucket_milk"),
        plain(336, "items/brick"),
        plain(337, "items/clay_ball"),
        plain(338, "items/reeds"),
        plain(339, "items/paper"),
        plain(340, "items/book_normal"),
        plain(341, "items/slimeball"),
        plain(342, "items/minecart_chest"),
        plain(343, "items/minecart_furnace"),
        plain(344, "items/egg"),
        plain(345, "items/compass"),
        plain(346, "items/fishing_rod_uncast"),
        plain(347, "items/clock"),
        plain(348, "items/glowstone_dust"),
        plain(349, "items/fish_cod_raw"),
        plain(350, "items/fish_cod_cooked"),
        plain(352, "items/bone"),
        plain(353, "items/sugar"),
        plain(354, "items/cake"),
        plain(355, "items/bed"),
        plain(356, "items/repeater"),
        plain(357, "items/cookie"),
        plain(358, "items/map_filled"),
        plain(359, "items/shears"),
        plain(360, "items/melon"),
        plain(361, "items/seeds_pumpkin"),
        plain(362, "items/seeds_melon"),
        plain(363, "items/beef_raw"),
        plain(364, "items/beef_cooked"),
        plain(365, "items/chicken_raw"),
        plain(366, "items/chicken_cooked"),
        plain(367, "items/rotten_flesh"),
        plain(368, "items/ender_pearl"),
        plain(369, "items/blaze_rod"),
        plain(370, "items/ghast_tear"),
        plain(371, "items/gold_nugget"),
        plain(372, "items/nether_wart"),
        plain(373, "items/potion_bottle_drinkable"),
        plain(374, "items/potion_bottle_empty"),
        plain(375, "items/spider_eye"),
        plain(376, "items/spider_eye_fermented"),
        plain(377, "items/blaze_powder"),
        plain(378, "items/magma_cream"),
        plain(379, "items/brewing_stand"),
        plain(380, "items/cauldron"),
        plain(381, "items/ender_eye"),
        plain(382, "items/melon_speckled"),
        plain(383, "items/egg_spawn"),
        plain(384, "items/experience_bottle"),
        plain(385, "items/fireball"),
        plain(386, "items/book_writable"),
        plain(387, "items/book_written"),
        plain(388, "items/emerald"),
        plain(389, "items/item_frame"),
        plain(390, "items/flower_pot"),
        plain(391, "items/carrot"),
        plain(392, "items/potato"),
        plain(393, "items/potato_baked"),
        plain(394, "items/potato_poisonous"),
        plain(395, "items/map_empty"),
        plain(396, "items/carrot_golden"),
        plain(397, "items/skull_skeleton"),
        plain(398, "items/carrot_on_a_stick"),
        plain(399, "items/nether_star"),
        plain(400, "items/pumpkin_pie"),
        plain(401, "items/fireworks"),
        plain(402, "items/fireworks_charge"),
        plain(403, "items/book_enchanted"),
        plain(404, "items/comparator"),
        plain(405, "items/netherbrick"),
        plain(406, "items/quartz"),
        plain(407, "items/minecart_tnt"),
        plain(408, "items/minecart_hopper"),
        plain(409, "items/prismarine_shard"),
        plain(410, "items/prismarine_crystals"),
        plain(411, "items/rabbit_raw"),
        plain(412, "items/rabbit_cooked"),
        plain(413, "items/rabbit_stew"),
        plain(414, "items/rabbit_foot"),
        plain(415, "items/rabbit_hide"),
        plain(417, "items/iron_horse_armor"),
        plain(418, "items/gold_horse_armor"),
        plain(419, "items/diamond_horse_armor"),
        plain(420, "items/lead"),
        plain(421, "items/name_tag"),
        plain(423, "items/mutton_raw"),
        plain(424, "items/mutton_cooked"),
        plain(427, "items/door_spruce"),
        plain(428, "items/door_birch"),
        plain(429, "items/door_jungle"),
        plain(430, "items/door_acacia"),
        plain(431, "items/door_dark_oak"),
        // Charcoal is coal with a damage value, not an id of its own.
        variant(263, 1, "items/charcoal"),
    ];
    // The sixteen dyes, in the order their damage values run.
    for (meta, texture) in DYES.iter().enumerate() {
        all.push(variant(351, meta as u16, texture));
    }
    all
}

/// The dye pictures, in damage-value order: ink sac first, bone meal last.
const DYES: [&str; 16] = [
    "items/dye_powder_black",
    "items/dye_powder_red",
    "items/dye_powder_green",
    "items/dye_powder_brown",
    "items/dye_powder_blue",
    "items/dye_powder_purple",
    "items/dye_powder_cyan",
    "items/dye_powder_silver",
    "items/dye_powder_gray",
    "items/dye_powder_pink",
    "items/dye_powder_lime",
    "items/dye_powder_yellow",
    "items/dye_powder_light_blue",
    "items/dye_powder_magenta",
    "items/dye_powder_orange",
    "items/dye_powder_white",
];

/// Every item picture in one texture, and where each one is.
pub struct Items {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    rects: HashMap<(i16, Option<u16>), [f32; 4]>,
}

impl Items {
    /// Build the sheet out of whatever the asset source has.
    #[must_use]
    pub fn build(assets: &dyn AssetSource) -> Self {
        let pictures = pictures();
        let count = u32::try_from(pictures.len()).unwrap_or(0);
        let down = count.div_ceil(ACROSS).max(1);
        let (width, height) = (ACROSS * TILE, down * TILE);
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        let mut rects = HashMap::new();

        for (index, picture) in pictures.iter().enumerate() {
            let index = index as u32;
            let (tx, ty) = ((index % ACROSS) * TILE, (index / ACROSS) * TILE);
            // A missing picture leaves the tile empty, which draws as nothing rather than as
            // a wrong thing. Not every pack has every item.
            if let Some(texture) = assets.texture(picture.texture) {
                paste(&mut rgba, width, tx, ty, &texture);
                rects.insert(
                    (picture.id, picture.meta),
                    [
                        tx as f32 / width as f32,
                        ty as f32 / height as f32,
                        TILE as f32 / width as f32,
                        TILE as f32 / height as f32,
                    ],
                );
            }
        }
        Self {
            width,
            height,
            rgba,
            rects,
        }
    }

    /// Where an item's picture is, if it has one. Blocks do not.
    #[must_use]
    pub fn rect(&self, id: i16, damage: u16) -> Option<[f32; 4]> {
        self.rects
            .get(&(id, Some(damage)))
            .or_else(|| self.rects.get(&(id, None)))
            .copied()
    }

    /// How many pictures made it into the sheet.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rects.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }
}

/// Copy a 16×16 picture into the sheet. Anything bigger is cropped, which is what happens to
/// the few items shipped at a higher resolution.
fn paste(into: &mut [u8], into_width: u32, x: u32, y: u32, from: &Texture) {
    for row in 0..TILE.min(from.height) {
        let src = (row * from.width * 4) as usize;
        let dst = (((y + row) * into_width + x) * 4) as usize;
        let run = (TILE.min(from.width) * 4) as usize;
        if src + run <= from.rgba.len() && dst + run <= into.len() {
            into[dst..dst + run].copy_from_slice(&from.rgba[src..src + run]);
        }
    }
}

/// A readable name for an item, for the tooltip and the debug readout.
///
/// The variant wins over the default, the same way the pictures do: damage value 1 on coal is
/// charcoal, not coal.
#[must_use]
pub fn name_of(id: i16, damage: u16) -> String {
    let all = pictures();
    let exact = all
        .iter()
        .find(|p| p.id == id && p.meta == Some(damage))
        .or_else(|| all.iter().find(|p| p.id == id && p.meta.is_none()));
    if let Some(picture) = exact {
        let short = picture.texture.rsplit('/').next().unwrap_or("");
        return short.replace('_', " ");
    }
    if id > 0 && id < 256 {
        format!("block {id}")
    } else {
        format!("item {id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Everything;
    impl AssetSource for Everything {
        fn texture(&self, _id: &str) -> Option<Texture> {
            Some(Texture::flat(TILE, TILE, [10, 20, 30, 255]))
        }
        fn describe(&self) -> String {
            "test".into()
        }
    }

    struct Nothing;
    impl AssetSource for Nothing {
        fn texture(&self, _id: &str) -> Option<Texture> {
            None
        }
        fn describe(&self) -> String {
            "test".into()
        }
    }

    #[test]
    fn every_item_in_the_table_lands_in_the_sheet() {
        let items = Items::build(&Everything);
        assert_eq!(items.len(), pictures().len());
        assert_eq!(items.width, ACROSS * TILE);
        assert!(items.height >= TILE);
    }

    #[test]
    fn a_pickaxe_has_a_picture_and_a_block_does_not() {
        let items = Items::build(&Everything);
        assert!(items.rect(278, 0).is_some(), "diamond pickaxe");
        assert!(items.rect(4, 0).is_none(), "cobblestone is drawn as a cube");
    }

    #[test]
    fn variants_win_over_the_default_but_fall_back_to_it() {
        let items = Items::build(&Everything);
        let coal = items.rect(263, 0).expect("coal");
        let charcoal = items.rect(263, 1).expect("charcoal");
        assert_ne!(coal, charcoal, "charcoal is its own picture");
        // An enchanted sword has a damage value and still looks like a sword.
        assert_eq!(items.rect(267, 0), items.rect(267, 250));
    }

    #[test]
    fn all_sixteen_dyes_are_there_and_are_different() {
        let items = Items::build(&Everything);
        let mut seen = std::collections::HashSet::new();
        for meta in 0..16u16 {
            let rect = items.rect(351, meta).expect("a dye");
            assert!(seen.insert(format!("{rect:?}")), "dye {meta} is a duplicate");
        }
    }

    #[test]
    fn a_pack_with_no_item_pictures_builds_an_empty_sheet_rather_than_failing() {
        let items = Items::build(&Nothing);
        assert!(items.is_empty());
        assert!(items.rect(278, 0).is_none());
    }

    #[test]
    fn names_are_readable() {
        assert_eq!(name_of(278, 0), "diamond pickaxe");
        assert_eq!(name_of(263, 1), "charcoal");
        assert_eq!(name_of(4, 0), "block 4");
    }
}
