//! What shape a block is, read from the game's own model files.
//!
//! Almost nothing in Minecraft is a cube. Stairs, slabs, fences, panes, torches, doors, rails,
//! carpets, snow layers, cakes and beds are not, and drawing them as cubes is the single most
//! obvious thing a reimplementation can get wrong — a fence becomes a wall, a torch becomes a
//! glowing block, a snow layer buries the grass it is sitting on.
//!
//! Writing those shapes out by hand would be hundreds of boxes of guesswork. The player's own
//! jar already has them: `assets/minecraft/blockstates/*.json` says which model a block state
//! uses and how it is turned, and `assets/minecraft/models/block/*.json` says what that model is
//! made of. Both are read here, so the geometry is the game's own rather than an impression of
//! it.
//!
//! Three things make that less simple than it sounds:
//!
//! - **Models inherit.** `stone_stairs` has no elements of its own; it takes them from `stairs`
//!   through a `parent` chain, and only fills in which textures go where.
//! - **Textures are named, then resolved.** A face says `#side`, the model says `"side": "#all"`,
//!   and the parent says `"all": "blocks/stone"`. The chain has to be followed to the end.
//! - **Variants are turned.** A stair facing west is the east-facing model with `"y": 180`,
//!   which rotates the boxes *and* permutes which face is which.

pub mod states;

use std::collections::HashMap;
use std::sync::Arc;

use ninefold_assets::AssetSource;
use ninefold_mesh::Face;

/// How many model pixels make a block. Everything in a model file is in these.
pub const PIXELS: f32 = 16.0;

/// One face of one box.
#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    /// The texture this face samples, resolved to a real name like `blocks/stone`.
    pub texture: String,
    /// Which part of that texture, in pixels: left, top, right, bottom.
    pub uv: [f32; 4],
    /// The neighbour that would hide this face, if any. A face with no cullface is drawn even
    /// when something is pressed against it — which is why a fence post still has sides.
    pub cull: Option<Face>,
    /// A quarter turn of the texture, 0, 90, 180 or 270.
    pub rotation: u16,
    /// Whether this face takes the block's biome colour.
    pub tinted: bool,
}

/// One box of a model.
#[derive(Debug, Clone, PartialEq)]
pub struct Box3 {
    /// Low corner, in model pixels.
    pub from: [f32; 3],
    pub to: [f32; 3],
    /// Which of the six sides are drawn, and how.
    pub faces: Vec<(Face, Surface)>,
    /// Whether this box is shaded by which way its faces point. Torches are not.
    pub shade: bool,
}

/// Everything a block state is made of.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shape {
    pub boxes: Vec<Box3>,
    /// Whether the shape fills its cube, and so hides its neighbours' faces.
    pub solid: bool,
    pub ambient_occlusion: bool,
}

impl Shape {
    /// A shape with nothing in it, for air and for anything unknown.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    /// Every texture this shape needs, so an atlas can be built of exactly those.
    pub fn textures(&self) -> impl Iterator<Item = &str> {
        self.boxes
            .iter()
            .flat_map(|box3| box3.faces.iter().map(|(_, face)| face.texture.as_str()))
    }
}

/// A model file as it is written, before anything is resolved.
#[derive(Debug, Clone, Default)]
struct Raw {
    parent: Option<String>,
    textures: HashMap<String, String>,
    elements: Option<Vec<serde_json::Value>>,
    ambient_occlusion: Option<bool>,
}

/// Reads and resolves models, keeping what it has already worked out.
pub struct Models {
    /// Shared rather than borrowed: the client holds this for as long as it runs, and a borrow
    /// would make everything that owns it self-referential.
    assets: Arc<dyn AssetSource>,
    /// Model files, by name, as they were written.
    raw: HashMap<String, Raw>,
    /// Blockstate files, by name.
    states: HashMap<String, serde_json::Value>,
}

impl Models {
    #[must_use]
    pub fn new(assets: Arc<dyn AssetSource>) -> Self {
        Self {
            assets,
            raw: HashMap::new(),
            states: HashMap::new(),
        }
    }

    /// Read one model file, or hand back the one already read.
    fn raw(&mut self, name: &str) -> Option<Raw> {
        if let Some(raw) = self.raw.get(name) {
            return Some(raw.clone());
        }
        let text = self.assets.text(&format!("models/{name}.json"))?;
        let parsed: serde_json::Value = serde_json::from_str(&text).ok()?;
        let mut textures = HashMap::new();
        if let Some(map) = parsed.get("textures").and_then(|t| t.as_object()) {
            for (key, value) in map {
                if let Some(value) = value.as_str() {
                    textures.insert(key.clone(), value.to_owned());
                }
            }
        }
        let raw = Raw {
            parent: parsed
                .get("parent")
                .and_then(|p| p.as_str())
                .map(str::to_owned),
            textures,
            elements: parsed
                .get("elements")
                .and_then(|e| e.as_array())
                .cloned(),
            ambient_occlusion: parsed.get("ambientocclusion").and_then(serde_json::Value::as_bool),
        };
        self.raw.insert(name.to_owned(), raw.clone());
        Some(raw)
    }

    /// Follow a model's parents, gathering textures from the whole chain and elements from the
    /// nearest one that has any.
    ///
    /// Nearest wins for elements and furthest loses for textures: a child overrides what its
    /// parent said, which is the whole point of the chain.
    fn resolve(&mut self, name: &str) -> Option<(Vec<serde_json::Value>, HashMap<String, String>, bool)> {
        let mut textures: HashMap<String, String> = HashMap::new();
        let mut elements = None;
        let mut ambient_occlusion = true;
        let mut next = Some(name.to_owned());
        // A malformed pack could point a model at itself; ten levels is far more than any real
        // chain, which is three.
        for _ in 0..10 {
            let Some(current) = next.take() else { break };
            let Some(raw) = self.raw(&current) else { break };
            for (key, value) in raw.textures {
                textures.entry(key).or_insert(value);
            }
            if elements.is_none() {
                elements = raw.elements;
            }
            if let Some(ao) = raw.ambient_occlusion {
                ambient_occlusion = ao;
            }
            next = raw.parent;
        }
        Some((elements?, textures, ambient_occlusion))
    }

    /// Turn `#side` into `blocks/stone`, following however many hops it takes.
    fn texture_of<'t>(textures: &'t HashMap<String, String>, name: &'t str) -> Option<String> {
        let mut name: &str = name;
        for _ in 0..8 {
            let Some(key) = name.strip_prefix('#') else {
                return Some(name.to_owned());
            };
            name = textures.get(key)?.as_str();
        }
        None
    }

    /// Build the shape for one model, turned by `x` and `y` degrees.
    #[must_use]
    pub fn shape(&mut self, model: &str, turn: (i32, i32), tinted: bool) -> Shape {
        let Some((elements, textures, ambient_occlusion)) = self.resolve(model) else {
            return Shape::empty();
        };
        let mut boxes = Vec::new();
        for element in &elements {
            if let Some(box3) = Self::element(element, &textures, tinted) {
                boxes.push(box3);
            }
        }
        for _ in 0..turn.1.rem_euclid(360) / 90 {
            boxes = boxes.into_iter().map(turn_y).collect();
        }
        for _ in 0..turn.0.rem_euclid(360) / 90 {
            boxes = boxes.into_iter().map(turn_x).collect();
        }
        let solid = boxes.iter().any(fills_the_cube);
        Shape {
            boxes,
            solid,
            ambient_occlusion,
        }
    }

    /// One element of a model file.
    fn element(
        element: &serde_json::Value,
        textures: &HashMap<String, String>,
        tinted: bool,
    ) -> Option<Box3> {
        let corner = |key: &str| -> Option<[f32; 3]> {
            let list = element.get(key)?.as_array()?;
            Some([
                list.first()?.as_f64()? as f32,
                list.get(1)?.as_f64()? as f32,
                list.get(2)?.as_f64()? as f32,
            ])
        };
        let from = corner("from")?;
        let to = corner("to")?;
        let shade = element
            .get("shade")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);

        let mut faces = Vec::new();
        for (name, face) in element.get("faces")?.as_object()? {
            let Some(side) = side_named(name) else {
                continue;
            };
            let Some(texture) = face
                .get("texture")
                .and_then(|t| t.as_str())
                .and_then(|t| Self::texture_of(textures, t))
            else {
                continue;
            };
            // A face with no uv of its own takes it from where the box sits, which is what makes
            // a slab's side show the bottom half of its texture without saying so.
            let uv = face
                .get("uv")
                .and_then(|uv| uv.as_array())
                .and_then(|uv| {
                    Some([
                        uv.first()?.as_f64()? as f32,
                        uv.get(1)?.as_f64()? as f32,
                        uv.get(2)?.as_f64()? as f32,
                        uv.get(3)?.as_f64()? as f32,
                    ])
                })
                .unwrap_or_else(|| default_uv(side, from, to));
            faces.push((
                side,
                Surface {
                    texture,
                    uv,
                    cull: face
                        .get("cullface")
                        .and_then(|c| c.as_str())
                        .and_then(side_named),
                    rotation: face
                        .get("rotation")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0) as u16,
                    // A tintindex of any value means "colour this by the block"; only grass,
                    // leaves, water and a few others have one at all.
                    tinted: tinted && face.get("tintindex").is_some(),
                },
            ));
        }
        Some(Box3 {
            from,
            to,
            faces,
            shade,
        })
    }

    /// Read a blockstate file and give back the model and rotation for one variant.
    ///
    /// A variant key is the property string the game uses, like
    /// `facing=east,half=bottom,shape=straight`. Where the key is not found the first variant is
    /// used, because a wrong shape is better than no block.
    #[must_use]
    pub fn variant(&mut self, block_state: &str, variant: &str) -> Option<(String, (i32, i32))> {
        if !self.states.contains_key(block_state) {
            let text = self
                .assets
                .text(&format!("blockstates/{block_state}.json"))?;
            let parsed: serde_json::Value = serde_json::from_str(&text).ok()?;
            self.states.insert(block_state.to_owned(), parsed);
        }
        let variants = self.states.get(block_state)?.get("variants")?.as_object()?;
        let chosen = variants
            .get(variant)
            .or_else(|| variants.values().next())?;
        // Some variants are a list of models to pick between at random; the first will do.
        let chosen = match chosen {
            serde_json::Value::Array(list) => list.first()?,
            other => other,
        };
        let model = chosen.get("model")?.as_str()?;
        let turn = (
            chosen.get("x").and_then(serde_json::Value::as_i64).unwrap_or(0) as i32,
            chosen.get("y").and_then(serde_json::Value::as_i64).unwrap_or(0) as i32,
        );
        Some((format!("block/{model}"), turn))
    }
}

/// Which side a model file means by a name.
fn side_named(name: &str) -> Option<Face> {
    match name {
        "down" => Some(Face::Down),
        "up" => Some(Face::Up),
        "north" => Some(Face::North),
        "south" => Some(Face::South),
        "west" => Some(Face::West),
        "east" => Some(Face::East),
        _ => None,
    }
}

/// Where a face samples when it does not say: the box's own outline, seen from that side.
fn default_uv(side: Face, from: [f32; 3], to: [f32; 3]) -> [f32; 4] {
    match side {
        Face::Down | Face::Up => [from[0], from[2], to[0], to[2]],
        Face::North | Face::South => [from[0], PIXELS - to[1], to[0], PIXELS - from[1]],
        Face::West | Face::East => [from[2], PIXELS - to[1], to[2], PIXELS - from[1]],
    }
}

/// Does this box fill the whole cube?
fn fills_the_cube(box3: &Box3) -> bool {
    box3.from.iter().all(|v| *v <= 0.001) && box3.to.iter().all(|v| *v >= PIXELS - 0.001)
}

/// Turn a box a quarter turn clockwise seen from above, as a blockstate's `"y"` does.
///
/// The box moves and the faces are renamed: the side that pointed east now points south. Both
/// halves matter — move the box without renaming the faces and a stair's step ends up in the
/// right place with the wrong textures on it.
fn turn_y(mut box3: Box3) -> Box3 {
    let (from, to) = (box3.from, box3.to);
    box3.from = [PIXELS - to[2], from[1], from[0]];
    box3.to = [PIXELS - from[2], to[1], to[0]];
    for (side, surface) in &mut box3.faces {
        *side = turn_face_y(*side);
        surface.cull = surface.cull.map(turn_face_y);
    }
    box3
}

const fn turn_face_y(side: Face) -> Face {
    match side {
        Face::North => Face::East,
        Face::East => Face::South,
        Face::South => Face::West,
        Face::West => Face::North,
        other => other,
    }
}

/// The same about the x axis, as a blockstate's `"x"` does — which is how upside-down stairs
/// and wall torches are described.
fn turn_x(mut box3: Box3) -> Box3 {
    let (from, to) = (box3.from, box3.to);
    box3.from = [from[0], PIXELS - to[2], from[1]];
    box3.to = [to[0], PIXELS - from[2], to[1]];
    for (side, surface) in &mut box3.faces {
        *side = turn_face_x(*side);
        surface.cull = surface.cull.map(turn_face_x);
    }
    box3
}

const fn turn_face_x(side: Face) -> Face {
    match side {
        Face::Down => Face::North,
        Face::North => Face::Up,
        Face::Up => Face::South,
        Face::South => Face::Down,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An asset source holding a few model files written out by hand.
    struct Written(HashMap<String, String>);

    impl Written {
        fn new(files: &[(&str, &str)]) -> Self {
            Self(
                files
                    .iter()
                    .map(|(name, body)| ((*name).to_owned(), (*body).to_owned()))
                    .collect(),
            )
        }
    }

    impl AssetSource for Written {
        fn texture(&self, _id: &str) -> Option<ninefold_assets::Texture> {
            None
        }
        fn text(&self, id: &str) -> Option<String> {
            self.0.get(id).cloned()
        }
        fn describe(&self) -> String {
            "written out by hand".into()
        }
    }

    const CUBE: &str = r##"{
        "elements": [{
            "from": [0, 0, 0], "to": [16, 16, 16],
            "faces": {
                "down": {"texture": "#down", "cullface": "down"},
                "up": {"texture": "#up", "cullface": "up"},
                "north": {"texture": "#north", "cullface": "north"},
                "south": {"texture": "#south", "cullface": "south"},
                "west": {"texture": "#west", "cullface": "west"},
                "east": {"texture": "#east", "cullface": "east"}
            }
        }]
    }"##;

    const CUBE_ALL: &str = r##"{
        "parent": "block/cube",
        "textures": {
            "down": "#all", "up": "#all", "north": "#all",
            "south": "#all", "west": "#all", "east": "#all"
        }
    }"##;

    const STONE: &str = r##"{"parent": "block/cube_all", "textures": {"all": "blocks/stone"}}"##;

    const SLAB: &str = r##"{
        "elements": [{
            "from": [0, 0, 0], "to": [16, 8, 16],
            "faces": {
                "down": {"texture": "#bottom", "cullface": "down"},
                "up": {"texture": "#top"},
                "north": {"texture": "#side", "cullface": "north"},
                "east": {"texture": "#side", "cullface": "east"}
            }
        }]
    }"##;

    fn models() -> Written {
        Written::new(&[
            ("models/block/cube.json", CUBE),
            ("models/block/cube_all.json", CUBE_ALL),
            ("models/block/stone.json", STONE),
            ("models/block/half_slab.json", SLAB),
            (
                "models/block/half_slab_stone.json",
                r##"{"parent": "block/half_slab", "textures": {"bottom": "blocks/stone_slab_top", "top": "blocks/stone_slab_top", "side": "blocks/stone_slab_side"}}"##,
            ),
            (
                "blockstates/stone_slab.json",
                r##"{"variants": {"half=bottom": {"model": "half_slab_stone"}, "half=top": {"model": "upper_slab_stone"}}}"##,
            ),
            (
                "blockstates/stone_stairs.json",
                r##"{"variants": {
                    "facing=east,half=bottom,shape=straight": {"model": "stone_stairs"},
                    "facing=west,half=bottom,shape=straight": {"model": "stone_stairs", "y": 180, "uvlock": true}
                }}"##,
            ),
        ])
    }

    #[test]
    fn a_model_takes_its_elements_from_its_parent_and_its_textures_from_itself() {
        let source = models();
        let mut models = Models::new(Arc::new(source));
        let shape = models.shape("block/stone", (0, 0), false);
        assert_eq!(shape.boxes.len(), 1, "one box, inherited from block/cube");
        assert!(shape.solid, "a full cube hides its neighbours");
        // Every face resolved the whole way: #down -> #all -> blocks/stone.
        assert_eq!(shape.boxes[0].faces.len(), 6);
        for (_, face) in &shape.boxes[0].faces {
            assert_eq!(face.texture, "blocks/stone");
        }
    }

    #[test]
    fn a_slab_is_half_a_block_and_does_not_hide_what_is_next_to_it() {
        let source = models();
        let mut models = Models::new(Arc::new(source));
        let shape = models.shape("block/half_slab_stone", (0, 0), false);
        let slab = &shape.boxes[0];
        assert_eq!(slab.to[1], 8.0, "half as tall");
        assert!(!shape.solid, "which is why it cannot hide a neighbour");
        // The face that was left out of the file is left out here too.
        assert!(slab.faces.iter().all(|(side, _)| *side != Face::South));
    }

    #[test]
    fn a_face_with_no_uv_takes_it_from_the_box() {
        let source = models();
        let mut models = Models::new(Arc::new(source));
        let shape = models.shape("block/half_slab_stone", (0, 0), false);
        let (_, north) = shape.boxes[0]
            .faces
            .iter()
            .find(|(side, _)| *side == Face::North)
            .expect("a north face");
        // A box 0..8 tall, seen from the side, samples the bottom half of its texture — and
        // texture coordinates run downwards, so that is 8..16 rather than 0..8.
        assert_eq!(north.uv, [0.0, 8.0, 16.0, 16.0]);
    }

    #[test]
    fn turning_a_model_moves_the_boxes_and_renames_the_faces() {
        let source = models();
        let mut models = Models::new(Arc::new(source));
        // A box filling the east half of the block.
        let east_half = Written::new(&[(
            "models/block/step.json",
            r##"{"elements": [{"from": [8, 0, 0], "to": [16, 16, 16],
                "faces": {"east": {"texture": "blocks/stone", "cullface": "east"}}}]}"##,
        )]);
        let mut turned = Models::new(Arc::new(east_half));
        let straight = turned.shape("block/step", (0, 0), false);
        assert_eq!(straight.boxes[0].from[0], 8.0);
        assert_eq!(straight.boxes[0].faces[0].0, Face::East);

        // A quarter turn puts it on the south side, with the face renamed to match.
        let quarter = turned.shape("block/step", (0, 90), false);
        assert_eq!(quarter.boxes[0].from, [0.0, 0.0, 8.0], "now the south half");
        assert_eq!(quarter.boxes[0].to, [16.0, 16.0, 16.0]);
        assert_eq!(quarter.boxes[0].faces[0].0, Face::South);
        assert_eq!(quarter.boxes[0].faces[0].1.cull, Some(Face::South));

        // And half a turn puts it on the west side.
        let half = turned.shape("block/step", (0, 180), false);
        assert_eq!(half.boxes[0].from, [0.0, 0.0, 0.0]);
        assert_eq!(half.boxes[0].to, [8.0, 16.0, 16.0]);
        assert_eq!(half.boxes[0].faces[0].0, Face::West);

        // Four quarter turns is where it started.
        let whole = turned.shape("block/step", (0, 360), false);
        assert_eq!(whole.boxes[0].from, straight.boxes[0].from);
        let _ = models.shape("block/stone", (0, 0), false);
    }

    #[test]
    fn turning_about_x_puts_a_slab_on_the_ceiling() {
        let source = models();
        let mut models = Models::new(Arc::new(source));
        let upside_down = models.shape("block/half_slab_stone", (180, 0), false);
        let slab = &upside_down.boxes[0];
        assert_eq!(slab.from[1], 8.0, "the slab is now the top half");
        assert_eq!(slab.to[1], 16.0);
        // And what was its underside now faces up.
        assert!(
            slab.faces
                .iter()
                .any(|(side, surface)| *side == Face::Up && surface.cull == Some(Face::Up))
        );
    }

    #[test]
    fn a_blockstate_says_which_model_and_how_it_is_turned() {
        let source = models();
        let mut models = Models::new(Arc::new(source));
        assert_eq!(
            models.variant("stone_slab", "half=bottom"),
            Some(("block/half_slab_stone".to_owned(), (0, 0)))
        );
        assert_eq!(
            models.variant("stone_stairs", "facing=west,half=bottom,shape=straight"),
            Some(("block/stone_stairs".to_owned(), (0, 180)))
        );
    }

    #[test]
    fn an_unknown_variant_falls_back_rather_than_vanishing() {
        let source = models();
        let mut models = Models::new(Arc::new(source));
        let fallback = models.variant("stone_stairs", "facing=nowhere");
        assert!(fallback.is_some(), "a wrong shape beats no block");
    }

    /// The whole chain against the player's own jar: every block id and metadata value through
    /// the state table, the blockstate files and the models. Ignored by default because it needs
    /// the game installed.
    #[test]
    #[ignore = "needs a real 1.8.9 install"]
    fn every_block_in_the_table_resolves_against_the_real_jar() {
        let Some(jar) = ninefold_assets::Vanilla::usual_places().into_iter().next() else {
            panic!("no 1.8.9 jar in the usual places");
        };
        let vanilla = ninefold_assets::Vanilla::open(&jar).expect("open the jar");
        let mut models = Models::new(Arc::new(vanilla));

        let (mut mapped, mut resolved, mut empty) = (0, 0, Vec::new());
        let mut textures = std::collections::HashSet::new();
        for id in 0..256u16 {
            for meta in 0..16u8 {
                let block = (id << 4) | u16::from(meta);
                let Some((state, key)) = states::state_of(block, states::Links::default()) else {
                    continue;
                };
                mapped += 1;
                let Some((model, turn)) = models.variant(&state, &key) else {
                    empty.push(format!("{state}[{key}] has no variant"));
                    continue;
                };
                let shape = models.shape(&model, turn, true);
                if shape.is_empty() {
                    empty.push(format!("{state}[{key}] -> {model} has no boxes"));
                    continue;
                }
                resolved += 1;
                for texture in shape.textures() {
                    textures.insert(texture.to_owned());
                }
            }
        }
        println!("{resolved} of {mapped} block states resolved to geometry");
        println!("{} distinct textures needed", textures.len());
        for line in empty.iter().take(20) {
            println!("  unresolved: {line}");
        }
        assert!(mapped > 500, "only {mapped} states are mapped at all");
        assert!(
            empty.len() * 20 < mapped,
            "{} of {mapped} states did not resolve",
            empty.len()
        );
        // The textures a world is actually made of have to be in there.
        for wanted in ["blocks/stone", "blocks/grass_top", "blocks/planks_oak"] {
            assert!(textures.contains(wanted), "{wanted} was never asked for");
        }
    }

    #[test]
    fn a_model_that_is_not_there_is_an_empty_shape() {
        let source = models();
        let mut models = Models::new(Arc::new(source));
        assert!(models.shape("block/not_a_model", (0, 0), false).is_empty());
        assert!(models.variant("not_a_block", "x=1").is_none());
    }

    #[test]
    fn a_model_that_is_its_own_parent_does_not_hang() {
        let source = Written::new(&[(
            "models/block/loop.json",
            r##"{"parent": "block/loop", "textures": {"all": "blocks/stone"}}"##,
        )]);
        let mut models = Models::new(Arc::new(source));
        assert!(models.shape("block/loop", (0, 0), false).is_empty());
    }

    #[test]
    fn every_texture_a_shape_needs_can_be_listed() {
        let source = models();
        let mut models = Models::new(Arc::new(source));
        let shape = models.shape("block/half_slab_stone", (0, 0), false);
        let mut names: Vec<&str> = shape.textures().collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names, vec!["blocks/stone_slab_side", "blocks/stone_slab_top"]);
    }
}
