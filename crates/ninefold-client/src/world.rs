//! Turning what the server sent into something the GPU can draw.

use std::collections::HashMap;

use ninefold_assets::AssetSource;
use ninefold_mesh::Blocks;
use ninefold_protocol::play::Session;
use ninefold_render::Vertex;

/// The blocks a flat world is made of, and the texture that stands for each.
///
/// A real block table comes with the rest of the renderer; this is enough to tell grass from
/// dirt from bedrock, which is what a first window needs to prove.
/// Grass and leaves ship greyscale and are tinted by biome at draw time — without this the
/// world comes out grey, which looks like a renderer bug and is not one. These are the plains
/// colours; a biome lookup replaces them when biomes are read out of the chunk data.
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

const TEXTURES: [(u16, &str); 10] = [
    (1, "blocks/stone"),
    (2, "blocks/grass_top"),
    (3, "blocks/dirt"),
    (4, "blocks/cobblestone"),
    (5, "blocks/planks_oak"),
    (7, "blocks/bedrock"),
    (12, "blocks/sand"),
    (13, "blocks/gravel"),
    (17, "blocks/log_oak"),
    (18, "blocks/leaves_oak"),
];

/// One colour per block id, averaged from the real texture so the world looks roughly right
/// before there is a texture atlas to sample properly.
pub struct Palette {
    colours: HashMap<u16, [f32; 3]>,
    unknown: [f32; 3],
}

impl Palette {
    #[must_use]
    pub fn from_assets(assets: &dyn AssetSource) -> Self {
        let mut colours = HashMap::new();
        for (id, name) in TEXTURES {
            let Some(mut colour) = assets.texture(name).map(|t| average(&t)) else {
                continue;
            };
            let tint = match id {
                2 => Some(GRASS_TINT),
                18 => Some(FOLIAGE_TINT),
                _ => None,
            };
            if let Some(tint) = tint {
                for (channel, t) in colour.iter_mut().zip(tint) {
                    *channel *= t;
                }
            }
            colours.insert(id, colour);
        }
        Self {
            colours,
            unknown: [0.8, 0.3, 0.8], // loud pink, so a missing block is obvious
        }
    }

    #[must_use]
    pub fn colour(&self, block: u16) -> [f32; 3] {
        *self.colours.get(&(block >> 4)).unwrap_or(&self.unknown)
    }
}

/// The average colour of a texture, ignoring fully transparent pixels.
fn average(texture: &ninefold_assets::Texture) -> [f32; 3] {
    let mut total = [0.0f64; 3];
    let mut counted = 0u32;
    for pixel in texture.rgba.as_chunks::<4>().0 {
        if pixel[3] < 8 {
            continue;
        }
        for (slot, channel) in total.iter_mut().zip(&pixel[..3]) {
            *slot += f64::from(*channel) / 255.0;
        }
        counted += 1;
    }
    if counted == 0 {
        return [1.0, 0.0, 1.0];
    }
    let n = f64::from(counted);
    [
        (total[0] / n) as f32,
        (total[1] / n) as f32,
        (total[2] / n) as f32,
    ]
}

/// The decoded world, as the mesher wants to see it.
struct World<'a>(&'a Session);

impl Blocks for World<'_> {
    fn block(&self, x: i32, y: i32, z: i32) -> u16 {
        self.0.block_at(x, y, z).unwrap_or(0)
    }
}

/// Mesh every chunk the session knows about.
#[must_use]
pub fn geometry(session: &Session, palette: &Palette) -> (Vec<Vertex>, Vec<u32>) {
    let world = World(session);
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (cx, cz) in session.columns.keys().copied() {
        for section_y in 0..16 {
            let mesh = ninefold_mesh::section(&world, (cx * 16, section_y * 16, cz * 16));
            let base = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
            vertices.extend(mesh.vertices.iter().map(|v| Vertex {
                position: v.position,
                normal: v.normal,
                colour: palette.colour(v.block),
            }));
            indices.extend(mesh.indices.iter().map(|i| i + base));
        }
    }
    (vertices, indices)
}
