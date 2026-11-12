//! Chunks into triangles.
//!
//! The only interesting decision here is which faces to leave out. A 16×16×16 section holds 4096
//! blocks and 24576 faces, almost all of them pressed against another block where nobody can see
//! them. Emitting a face only where its neighbour is see-through takes a flat world from tens of
//! thousands of faces to a few hundred, and that difference is the whole reason a chunk renderer
//! is possible at all.
//!
//! This is deliberately the simple version: one quad per visible face, no merging of neighbours
//! into larger quads. Greedy meshing comes later, once there is something on screen to compare
//! against.

/// Which way a face points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Face {
    pub const ALL: [Face; 6] = [
        Face::Down,
        Face::Up,
        Face::North,
        Face::South,
        Face::West,
        Face::East,
    ];

    /// The neighbour this face looks at.
    #[must_use]
    pub const fn offset(self) -> (i32, i32, i32) {
        match self {
            Face::Down => (0, -1, 0),
            Face::Up => (0, 1, 0),
            Face::North => (0, 0, -1),
            Face::South => (0, 0, 1),
            Face::West => (-1, 0, 0),
            Face::East => (1, 0, 0),
        }
    }

    /// Unit normal, for lighting.
    #[must_use]
    pub const fn normal(self) -> [f32; 3] {
        let (x, y, z) = self.offset();
        [x as f32, y as f32, z as f32]
    }
}

/// One corner of a face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Where in the texture, 0..1 on each axis.
    pub uv: [f32; 2],
    /// `id << 4 | metadata`, so the renderer can look up which texture to use.
    pub block: u16,
}

/// Geometry for one chunk section.
#[derive(Debug, Default, Clone)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    /// Three per triangle, two triangles per face.
    pub indices: Vec<u32>,
}

impl Mesh {
    #[must_use]
    pub fn faces(&self) -> usize {
        self.indices.len() / 6
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// Somewhere to ask what block is where. Coordinates are absolute, so a mesher can see into the
/// chunk next door and not draw a wall between them.
pub trait Blocks {
    /// `id << 4 | metadata`; 0 is air.
    fn block(&self, x: i32, y: i32, z: i32) -> u16;

    /// Can you see through it? Air for now; glass, water and leaves need a block table.
    fn see_through(&self, x: i32, y: i32, z: i32) -> bool {
        self.block(x, y, z) >> 4 == 0
    }
}

/// Build the geometry for the 16×16×16 section whose corner is at `origin`.
#[must_use]
pub fn section(world: &impl Blocks, origin: (i32, i32, i32)) -> Mesh {
    let mut mesh = Mesh::default();
    let (ox, oy, oz) = origin;
    for dy in 0..16 {
        for dz in 0..16 {
            for dx in 0..16 {
                let (x, y, z) = (ox + dx, oy + dy, oz + dz);
                let block = world.block(x, y, z);
                if block >> 4 == 0 {
                    continue;
                }
                for face in Face::ALL {
                    let (nx, ny, nz) = face.offset();
                    // A face is only worth drawing if you could see through what is next to it.
                    if world.see_through(x + nx, y + ny, z + nz) {
                        push_face(&mut mesh, (x, y, z), face, block);
                    }
                }
            }
        }
    }
    mesh
}

/// Add one quad, as two triangles.
fn push_face(mesh: &mut Mesh, at: (i32, i32, i32), face: Face, block: u16) {
    let (x, y, z) = (at.0 as f32, at.1 as f32, at.2 as f32);
    // Corners, wound anticlockwise seen from outside, so back-face culling keeps the right ones.
    let corners: [[f32; 3]; 4] = match face {
        Face::Down => [
            [x, y, z],
            [x, y, z + 1.0],
            [x + 1.0, y, z + 1.0],
            [x + 1.0, y, z],
        ],
        Face::Up => [
            [x, y + 1.0, z],
            [x + 1.0, y + 1.0, z],
            [x + 1.0, y + 1.0, z + 1.0],
            [x, y + 1.0, z + 1.0],
        ],
        Face::North => [
            [x, y, z],
            [x + 1.0, y, z],
            [x + 1.0, y + 1.0, z],
            [x, y + 1.0, z],
        ],
        Face::South => [
            [x, y, z + 1.0],
            [x, y + 1.0, z + 1.0],
            [x + 1.0, y + 1.0, z + 1.0],
            [x + 1.0, y, z + 1.0],
        ],
        Face::West => [
            [x, y, z],
            [x, y + 1.0, z],
            [x, y + 1.0, z + 1.0],
            [x, y, z + 1.0],
        ],
        Face::East => [
            [x + 1.0, y, z],
            [x + 1.0, y, z + 1.0],
            [x + 1.0, y + 1.0, z + 1.0],
            [x + 1.0, y + 1.0, z],
        ],
    };
    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let first = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
    for (corner, uv) in corners.iter().zip(uvs) {
        mesh.vertices.push(Vertex {
            position: *corner,
            normal: face.normal(),
            uv,
            block,
        });
    }
    mesh.indices
        .extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing anywhere.
    struct Void;
    impl Blocks for Void {
        fn block(&self, _x: i32, _y: i32, _z: i32) -> u16 {
            0
        }
    }

    /// One stone block at the given spot.
    struct OneBlock(i32, i32, i32);
    impl Blocks for OneBlock {
        fn block(&self, x: i32, y: i32, z: i32) -> u16 {
            if (x, y, z) == (self.0, self.1, self.2) {
                1 << 4
            } else {
                0
            }
        }
    }

    /// Ground everywhere below `top`, like a flat world, going on forever sideways.
    struct Ground {
        top: i32,
    }
    impl Blocks for Ground {
        fn block(&self, _x: i32, y: i32, _z: i32) -> u16 {
            if y < self.top { 1 << 4 } else { 0 }
        }
    }

    #[test]
    fn empty_space_makes_no_geometry() {
        let mesh = section(&Void, (0, 0, 0));
        assert!(mesh.is_empty());
        assert_eq!(mesh.faces(), 0);
    }

    #[test]
    fn a_lone_block_has_all_six_faces() {
        let mesh = section(&OneBlock(3, 4, 5), (0, 0, 0));
        assert_eq!(mesh.faces(), 6);
        assert_eq!(mesh.vertices.len(), 24, "four corners a face");
        assert_eq!(mesh.indices.len(), 36, "six indices a face");
    }

    #[test]
    fn two_blocks_side_by_side_hide_the_faces_between_them() {
        struct Pair;
        impl Blocks for Pair {
            fn block(&self, x: i32, y: i32, z: i32) -> u16 {
                if y == 0 && z == 0 && (x == 0 || x == 1) {
                    1 << 4
                } else {
                    0
                }
            }
        }
        // Twelve faces in total, minus the two touching each other.
        assert_eq!(section(&Pair, (0, 0, 0)).faces(), 10);
    }

    #[test]
    fn a_buried_block_is_not_drawn_at_all() {
        // A section cut out of the middle of solid ground: every face has a neighbour.
        let mesh = section(&Ground { top: 1000 }, (0, 0, 0));
        assert_eq!(mesh.faces(), 0, "nothing is visible inside solid rock");
    }

    #[test]
    fn flat_ground_shows_only_its_surface() {
        // Ground up to y=4, so the section at y=0 has its top layer exposed and nothing else:
        // the sides continue into the neighbouring chunks, which the mesher can see.
        let mesh = section(&Ground { top: 4 }, (0, 0, 0));
        assert_eq!(mesh.faces(), 16 * 16, "one face per column of the surface");
        assert!(
            mesh.vertices.iter().all(|v| v.normal == [0.0, 1.0, 0.0]),
            "all of them pointing up"
        );
        assert!(
            mesh.vertices.iter().all(|v| v.position[1] == 4.0),
            "all at the top of the last solid block"
        );
    }

    #[test]
    fn a_face_is_a_unit_square_in_the_right_place() {
        let mesh = section(&OneBlock(0, 0, 0), (0, 0, 0));
        let top: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| v.normal == [0.0, 1.0, 0.0])
            .collect();
        assert_eq!(top.len(), 4);
        for v in &top {
            assert_eq!(v.position[1], 1.0, "the top of a block at y=0 is y=1");
            assert!((0.0..=1.0).contains(&v.position[0]));
            assert!((0.0..=1.0).contains(&v.position[2]));
        }
        // The four corners are all different.
        let mut corners: Vec<_> = top.iter().map(|v| [v.position[0], v.position[2]]).collect();
        corners.dedup();
        assert_eq!(corners.len(), 4);
    }

    #[test]
    fn metadata_rides_along_so_the_renderer_can_pick_a_texture() {
        struct Wool;
        impl Blocks for Wool {
            fn block(&self, x: i32, _y: i32, _z: i32) -> u16 {
                if x == 0 { (35 << 4) | 14 } else { 0 }
            }
        }
        let mesh = section(&Wool, (0, 0, 0));
        assert!(mesh.vertices.iter().all(|v| v.block >> 4 == 35));
        assert!(mesh.vertices.iter().all(|v| v.block & 0xF == 14));
    }
}
