//! What are you looking at?
//!
//! Walks the ray from the eye through the grid, visiting every block it passes through in order,
//! rather than stepping along it in small increments and hoping none are missed. Amanatides and
//! Woo's method: work out how far the ray travels to cross the next boundary on each axis, and
//! always advance whichever is nearest.
//!
//! Stepping by a fixed distance instead is the usual shortcut and it is wrong in a way that only
//! shows up at glancing angles — you reach through the corner of a block without noticing it.

use crate::physics::World;

/// Which face of the block was hit, as the protocol numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Bottom = 0,
    Top = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

/// The block the ray met, and from which side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub side: Side,
}

impl Hit {
    /// The empty block next to the one that was hit — where something would be placed.
    #[must_use]
    pub const fn against(&self) -> (i32, i32, i32) {
        match self.side {
            Side::Bottom => (self.x, self.y - 1, self.z),
            Side::Top => (self.x, self.y + 1, self.z),
            Side::North => (self.x, self.y, self.z - 1),
            Side::South => (self.x, self.y, self.z + 1),
            Side::West => (self.x - 1, self.y, self.z),
            Side::East => (self.x + 1, self.y, self.z),
        }
    }
}

/// How far a 1.8 player can reach.
pub const REACH: f64 = 4.5;

/// The first solid block along the ray, within `distance`.
#[must_use]
pub fn cast(world: &impl World, from: [f64; 3], direction: [f64; 3], distance: f64) -> Option<Hit> {
    let length =
        (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2])
            .sqrt();
    if length == 0.0 {
        return None;
    }
    let dir = [
        direction[0] / length,
        direction[1] / length,
        direction[2] / length,
    ];

    let mut block = [
        from[0].floor() as i32,
        from[1].floor() as i32,
        from[2].floor() as i32,
    ];
    // Which way each axis steps, and how far along the ray one whole block is on that axis.
    let step = [sign(dir[0]), sign(dir[1]), sign(dir[2])];
    let delta = [
        if dir[0] == 0.0 {
            f64::INFINITY
        } else {
            (1.0 / dir[0]).abs()
        },
        if dir[1] == 0.0 {
            f64::INFINITY
        } else {
            (1.0 / dir[1]).abs()
        },
        if dir[2] == 0.0 {
            f64::INFINITY
        } else {
            (1.0 / dir[2]).abs()
        },
    ];
    // How far to the first boundary on each axis.
    let mut next = [0.0; 3];
    for axis in 0..3 {
        next[axis] = if dir[axis] == 0.0 {
            f64::INFINITY
        } else if dir[axis] > 0.0 {
            ((block[axis] + 1) as f64 - from[axis]) * delta[axis]
        } else {
            (from[axis] - block[axis] as f64) * delta[axis]
        };
    }

    // Standing inside a block: there is nothing to point at.
    if world.solid(block[0], block[1], block[2]) {
        return None;
    }

    let mut travelled = 0.0;
    while travelled <= distance {
        // Cross whichever boundary comes first.
        let axis = if next[0] < next[1] && next[0] < next[2] {
            0
        } else if next[1] < next[2] {
            1
        } else {
            2
        };
        travelled = next[axis];
        if travelled > distance {
            return None;
        }
        block[axis] += step[axis];
        next[axis] += delta[axis];

        if world.solid(block[0], block[1], block[2]) {
            // The face crossed to get here is the one being looked at, so it faces back
            // along the step.
            let side = match (axis, step[axis]) {
                (0, 1) => Side::West,
                (0, _) => Side::East,
                (1, 1) => Side::Bottom,
                (1, _) => Side::Top,
                (2, 1) => Side::North,
                _ => Side::South,
            };
            return Some(Hit {
                x: block[0],
                y: block[1],
                z: block[2],
                side,
            });
        }
    }
    None
}

const fn sign(v: f64) -> i32 {
    if v > 0.0 { 1 } else { -1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::Floor;

    /// A single block at the origin.
    struct One;
    impl World for One {
        fn solid(&self, x: i32, y: i32, z: i32) -> bool {
            (x, y, z) == (0, 0, 0)
        }
    }

    #[test]
    fn looking_straight_down_finds_the_floor_underfoot() {
        let hit = cast(
            &Floor { ground: 4 },
            [0.5, 6.0, 0.5],
            [0.0, -1.0, 0.0],
            REACH,
        )
        .expect("the floor is right there");
        assert_eq!((hit.x, hit.y, hit.z), (0, 3, 0));
        assert_eq!(hit.side, Side::Top, "hit from above");
        assert_eq!(hit.against(), (0, 4, 0), "you would place on top of it");
    }

    #[test]
    fn a_block_is_hit_from_whichever_side_you_approach() {
        // From the east, looking west.
        let hit = cast(&One, [5.5, 0.5, 0.5], [-1.0, 0.0, 0.0], REACH).expect("hit");
        assert_eq!((hit.x, hit.y, hit.z), (0, 0, 0));
        assert_eq!(hit.side, Side::East);
        assert_eq!(hit.against(), (1, 0, 0));

        // From the south, looking north.
        let hit = cast(&One, [0.5, 0.5, 4.0], [0.0, 0.0, -1.0], REACH).expect("hit");
        assert_eq!(hit.side, Side::South);
        assert_eq!(hit.against(), (0, 0, 1));
    }

    #[test]
    fn nothing_within_reach_is_nothing() {
        assert!(cast(&One, [20.5, 0.5, 0.5], [-1.0, 0.0, 0.0], REACH).is_none());
        assert!(cast(&One, [0.5, 0.5, 0.5], [0.0, 1.0, 0.0], REACH).is_none());
    }

    #[test]
    fn reach_is_measured_along_the_ray_not_per_axis() {
        // A block just beyond reach diagonally: each axis is within 4.5, the distance is not.
        struct Far;
        impl World for Far {
            fn solid(&self, x: i32, y: i32, z: i32) -> bool {
                (x, y, z) == (4, 0, 4)
            }
        }
        assert!(
            cast(&Far, [0.5, 0.5, 0.5], [1.0, 0.0, 1.0], REACH).is_none(),
            "5.0 blocks away diagonally is out of a 4.5 reach"
        );
    }

    #[test]
    fn a_glancing_ray_does_not_slip_through_a_corner() {
        // Two blocks meeting at a corner; a ray along the diagonal must hit one of them rather
        // than pass between. This is what stepping by a fixed distance gets wrong.
        struct Corner;
        impl World for Corner {
            fn solid(&self, x: i32, y: i32, z: i32) -> bool {
                y == 0 && ((x, z) == (1, 0) || (x, z) == (0, 1))
            }
        }
        let hit = cast(&Corner, [0.5, 0.5, 0.5], [1.0, 0.0, 1.0], REACH);
        assert!(hit.is_some(), "the ray must meet one of the two blocks");
    }

    #[test]
    fn standing_inside_a_block_points_at_nothing() {
        assert!(cast(&One, [0.5, 0.5, 0.5], [1.0, 0.0, 0.0], REACH).is_none());
    }
}
