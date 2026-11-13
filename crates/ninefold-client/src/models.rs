//! Mobs and players, as boxes with their own skin on.
//!
//! Every model in 1.8 is a handful of rectangular boxes, and each box's six faces take their
//! pixels from one unwrapped cross on the mob's texture. The layout is fixed: for a box `w` wide,
//! `h` tall and `d` deep placed at `(u, v)` on the sheet,
//!
//! ```text
//!        +--------+--------+
//!        |  top   | bottom |          d tall
//! +------+--------+--------+------+
//! | left | front  | right  | back |   h tall
//! +------+--------+--------+------+
//!    d       w        d       w
//! ```
//!
//! which is why a player's face is always at (8, 8) on a skin. Getting that table right is the
//! whole trick; everything else is six quads per box.
//!
//! The limits, stated plainly: nothing here animates. Legs do not swing and heads turn with the
//! body, because the server does not send limb angles — vanilla works them out from how far the
//! entity has moved, and that belongs with a proper entity tick rather than here. The quadruped
//! models are hand-placed rather than taken from the game's own numbers, so a pig is a pig and
//! not a measurement.

use ninefold_protocol::play::Kind;
use ninefold_render::Vertex;

/// One box, in model pixels, with its feet at y = 0 and its face towards -Z.
pub struct Cuboid {
    /// Low corner, in sixteenths of a block.
    pub from: [f32; 3],
    /// High corner.
    pub to: [f32; 3],
    /// Where this box's unwrapped cross starts on the sheet.
    pub uv: [f32; 2],
    /// A box whose texture was drawn for it standing up, but which lies on its side — every
    /// quadruped's body. The faces are the same; which part of the sheet each one takes is not.
    pub lying: bool,
}

impl Cuboid {
    const fn new(from: [f32; 3], to: [f32; 3], uv: [f32; 2]) -> Self {
        Self {
            from,
            to,
            uv,
            lying: false,
        }
    }

    const fn lying(from: [f32; 3], to: [f32; 3], uv: [f32; 2]) -> Self {
        Self {
            from,
            to,
            uv,
            lying: true,
        }
    }

    /// The size the texture was unwrapped at: for a lying box, the standing size.
    fn texture_size(&self) -> (f32, f32, f32) {
        let (w, h, d) = (
            self.to[0] - self.from[0],
            self.to[1] - self.from[1],
            self.to[2] - self.from[2],
        );
        if self.lying { (w, d, h) } else { (w, h, d) }
    }
}

/// Everything one kind of thing is made of.
pub struct Model {
    /// Where its skin lives in the assets.
    pub texture: &'static str,
    pub boxes: &'static [Cuboid],
    /// Vanilla draws players slightly smaller than their model, so they fit their own hitbox.
    pub scale: f32,
}

/// A standing figure: player, zombie, villager. Thirty-two pixels tall.
static BIPED: [Cuboid; 6] = [
    Cuboid::new([-4.0, 24.0, -4.0], [4.0, 32.0, 4.0], [0.0, 0.0]), // head
    Cuboid::new([-4.0, 12.0, -2.0], [4.0, 24.0, 2.0], [16.0, 16.0]), // body
    Cuboid::new([-8.0, 12.0, -2.0], [-4.0, 24.0, 2.0], [40.0, 16.0]), // right arm
    Cuboid::new([4.0, 12.0, -2.0], [8.0, 24.0, 2.0], [40.0, 16.0]), // left arm
    Cuboid::new([-4.0, 0.0, -2.0], [0.0, 12.0, 2.0], [0.0, 16.0]), // right leg
    Cuboid::new([0.0, 0.0, -2.0], [4.0, 12.0, 2.0], [0.0, 16.0]),  // left leg
];

/// A skeleton is a biped with two-pixel limbs.
static SKELETON: [Cuboid; 6] = [
    Cuboid::new([-4.0, 24.0, -4.0], [4.0, 32.0, 4.0], [0.0, 0.0]),
    Cuboid::new([-4.0, 12.0, -2.0], [4.0, 24.0, 2.0], [16.0, 16.0]),
    Cuboid::new([-6.0, 12.0, -1.0], [-4.0, 24.0, 1.0], [40.0, 16.0]),
    Cuboid::new([4.0, 12.0, -1.0], [6.0, 24.0, 1.0], [40.0, 16.0]),
    Cuboid::new([-3.0, 0.0, -1.0], [-1.0, 12.0, 1.0], [0.0, 16.0]),
    Cuboid::new([1.0, 0.0, -1.0], [3.0, 12.0, 1.0], [0.0, 16.0]),
];

/// Four stumpy legs, a tall body and a head that is mostly jaw.
static CREEPER: [Cuboid; 6] = [
    Cuboid::new([-4.0, 18.0, -4.0], [4.0, 26.0, 4.0], [0.0, 0.0]), // head
    Cuboid::new([-4.0, 6.0, -2.0], [4.0, 18.0, 2.0], [16.0, 16.0]), // body
    Cuboid::new([-4.0, 0.0, -6.0], [0.0, 6.0, -2.0], [0.0, 16.0]), // front right
    Cuboid::new([0.0, 0.0, -6.0], [4.0, 6.0, -2.0], [0.0, 16.0]),  // front left
    Cuboid::new([-4.0, 0.0, 2.0], [0.0, 6.0, 6.0], [0.0, 16.0]),   // back right
    Cuboid::new([0.0, 0.0, 2.0], [4.0, 6.0, 6.0], [0.0, 16.0]),    // back left
];

static PIG: [Cuboid; 6] = [
    Cuboid::lying([-5.0, 6.0, -8.0], [5.0, 14.0, 8.0], [28.0, 8.0]), // body
    Cuboid::new([-4.0, 4.0, -14.0], [4.0, 12.0, -6.0], [0.0, 0.0]),  // head
    Cuboid::new([-5.0, 0.0, -7.0], [-1.0, 6.0, -3.0], [0.0, 16.0]),
    Cuboid::new([1.0, 0.0, -7.0], [5.0, 6.0, -3.0], [0.0, 16.0]),
    Cuboid::new([-5.0, 0.0, 3.0], [-1.0, 6.0, 7.0], [0.0, 16.0]),
    Cuboid::new([1.0, 0.0, 3.0], [5.0, 6.0, 7.0], [0.0, 16.0]),
];

static COW: [Cuboid; 6] = [
    Cuboid::lying([-6.0, 10.0, -10.0], [6.0, 20.0, 10.0], [18.0, 4.0]),
    Cuboid::new([-4.0, 14.0, -16.0], [4.0, 22.0, -8.0], [0.0, 0.0]),
    Cuboid::new([-6.0, 0.0, -8.0], [-2.0, 10.0, -4.0], [0.0, 16.0]),
    Cuboid::new([2.0, 0.0, -8.0], [6.0, 10.0, -4.0], [0.0, 16.0]),
    Cuboid::new([-6.0, 0.0, 4.0], [-2.0, 10.0, 8.0], [0.0, 16.0]),
    Cuboid::new([2.0, 0.0, 4.0], [6.0, 10.0, 8.0], [0.0, 16.0]),
];

static SHEEP: [Cuboid; 6] = [
    Cuboid::lying([-4.0, 10.0, -8.0], [4.0, 18.0, 8.0], [28.0, 8.0]),
    Cuboid::new([-3.0, 12.0, -14.0], [3.0, 18.0, -8.0], [0.0, 0.0]),
    Cuboid::new([-4.0, 0.0, -6.0], [-1.0, 10.0, -3.0], [0.0, 16.0]),
    Cuboid::new([1.0, 0.0, -6.0], [4.0, 10.0, -3.0], [0.0, 16.0]),
    Cuboid::new([-4.0, 0.0, 3.0], [-1.0, 10.0, 6.0], [0.0, 16.0]),
    Cuboid::new([1.0, 0.0, 3.0], [4.0, 10.0, 6.0], [0.0, 16.0]),
];

static CHICKEN: [Cuboid; 5] = [
    Cuboid::lying([-3.0, 5.0, -4.0], [3.0, 11.0, 4.0], [0.0, 9.0]),
    Cuboid::new([-2.0, 9.0, -6.0], [2.0, 13.0, -2.0], [0.0, 0.0]),
    Cuboid::new([-2.0, 10.0, -8.0], [2.0, 12.0, -6.0], [14.0, 0.0]), // beak
    Cuboid::new([-3.0, 0.0, -2.0], [-1.0, 5.0, 1.0], [26.0, 0.0]),
    Cuboid::new([1.0, 0.0, -2.0], [3.0, 5.0, 1.0], [26.0, 0.0]),
];

static PLAYER_MODEL: Model = Model {
    texture: "entity/steve",
    boxes: &BIPED,
    // Vanilla's own number: a 32-pixel model standing in a 1.8-block hitbox.
    scale: 0.9375,
};

static MODELS: &[(u8, Model)] = &[
    (
        50,
        Model {
            texture: "entity/creeper/creeper",
            boxes: &CREEPER,
            scale: 1.0,
        },
    ),
    (
        51,
        Model {
            texture: "entity/skeleton/skeleton",
            boxes: &SKELETON,
            scale: 0.9375,
        },
    ),
    (
        54,
        Model {
            texture: "entity/zombie/zombie",
            boxes: &BIPED,
            scale: 0.9375,
        },
    ),
    (
        57,
        Model {
            texture: "entity/zombie_pigman",
            boxes: &BIPED,
            scale: 0.9375,
        },
    ),
    (
        120,
        Model {
            texture: "entity/villager/villager",
            boxes: &BIPED,
            scale: 0.9375,
        },
    ),
    (
        90,
        Model {
            texture: "entity/pig/pig",
            boxes: &PIG,
            scale: 1.0,
        },
    ),
    (
        91,
        Model {
            texture: "entity/sheep/sheep",
            boxes: &SHEEP,
            scale: 1.0,
        },
    ),
    (
        92,
        Model {
            texture: "entity/cow/cow",
            boxes: &COW,
            scale: 1.0,
        },
    ),
    (
        96,
        Model {
            texture: "entity/cow/mooshroom",
            boxes: &COW,
            scale: 1.0,
        },
    ),
    (
        93,
        Model {
            texture: "entity/chicken",
            boxes: &CHICKEN,
            scale: 1.0,
        },
    ),
];

/// The model for something, if there is one. Anything else is still drawn as a box.
#[must_use]
pub fn model_for(kind: Kind) -> Option<&'static Model> {
    match kind {
        Kind::Player => Some(&PLAYER_MODEL),
        Kind::Mob(id) => MODELS.iter().find(|(m, _)| *m == id).map(|(_, model)| model),
        Kind::Object(_) => None,
    }
}

/// Every texture a model might need, so they can all be packed into one sheet.
#[must_use]
pub fn textures() -> Vec<&'static str> {
    let mut all = vec![PLAYER_MODEL.texture];
    for (_, model) in MODELS {
        if !all.contains(&model.texture) {
            all.push(model.texture);
        }
    }
    all
}

/// Which face of a box is which, and where each one samples from.
#[derive(Clone, Copy)]
enum Side {
    West,
    East,
    Up,
    Down,
    North,
    South,
}

impl Side {
    const ALL: [Side; 6] = [
        Side::West,
        Side::East,
        Side::Up,
        Side::Down,
        Side::North,
        Side::South,
    ];

    /// The four corners of this side, bottom left first, wound anticlockwise from outside.
    fn corners(self, from: [f32; 3], to: [f32; 3]) -> [[f32; 3]; 4] {
        let ([x0, y0, z0], [x1, y1, z1]) = (from, to);
        match self {
            Side::North => [
                [x1, y0, z0],
                [x0, y0, z0],
                [x0, y1, z0],
                [x1, y1, z0],
            ],
            Side::South => [
                [x0, y0, z1],
                [x1, y0, z1],
                [x1, y1, z1],
                [x0, y1, z1],
            ],
            Side::West => [
                [x0, y0, z0],
                [x0, y0, z1],
                [x0, y1, z1],
                [x0, y1, z0],
            ],
            Side::East => [
                [x1, y0, z1],
                [x1, y0, z0],
                [x1, y1, z0],
                [x1, y1, z1],
            ],
            Side::Up => [
                [x0, y1, z1],
                [x1, y1, z1],
                [x1, y1, z0],
                [x0, y1, z0],
            ],
            Side::Down => [
                [x0, y0, z0],
                [x1, y0, z0],
                [x1, y0, z1],
                [x0, y0, z1],
            ],
        }
    }

    fn normal(self) -> [f32; 3] {
        match self {
            Side::West => [-1.0, 0.0, 0.0],
            Side::East => [1.0, 0.0, 0.0],
            Side::Up => [0.0, 1.0, 0.0],
            Side::Down => [0.0, -1.0, 0.0],
            Side::North => [0.0, 0.0, -1.0],
            Side::South => [0.0, 0.0, 1.0],
        }
    }

    /// Where this side sits in the box's unwrapped cross, in texture pixels.
    fn patch(self, uv: [f32; 2], size: (f32, f32, f32)) -> [f32; 4] {
        let ([u, v], (w, h, d)) = (uv, size);
        match self {
            Side::West => [u, v + d, d, h],
            Side::East => [u + d + w, v + d, d, h],
            Side::Up => [u + d, v, w, d],
            Side::Down => [u + d + w, v, w, d],
            Side::North => [u + d, v + d, w, h],
            Side::South => [u + d + w + d, v + d, w, h],
        }
    }

    /// Which patch a lying box's side takes. The box was unwrapped standing up, so its top is
    /// now its front, its front is now its bottom, and so on round the roll.
    fn lying(self) -> Self {
        match self {
            Side::Up => Side::North,
            Side::Down => Side::South,
            Side::North => Side::Down,
            Side::South => Side::Up,
            other => other,
        }
    }
}

/// Where a model's texture is in the packed sheet.
#[derive(Debug, Clone, Copy)]
pub struct Placed {
    /// Position in the sheet, in sheet pixels.
    pub at: [f32; 2],
    /// The whole sheet's size.
    pub sheet: [f32; 2],
}

impl Placed {
    /// One patch of this texture, in its own pixels, as a rectangle of the whole sheet.
    #[must_use]
    pub fn patch(self, at: [f32; 2], size: [f32; 2]) -> [f32; 4] {
        [
            (self.at[0] + at[0]) / self.sheet[0],
            (self.at[1] + at[1]) / self.sheet[1],
            size[0] / self.sheet[0],
            size[1] / self.sheet[1],
        ]
    }
}

/// Add one model to a mesh, standing at `position`, turned to `yaw`.
pub fn build(
    model: &Model,
    placed: Placed,
    position: [f32; 3],
    yaw: f32,
    light: f32,
    out: (&mut Vec<Vertex>, &mut Vec<u32>),
) {
    let (vertices, indices) = out;
    // A model faces -Z and a yaw of zero faces +Z, so it is turned half a circle to start with.
    let turn = (yaw + 180.0).to_radians();
    let (sin, cos) = (turn.sin(), turn.cos());
    let scale = model.scale / 16.0;
    let place = |local: [f32; 3]| {
        let (lx, ly, lz) = (local[0] * scale, local[1] * scale, local[2] * scale);
        [
            position[0] + lx * cos - lz * sin,
            position[1] + ly,
            position[2] + lx * sin + lz * cos,
        ]
    };

    for cuboid in model.boxes {
        let size = cuboid.texture_size();
        for side in Side::ALL {
            let patch = if cuboid.lying {
                side.lying().patch(cuboid.uv, size)
            } else {
                side.patch(cuboid.uv, size)
            };
            let [px, py, pw, ph] = patch;
            // Into the packed sheet: this texture's own pixels, offset to where it was placed.
            let u = |x: f32| (placed.at[0] + x) / placed.sheet[0];
            let v = |y: f32| (placed.at[1] + y) / placed.sheet[1];
            // A lying box's side faces take a patch that is on its side too, so their corners
            // are mapped a quarter turn round.
            let uvs = if cuboid.lying && matches!(side, Side::West | Side::East) {
                [
                    [u(px), v(py)],
                    [u(px + pw), v(py)],
                    [u(px + pw), v(py + ph)],
                    [u(px), v(py + ph)],
                ]
            } else {
                [
                    [u(px), v(py + ph)],
                    [u(px + pw), v(py + ph)],
                    [u(px + pw), v(py)],
                    [u(px), v(py)],
                ]
            };
            let corners = side.corners(cuboid.from, cuboid.to);
            let normal = side.normal();
            let normal = [
                normal[0] * cos - normal[2] * sin,
                normal[1],
                normal[0] * sin + normal[2] * cos,
            ];
            let first = u32::try_from(vertices.len()).unwrap_or(u32::MAX);
            for (corner, uv) in corners.iter().zip(uvs) {
                vertices.push(Vertex::plain(place(*corner), normal, uv, light));
            }
            // Corners are listed anticlockwise seen from outside, so the triangles are
            // straightforward: 0-1-2 and 0-2-3.
            indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placed() -> Placed {
        Placed {
            at: [0.0, 0.0],
            sheet: [64.0, 64.0],
        }
    }

    #[test]
    fn a_players_face_is_where_every_skin_puts_it() {
        // The front of the head box: eight pixels in and eight down, which is the one coordinate
        // in this file that every Minecraft skin in existence depends on.
        let head = &BIPED[0];
        assert_eq!(
            Side::North.patch(head.uv, head.texture_size()),
            [8.0, 8.0, 8.0, 8.0]
        );
        assert_eq!(
            Side::Up.patch(head.uv, head.texture_size()),
            [8.0, 0.0, 8.0, 8.0],
            "the top of the head is above the face"
        );
    }

    #[test]
    fn every_quad_is_wound_outwards() {
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        build(
            &PLAYER_MODEL,
            placed(),
            [0.0, 0.0, 0.0],
            0.0,
            1.0,
            (&mut vertices, &mut indices),
        );
        assert_eq!(indices.len(), BIPED.len() * 6 * 6, "six faces a box");
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
            assert!(dot > 0.0, "a face pointing {n:?} is wound inwards");
        }
    }

    #[test]
    fn a_player_stands_in_their_own_hitbox() {
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        build(
            &PLAYER_MODEL,
            placed(),
            [10.0, 64.0, -3.0],
            0.0,
            1.0,
            (&mut vertices, &mut indices),
        );
        let top = vertices
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MIN, f32::max);
        let bottom = vertices
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MAX, f32::min);
        assert!((bottom - 64.0).abs() < 1e-4, "feet on the ground: {bottom}");
        assert!(
            (top - (64.0 + 1.875)).abs() < 1e-3,
            "a player is 1.875 blocks of model: {top}"
        );
        // And within half a block of the middle on each side.
        for v in &vertices {
            assert!((v.position[0] - 10.0).abs() <= 0.5);
            assert!((v.position[2] + 3.0).abs() <= 0.5);
        }
    }

    #[test]
    fn turning_a_model_moves_its_front_round() {
        let normals = |yaw: f32| {
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            build(
                &PLAYER_MODEL,
                placed(),
                [0.0, 0.0, 0.0],
                yaw,
                1.0,
                (&mut vertices, &mut indices),
            );
            vertices.iter().map(|v| v.normal).collect::<Vec<_>>()
        };
        // A model's face is its -Z side, and a yaw of zero faces south, so at yaw zero the face
        // has to be pointing +Z.
        assert!(
            normals(0.0).iter().any(|n| n[2] > 0.99),
            "yaw 0 should look south"
        );
        assert!(
            normals(90.0).iter().any(|n| n[0] < -0.99),
            "yaw 90 should look west"
        );
        assert!(
            normals(180.0).iter().any(|n| n[2] < -0.99),
            "yaw 180 should look north"
        );
    }

    #[test]
    fn a_lying_body_takes_the_patch_that_fits_it() {
        let body = &PIG[0];
        assert!(body.lying);
        // Ten wide, sixteen long, eight tall in the world; unwrapped as ten by sixteen by eight.
        assert_eq!(body.texture_size(), (10.0, 16.0, 8.0));
        let top = Side::Up.lying().patch(body.uv, body.texture_size());
        assert_eq!(
            [top[2], top[3]],
            [10.0, 16.0],
            "a pig's back is as long as the pig"
        );
    }

    #[test]
    fn everything_with_a_model_has_a_texture_in_the_list() {
        let textures = textures();
        for (id, model) in MODELS {
            assert!(
                textures.contains(&model.texture),
                "mob {id} needs {}",
                model.texture
            );
        }
        assert!(textures.contains(&"entity/steve"));
    }

    #[test]
    fn things_with_no_model_say_so() {
        assert!(model_for(Kind::Player).is_some());
        assert!(model_for(Kind::Mob(50)).is_some());
        assert!(model_for(Kind::Mob(63)).is_none(), "no ender dragon yet");
        assert!(model_for(Kind::Object(1)).is_none());
    }
}
