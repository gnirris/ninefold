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

/// What a block is shaped like.
///
/// Almost everything is a cube, which is why a chunk renderer can be fast at all. The exception
/// that cannot be skipped is the crossed sprite: grass, flowers, saplings, crops and dead bushes
/// are two intersecting planes, and drawn as cubes they are unmistakably wrong — a flower
/// becomes a solid block of petals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Cube,
    /// Two planes crossing on the diagonal.
    Cross,
    /// There but not drawn.
    Invisible,
}

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
    /// `id << 4 | metadata`, so the renderer can look up which texture to use.
    pub block: u16,
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

    /// Cube unless something knows better.
    fn shape(&self, _x: i32, _y: i32, _z: i32) -> Shape {
        Shape::Cube
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
                match world.shape(x, y, z) {
                    Shape::Invisible => continue,
                    Shape::Cross => {
                        // A sprite is lit by the air it stands in, not by a neighbour: there is
                        // no face pressed against anything to take the light from.
                        let light = f32::from(world.light(x, y, z)) / 15.0;
                        push_cross(&mut mesh, (x, y, z), block, light.max(0.05));
                        continue;
                    }
                    Shape::Cube => {}
                }
                for face in Face::ALL {
                    let (nx, ny, nz) = face.offset();
                    // A face is only worth drawing if what is next to it does not hide it.
                    if !world.covers(x + nx, y + ny, z + nz, block) {
                        // Light is sampled in front of the face: inside a solid block it is
                        // always zero, which would make every surface black.
                        let front = (x + nx, y + ny, z + nz);
                        let (along, across) = face.corner_axes();
                        let corners = [
                            corner_light(world, front, neg(along), neg(across)),
                            corner_light(world, front, along, neg(across)),
                            corner_light(world, front, along, across),
                            corner_light(world, front, neg(along), across),
                        ];
                        push_face(&mut mesh, (x, y, z), face, block, corners);
                    }
                }
            }
        }
    }
    mesh
}

/// Add one quad, as two triangles.
const fn neg((x, y, z): (i32, i32, i32)) -> (i32, i32, i32) {
    (-x, -y, -z)
}

fn push_face(
    mesh: &mut Mesh,
    at: (i32, i32, i32),
    face: Face,
    block: u16,
    corners_light: [f32; 4],
) {
    let (x, y, z) = (at.0 as f32, at.1 as f32, at.2 as f32);
    let (x1, y1, z1) = (x + 1.0, y + 1.0, z + 1.0);
    // Bottom left, bottom right, top right, top left — *as seen from outside the block*, which
    // is the only ordering that puts a texture the right way up and the right way round. Getting
    // this wrong does not stop anything rendering: it rotates the bark on a log by ninety
    // degrees on two sides of it and mirrors it on the other two, which is exactly the kind of
    // wrong that is easy to look straight past.
    //
    // Which corner is "bottom left" depends on where the viewer is. Standing north of a block
    // and looking at its north face, world +x is on your left, so the bottom left corner is the
    // one at maximum x — and that is why these are not simply the corners in order.
    let corners: [[f32; 3]; 4] = match face {
        Face::North => [[x1, y, z], [x, y, z], [x, y1, z], [x1, y1, z]],
        Face::South => [[x, y, z1], [x1, y, z1], [x1, y1, z1], [x, y1, z1]],
        Face::West => [[x, y, z], [x, y, z1], [x, y1, z1], [x, y1, z]],
        Face::East => [[x1, y, z1], [x1, y, z], [x1, y1, z], [x1, y1, z1]],
        // Seen from above with north at the top of the texture, which is how Minecraft lays out
        // every top face.
        Face::Up => [[x, y1, z1], [x1, y1, z1], [x1, y1, z], [x, y1, z]],
        Face::Down => [[x, y, z], [x1, y, z], [x1, y, z1], [x, y, z1]],
    };
    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let first = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
    for ((corner, uv), light) in corners.iter().zip(uvs).zip(corners_light) {
        mesh.vertices.push(Vertex {
            position: *corner,
            normal: face.normal(),
            uv,
            block,
            at: [at.0, at.1, at.2],
            light,
        });
    }
    // The corners run anticlockwise seen from outside, so the two triangles are the obvious
    // ones. Reverse them and every block is drawn inside out: the world renders, and you are
    // looking at the undersides of it.
    mesh.indices
        .extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
}

/// How far a crossed sprite's planes sit in from the block's edges.
///
/// Vanilla's cross model is 14.4 pixels wide across a 16-pixel block, rotated 45 degrees, which
/// puts its corners a touch inside the block rather than exactly on them.
const CROSS_INSET: f32 = 0.182;

/// Two crossed planes, each drawn from both sides.
///
/// Both sides matter: back faces are culled, so one quad per plane would make a flower vanish
/// when walked round.
fn push_cross(mesh: &mut Mesh, at: (i32, i32, i32), block: u16, light: f32) {
    let (x, y, z) = (at.0 as f32, at.1 as f32, at.2 as f32);
    let (lo, hi) = (CROSS_INSET, 1.0 - CROSS_INSET);
    let (bottom, top) = (y, y + 1.0);
    let plane = |ax: f32, az: f32, bx: f32, bz: f32| {
        [
            [x + ax, bottom, z + az],
            [x + bx, bottom, z + bz],
            [x + bx, top, z + bz],
            [x + ax, top, z + az],
        ]
    };
    for corners in [plane(lo, lo, hi, hi), plane(hi, lo, lo, hi)] {
        push_quad(mesh, at, corners, block, light);
        // The same plane the other way round, so it is there from behind as well.
        let [a, b, c, d] = corners;
        push_quad(mesh, at, [b, a, d, c], block, light);
    }
}

/// One quad, four corners, bottom left first.
///
/// Sprites are given an upward normal whatever way they face, because that is how vanilla draws
/// them: a flower is not shaded darker for facing away from the sun, and all four of a sprite's
/// quads sample the same texture, so nothing is lost by it.
fn push_quad(mesh: &mut Mesh, at: (i32, i32, i32), corners: [[f32; 3]; 4], block: u16, light: f32) {
    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let first = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
    for (corner, uv) in corners.iter().zip(uvs) {
        mesh.vertices.push(Vertex {
            position: *corner,
            normal: [0.0, 1.0, 0.0],
            uv,
            block,
            at: [at.0, at.1, at.2],
            light,
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

    /// The bug this exists to catch: a face wound the wrong way round is invisible from outside
    /// and visible from inside, so the world renders and looks wrong in a way that is very hard
    /// to read from a screenshot.
    #[test]
    fn every_face_is_wound_to_match_the_way_it_points() {
        let mesh = section(&OneBlock(0, 0, 0), (0, 0, 0));
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
    fn a_sprite_is_four_quads_and_stays_inside_its_block() {
        struct Flower;
        impl Blocks for Flower {
            fn block(&self, x: i32, y: i32, z: i32) -> u16 {
                if (x, y, z) == (2, 3, 4) { 38 << 4 } else { 0 }
            }
            fn see_through(&self, _x: i32, _y: i32, _z: i32) -> bool {
                true
            }
            fn shape(&self, x: i32, y: i32, z: i32) -> Shape {
                if (x, y, z) == (2, 3, 4) {
                    Shape::Cross
                } else {
                    Shape::Cube
                }
            }
        }
        let mesh = section(&Flower, (0, 0, 0));
        assert_eq!(mesh.faces(), 4, "two planes, both sides of each");
        for v in &mesh.vertices {
            assert!(
                (2.0..=3.0).contains(&v.position[0]) && (4.0..=5.0).contains(&v.position[2]),
                "a sprite pokes out of its own block at {:?}",
                v.position
            );
            assert!((3.0..=4.0).contains(&v.position[1]));
        }
        // Both facings are present, so it is visible from either side. A sprite's planes stand
        // upright on the diagonal, so the direction they face is in x and z — checking the y of
        // the cross product would say nothing at all.
        let mut forwards = 0;
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let v: Vec<_> = triangle.iter().map(|i| mesh.vertices[*i as usize]).collect();
            let edge = |a: usize, b: usize| {
                [
                    v[a].position[0] - v[b].position[0],
                    v[a].position[1] - v[b].position[1],
                    v[a].position[2] - v[b].position[2],
                ]
            };
            let (u, w) = (edge(1, 0), edge(2, 0));
            let across = u[1] * w[2] - u[2] * w[1];
            assert!(across.abs() > 1e-6, "a sprite's plane faces nowhere");
            if across > 0.0 {
                forwards += 1;
            }
        }
        assert_eq!(
            forwards, 4,
            "half the triangles face one way and half the other"
        );
    }

    #[test]
    fn an_invisible_block_makes_nothing() {
        struct Hidden;
        impl Blocks for Hidden {
            fn block(&self, _x: i32, _y: i32, _z: i32) -> u16 {
                1 << 4
            }
            fn see_through(&self, _x: i32, _y: i32, _z: i32) -> bool {
                true
            }
            fn shape(&self, _x: i32, _y: i32, _z: i32) -> Shape {
                Shape::Invisible
            }
        }
        assert!(section(&Hidden, (0, 0, 0)).is_empty());
    }

    /// The bug this exists to catch: a face whose corners are in the wrong order still renders,
    /// still culls correctly and still has the right texture on it — it is just rotated or
    /// mirrored. On stone nobody can tell. On a log, the bark runs sideways.
    #[test]
    fn a_texture_is_the_right_way_up_and_the_right_way_round() {
        let mesh = section(&OneBlock(0, 0, 0), (0, 0, 0));
        for face in Face::ALL {
            let quad: Vec<_> = mesh
                .vertices
                .iter()
                .filter(|v| v.normal == face.normal())
                .collect();
            assert_eq!(quad.len(), 4, "{face:?} should be one quad");
            let at = |u: f32, v: f32| {
                quad.iter()
                    .find(|c| c.uv == [u, v])
                    .unwrap_or_else(|| panic!("{face:?} has no corner at uv {u},{v}"))
                    .position
            };
            let (bottom_left, bottom_right, top_left) = (at(0.0, 1.0), at(1.0, 1.0), at(0.0, 0.0));

            // The top of the texture is the top of the face. On the up and down faces there is
            // no "up": the top face has north at the top of the texture, as Minecraft lays them
            // out, and the bottom face is its mirror because you are looking at it from under.
            match face {
                Face::Up => assert!(
                    top_left[2] < bottom_left[2],
                    "the top of a top face's texture should point north"
                ),
                Face::Down => assert!(
                    top_left[2] > bottom_left[2],
                    "a bottom face is the mirror of a top one, seen from below"
                ),
                _ => assert!(
                    top_left[1] > bottom_left[1],
                    "{face:?}: the texture is upside down"
                ),
            }

            // And u increases to the right as seen from outside the block, which is up × normal.
            let n = face.normal();
            let right = [
                1.0 * n[2] - 0.0 * n[1],
                0.0 * n[0] - 0.0 * n[2],
                0.0 * n[1] - 1.0 * n[0],
            ];
            let along = [
                bottom_right[0] - bottom_left[0],
                bottom_right[1] - bottom_left[1],
                bottom_right[2] - bottom_left[2],
            ];
            let dot = along[0] * right[0] + along[1] * right[1] + along[2] * right[2];
            if !matches!(face, Face::Up | Face::Down) {
                assert!(dot > 0.0, "{face:?}: the texture is mirrored ({dot})");
            }
        }
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
