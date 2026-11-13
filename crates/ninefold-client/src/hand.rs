//! What is in front of the camera: the player's own hand, and the cracks on a block.
//!
//! Both are world geometry rather than interface, because both have to sit in the world's
//! perspective — a flat picture of a sword in the corner of the screen reads as a sticker, and
//! cracks drawn in two dimensions would not bend round the block they are on.
//!
//! The hand is built in the camera's own frame: right, up, and back towards the viewer. That
//! triple is right-handed, which means the same corner ordering the chunk mesher uses works here
//! without a second winding convention to get wrong.

use ninefold_mesh::Face;
use ninefold_protocol::item::Slot;
use ninefold_render::{Sheet, Vertex};

use crate::atlas::Atlas;
use crate::items::Items;
use crate::models;

/// The camera's own axes, to build things in front of it.
pub struct Basis {
    pub eye: [f32; 3],
    /// Towards the viewer, so that (right, up, back) is right-handed.
    pub back: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
}

impl Basis {
    /// Work out the axes from where the camera is and which way it looks.
    #[must_use]
    pub fn new(eye: [f32; 3], facing: [f32; 3]) -> Self {
        let forward = normalise(facing);
        let right = normalise(cross(forward, [0.0, 1.0, 0.0]));
        let up = cross(right, forward);
        Self {
            eye,
            back: [-forward[0], -forward[1], -forward[2]],
            right,
            up,
        }
    }

    /// A point given in camera space: right, up, back.
    fn at(&self, local: [f32; 3]) -> [f32; 3] {
        [
            self.eye[0] + self.right[0] * local[0] + self.up[0] * local[1] + self.back[0] * local[2],
            self.eye[1] + self.right[1] * local[0] + self.up[1] * local[1] + self.back[1] * local[2],
            self.eye[2] + self.right[2] * local[0] + self.up[2] * local[1] + self.back[2] * local[2],
        ]
    }

    /// A direction in camera space, for the face normals.
    fn way(&self, local: [f32; 3]) -> [f32; 3] {
        let origin = self.at([0.0, 0.0, 0.0]);
        let point = self.at(local);
        normalise([
            point[0] - origin[0],
            point[1] - origin[1],
            point[2] - origin[2],
        ])
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalise(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length < 1e-6 {
        return [0.0, 0.0, 1.0];
    }
    [v[0] / length, v[1] / length, v[2] / length]
}

/// The corners of one face of a box, bottom left first as seen from outside.
///
/// The same ordering as the chunk mesher, for the same reason: anything else puts the texture on
/// sideways.
fn corners(face: Face, from: [f32; 3], to: [f32; 3]) -> [[f32; 3]; 4] {
    let ([x, y, z], [x1, y1, z1]) = (from, to);
    match face {
        Face::North => [[x1, y, z], [x, y, z], [x, y1, z], [x1, y1, z]],
        Face::South => [[x, y, z1], [x1, y, z1], [x1, y1, z1], [x, y1, z1]],
        Face::West => [[x, y, z], [x, y, z1], [x, y1, z1], [x, y1, z]],
        Face::East => [[x1, y, z1], [x1, y, z], [x1, y1, z], [x1, y1, z1]],
        Face::Up => [[x, y1, z1], [x1, y1, z1], [x1, y1, z], [x, y1, z]],
        Face::Down => [[x, y, z], [x1, y, z], [x1, y, z1], [x, y, z1]],
    }
}

/// Where each corner samples, in the order [`corners`] returns them.
const CORNER_UV: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

/// The eight corners of a box, named by which side of the middle they are on.
///
/// The same ordering as [`corners`], written in terms of the box's own axes rather than the
/// world's, so a box can be turned without a second winding convention to get wrong.
const BOX_FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
    // -c, +c
    ([0.0, 0.0, -1.0], [[1.0, -1.0, -1.0], [-1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [1.0, 1.0, -1.0]]),
    ([0.0, 0.0, 1.0], [[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]]),
    // -a, +a
    ([-1.0, 0.0, 0.0], [[-1.0, -1.0, -1.0], [-1.0, -1.0, 1.0], [-1.0, 1.0, 1.0], [-1.0, 1.0, -1.0]]),
    ([1.0, 0.0, 0.0], [[1.0, -1.0, 1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [1.0, 1.0, 1.0]]),
    // +b, -b
    ([0.0, 1.0, 0.0], [[-1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0]]),
    ([0.0, -1.0, 0.0], [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, -1.0, 1.0], [-1.0, -1.0, 1.0]]),
];

/// A mesh being built.
struct Build {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

impl Build {
    fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    fn quad(&mut self, points: [[f32; 3]; 4], normal: [f32; 3], tile: [f32; 4], light: f32) {
        let first = u32::try_from(self.vertices.len()).unwrap_or(u32::MAX);
        for (point, uv) in points.iter().zip(CORNER_UV) {
            self.vertices.push(Vertex::plain(
                *point,
                normal,
                [tile[0] + uv[0] * tile[2], tile[1] + uv[1] * tile[3]],
                light,
            ));
        }
        self.indices
            .extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
    }

    /// A box turned to point somewhere, with the same picture on every side.
    ///
    /// `along` is the direction its long axis runs, in camera space.
    fn turned(
        &mut self,
        basis: &Basis,
        centre: [f32; 3],
        shape: ([f32; 3], [f32; 3]),
        tile: [f32; 4],
        light: f32,
    ) {
        let (along, half) = shape;
        let c = normalise(along);
        let a = normalise(cross([0.0, 1.0, 0.0], c));
        let b = cross(c, a);
        let point = |[pa, pb, pc]: [f32; 3]| {
            let local = [
                centre[0] + a[0] * pa * half[0] + b[0] * pb * half[1] + c[0] * pc * half[2],
                centre[1] + a[1] * pa * half[0] + b[1] * pb * half[1] + c[1] * pc * half[2],
                centre[2] + a[2] * pa * half[0] + b[2] * pb * half[1] + c[2] * pc * half[2],
            ];
            basis.at(local)
        };
        for (axis, face) in BOX_FACES {
            let normal = basis.way([
                a[0] * axis[0] + b[0] * axis[1] + c[0] * axis[2],
                a[1] * axis[0] + b[1] * axis[1] + c[1] * axis[2],
                a[2] * axis[0] + b[2] * axis[1] + c[2] * axis[2],
            ]);
            self.quad(face.map(point), normal, tile, light);
        }
    }

    fn finish(self) -> (Vec<Vertex>, Vec<u32>) {
        (self.vertices, self.indices)
    }
}

/// The cracks spreading over a block, at stage 0..9.
///
/// Drawn on a cube a hair bigger than the block, so the cracks sit on its surface rather than
/// fighting it for the same depth.
#[must_use]
pub fn cracks(atlas: &Atlas, at: (i32, i32, i32), stage: usize) -> (Vec<Vertex>, Vec<u32>) {
    let Some(tile) = atlas.crack(stage) else {
        return (Vec::new(), Vec::new());
    };
    const OUT: f32 = 0.003;
    let from = [at.0 as f32 - OUT, at.1 as f32 - OUT, at.2 as f32 - OUT];
    let to = [
        at.0 as f32 + 1.0 + OUT,
        at.1 as f32 + 1.0 + OUT,
        at.2 as f32 + 1.0 + OUT,
    ];
    let mut build = Build::new();
    for face in Face::ALL {
        build.quad(corners(face, from, to), face.normal(), tile, 1.0);
    }
    build.finish()
}

/// Everything needed to draw whatever is in hand.
pub struct Holding<'a> {
    pub atlas: &'a Atlas,
    pub items: &'a Items,
    /// Where the player's own skin is in the mob sheet, for an empty hand.
    pub skin: Option<models::Placed>,
    pub held: Option<Slot>,
    /// How far through a swing, 0 to 1.
    pub swing: f32,
    /// How lit the player's own square is.
    pub light: f32,
    /// Window width over height. The hand is placed as a fraction of what the camera can see,
    /// not at fixed coordinates, or a tall window pushes it off the side of the screen.
    pub aspect: f32,
}

/// The camera's field of view, which has to match what the renderer projects with.
const FOV_DEGREES: f32 = 70.0;

/// How much of the way to the corner of the view the hand sits.
const TOWARDS_CORNER: f32 = 0.62;

impl Holding<'_> {
    /// Half the width and height the camera can see at a given distance in front of it.
    fn visible(&self, depth: f32) -> (f32, f32) {
        let half_height = depth * (FOV_DEGREES.to_radians() / 2.0).tan();
        (half_height * self.aspect.max(0.1), half_height)
    }

    /// Where to put the middle of something, so that all of it stays on screen.
    ///
    /// `near` is how far away its closest part is, because the view narrows towards the camera
    /// and that end is what leaves the frustum first. `reach` is how far it sticks out sideways
    /// — its width, not its length. An arm is long and thin, and measuring it by its length
    /// pushes it into the middle of the screen where its own length is not the problem.
    fn corner(&self, near: f32, reach: f32) -> (f32, f32) {
        let (wide, tall) = self.visible(near.max(0.05));
        (
            ((wide - reach).max(0.0)) * TOWARDS_CORNER,
            -((tall - reach).max(0.0)) * TOWARDS_CORNER,
        )
    }
}

/// Build the hand in front of the camera.
///
/// Returns which sheet the mesh samples from, because a block comes out of the block atlas, an
/// item out of the item sheet and a bare arm out of the player's own skin.
#[must_use]
pub fn held(basis: &Basis, what: &Holding<'_>) -> (Vec<Vertex>, Vec<u32>, Sheet) {
    // Vanilla's swing is an arc: the hand drops and comes back, moving towards the middle of
    // the screen on the way. One sine for the arc and one for the dip is enough to read as the
    // same motion.
    let swing = what.swing.clamp(0.0, 1.0);
    let arc = (swing * std::f32::consts::PI).sin();
    let dip = (swing * std::f32::consts::TAU).sin();
    let light = what.light.clamp(0.35, 1.0);

    let block = what.held.and_then(Slot::as_block);
    if let Some(block) = block {
        // A block is held as a block: a small cube, turned so three faces show.
        let (depth, half) = (0.95, 0.13);
        let (x, y) = what.corner(depth - half, half);
        let centre = [x - arc * 0.12, y - arc * 0.14, -depth + dip * 0.05];
        let mut build = Build::new();
        for face in Face::ALL {
            let tile = what.atlas.rect(block, face);
            let points = corners(
                face,
                [centre[0] - half, centre[1] - half, centre[2] - half],
                [centre[0] + half, centre[1] + half, centre[2] + half],
            )
            .map(|point| basis.at(point));
            let normal = basis.way(face.normal());
            build.quad(points, normal, tile, light);
        }
        let (vertices, indices) = build.finish();
        return (vertices, indices, Sheet::Blocks);
    }

    if let Some(stack) = what.held
        && let Some(tile) = what.items.rect(stack.id, stack.damage)
    {
        // An item is a flat picture, held at an angle and drawn from both sides.
        let (depth, w, h) = (0.95, 0.17, 0.17);
        let lean = 0.10 + arc * 0.06;
        let (x, y) = what.corner(depth - lean, w);
        let centre = [x - arc * 0.14, y - arc * 0.16, -depth];
        let a = [centre[0] - w, centre[1] - h, centre[2] - lean];
        let b = [centre[0] + w, centre[1] - h, centre[2] + lean];
        let c = [centre[0] + w, centre[1] + h, centre[2] + lean];
        let d = [centre[0] - w, centre[1] + h, centre[2] - lean];
        let mut build = Build::new();
        let front = basis.way([0.0, 0.0, 1.0]);
        let back = [-front[0], -front[1], -front[2]];
        build.quad([a, b, c, d].map(|p| basis.at(p)), front, tile, light);
        build.quad([b, a, d, c].map(|p| basis.at(p)), back, tile, light);
        let (vertices, indices) = build.finish();
        return (vertices, indices, Sheet::Items);
    }

    // Nothing in hand: the arm itself, out of the player's own skin.
    let Some(skin) = what.skin else {
        return (Vec::new(), Vec::new(), Sheet::Mobs);
    };
    let (depth, half_w, half_d) = (1.0f32, 0.055f32, 0.28f32);
    let (x, y) = what.corner(depth - half_d, half_w);
    let centre = [x - arc * 0.10, y - arc * 0.16, -depth + arc * 0.14];
    // Vanilla's arm comes in from the bottom corner on the diagonal rather than lying flat
    // along the view, which is what makes it read as an arm and not as a plank.
    let along = [0.34, -0.42, 1.0];
    let mut build = Build::new();
    // The arm's own patch on a skin: four pixels across, twelve down, at (44, 20).
    let tile = skin.patch([44.0, 20.0], [4.0, 12.0]);
    build.turned(
        basis,
        centre,
        (along, [half_w, half_w, half_d]),
        tile,
        light,
    );
    let (vertices, indices) = build.finish();
    (vertices, indices, Sheet::Mobs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ninefold_assets::{AssetSource, Texture};

    struct Flat;
    impl AssetSource for Flat {
        fn texture(&self, _id: &str) -> Option<Texture> {
            Some(Texture::flat(16, 16, [100, 100, 100, 255]))
        }
        fn describe(&self) -> String {
            "test".into()
        }
    }

    fn basis() -> Basis {
        Basis::new([0.0, 70.0, 0.0], [0.0, 0.0, 1.0])
    }

    #[test]
    fn the_camera_axes_are_at_right_angles_and_right_handed() {
        for facing in [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.3, -0.8, 0.5]] {
            let b = Basis::new([0.0; 3], facing);
            let dot = |a: [f32; 3], c: [f32; 3]| a[0] * c[0] + a[1] * c[1] + a[2] * c[2];
            assert!(dot(b.right, b.up).abs() < 1e-5, "right and up {facing:?}");
            assert!(dot(b.right, b.back).abs() < 1e-5, "right and back");
            assert!(dot(b.up, b.back).abs() < 1e-5, "up and back");
            // Right-handed: right cross up is back, not forward.
            let handed = cross(b.right, b.up);
            assert!(dot(handed, b.back) > 0.99, "left-handed for {facing:?}");
        }
    }

    #[test]
    fn a_held_block_is_a_cube_in_front_of_the_camera() {
        let atlas = Atlas::build(&Flat);
        let items = Items::build(&Flat);
        let basis = basis();
        let (vertices, indices, sheet) = held(
            &basis,
            &Holding {
                atlas: &atlas,
                items: &items,
                skin: None,
                held: Some(Slot::new(4, 1)),
                swing: 0.0,
                light: 1.0,
                aspect: 1.78,
            },
        );
        assert_eq!(sheet, Sheet::Blocks);
        assert_eq!(indices.len(), 36, "six faces");
        // In front of the camera, which is at y=70 looking along +z.
        for v in &vertices {
            assert!(v.position[2] > 0.0, "behind the camera: {:?}", v.position);
        }
        // And down and to the right of the middle. The top corner of the cube pokes a little
        // above the eye line, as it does in the real game, so this is about where it sits
        // overall rather than about every corner of it.
        let middle = vertices.iter().fold([0.0f32; 3], |mut sum, v| {
            for axis in 0..3 {
                sum[axis] += v.position[axis] / vertices.len() as f32;
            }
            sum
        });
        assert!(middle[1] < 70.0, "the hand is below the eye line");
        // The camera faces +z, so the viewer's right is world -x.
        assert!(middle[0] < 0.0, "the hand is on the right");
    }

    #[test]
    fn an_item_is_flat_and_drawn_from_both_sides() {
        let atlas = Atlas::build(&Flat);
        let items = Items::build(&Flat);
        let (_, indices, sheet) = held(
            &basis(),
            &Holding {
                atlas: &atlas,
                items: &items,
                skin: None,
                held: Some(Slot::new(278, 1)),
                swing: 0.0,
                light: 1.0,
                aspect: 1.78,
            },
        );
        assert_eq!(sheet, Sheet::Items);
        assert_eq!(indices.len(), 12, "one quad, two sides");
    }

    #[test]
    fn swinging_moves_the_hand() {
        let atlas = Atlas::build(&Flat);
        let items = Items::build(&Flat);
        let hand = |swing| {
            held(
                &basis(),
                &Holding {
                    atlas: &atlas,
                    items: &items,
                    skin: None,
                    held: Some(Slot::new(4, 1)),
                    swing,
                    light: 1.0,
                    aspect: 1.78,
                },
            )
            .0[0]
                .position
        };
        let still = hand(0.0);
        let mid = hand(0.5);
        assert_ne!(still, mid, "a swing that does not move is not a swing");
        // And it comes back to where it started.
        let done = hand(1.0);
        for axis in 0..3 {
            assert!((still[axis] - done[axis]).abs() < 1e-4);
        }
    }

    #[test]
    fn the_hand_stays_on_screen_whatever_shape_the_window_is() {
        let atlas = Atlas::build(&Flat);
        let items = Items::build(&Flat);
        // A tall window and a wide one. In both, the hand has to be inside what the camera can
        // see — a fixed offset puts it off the side of a portrait window entirely.
        for aspect in [0.6f32, 1.0, 1.78, 3.5] {
            let (vertices, ..) = held(
                &basis(),
                &Holding {
                    atlas: &atlas,
                    items: &items,
                    skin: None,
                    held: Some(Slot::new(4, 1)),
                    swing: 0.0,
                    light: 1.0,
                    aspect,
                },
            );
            for v in &vertices {
                // Back into camera space: the basis looks along +z from the origin height.
                let (x, y, z) = (v.position[0], v.position[1] - 70.0, v.position[2]);
                let half_height = z * (70.0f32.to_radians() / 2.0).tan();
                assert!(
                    x.abs() <= half_height * aspect,
                    "aspect {aspect}: x {x} outside {}",
                    half_height * aspect
                );
                assert!(y.abs() <= half_height, "aspect {aspect}: y {y} off screen");
            }
        }
    }

    #[test]
    fn cracks_wrap_the_block_they_are_on() {
        let atlas = Atlas::build(&Flat);
        let (vertices, indices) = cracks(&atlas, (3, 64, -2), 4);
        assert_eq!(indices.len(), 36, "all six faces crack");
        for v in &vertices {
            assert!((v.position[0] - 3.5).abs() <= 0.51, "{:?}", v.position);
            assert!((v.position[1] - 64.5).abs() <= 0.51);
            assert!((v.position[2] + 1.5).abs() <= 0.51);
        }
    }

    #[test]
    fn a_stage_that_does_not_exist_draws_nothing() {
        let atlas = Atlas::build(&Flat);
        assert!(cracks(&atlas, (0, 0, 0), 99).1.is_empty());
    }
}
