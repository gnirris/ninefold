//! Turning what the server sent into something the GPU can draw.
//!
//! Two passes over a column, and the split is the point. The first works out which block states
//! are in it and bakes any that have not been seen before; the second meshes, with every shape
//! already in hand. Doing it in one pass would mean baking a model in the middle of the inner
//! loop, which is both slow and impossible to borrow.

use ninefold_mesh::Blocks;
use ninefold_mesh::shape::Baked;
use ninefold_model::states::{self, Links};
use ninefold_protocol::play::Session;
use ninefold_render::Vertex;

use crate::atlas::{self, Atlas};
use crate::shapes::Shapes;

/// The decoded world, as the mesher wants to see it.
struct World<'a> {
    session: &'a Session,
    shapes: &'a Shapes,
}

impl World<'_> {
    fn at(&self, x: i32, y: i32, z: i32) -> u16 {
        self.session.block_at(x, y, z).unwrap_or(0)
    }
}

impl Blocks for World<'_> {
    fn block(&self, x: i32, y: i32, z: i32) -> u16 {
        self.at(x, y, z)
    }

    fn see_through(&self, x: i32, y: i32, z: i32) -> bool {
        !self
            .shapes
            .get(self.at(x, y, z), links_of(self.session, (x, y, z)))
            .is_some_and(|baked| baked.solid)
    }

    fn covers(&self, x: i32, y: i32, z: i32, against: u16) -> bool {
        let here = self.at(x, y, z);
        let solid = self
            .shapes
            .get(here, links_of(self.session, (x, y, z)))
            .is_some_and(|baked| baked.solid);
        atlas::covers(here, against, solid)
    }

    fn light(&self, x: i32, y: i32, z: i32) -> u8 {
        self.session.light_at(x, y, z)
    }

    fn baked(&self, x: i32, y: i32, z: i32) -> Option<&Baked> {
        let block = self.at(x, y, z);
        if block >> 4 == 0 {
            return None;
        }
        // A block with no model of its own is drawn as a plain cube rather than not at all, so a
        // gap in the state table is loud instead of invisible.
        self.shapes
            .get(block, links_of(self.session, (x, y, z)))
            .or_else(|| Some(self.shapes.fallback()))
    }
}

/// Which neighbours a fence, pane or wall is joined to.
///
/// Anything you cannot walk through counts, and so does another connecting block — which is what
/// makes a fence meet a wall and a pane meet a block of glass. Asking the neighbour for its own
/// shape would be the exact answer and would also ask the neighbour about *its* neighbours, so
/// this uses the cheap test instead.
#[must_use]
pub fn links_of(session: &Session, at: (i32, i32, i32)) -> Links {
    let (x, y, z) = at;
    let block = session.block_at(x, y, z).unwrap_or(0);
    if !states::connects(block) {
        return Links::default();
    }
    let joins = |dx: i32, dz: i32| {
        let neighbour = session.block_at(x + dx, y, z + dz).unwrap_or(0);
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

/// Mesh one chunk column, with atlas coordinates and biome colours on every corner.
///
/// One column at a time is the point: the server loads and unloads chunks constantly as you
/// walk, and rebuilding the whole world each time is what made this stutter and made terrain
/// blink out while it worked.
#[must_use]
pub fn column(
    session: &Session,
    atlas: &Atlas,
    shapes: &mut Shapes,
    at: (i32, i32),
) -> (Vec<Vertex>, Vec<u32>) {
    // Everything in the column, and a block of margin all round it: culling asks the
    // neighbours whether they are solid, and a neighbour in the next column over has to have
    // been baked for that question to have an answer.
    for x in (at.0 * 16 - 1)..(at.0 * 16 + 17) {
        for z in (at.1 * 16 - 1)..(at.1 * 16 + 17) {
            for y in 0..256 {
                let block = session.block_at(x, y, z).unwrap_or(0);
                if block >> 4 == 0 {
                    continue;
                }
                shapes.prepare(atlas, block, links_of(session, (x, y, z)));
            }
        }
    }

    let world = World { session, shapes };
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for section_y in 0..16 {
        let mesh = ninefold_mesh::section(&world, (at.0 * 16, section_y * 16, at.1 * 16));
        let base = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
        vertices.extend(mesh.vertices.iter().map(|v| {
            // The tint is a number on the vertex; which colour that number means depends on the
            // biome the block is standing in, which is why it cannot be baked into the atlas.
            let tint = atlas::Tint::from_code(v.tint).map_or([1.0, 1.0, 1.0], |tint| {
                atlas.biome_colour(tint, session.biome_at(v.at[0], v.at[2]))
            });
            Vertex {
                position: v.position,
                normal: v.normal,
                uv: v.uv,
                tint,
                light: v.light,
            }
        }));
        indices.extend(mesh.indices.iter().map(|i| i + base));
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_blocks_that_join_up_look_at_their_neighbours() {
        // A stone block is never "joined" to anything, however much stone is round it.
        assert!(!states::connects(1 << 4));
        assert!(states::connects(85 << 4), "a fence is");
    }

    #[test]
    fn a_tint_number_becomes_a_colour_and_no_number_means_no_colour() {
        assert_eq!(atlas::Tint::from_code(0), None);
        assert_eq!(
            atlas::Tint::from_code(atlas::Tint::Grass.code()),
            Some(atlas::Tint::Grass)
        );
    }
}
