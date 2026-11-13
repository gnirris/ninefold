//! Turning what the server sent into something the GPU can draw.
//!
//! The shape of this file is dictated by one number: meshing a column happens on the frame
//! thread, and a frame at 120 is eight milliseconds long. Asking the session for a block means a
//! hash of its chunk coordinate, a lookup, a bounds check and an `Option`; doing that for every
//! block, six times over for its neighbours, and again for every face, costs twenty-odd
//! milliseconds a column and turns chunk loading into a stutter.
//!
//! So the neighbourhood is flattened first. The nine columns involved are fetched once, their
//! blocks copied into one array indexed by arithmetic, and each cell's baked shape resolved into
//! a parallel array. After that the mesher does no hashing at all.

use std::collections::HashMap;
use std::sync::Arc;

use ninefold_mesh::Blocks;
use ninefold_mesh::shape::Baked;
use ninefold_model::states::{self, Links};
use ninefold_protocol::play::Session;
use ninefold_render::Vertex;

use crate::atlas::{self, Atlas};
use crate::shapes::Shapes;

/// A column is sixteen blocks across, and the mesher needs one block of margin all round it to
/// know what its edge faces are up against.
const SPAN: usize = 18;
const HEIGHT: usize = 256;

/// The blocks around one column, flattened.
struct Near {
    /// `id << 4 | metadata`, indexed x fastest, then z, then y.
    blocks: Vec<u16>,
    /// The baked shape of each cell, already resolved. `None` is air or something undrawable.
    shapes: Vec<Option<Arc<Baked>>>,
    /// How bright each cell is, 0..15.
    ///
    /// Flattened for the same reason the blocks are, and more urgently: smooth lighting asks
    /// about four neighbours per corner and four corners per face, so a single face costs
    /// nearly thirty of these. Through a hash map that is most of the meshing time.
    light: Vec<u8>,
    /// World coordinates of index zero.
    base: (i32, i32),
}

/// Where a world position lives in the flat arrays, if it is inside the window at all.
fn index_of(base: (i32, i32), x: i32, y: i32, z: i32) -> Option<usize> {
    if !(0..256).contains(&y) {
        return None;
    }
    let dx = usize::try_from(x - base.0).ok()?;
    let dz = usize::try_from(z - base.1).ok()?;
    if dx >= SPAN || dz >= SPAN {
        return None;
    }
    Some((y as usize * SPAN + dz) * SPAN + dx)
}

impl Near {
    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        index_of(self.base, x, y, z)
    }
}

impl Blocks for Near {
    fn block(&self, x: i32, y: i32, z: i32) -> u16 {
        self.index(x, y, z).map_or(0, |at| self.blocks[at])
    }

    fn see_through(&self, x: i32, y: i32, z: i32) -> bool {
        !self
            .index(x, y, z)
            .and_then(|at| self.shapes[at].as_deref())
            .is_some_and(|baked| baked.solid)
    }

    fn covers(&self, x: i32, y: i32, z: i32, against: u16) -> bool {
        let Some(at) = self.index(x, y, z) else {
            // Outside the window there is nothing to judge by, and hiding a face against the
            // unknown would punch a hole in the edge of every column.
            return false;
        };
        let solid = self.shapes[at].as_deref().is_some_and(|baked| baked.solid);
        atlas::covers(self.blocks[at], against, solid)
    }

    fn light(&self, x: i32, y: i32, z: i32) -> u8 {
        // Full daylight outside the window, which is what the sky is and what the edge of a
        // column is up against.
        self.index(x, y, z).map_or(15, |at| self.light[at])
    }

    fn baked(&self, x: i32, y: i32, z: i32) -> Option<&Baked> {
        self.index(x, y, z).and_then(|at| self.shapes[at].as_deref())
    }
}

/// Which neighbours a fence, pane or wall is joined to, read out of the flat array.
///
/// Anything you cannot walk through counts, and so does another connecting block — which is what
/// makes a fence meet a wall and a pane meet a block of glass.
fn links_in(blocks: &[u16], base: (i32, i32), x: i32, y: i32, z: i32) -> Links {
    let at = |dx: i32, dz: i32| -> u16 {
        let (ix, iz) = (x + dx - base.0, z + dz - base.1);
        if !(0..SPAN as i32).contains(&ix) || !(0..SPAN as i32).contains(&iz) {
            return 0;
        }
        blocks[(y as usize * SPAN + iz as usize) * SPAN + ix as usize]
    };
    let joins = |dx: i32, dz: i32| {
        let neighbour = at(dx, dz);
        neighbour >> 4 != 0
            && (ninefold_protocol::blocks::solid(neighbour) || states::connects(neighbour))
    };
    Links {
        north: joins(0, -1),
        south: joins(0, 1),
        west: joins(-1, 0),
        east: joins(1, 0),
    }
}

/// Copy the nine columns around `at` into one flat array.
///
/// Nine hash lookups rather than eighty thousand: within a column the blocks are already a flat
/// array, so this is arithmetic and a copy.
fn snapshot(session: &Session, at: (i32, i32)) -> (Vec<u16>, Vec<u8>, (i32, i32)) {
    let base = (at.0 * 16 - 1, at.1 * 16 - 1);
    let mut blocks = vec![0u16; SPAN * SPAN * HEIGHT];
    // Daylight where nothing is known, so an unloaded neighbour does not cast a shadow.
    let mut light = vec![15u8; SPAN * SPAN * HEIGHT];
    let mut columns: HashMap<(i32, i32), &ninefold_protocol::chunk::Column> = HashMap::new();
    for cx in (at.0 - 1)..=(at.0 + 1) {
        for cz in (at.1 - 1)..=(at.1 + 1) {
            if let Some(column) = session.columns.get(&(cx, cz)) {
                columns.insert((cx, cz), column);
            }
        }
    }
    for dz in 0..SPAN {
        for dx in 0..SPAN {
            let (x, z) = (base.0 + dx as i32, base.1 + dz as i32);
            let Some(column) = columns.get(&(x.div_euclid(16), z.div_euclid(16))) else {
                continue;
            };
            let (ix, iz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
            // Section by section rather than block by block: most of a column is sky, and an
            // absent section is skipped whole instead of asked about sixteen times.
            for (section_y, section) in column.sections.iter().enumerate() {
                let Some(section) = section else {
                    continue;
                };
                for dy in 0..16 {
                    let at = ((section_y * 16 + dy) * SPAN + dz) * SPAN + dx;
                    let block = section.at(ix, dy, iz);
                    if block >> 4 != 0 {
                        blocks[at] = block;
                    }
                    light[at] = section.light(ix, dy, iz);
                }
            }
        }
    }
    (blocks, light, base)
}

/// Everything the meshing thread needs for one column, and nothing it cannot be sent.
///
/// Gathering this is the part that has to happen on the frame thread, because it reads the
/// session and bakes models; it costs about a millisecond. The meshing itself is four or five,
/// and happens somewhere else.
pub struct Job {
    pub at: (i32, i32),
    base: (i32, i32),
    blocks: Vec<u16>,
    light: Vec<u8>,
    shapes: Vec<Option<Arc<Baked>>>,
    /// The biome at each column of the window, for tinting.
    biomes: Vec<u8>,
    atlas: Arc<Atlas>,
}

/// A column that has been meshed.
pub struct Finished {
    pub at: (i32, i32),
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

/// Gather what one column needs, on the thread that can read the session.
#[must_use]
pub fn prepare(
    session: &Session,
    atlas: &Arc<Atlas>,
    shapes: &mut Shapes,
    at: (i32, i32),
) -> Job {
    let (blocks, light, base) = snapshot(session, at);

    // Every distinct block state in the neighbourhood, baked once. A column has a few dozen,
    // not eighty thousand.
    let mut states: Vec<(u16, u8)> = Vec::new();
    let mut index_of_state: HashMap<(u16, u8), usize> = HashMap::new();
    let mut which: Vec<u16> = vec![u16::MAX; blocks.len()];
    // Terrain comes in long runs of the same block, so remembering the last answer saves most
    // of the lookups outright.
    let mut last: Option<((u16, u8), usize)> = None;
    for (index, block) in blocks.iter().enumerate() {
        if block >> 4 == 0 {
            continue;
        }
        let links = if states::connects(*block) {
            let (dx, dz, y) = (index % SPAN, (index / SPAN) % SPAN, index / (SPAN * SPAN));
            links_in(&blocks, base, base.0 + dx as i32, y as i32, base.1 + dz as i32)
        } else {
            Links::default()
        };
        let key = (*block, links.bits());
        let at = match last {
            Some((seen, at)) if seen == key => at,
            _ => {
                let at = *index_of_state.entry(key).or_insert_with(|| {
                    states.push(key);
                    states.len() - 1
                });
                last = Some((key, at));
                at
            }
        };
        which[index] = u16::try_from(at).unwrap_or(u16::MAX);
    }
    for (block, bits) in &states {
        shapes.prepare(atlas, *block, Links::from_bits(*bits));
    }
    let resolved: Vec<Option<Arc<Baked>>> = states
        .iter()
        .map(|(block, bits)| {
            shapes
                .shared(*block, Links::from_bits(*bits))
                .or_else(|| Some(shapes.fallback_shared()))
        })
        .collect();
    let cells: Vec<Option<Arc<Baked>>> = which
        .iter()
        .map(|at| {
            if *at == u16::MAX {
                None
            } else {
                resolved[*at as usize].clone()
            }
        })
        .collect();

    let mut biomes = vec![1u8; SPAN * SPAN];
    for dz in 0..SPAN {
        for dx in 0..SPAN {
            biomes[dz * SPAN + dx] =
                session.biome_at(base.0 + dx as i32, base.1 + dz as i32);
        }
    }

    Job {
        at,
        base,
        blocks,
        light,
        shapes: cells,
        biomes,
        atlas: atlas.clone(),
    }
}

/// Mesh a column. Nothing here touches the session, so it runs anywhere.
#[must_use]
pub fn build(job: Job) -> Finished {
    let Job {
        at,
        base,
        blocks,
        light,
        shapes,
        biomes,
        atlas,
    } = job;
    let near = Near {
        blocks,
        shapes,
        light,
        base,
    };
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for section_y in 0..16 {
        let mesh = ninefold_mesh::section(&near, (at.0 * 16, section_y * 16, at.1 * 16));
        let base_vertex = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
        vertices.extend(mesh.vertices.iter().map(|v| {
            // The tint is a number on the vertex; which colour that number means depends on the
            // biome the block is standing in, which is why it cannot be baked into the atlas.
            let tint = atlas::Tint::from_code(v.tint).map_or([1.0, 1.0, 1.0], |tint| {
                let (dx, dz) = (v.at[0] - base.0, v.at[2] - base.1);
                let biome = usize::try_from(dz)
                    .ok()
                    .zip(usize::try_from(dx).ok())
                    .and_then(|(dz, dx)| biomes.get(dz * SPAN + dx))
                    .copied()
                    .unwrap_or(1);
                atlas.biome_colour(tint, biome)
            });
            Vertex {
                position: v.position,
                normal: v.normal,
                uv: v.uv,
                tint,
                light: v.light,
            }
        }));
        indices.extend(mesh.indices.iter().map(|i| i + base_vertex));
    }
    Finished {
        at,
        vertices,
        indices,
    }
}

/// The world as the physics walks into it.
///
/// The same shapes the renderer draws: a slab you stand on half way up, a snow layer you walk
/// over, a fence you cannot jump. Anything whose shape has not been baked yet falls back to the
/// old answer — a whole cube if it is solid — because the alternative is falling through a chunk
/// the mesher has not reached.
pub struct Collide<'a> {
    pub session: &'a Session,
    pub shapes: &'a Shapes,
}

impl Collide<'_> {
    /// What a block is joined to, for the handful of blocks that care.
    fn links(&self, x: i32, y: i32, z: i32) -> Links {
        let block = self.session.block_at(x, y, z).unwrap_or(0);
        if !states::connects(block) {
            return Links::default();
        }
        let joins = |dx: i32, dz: i32| {
            let neighbour = self.session.block_at(x + dx, y, z + dz).unwrap_or(0);
            neighbour >> 4 != 0
                && (ninefold_protocol::blocks::solid(neighbour) || states::connects(neighbour))
        };
        Links {
            north: joins(0, -1),
            south: joins(0, 1),
            west: joins(-1, 0),
            east: joins(1, 0),
        }
    }
}

impl ninefold_protocol::physics::World for Collide<'_> {
    fn solid(&self, x: i32, y: i32, z: i32) -> bool {
        self.session
            .block_at(x, y, z)
            .is_some_and(ninefold_protocol::blocks::solid)
    }

    fn boxes(&self, x: i32, y: i32, z: i32) -> &[([f32; 3], [f32; 3])] {
        let block = self.session.block_at(x, y, z).unwrap_or(0);
        match self.shapes.get(block, self.links(x, y, z)) {
            Some(baked) => &baked.collision,
            None if ninefold_protocol::blocks::solid(block) => {
                ninefold_protocol::physics::FULL_CUBE
            }
            None => &[],
        }
    }

    fn fluid(&self, x: i32, y: i32, z: i32) -> ninefold_protocol::blocks::Fluid {
        self.session.block_at(x, y, z).map_or(
            ninefold_protocol::blocks::Fluid::None,
            ninefold_protocol::blocks::fluid,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_blocks_that_join_up_look_at_their_neighbours() {
        assert!(!states::connects(1 << 4), "stone joins nothing");
        assert!(states::connects(85 << 4), "a fence does");
    }

    #[test]
    fn a_tint_number_becomes_a_colour_and_no_number_means_no_colour() {
        assert_eq!(atlas::Tint::from_code(0), None);
        assert_eq!(
            atlas::Tint::from_code(atlas::Tint::Grass.code()),
            Some(atlas::Tint::Grass)
        );
    }

    #[test]
    fn the_flat_window_covers_the_column_and_one_block_round_it() {
        // A column's own sixteen, plus a block of margin on each side, which is what the mesher
        // needs to know whether an edge face is up against anything.
        assert_eq!(SPAN, 18);
        let base = (-1, -1);
        assert!(index_of(base, -1, 0, -1).is_some(), "the margin is inside");
        assert!(index_of(base, 16, 0, 16).is_some(), "and the far margin");
        assert!(index_of(base, -2, 0, 0).is_none(), "one further out is not");
        assert!(index_of(base, 0, 256, 0).is_none(), "nor is the sky");
        assert!(index_of(base, 0, -1, 0).is_none(), "nor below the world");
        // And no two cells share an index.
        let mut seen = std::collections::HashSet::new();
        for y in [0, 1, 255] {
            for z in -1..17 {
                for x in -1..17 {
                    assert!(seen.insert(index_of(base, x, y, z).expect("inside")));
                }
            }
        }
    }
}
