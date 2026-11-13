//! Turning the game's model files into something a chunk mesher can stamp out.
//!
//! The models arrive in pixels, with texture names and property strings; the mesher wants
//! corners in block units and texture coordinates already resolved against the atlas. Doing that
//! per block per frame would be absurd, so each distinct block state is baked once and kept.
//!
//! A state is a block id, its metadata, and — for fences, panes and walls — which neighbours it
//! is joined to, because that changes which model the game uses.

use std::collections::HashMap;
use std::sync::Arc;

use ninefold_assets::AssetSource;
use ninefold_mesh::shape::{Baked, Quad};
use ninefold_mesh::Face;
use ninefold_model::states::{self, Links};
use ninefold_model::{Models, PIXELS};

use crate::atlas::{self, Atlas};

/// Two elements of a model can sit in exactly the same place — a grass block's side and the
/// green overlay laid over it — and two faces at the same depth flicker against each other as
/// the camera moves. Each element after the first is nudged out by this much.
const LAYER: f32 = 0.0008;

/// Every block state that has been baked, and the models they came from.
pub struct Shapes {
    models: Models,
    /// Shared rather than owned, so a shape can be handed to the meshing thread without copying
    /// it — a column needs a few dozen of them and they are not small.
    baked: HashMap<(u16, u8), Arc<Baked>>,
    /// What an unmapped block is drawn as: a plain cube, so something unknown is visible rather
    /// than missing.
    fallback: Arc<Baked>,
}

impl Shapes {
    #[must_use]
    pub fn new(assets: Arc<dyn AssetSource>) -> Self {
        Self {
            models: Models::new(assets),
            baked: HashMap::new(),
            fallback: Arc::new(Baked::default()),
        }
    }

    /// Every texture every mapped block state needs, so the atlas can be built of exactly those.
    ///
    /// This walks all 256 ids and all 16 metadata values once at startup. It is the only full
    /// pass over the model files, and it is what lets the atlas be built before anything is
    /// drawn.
    #[must_use]
    pub fn every_texture(&mut self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for id in 0..256u16 {
            for meta in 0..16u8 {
                let block = (id << 4) | u16::from(meta);
                let Some((state, key)) = states::state_of(block, Links::default()) else {
                    continue;
                };
                let Some((model, turn)) = self.models.variant(&state, &key) else {
                    continue;
                };
                for texture in self.models.shape(&model, turn, true).textures() {
                    if seen.insert(texture.to_owned()) {
                        names.push(texture.to_owned());
                    }
                }
            }
        }
        // Liquids are drawn by this client rather than by a model, so nothing above asks for
        // their textures.
        for extra in [
            "blocks/water_still",
            "blocks/water_flow",
            "blocks/lava_still",
            "blocks/lava_flow",
            // The blocks the game draws for itself still need something to be a cube of.
            "blocks/planks_oak",
            "blocks/obsidian",
            "blocks/soul_sand",
        ] {
            if seen.insert(extra.to_owned()) {
                names.push(extra.to_owned());
            }
        }
        names
    }

    /// Make sure this state is baked and ready to be handed out.
    pub fn prepare(&mut self, atlas: &Atlas, block: u16, links: Links) {
        let key = (block, links.bits());
        if self.baked.contains_key(&key) {
            return;
        }
        let baked = self.bake(atlas, block, links);
        self.baked.insert(key, Arc::new(baked));
    }

    /// The shape of a state that has already been prepared.
    #[must_use]
    pub fn get(&self, block: u16, links: Links) -> Option<&Baked> {
        let baked = self.baked.get(&(block, links.bits()))?;
        (!baked.is_empty()).then(|| baked.as_ref())
    }

    /// The same, to keep and send somewhere else.
    #[must_use]
    pub fn shared(&self, block: u16, links: Links) -> Option<Arc<Baked>> {
        let baked = self.baked.get(&(block, links.bits()))?;
        (!baked.is_empty()).then(|| baked.clone())
    }

    /// Build the fallback cube, once an atlas exists to point it at.
    pub fn set_fallback(&mut self, atlas: &Atlas) {
        self.fallback = Arc::new(cube(atlas, "blocks/stone", 0));
    }

    /// Anything with no model at all: a plain cube, so an unknown block is obvious.
    #[must_use]
    pub fn fallback_shared(&self) -> Arc<Baked> {
        self.fallback.clone()
    }

    /// How many distinct states have been baked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.baked.len()
    }


    fn bake(&mut self, atlas: &Atlas, block: u16, links: Links) -> Baked {
        // Liquids have no model file: the real game draws them with a renderer of its own, and
        // so does this one — a full cube of the still texture, which is right for a source block
        // and a little too tall for a flowing one.
        if matches!(block >> 4, 8 | 9) {
            return liquid(atlas, "blocks/water_still", atlas::Tint::Water.code());
        }
        if matches!(block >> 4, 10 | 11) {
            return liquid(atlas, "blocks/lava_still", 0);
        }
        let Some((state, key)) = states::state_of(block, links) else {
            // A handful of blocks have no model because the game draws them with a renderer of
            // their own — chests, signs, beds, banners. A cube of the right material is wrong,
            // but it is the right size and the right colour, which is nearer than nothing.
            return match tile_entity(block) {
                Some(texture) => cube(atlas, texture, 0),
                None => Baked::default(),
            };
        };
        let Some((model, turn)) = self.models.variant(&state, &key) else {
            return Baked::default();
        };
        let tint = atlas::tint_of(block).map_or(0, atlas::Tint::code);
        let shape = self.models.shape(&model, turn, tint != 0);

        let mut quads = Vec::new();
        for (layer, box3) in shape.boxes.iter().enumerate() {
            let out = layer as f32 * LAYER;
            for (side, surface) in &box3.faces {
                let tile = atlas.rect(&surface.texture);
                let corners = corners_of(*side, box3.from, box3.to, out);
                let boundary = on_the_edge(*side, box3.from, box3.to);
                quads.push(Quad {
                    corners,
                    uv: uv_of(surface.uv, surface.rotation, tile),
                    normal: side.normal(),
                    cull: surface.cull,
                    tint: if surface.tinted { tint } else { 0 },
                    shade: box3.shade,
                    boundary,
                });
            }
        }

        // What you walk into. The model is the right answer for almost everything — a slab is
        // half a block to stand on because its model is half a block — but not for the things
        // you walk straight through, and not for a fence, which the game makes taller than it
        // looks so you cannot jump it.
        let collision = if ninefold_protocol::blocks::passable(block) {
            Vec::new()
        } else {
            let tall = states::connects(block) && !matches!(block >> 4, 101 | 102 | 160);
            shape
                .boxes
                .iter()
                .map(|box3| {
                    let from = box3.from.map(|v| v / PIXELS);
                    let mut to = box3.to.map(|v| v / PIXELS);
                    if tall && to[1] >= 0.99 {
                        to[1] = 1.5;
                    }
                    (from, to)
                })
                .collect()
        };

        Baked {
            quads,
            solid: shape.solid,
            collision,
            ambient_occlusion: shape.ambient_occlusion,
        }
    }
}

/// What a block the game draws for itself is roughly made of.
///
/// These are the blocks with no model file at all: a chest is a model in code, not in JSON. The
/// texture here is the one vanilla uses for their breaking particles, which is as close as a
/// cube can get.
fn tile_entity(block: u16) -> Option<&'static str> {
    match block >> 4 {
        26 | 54 | 63 | 68 | 146 | 176 | 177 => Some("blocks/planks_oak"),
        130 => Some("blocks/obsidian"),
        138 => Some("blocks/obsidian"),
        144 => Some("blocks/soul_sand"),
        _ => None,
    }
}

/// A full cube of one texture, for liquids and for the fallback.
fn cube(atlas: &Atlas, texture: &str, tint: u8) -> Baked {
    let tile = atlas.rect(texture);
    let quads = Face::ALL
        .iter()
        .map(|side| Quad {
            corners: corners_of(*side, [0.0; 3], [PIXELS; 3], 0.0),
            uv: uv_of([0.0, 0.0, PIXELS, PIXELS], 0, tile),
            normal: side.normal(),
            cull: Some(*side),
            tint,
            shade: true,
            boundary: true,
        })
        .collect();
    Baked {
        quads,
        solid: true,
        collision: vec![([0.0; 3], [1.0; 3])],
        ambient_occlusion: true,
    }
}

/// A liquid: a cube you can see through and walk into.
fn liquid(atlas: &Atlas, texture: &str, tint: u8) -> Baked {
    Baked {
        solid: false,
        collision: Vec::new(),
        ..cube(atlas, texture, tint)
    }
}

/// Is this face on the edge of its block, where there is a neighbour in front of it?
fn on_the_edge(side: Face, from: [f32; 3], to: [f32; 3]) -> bool {
    match side {
        Face::West => from[0] <= 0.01,
        Face::East => to[0] >= PIXELS - 0.01,
        Face::Down => from[1] <= 0.01,
        Face::Up => to[1] >= PIXELS - 0.01,
        Face::North => from[2] <= 0.01,
        Face::South => to[2] >= PIXELS - 0.01,
    }
}

/// The four corners of one side of a box, in block units, bottom left first as seen from
/// outside — the order the whole renderer agrees on.
fn corners_of(side: Face, from: [f32; 3], to: [f32; 3], out: f32) -> [[f32; 3]; 4] {
    let [x, y, z] = from.map(|v| v / PIXELS);
    let [x1, y1, z1] = to.map(|v| v / PIXELS);
    // Nudged along the face's own normal, so two elements in the same place do not flicker.
    let (x, y, z, x1, y1, z1) = match side {
        Face::West => (x - out, y, z, x1, y1, z1),
        Face::East => (x, y, z, x1 + out, y1, z1),
        Face::Down => (x, y - out, z, x1, y1, z1),
        Face::Up => (x, y, z, x1, y1 + out, z1),
        Face::North => (x, y, z - out, x1, y1, z1),
        Face::South => (x, y, z, x1, y1, z1 + out),
    };
    match side {
        Face::North => [[x1, y, z], [x, y, z], [x, y1, z], [x1, y1, z]],
        Face::South => [[x, y, z1], [x1, y, z1], [x1, y1, z1], [x, y1, z1]],
        Face::West => [[x, y, z], [x, y, z1], [x, y1, z1], [x, y1, z]],
        Face::East => [[x1, y, z1], [x1, y, z], [x1, y1, z], [x1, y1, z1]],
        Face::Up => [[x, y1, z1], [x1, y1, z1], [x1, y1, z], [x, y1, z]],
        Face::Down => [[x, y, z], [x1, y, z], [x1, y, z1], [x, y, z1]],
    }
}

/// Where each corner samples, in the same order, with the face's own quarter turns applied.
fn uv_of(uv: [f32; 4], rotation: u16, tile: [f32; 4]) -> [[f32; 2]; 4] {
    let [u0, v0, u1, v1] = uv;
    // Bottom left, bottom right, top right, top left — texture coordinates run downwards, so the
    // bottom of a face is the larger v.
    let mut corners = [
        Atlas::uv_in(tile, [u0, v1]),
        Atlas::uv_in(tile, [u1, v1]),
        Atlas::uv_in(tile, [u1, v0]),
        Atlas::uv_in(tile, [u0, v0]),
    ];
    corners.rotate_left((rotation as usize / 90) % 4);
    corners
}

/// Where the interface asks what a block looks like, so a slot can draw it.
impl crate::gui::BlockShapes for Shapes {
    fn of(&self, block: u16) -> Option<&Baked> {
        // A block in a slot is drawn as it would stand alone, joined to nothing.
        self.get(block, Links::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ninefold_assets::Texture;

    /// An asset source with a handful of models and a texture for everything.
    struct Pack;

    impl AssetSource for Pack {
        fn texture(&self, _id: &str) -> Option<Texture> {
            Some(Texture::flat(16, 16, [90, 90, 90, 255]))
        }
        fn text(&self, id: &str) -> Option<String> {
            let body = match id {
                "blockstates/stone.json" => r##"{"variants":{"normal":{"model":"stone"}}}"##,
                "blockstates/stone_slab.json" => {
                    r##"{"variants":{"half=bottom":{"model":"half_slab"},"half=top":{"model":"half_slab"}}}"##
                }
                "blockstates/fence.json" => {
                    r##"{"variants":{"east=false,north=false,south=false,west=false":{"model":"fence_post"}}}"##
                }
                "models/block/stone.json" => {
                    r##"{"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{
                        "down":{"texture":"blocks/stone","cullface":"down"},
                        "up":{"texture":"blocks/stone","cullface":"up"},
                        "north":{"texture":"blocks/stone","cullface":"north"},
                        "south":{"texture":"blocks/stone","cullface":"south"},
                        "west":{"texture":"blocks/stone","cullface":"west"},
                        "east":{"texture":"blocks/stone","cullface":"east"}}}]}"##
                }
                "models/block/half_slab.json" => {
                    r##"{"elements":[{"from":[0,0,0],"to":[16,8,16],"faces":{
                        "down":{"texture":"blocks/stone_slab_top","cullface":"down"},
                        "up":{"texture":"blocks/stone_slab_top"},
                        "north":{"texture":"blocks/stone_slab_side","cullface":"north"}}}]}"##
                }
                "models/block/fence_post.json" => {
                    r##"{"elements":[{"from":[6,0,6],"to":[10,16,10],"faces":{
                        "north":{"texture":"blocks/planks_oak"},
                        "up":{"texture":"blocks/planks_oak","cullface":"up"}}}]}"##
                }
                _ => return None,
            };
            Some(body.to_owned())
        }
        fn describe(&self) -> String {
            "test pack".into()
        }
    }

    fn shapes() -> (Shapes, Atlas) {
        let mut shapes = Shapes::new(Arc::new(Pack));
        let wanted = shapes.every_texture();
        let atlas = Atlas::build(&Pack, &wanted);
        (shapes, atlas)
    }

    #[test]
    fn the_textures_a_world_needs_are_collected_before_anything_is_drawn() {
        let mut shapes = Shapes::new(Arc::new(Pack));
        let wanted = shapes.every_texture();
        assert!(wanted.contains(&"blocks/stone".to_owned()));
        assert!(wanted.contains(&"blocks/stone_slab_top".to_owned()));
        // Liquids have no model, so their textures are added by hand or never asked for.
        assert!(wanted.contains(&"blocks/water_still".to_owned()));
        // And nothing is in there twice.
        let mut sorted = wanted.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), wanted.len());
    }

    #[test]
    fn a_cube_bakes_to_six_culled_faces() {
        let (mut shapes, atlas) = shapes();
        let stone = 1 << 4;
        shapes.prepare(&atlas, stone, Links::default());
        let baked = shapes.get(stone, Links::default()).expect("stone");
        assert_eq!(baked.quads.len(), 6);
        assert!(baked.solid, "a full cube hides its neighbours");
        assert!(baked.quads.iter().all(|q| q.cull.is_some()));
        assert!(baked.quads.iter().all(|q| q.boundary));
        assert_eq!(baked.collision.len(), 1);
        assert_eq!(baked.height(), 1.0);
    }

    #[test]
    fn a_slab_is_half_a_block_to_stand_on() {
        let (mut shapes, atlas) = shapes();
        let slab = 44 << 4;
        shapes.prepare(&atlas, slab, Links::default());
        let baked = shapes.get(slab, Links::default()).expect("a slab");
        assert!(!baked.solid, "which is why it cannot hide a neighbour");
        assert_eq!(baked.height(), 0.5, "and why you can step onto it");
        // Its top face is not on the edge of the block, so it takes its own light.
        let top = baked
            .quads
            .iter()
            .find(|q| q.normal == [0.0, 1.0, 0.0])
            .expect("a top");
        assert!(!top.boundary);
        assert_eq!(top.corners[0][1], 0.5, "half way up");
    }

    #[test]
    fn a_fence_is_taller_to_walk_into_than_it_looks() {
        let (mut shapes, atlas) = shapes();
        let fence = 85 << 4;
        shapes.prepare(&atlas, fence, Links::default());
        let baked = shapes.get(fence, Links::default()).expect("a fence");
        assert_eq!(baked.height(), 1.5, "you cannot jump a fence");
        // The post's own sides have no cullface, so they are drawn from inside the block.
        assert!(baked.quads.iter().any(|q| q.cull.is_none()));
    }

    #[test]
    fn a_liquid_has_no_model_and_is_still_drawn() {
        let (mut shapes, atlas) = shapes();
        for water in [8 << 4, 9 << 4] {
            shapes.prepare(&atlas, water, Links::default());
            let baked = shapes.get(water, Links::default()).expect("water");
            assert_eq!(baked.quads.len(), 6);
            assert!(!baked.solid, "you can see out of water");
            assert!(baked.collision.is_empty(), "and swim through it");
            assert!(baked.quads.iter().all(|q| q.tint == atlas::Tint::Water.code()));
        }
    }

    #[test]
    fn plants_are_walked_through_rather_than_collided_with() {
        let (mut shapes, atlas) = shapes();
        // Tall grass has no model in this test pack, but it is passable whatever shape it is.
        let grass = (31 << 4) | 1;
        shapes.prepare(&atlas, grass, Links::default());
        assert!(
            shapes
                .get(grass, Links::default())
                .is_none_or(|baked| baked.collision.is_empty())
        );
    }

    #[test]
    fn baking_twice_does_not_bake_twice() {
        let (mut shapes, atlas) = shapes();
        shapes.prepare(&atlas, 1 << 4, Links::default());
        let once = shapes.len();
        shapes.prepare(&atlas, 1 << 4, Links::default());
        assert_eq!(shapes.len(), once);
    }

    #[test]
    fn a_blocks_own_state_and_its_joins_are_different_shapes() {
        let (mut shapes, atlas) = shapes();
        let fence = 85 << 4;
        shapes.prepare(&atlas, fence, Links::default());
        shapes.prepare(
            &atlas,
            fence,
            Links {
                north: true,
                ..Links::default()
            },
        );
        assert_eq!(shapes.len(), 2, "joined and alone are baked separately");
    }

    #[test]
    fn a_face_samples_the_part_of_the_texture_its_model_says() {
        let tile = [0.0, 0.0, 0.25, 0.25];
        // The whole texture, no rotation: bottom left is the bottom left of the tile.
        let whole = uv_of([0.0, 0.0, 16.0, 16.0], 0, tile);
        assert_eq!(whole[0], [0.0, 0.25], "bottom left");
        assert_eq!(whole[2], [0.25, 0.0], "top right");
        // The bottom half of a texture, as a slab's side takes.
        let half = uv_of([0.0, 8.0, 16.0, 16.0], 0, tile);
        assert_eq!(half[0], [0.0, 0.25]);
        assert_eq!(half[3], [0.0, 0.125], "the top of the face is half way up");
    }

    #[test]
    fn a_quarter_turn_of_a_texture_moves_its_corners_round() {
        let tile = [0.0, 0.0, 1.0, 1.0];
        let straight = uv_of([0.0, 0.0, 16.0, 16.0], 0, tile);
        let turned = uv_of([0.0, 0.0, 16.0, 16.0], 90, tile);
        assert_ne!(straight, turned);
        // Four quarter turns is where it started.
        assert_eq!(uv_of([0.0, 0.0, 16.0, 16.0], 360, tile), straight);
    }

    #[test]
    fn every_face_of_a_baked_cube_is_wound_outwards() {
        let (mut shapes, atlas) = shapes();
        shapes.prepare(&atlas, 1 << 4, Links::default());
        let baked = shapes.get(1 << 4, Links::default()).unwrap();
        for quad in &baked.quads {
            let edge = |a: usize, b: usize| {
                [
                    quad.corners[a][0] - quad.corners[b][0],
                    quad.corners[a][1] - quad.corners[b][1],
                    quad.corners[a][2] - quad.corners[b][2],
                ]
            };
            let (u, w) = (edge(1, 0), edge(2, 0));
            let wound = [
                u[1] * w[2] - u[2] * w[1],
                u[2] * w[0] - u[0] * w[2],
                u[0] * w[1] - u[1] * w[0],
            ];
            let n = quad.normal;
            let dot = wound[0] * n[0] + wound[1] * n[1] + wound[2] * n[2];
            assert!(dot > 0.0, "a face pointing {n:?} is wound inwards");
        }
    }

    #[test]
    fn an_unknown_block_gets_a_cube_rather_than_nothing() {
        let (mut shapes, atlas) = shapes();
        shapes.set_fallback(&atlas);
        assert_eq!(shapes.fallback_shared().quads.len(), 6);
        assert!(shapes.fallback_shared().solid);
    }
}
