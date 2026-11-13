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

pub mod shape;

pub use shape::{Baked, Quad};

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

    /// Two directions across the face, used to find the blocks meeting at each corner.
    #[must_use]
    pub const fn corner_axes(self) -> ((i32, i32, i32), (i32, i32, i32)) {
        match self {
            Face::Down | Face::Up => ((1, 0, 0), (0, 0, 1)),
            Face::North | Face::South => ((1, 0, 0), (0, 1, 0)),
            Face::West | Face::East => ((0, 0, 1), (0, 1, 0)),
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
    /// Which of the block colour maps applies to this corner, 0 for none. The texture is
    /// already resolved into the uv, so the block id is no longer needed here.
    pub tint: u8,
    /// Which block this corner belongs to. The position alone cannot say: a corner at x=3 is
    /// shared by the blocks at 2 and 3, and biome tinting has to know which one it came from.
    pub at: [i32; 3],
    /// How lit this face is, 0..1, taken from the block in front of it — which is the one the
    /// light actually reaches, not the solid block the face belongs to.
    pub light: f32,
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

    /// Does the block here hide the face of `against` that looks at it?
    ///
    /// Not the same question as "can you see through it", and the difference is where every
    /// chunk renderer's worst-looking bug lives. Two leaf blocks are both see-through, and
    /// hiding the faces between them turns a canopy into a checkerboard you can see the sky
    /// through. Two water blocks are also both see-through, and *not* hiding the faces between
    /// them fills an ocean with invisible geometry. Only the block being asked about knows.
    fn covers(&self, x: i32, y: i32, z: i32, _against: u16) -> bool {
        !self.see_through(x, y, z)
    }

    /// How bright it is here, 0..15. Flat daylight unless something knows better.
    fn light(&self, _x: i32, _y: i32, _z: i32) -> u8 {
        15
    }

    /// What this block is made of, or `None` for nothing to draw.
    ///
    /// Baked once per block state by the caller and handed back by reference, because this is
    /// asked four thousand times a section.
    fn baked(&self, _x: i32, _y: i32, _z: i32) -> Option<&Baked> {
        None
    }
}

/// The four blocks that meet at one corner of a face, on the lit side.
///
/// Minecraft's smooth lighting averages those four, and darkens the corner where they are
/// solid — which is what gives terrain its soft shading in the crevices. Flat per-face light
/// makes a cave look like a box of identical grey walls.
fn corner_light(
    world: &impl Blocks,
    front: (i32, i32, i32),
    along: (i32, i32, i32),
    across: (i32, i32, i32),
) -> f32 {
    let at = |(x, y, z): (i32, i32, i32)| (x, y, z);
    let side_a = at((front.0 + along.0, front.1 + along.1, front.2 + along.2));
    let side_b = at((front.0 + across.0, front.1 + across.1, front.2 + across.2));
    let diagonal = at((
        front.0 + along.0 + across.0,
        front.1 + along.1 + across.1,
        front.2 + along.2 + across.2,
    ));

    let solid = |(x, y, z): (i32, i32, i32)| !world.see_through(x, y, z);
    let light = |(x, y, z): (i32, i32, i32)| f32::from(world.light(x, y, z)) / 15.0;

    // Two solid sides close the corner off: the diagonal cannot contribute, however bright it
    // is, because no light reaches round two walls.
    let blocked = if solid(side_a) && solid(side_b) {
        3.0
    } else {
        f32::from(solid(side_a)) + f32::from(solid(side_b)) + f32::from(solid(diagonal))
    };
    let mut total = light(front);
    let mut counted = 1.0;
    for neighbour in [side_a, side_b, diagonal] {
        if !solid(neighbour) {
            total += light(neighbour);
            counted += 1.0;
        }
    }
    let average = total / counted;
    // Ambient occlusion: a corner with things crowding it is darker.
    average * (1.0 - 0.22 * blocked / 3.0)
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
                let Some(baked) = world.baked(x, y, z) else {
                    continue;
                };
                for quad in &baked.quads {
                    // A face is skipped only if it says which neighbour hides it and that
                    // neighbour does. Everything else is drawn: a fence post's own sides have no
                    // cullface and are visible from inside the block.
                    if let Some(side) = quad.cull {
                        let (nx, ny, nz) = side.offset();
                        if world.covers(x + nx, y + ny, z + nz, block) {
                            continue;
                        }
                    }
                    stamp(&mut mesh, world, (x, y, z), quad, baked.ambient_occlusion);
                }
            }
        }
    }
    mesh
}

/// Put one of a block's quads into the mesh at a block position.
fn stamp(
    mesh: &mut Mesh,
    world: &impl Blocks,
    at: (i32, i32, i32),
    quad: &Quad,
    ambient_occlusion: bool,
) {
    let (x, y, z) = at;
    let face = face_of(quad.normal);
    // Light comes from the air in front of a face, not from inside the solid block it belongs
    // to — but only a face on the edge of its block has air in front of it. One inside the
    // block, like a fence post's side, takes the block's own light instead.
    let front = if quad.boundary {
        let (nx, ny, nz) = face.offset();
        (x + nx, y + ny, z + nz)
    } else {
        (x, y, z)
    };
    let corners = if ambient_occlusion && quad.boundary {
        let (along, across) = face.corner_axes();
        [
            corner_light(world, front, neg(along), neg(across)),
            corner_light(world, front, along, neg(across)),
            corner_light(world, front, along, across),
            corner_light(world, front, neg(along), across),
        ]
    } else {
        let flat = f32::from(world.light(front.0, front.1, front.2)) / 15.0;
        [flat; 4]
    };
    // A face that is not shaded by its direction is drawn at full brightness, as a torch is.
    let normal = if quad.shade {
        quad.normal
    } else {
        [0.0, 1.0, 0.0]
    };

    let first = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
    for ((corner, uv), light) in quad.corners.iter().zip(quad.uv).zip(corners) {
        mesh.vertices.push(Vertex {
            position: [
                x as f32 + corner[0],
                y as f32 + corner[1],
                z as f32 + corner[2],
            ],
            normal,
            uv,
            tint: quad.tint,
            at: [x, y, z],
            light,
        });
    }
    mesh.indices
        .extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
}

/// Which way a face points, from its normal.
#[must_use]
pub fn face_of(normal: [f32; 3]) -> Face {
    match normal {
        [_, y, _] if y > 0.5 => Face::Up,
        [_, y, _] if y < -0.5 => Face::Down,
        [x, _, _] if x > 0.5 => Face::East,
        [x, _, _] if x < -0.5 => Face::West,
        [_, _, z] if z > 0.5 => Face::South,
        _ => Face::North,
    }
}

const fn neg((x, y, z): (i32, i32, i32)) -> (i32, i32, i32) {
    (-x, -y, -z)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The corners of one face of a unit cube, in the order a baker should produce them:
    /// bottom left first, as seen from outside.
    fn face_corners(side: Face) -> [[f32; 3]; 4] {
        match side {
            Face::North => [
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            Face::South => [
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 1.0],
                [1.0, 1.0, 1.0],
                [0.0, 1.0, 1.0],
            ],
            Face::West => [
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 1.0, 1.0],
                [0.0, 1.0, 0.0],
            ],
            Face::East => [
                [1.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [1.0, 1.0, 1.0],
            ],
            Face::Up => [
                [0.0, 1.0, 1.0],
                [1.0, 1.0, 1.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            Face::Down => [
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 0.0, 1.0],
                [0.0, 0.0, 1.0],
            ],
        }
    }

    /// A full cube, every face culled by its own neighbour.
    fn cube() -> Baked {
        let quads = Face::ALL
            .iter()
            .map(|side| Quad {
                corners: face_corners(*side),
                uv: [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                normal: side.normal(),
                cull: Some(*side),
                tint: 0,
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

    /// A post in the middle of its block: nothing culls it, and none of it touches the edges.
    fn post() -> Baked {
        let shrink = |corner: [f32; 3]| {
            [
                0.375 + corner[0] * 0.25,
                corner[1],
                0.375 + corner[2] * 0.25,
            ]
        };
        let quads = Face::ALL
            .iter()
            .map(|side| Quad {
                corners: face_corners(*side).map(shrink),
                uv: [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                normal: side.normal(),
                cull: None,
                tint: 0,
                shade: true,
                boundary: false,
            })
            .collect();
        Baked {
            quads,
            solid: false,
            collision: vec![([0.375, 0.0, 0.375], [0.625, 1.0, 0.625])],
            ambient_occlusion: true,
        }
    }

    /// A world of one kind of block, with a shape per block id.
    struct World {
        blocks: Box<dyn Fn(i32, i32, i32) -> u16>,
        cube: Baked,
        post: Baked,
    }

    impl World {
        fn new(blocks: impl Fn(i32, i32, i32) -> u16 + 'static) -> Self {
            Self {
                blocks: Box::new(blocks),
                cube: cube(),
                post: post(),
            }
        }
    }

    impl Blocks for World {
        fn block(&self, x: i32, y: i32, z: i32) -> u16 {
            (self.blocks)(x, y, z)
        }
        fn see_through(&self, x: i32, y: i32, z: i32) -> bool {
            !matches!(self.block(x, y, z) >> 4, 1)
        }
        fn covers(&self, x: i32, y: i32, z: i32, _against: u16) -> bool {
            self.block(x, y, z) >> 4 == 1
        }
        fn baked(&self, x: i32, y: i32, z: i32) -> Option<&Baked> {
            match self.block(x, y, z) >> 4 {
                1 => Some(&self.cube),
                85 => Some(&self.post),
                _ => None,
            }
        }
    }

    #[test]
    fn empty_space_makes_no_geometry() {
        let mesh = section(&World::new(|_, _, _| 0), (0, 0, 0));
        assert!(mesh.is_empty());
        assert_eq!(mesh.faces(), 0);
    }

    #[test]
    fn a_block_with_no_shape_is_not_drawn() {
        // Id 99 has no baked shape in this world, so nothing comes out even though it is there.
        let mesh = section(&World::new(|_, _, _| 99 << 4), (0, 0, 0));
        assert!(mesh.is_empty());
    }

    #[test]
    fn a_lone_cube_has_all_six_faces() {
        let mesh = section(
            &World::new(|x, y, z| if (x, y, z) == (3, 4, 5) { 1 << 4 } else { 0 }),
            (0, 0, 0),
        );
        assert_eq!(mesh.faces(), 6);
        assert_eq!(mesh.vertices.len(), 24, "four corners a face");
    }

    #[test]
    fn a_face_is_hidden_by_the_neighbour_it_names() {
        let pair = World::new(|x, y, z| {
            if y == 0 && z == 0 && (x == 0 || x == 1) {
                1 << 4
            } else {
                0
            }
        });
        // Twelve faces, minus the two pressed against each other.
        assert_eq!(section(&pair, (0, 0, 0)).faces(), 10);
    }

    #[test]
    fn a_buried_cube_is_not_drawn_at_all() {
        let solid = World::new(|_, _, _| 1 << 4);
        assert_eq!(section(&solid, (0, 0, 0)).faces(), 0);
    }

    #[test]
    fn a_face_with_no_cullface_is_drawn_whatever_is_next_to_it() {
        // A post surrounded by solid blocks still has all six of its faces, because none of them
        // say what would hide them. Culling them is how a fence becomes an invisible fence.
        let fenced = World::new(|x, y, z| {
            if (x, y, z) == (0, 0, 0) {
                85 << 4
            } else {
                1 << 4
            }
        });
        let mesh = section(&fenced, (0, 0, 0));
        // The stone around it contributes faces of its own, so only the post's are counted —
        // every vertex records which block it came from.
        let mine = mesh
            .vertices
            .iter()
            .filter(|v| v.at == [0, 0, 0])
            .count();
        assert_eq!(mine, 24, "all six of the post's faces are there");
    }

    #[test]
    fn quads_land_at_the_block_they_belong_to() {
        let mesh = section(
            &World::new(|x, y, z| if (x, y, z) == (2, 3, 4) { 1 << 4 } else { 0 }),
            (0, 0, 0),
        );
        for v in &mesh.vertices {
            assert!((2.0..=3.0).contains(&v.position[0]), "{:?}", v.position);
            assert!((3.0..=4.0).contains(&v.position[1]));
            assert!((4.0..=5.0).contains(&v.position[2]));
            assert_eq!(v.at, [2, 3, 4], "and know which block they came from");
        }
    }

    /// The bug this exists to catch: a quad wound the wrong way round is invisible from outside
    /// and visible from inside, so the world renders and looks wrong in a way that is very hard
    /// to read from a screenshot.
    #[test]
    fn every_face_is_wound_to_match_the_way_it_points() {
        let mesh = section(
            &World::new(|x, y, z| if (x, y, z) == (0, 0, 0) { 1 << 4 } else { 0 }),
            (0, 0, 0),
        );
        assert_eq!(mesh.faces(), 6);
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let v: Vec<_> = triangle
                .iter()
                .map(|i| mesh.vertices[*i as usize])
                .collect();
            let edge = |a: usize, b: usize| {
                [
                    v[a].position[0] - v[b].position[0],
                    v[a].position[1] - v[b].position[1],
                    v[a].position[2] - v[b].position[2],
                ]
            };
            let (u, w) = (edge(1, 0), edge(2, 0));
            // Right-hand rule: this is the direction the triangle faces.
            let wound = [
                u[1] * w[2] - u[2] * w[1],
                u[2] * w[0] - u[0] * w[2],
                u[0] * w[1] - u[1] * w[0],
            ];
            let normal = v[0].normal;
            let dot = wound[0] * normal[0] + wound[1] * normal[1] + wound[2] * normal[2];
            assert!(
                dot > 0.0,
                "a face pointing {normal:?} is wound towards {wound:?} — it would be culled"
            );
        }
    }

    #[test]
    fn a_normal_names_the_face_it_belongs_to() {
        for face in Face::ALL {
            assert_eq!(face_of(face.normal()), face, "{face:?} round trips");
        }
    }

    #[test]
    fn light_comes_from_in_front_of_a_face_and_not_from_inside_the_block() {
        /// Pitch black everywhere except one cell, so where the light was read from is obvious.
        struct Lit {
            inner: World,
        }
        impl Blocks for Lit {
            fn block(&self, x: i32, y: i32, z: i32) -> u16 {
                self.inner.block(x, y, z)
            }
            fn see_through(&self, x: i32, y: i32, z: i32) -> bool {
                self.inner.see_through(x, y, z)
            }
            fn covers(&self, x: i32, y: i32, z: i32, against: u16) -> bool {
                self.inner.covers(x, y, z, against)
            }
            fn baked(&self, x: i32, y: i32, z: i32) -> Option<&Baked> {
                self.inner.baked(x, y, z)
            }
            fn light(&self, _x: i32, y: i32, _z: i32) -> u8 {
                // Bright above the block, dark at it and below.
                if y > 0 { 15 } else { 0 }
            }
        }
        let world = Lit {
            inner: World::new(|x, y, z| if (x, y, z) == (0, 0, 0) { 1 << 4 } else { 0 }),
        };
        let mesh = section(&world, (0, 0, 0));
        let top: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| v.normal == [0.0, 1.0, 0.0])
            .collect();
        assert!(
            top.iter().all(|v| v.light > 0.5),
            "the top face is lit by the air above it"
        );
        let bottom: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| v.normal == [0.0, -1.0, 0.0])
            .collect();
        assert!(
            bottom.iter().all(|v| v.light < 0.5),
            "and the underside is not"
        );
    }

    #[test]
    fn an_interior_face_takes_the_light_of_its_own_block() {
        struct Dark(World);
        impl Blocks for Dark {
            fn block(&self, x: i32, y: i32, z: i32) -> u16 {
                self.0.block(x, y, z)
            }
            fn covers(&self, x: i32, y: i32, z: i32, against: u16) -> bool {
                self.0.covers(x, y, z, against)
            }
            fn baked(&self, x: i32, y: i32, z: i32) -> Option<&Baked> {
                self.0.baked(x, y, z)
            }
            fn light(&self, x: i32, _y: i32, _z: i32) -> u8 {
                // Only the post's own column is lit.
                if x == 0 { 15 } else { 0 }
            }
        }
        let world = Dark(World::new(|x, y, z| {
            if (x, y, z) == (0, 0, 0) { 85 << 4 } else { 0 }
        }));
        let mesh = section(&world, (0, 0, 0));
        assert!(
            mesh.vertices.iter().all(|v| v.light > 0.5),
            "a post inside a lit block is lit, not dark from its neighbours"
        );
    }

    #[test]
    fn the_tint_rides_along_so_a_biome_can_colour_it() {
        let mut leaves = cube();
        for quad in &mut leaves.quads {
            quad.tint = 2;
        }
        struct Tinted(Baked);
        impl Blocks for Tinted {
            fn block(&self, _x: i32, _y: i32, _z: i32) -> u16 {
                18 << 4
            }
            fn covers(&self, _x: i32, _y: i32, _z: i32, _against: u16) -> bool {
                false
            }
            fn baked(&self, x: i32, y: i32, z: i32) -> Option<&Baked> {
                ((x, y, z) == (0, 0, 0)).then_some(&self.0)
            }
        }
        let mesh = section(&Tinted(leaves), (0, 0, 0));
        assert!(!mesh.is_empty());
        assert!(mesh.vertices.iter().all(|v| v.tint == 2));
    }

    #[test]
    fn how_tall_a_shape_is_comes_from_what_you_collide_with() {
        assert_eq!(cube().height(), 1.0);
        let snow = Baked {
            collision: vec![([0.0; 3], [1.0, 0.125, 1.0])],
            ..Baked::default()
        };
        assert_eq!(snow.height(), 0.125, "one layer of snow");
        assert_eq!(Baked::default().height(), 0.0, "nothing to stand on");
    }
}
