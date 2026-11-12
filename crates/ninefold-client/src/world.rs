//! Turning what the server sent into something the GPU can draw.

use ninefold_mesh::{Blocks, Face};
use ninefold_protocol::play::Session;
use ninefold_render::Vertex;

use crate::atlas::Atlas;

/// The decoded world, as the mesher wants to see it.
struct World<'a>(&'a Session);

impl Blocks for World<'_> {
    fn block(&self, x: i32, y: i32, z: i32) -> u16 {
        let block = self.0.block_at(x, y, z).unwrap_or(0);
        // Plants would be cubes of a crossed sprite, which looks worse than nothing.
        if crate::atlas::drawn(block) { block } else { 0 }
    }

    fn see_through(&self, x: i32, y: i32, z: i32) -> bool {
        !crate::atlas::opaque(self.block(x, y, z))
    }

    fn light(&self, x: i32, y: i32, z: i32) -> u8 {
        self.0.light_at(x, y, z)
    }
}

/// Which way a face points, back from its normal. The mesher writes unit normals, so this is
/// exact rather than a nearest match.
fn face_of(normal: [f32; 3]) -> Face {
    match normal {
        [_, y, _] if y > 0.5 => Face::Up,
        [_, y, _] if y < -0.5 => Face::Down,
        [x, _, _] if x > 0.5 => Face::East,
        [x, _, _] if x < -0.5 => Face::West,
        [_, _, z] if z > 0.5 => Face::South,
        _ => Face::North,
    }
}

/// Mesh one chunk column, with atlas coordinates on every corner.
///
/// One column at a time is the point: the server loads and unloads chunks constantly as you
/// walk, and rebuilding the whole world each time is what made this stutter and made terrain
/// blink out while it worked.
#[must_use]
pub fn column(session: &Session, atlas: &Atlas, at: (i32, i32)) -> (Vec<Vertex>, Vec<u32>) {
    let world = World(session);
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for section_y in 0..16 {
        let mesh = ninefold_mesh::section(&world, (at.0 * 16, section_y * 16, at.1 * 16));
        let base = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
        vertices.extend(mesh.vertices.iter().map(|v| Vertex {
            position: v.position,
            normal: v.normal,
            uv: atlas.uv(v.block, face_of(v.normal), v.uv),
            light: v.light,
        }));
        indices.extend(mesh.indices.iter().map(|i| i + base));
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_names_the_face_it_belongs_to() {
        for face in Face::ALL {
            assert_eq!(face_of(face.normal()), face, "{face:?} round trips");
        }
    }
}
