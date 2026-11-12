//! Turning what the server sent into something the GPU can draw.

use ninefold_mesh::{Blocks, Face, Shape};
use ninefold_protocol::play::Session;
use ninefold_render::Vertex;

use crate::atlas::Atlas;

/// The decoded world, as the mesher wants to see it.
struct World<'a>(&'a Session);

impl Blocks for World<'_> {
    fn block(&self, x: i32, y: i32, z: i32) -> u16 {
        self.0.block_at(x, y, z).unwrap_or(0)
    }

    fn see_through(&self, x: i32, y: i32, z: i32) -> bool {
        !crate::atlas::opaque(self.block(x, y, z))
    }

    fn covers(&self, x: i32, y: i32, z: i32, against: u16) -> bool {
        crate::atlas::covers(self.block(x, y, z), against)
    }

    fn light(&self, x: i32, y: i32, z: i32) -> u8 {
        self.0.light_at(x, y, z)
    }

    fn shape(&self, x: i32, y: i32, z: i32) -> Shape {
        crate::atlas::shape_of(self.block(x, y, z))
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

/// How far a grass block's green fringe sits outside the dirt underneath it.
///
/// It has to be something: the overlay is a second quad in exactly the same place as the side it
/// covers, and two quads at the same depth flicker against each other as the camera moves.
const OVERLAY_LIFT: f32 = 0.002;

/// Mesh one chunk column, with atlas coordinates and biome colours on every corner.
///
/// One column at a time is the point: the server loads and unloads chunks constantly as you
/// walk, and rebuilding the whole world each time is what made this stutter and made terrain
/// blink out while it worked.
#[must_use]
pub fn column(session: &Session, atlas: &Atlas, at: (i32, i32)) -> (Vec<Vertex>, Vec<u32>) {
    let world = World(session);
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    // Every quad the mesher emits is four consecutive vertices, so a face can be read back out
    // of the mesh in groups of four — which is what the overlay pass needs.
    for section_y in 0..16 {
        let mesh = ninefold_mesh::section(&world, (at.0 * 16, section_y * 16, at.1 * 16));
        for quad in mesh.vertices.as_chunks::<4>().0 {
            let block = quad[0].block;
            let [bx, _, bz] = quad[0].at;
            let face = face_of(quad[0].normal);
            let tint = atlas
                .tint_of(block)
                .map_or([1.0, 1.0, 1.0], |tint| {
                    atlas.biome_colour(tint, session.biome_at(bx, bz))
                });
            push(&mut vertices, &mut indices, quad.map(|v| Vertex {
                position: v.position,
                normal: v.normal,
                uv: atlas.uv(block, face, v.uv),
                tint,
                light: v.light,
            }));

            // Grass is dirt with a green edge laid over its sides, which is two quads in
            // vanilla as well: the fringe has to be biome-coloured while the dirt must not be.
            if matches!(
                face,
                Face::North | Face::South | Face::East | Face::West
            ) && let Some((tile, kind)) = atlas.overlay_of(block)
            {
                let colour = atlas.biome_colour(kind, session.biome_at(bx, bz));
                let normal = quad[0].normal;
                push(&mut vertices, &mut indices, quad.map(|v| Vertex {
                    position: [
                        v.position[0] + normal[0] * OVERLAY_LIFT,
                        v.position[1] + normal[1] * OVERLAY_LIFT,
                        v.position[2] + normal[2] * OVERLAY_LIFT,
                    ],
                    normal,
                    uv: Atlas::uv_in(tile, v.uv),
                    tint: colour,
                    light: v.light,
                }));
            }
        }
    }
    (vertices, indices)
}

/// Add one quad and the two triangles that draw it.
///
/// The winding matches the mesher's: reverse it and every face is culled.
fn push(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, quad: [Vertex; 4]) {
    let first = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
    vertices.extend_from_slice(&quad);
    indices.extend_from_slice(&[first, first + 2, first + 1, first, first + 3, first + 2]);
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

    #[test]
    fn a_quads_triangles_face_the_way_the_quad_does() {
        // The same check the mesher makes, on the indices this module writes rather than the
        // ones it writes: they have to agree or every face is culled.
        let corner = |position: [f32; 3]| Vertex::plain(position, [0.0, 0.0, -1.0], [0.0, 0.0], 1.0);
        // The corner order the mesher emits for a north-facing quad, which is the order the
        // index list below is written for.
        let quad = [
            corner([0.0, 0.0, 0.0]),
            corner([1.0, 0.0, 0.0]),
            corner([1.0, 1.0, 0.0]),
            corner([0.0, 1.0, 0.0]),
        ];
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        push(&mut vertices, &mut indices, quad);
        assert_eq!(indices.len(), 6);
        for triangle in indices.as_chunks::<3>().0 {
            let v: Vec<_> = triangle.iter().map(|i| vertices[*i as usize]).collect();
            let edge = |a: usize, b: usize| {
                [
                    v[a].position[0] - v[b].position[0],
                    v[a].position[1] - v[b].position[1],
                    v[a].position[2] - v[b].position[2],
                ]
            };
            let (u, w) = (edge(1, 0), edge(2, 0));
            let wound = [
                u[1] * w[2] - u[2] * w[1],
                u[2] * w[0] - u[0] * w[2],
                u[0] * w[1] - u[1] * w[0],
            ];
            let n = v[0].normal;
            let dot = wound[0] * n[0] + wound[1] * n[1] + wound[2] * n[2];
            assert!(dot > 0.0, "wound {wound:?} against normal {n:?}");
        }
    }
}
