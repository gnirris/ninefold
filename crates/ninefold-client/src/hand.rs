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

/// One step of the transform that puts something in the player's hand.
///
/// Applied innermost-last, the way a graphics stack does it: the list reads from the camera
/// inwards, and a point goes through it backwards.
#[derive(Debug, Clone, Copy)]
enum Step {
    Move([f32; 3]),
    /// Degrees about x, y or z, by the right-hand rule.
    Turn(u8, f32),
    Size(f32),
}

/// Put a point in item space through a chain of steps.
fn through(point: [f32; 3], steps: &[Step]) -> [f32; 3] {
    let mut p = point;
    for step in steps.iter().rev() {
        p = match *step {
            Step::Move(by) => [p[0] + by[0], p[1] + by[1], p[2] + by[2]],
            Step::Size(by) => [p[0] * by, p[1] * by, p[2] * by],
            Step::Turn(axis, degrees) => {
                let (sin, cos) = degrees.to_radians().sin_cos();
                match axis {
                    0 => [p[0], p[1] * cos - p[2] * sin, p[1] * sin + p[2] * cos],
                    1 => [p[0] * cos + p[2] * sin, p[1], -p[0] * sin + p[2] * cos],
                    _ => [p[0] * cos - p[1] * sin, p[0] * sin + p[1] * cos, p[2]],
                }
            }
        };
    }
    p
}

/// The two curves 1.8 drives every first-person swing with.
///
/// Not one sine but two, against different arguments: `sin(swing² · π)` rises late and
/// `sin(√swing · π)` rises early, and using them on different axes is what makes the hand arc
/// out and back rather than simply bobbing. Guessing with a single sine — which is what this
/// did before — looks like a hand on a spring.
fn swing_curves(swing: f32) -> (f32, f32) {
    let swing = swing.clamp(0.0, 1.0);
    (
        (swing * swing * std::f32::consts::PI).sin(),
        (swing.sqrt() * std::f32::consts::PI).sin(),
    )
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
    /// The camera's vertical field of view, which the player can change.
    pub fov: f32,
    /// How far down the hand is sitting while a new item comes up, 1 to 0.
    pub equip: f32,
}

/// How much of the way to the corner of the view the hand sits.
const TOWARDS_CORNER: f32 = 0.62;

/// How far in front of the eye everything in hand is held. Vanilla's own number.
const DEPTH: f32 = 0.72;

impl Holding<'_> {
    /// Half the width and height the camera can see at a given distance in front of it.
    fn visible(&self, depth: f32) -> (f32, f32) {
        let half_height = depth * (self.fov.to_radians() / 2.0).tan();
        (half_height * self.aspect.max(0.1), half_height)
    }

}

/// Build the hand in front of the camera.
///
/// Returns which sheet the mesh samples from, because a block comes out of the block atlas, an
/// item out of the item sheet and a bare arm out of the player's own skin.
///
/// The placement is 1.8's: 0.56 right, 0.52 down and 0.72 forward of the eye, dropped by the
/// equip animation, turned 45 degrees, then swung by the two curves above on three axes. The one
/// deliberate difference is that the sideways offset is scaled for the window's shape — vanilla's
/// 0.56 sits 62% of the way to the edge of a 16:9 view and off the edge entirely of a tall one.
#[must_use]
pub fn held(basis: &Basis, what: &Holding<'_>) -> (Vec<Vertex>, Vec<u32>, Sheet) {
    let (late, early) = swing_curves(what.swing);
    let equip = what.equip.clamp(0.0, 1.0);
    let light = what.light.clamp(0.35, 1.0);
    // 0.56 at 16:9, and the same fraction of the view at any other shape.
    let (wide, _) = what.visible(DEPTH);
    let across = (wide * TOWARDS_CORNER).min(0.56);

    // What every held thing shares, from the camera inwards.
    let common = [
        Step::Move([across, -0.52, -DEPTH]),
        Step::Move([0.0, equip * -0.6, 0.0]),
        Step::Turn(1, 45.0),
        Step::Turn(1, late * -20.0),
        Step::Turn(2, early * -20.0),
        Step::Turn(0, early * -80.0),
        Step::Size(0.4),
    ];

    let block = what.held.and_then(Slot::as_block);
    if let Some(block) = block {
        // A block keeps its own shape: the model's first-person transform is another 45 degrees
        // and another four tenths, which is why a held block is small and seen corner-on.
        let steps: Vec<Step> = common
            .iter()
            .copied()
            .chain([Step::Turn(1, 45.0), Step::Size(0.4)])
            .collect();
        let mut build = Build::new();
        for face in Face::ALL {
            let tile = what.atlas.rect(block, face);
            let points = corners(face, [-0.5, -0.5, -0.5], [0.5, 0.5, 0.5])
                .map(|point| basis.at(through(point, &steps)));
            // A turned cube's faces point wherever the chain left them, so the normal is taken
            // from the geometry rather than from which face it started as.
            let normal = face_normal(points, basis);
            build.quad(points, normal, tile, light);
        }
        let (vertices, indices) = build.finish();
        return (vertices, indices, Sheet::Blocks);
    }

    if let Some(stack) = what.held
        && let Some(tile) = what.items.rect(stack.id, stack.damage)
    {
        // A flat item is the `generated` model: turned most of the way round, tipped, lifted
        // four pixels and pushed out two, then scaled up — which is what points a sword up and
        // to the left instead of lying flat across the view.
        let steps: Vec<Step> = common
            .iter()
            .copied()
            .chain([
                Step::Move([0.0, 4.0 / 16.0, 2.0 / 16.0]),
                Step::Turn(1, -135.0),
                Step::Turn(2, 25.0),
                Step::Size(1.7),
            ])
            .collect();
        let quad = [
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ]
        .map(|point| basis.at(through(point, &steps)));
        let mut build = Build::new();
        let normal = face_normal(quad, basis);
        let behind = [-normal[0], -normal[1], -normal[2]];
        build.quad(quad, normal, tile, light);
        let [a, b, c, d] = quad;
        build.quad([b, a, d, c], behind, tile, light);
        let (vertices, indices) = build.finish();
        return (vertices, indices, Sheet::Items);
    }

    // Nothing in hand: the arm itself, out of the player's own skin. Vanilla swings this on its
    // own chain — further round, and shoved across and down as it goes.
    let Some(skin) = what.skin else {
        return (Vec::new(), Vec::new(), Sheet::Mobs);
    };
    let steps = [
        Step::Move([
            early * -0.3,
            (early * 2.0 * std::f32::consts::PI).sin() * 0.4,
            late * -0.4,
        ]),
        Step::Move([across * 1.1, -0.62 - equip * 0.6, -DEPTH]),
        Step::Turn(1, 45.0),
        Step::Turn(1, early * 70.0),
        Step::Turn(2, late * -20.0),
        // The arm is a quarter of a block across and three quarters long, in block units.
        Step::Size(0.25),
    ];
    let tile = skin.patch([44.0, 20.0], [4.0, 12.0]);
    let mut build = Build::new();
    for face in Face::ALL {
        let points = corners(face, [-0.5, -0.5, -0.5], [0.5, 0.5, 2.6])
            .map(|point| basis.at(through(point, &steps)));
        let normal = face_normal(points, basis);
        build.quad(points, normal, tile, light);
    }
    let (vertices, indices) = build.finish();
    (vertices, indices, Sheet::Mobs)
}

/// Which way a quad faces, worked out from its own corners.
///
/// Needed because every one of these has been turned by the chain above, so where a face
/// started out pointing says nothing about where it ends up.
fn face_normal(points: [[f32; 3]; 4], basis: &Basis) -> [f32; 3] {
    let edge = |a: usize, b: usize| {
        [
            points[a][0] - points[b][0],
            points[a][1] - points[b][1],
            points[a][2] - points[b][2],
        ]
    };
    let normal = cross(edge(1, 0), edge(3, 0));
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if length < 1e-6 {
        // A degenerate quad has no direction; face the viewer rather than nothing.
        return basis.way([0.0, 0.0, 1.0]);
    }
    [normal[0] / length, normal[1] / length, normal[2] / length]
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
                fov: 70.0,
                equip: 0.0,
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
            for (axis, total) in sum.iter_mut().enumerate() {
                *total += v.position[axis] / vertices.len() as f32;
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
                fov: 70.0,
                equip: 0.0,
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
                    fov: 70.0,
                    equip: 0.0,
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
    fn the_hand_is_in_the_corner_whatever_shape_the_window_is() {
        let atlas = Atlas::build(&Flat);
        let items = Items::build(&Flat);
        // Vanilla holds things 0.52 below the eye at 0.72 away, and the view only reaches 0.50
        // at that distance — so the held item really is cropped by the bottom of the screen, and
        // a test demanding all of it be visible would be testing for the wrong thing. What has
        // to hold is that it is down and to the right, and that some of it can be seen.
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
                    fov: 70.0,
                    equip: 0.0,
                },
            );
            let mut seen = 0;
            let mut middle = [0.0f32; 3];
            for v in &vertices {
                // Back into camera space: the basis looks along +z from the eye's height.
                let (x, y, z) = (v.position[0], v.position[1] - 70.0, v.position[2]);
                let half_height = z * (70.0f32.to_radians() / 2.0).tan();
                if x.abs() <= half_height * aspect && y.abs() <= half_height {
                    seen += 1;
                }
                for (axis, total) in middle.iter_mut().enumerate() {
                    *total += [x, y, z][axis] / vertices.len() as f32;
                }
            }
            assert!(seen > 0, "aspect {aspect}: none of the hand is on screen");
            assert!(middle[1] < 0.0, "aspect {aspect}: the hand is not below the eye");
            // The camera faces +z, so the viewer's right is world -x.
            assert!(middle[0] < 0.0, "aspect {aspect}: the hand is not on the right");
            assert!(middle[2] > 0.0, "aspect {aspect}: the hand is behind the camera");
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
