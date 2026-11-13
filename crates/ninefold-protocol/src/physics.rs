//! 1.8 player movement.
//!
//! The whole point of targeting 1.8.9 is how it feels to move and fight in it, so these numbers
//! are the project, not an implementation detail. They are checked against the speeds and jump
//! height the game is known for rather than against anything this file decided:
//!
//! | | |
//! |---|---|
//! | walking | 4.317 blocks/second |
//! | sprinting | 5.612 blocks/second |
//! | jump apex | 1.2522 blocks |
//! | terminal fall | 3.92 blocks/tick |
//!
//! The order inside a tick matters as much as the constants. Friction applies to the motion
//! carried in from last tick *before* this tick's input is added, and gravity applies *after*
//! the position has moved. Swap either and the numbers above come out wrong — which is exactly
//! what the tests catch.

/// Twenty ticks a second.
pub const TPS: f64 = 20.0;

/// Pulled off vertical speed every tick.
const GRAVITY: f64 = 0.08;
/// What is left of vertical speed after gravity.
const DRAG: f64 = 0.98;
/// Horizontal drag in the air.
const AIR_DRAG: f64 = 0.91;
/// How slippery ordinary blocks are. Ice is 0.98, packed ice 0.98, slime 0.8.
pub const DEFAULT_SLIPPERINESS: f64 = 0.6;
/// The player's base movement speed attribute.
const WALK_SPEED: f64 = 0.1;
/// Sprinting is thirty percent faster.
const SPRINT: f64 = 1.3;
/// Holding a direction gives 0.98, not 1.0.
const INPUT: f64 = 0.98;
/// Sneaking cuts the input down.
const SNEAK: f64 = 0.3;
/// Straight up, at the start of a jump.
const JUMP: f64 = 0.42;
/// A sprinting jump is shoved forwards as well.
const SPRINT_JUMP: f64 = 0.2;
/// How much steering there is in mid-air. Almost none, which is the 1.8 feel.
const AIR_CONTROL: f64 = 0.02;
const AIR_CONTROL_SPRINT: f64 = 0.026;
/// Motion this small is dropped, so a standing player really stops.
const STOPPED: f64 = 0.005;
/// In water almost nothing carries over from the tick before, which is why swimming feels like
/// wading rather than running.
const WATER_DRAG: f64 = 0.8;
/// Lava is thicker still.
const LAVA_DRAG: f64 = 0.5;
/// You sink slowly rather than falling.
const FLUID_GRAVITY: f64 = 0.02;
/// How hard a stroke pushes you along in a liquid.
const SWIM_PUSH: f64 = 0.02;
/// Holding jump in water swims upwards.
const SWIM_UP: f64 = 0.04;
/// How high a step the player walks up without jumping. Stairs and slabs are built around it,
/// and without it a staircase has to be jumped up one block at a time.
const STEP_HEIGHT: f64 = 0.6;

/// The constant that makes ground acceleration come out at exactly the walk speed:
/// `0.6 * 0.91` cubed.
const GROUND_ACCEL: f64 = 0.163_277_136 - 0.000_505_8;

/// What the player is holding down this tick.
#[derive(Debug, Clone, Copy, Default)]
pub struct Input {
    /// 1.0 forwards, -1.0 backwards.
    pub forward: f64,
    /// 1.0 left, -1.0 right.
    pub strafe: f64,
    pub jump: bool,
    pub sprint: bool,
    pub sneak: bool,
    /// Degrees, as the protocol sends them.
    pub yaw: f64,
}

/// Where the player is and how fast.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Body {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub motion_x: f64,
    pub motion_y: f64,
    pub motion_z: f64,
    pub on_ground: bool,
}

/// A block that fills its cube, for worlds that only know solid from not.
pub const FULL_CUBE: &[([f32; 3], [f32; 3])] = &[([0.0, 0.0, 0.0], [1.0, 1.0, 1.0])];

/// Whatever the player might walk into.
pub trait World {
    /// Is the block at these coordinates something you cannot walk through?
    fn solid(&self, x: i32, y: i32, z: i32) -> bool;

    /// The boxes this block is actually made of, in block-local units.
    ///
    /// Almost nothing in Minecraft fills its cube, and the difference is felt rather than seen:
    /// a slab you can step onto, a snow layer you walk over, a fence you cannot jump. The
    /// default is the old answer — a whole cube or nothing — so a world that knows no better
    /// still works.
    fn boxes(&self, x: i32, y: i32, z: i32) -> &[([f32; 3], [f32; 3])] {
        if self.solid(x, y, z) { FULL_CUBE } else { &[] }
    }

    /// Is it a liquid, and which? Nothing, unless something knows better.
    fn fluid(&self, _x: i32, _y: i32, _z: i32) -> crate::blocks::Fluid {
        crate::blocks::Fluid::None
    }
}

/// Nothing anywhere: useful for testing the integrator on its own.
pub struct Empty;

impl World for Empty {
    fn solid(&self, _x: i32, _y: i32, _z: i32) -> bool {
        false
    }
}

/// Solid everywhere below `ground`, for checking walking and jumping.
pub struct Floor {
    pub ground: i32,
}

impl World for Floor {
    fn solid(&self, _x: i32, y: i32, _z: i32) -> bool {
        y < self.ground
    }
}

/// The player's box: 0.6 across, 1.8 tall, standing on its own position.
const WIDTH: f64 = 0.6;
const HEIGHT: f64 = 1.8;

impl Body {
    /// Which liquid the player is in, judged at the eyes-down middle of the body as 1.8 does.
    fn submerged(&self, world: &impl World) -> crate::blocks::Fluid {
        world.fluid(
            self.x.floor() as i32,
            (self.y + 0.4).floor() as i32,
            self.z.floor() as i32,
        )
    }

    /// Advance one tick in a liquid. Nothing carries over, gravity is gentle, and jumping swims.
    fn tick_in_fluid(&mut self, input: Input, world: &impl World, drag: f64) {
        let mut forward = input.forward * INPUT;
        let mut strafe = input.strafe * INPUT;
        if input.sneak {
            forward *= SNEAK;
            strafe *= SNEAK;
        }
        let push = forward * forward + strafe * strafe;
        if push >= 1e-4 {
            let scale = SWIM_PUSH / push.sqrt().max(1.0);
            let (forward, strafe) = (forward * scale, strafe * scale);
            let yaw = input.yaw.to_radians();
            let (sin, cos) = (yaw.sin(), yaw.cos());
            self.motion_x += strafe * cos - forward * sin;
            self.motion_z += forward * cos + strafe * sin;
        }
        if input.jump {
            self.motion_y += SWIM_UP;
        }

        self.move_by(world);

        self.motion_x *= drag;
        self.motion_y *= drag;
        self.motion_z *= drag;
        self.motion_y -= FLUID_GRAVITY;
    }

    /// Advance one tick.
    pub fn tick(&mut self, input: Input, world: &impl World) {
        match self.submerged(world) {
            crate::blocks::Fluid::Water => return self.tick_in_fluid(input, world, WATER_DRAG),
            crate::blocks::Fluid::Lava => return self.tick_in_fluid(input, world, LAVA_DRAG),
            crate::blocks::Fluid::None => {}
        }
        // Last tick's motion decays first — before this tick's input is added. Doing it the other
        // way round makes walking come out at 2.4 blocks/second instead of 4.317.
        let friction = if self.on_ground {
            DEFAULT_SLIPPERINESS * AIR_DRAG
        } else {
            AIR_DRAG
        };
        self.motion_x *= friction;
        self.motion_z *= friction;

        // How hard the player can push themselves along, which depends on what they are standing
        // on: GROUND_ACCEL / friction³ is 1.0 on ordinary blocks and larger on ice.
        let acceleration = if self.on_ground {
            let speed = WALK_SPEED * if input.sprint { SPRINT } else { 1.0 };
            speed * (GROUND_ACCEL / (friction * friction * friction))
        } else if input.sprint {
            AIR_CONTROL_SPRINT
        } else {
            AIR_CONTROL
        };

        let mut forward = input.forward * INPUT;
        let mut strafe = input.strafe * INPUT;
        if input.sneak {
            forward *= SNEAK;
            strafe *= SNEAK;
        }
        let push = forward * forward + strafe * strafe;
        if push >= 1e-4 {
            // Diagonal input is normalised, so walking at an angle is not faster.
            let scale = acceleration / push.sqrt().max(1.0);
            let (forward, strafe) = (forward * scale, strafe * scale);
            let yaw = input.yaw.to_radians();
            let (sin, cos) = (yaw.sin(), yaw.cos());
            self.motion_x += strafe * cos - forward * sin;
            self.motion_z += forward * cos + strafe * sin;
        }

        if input.jump && self.on_ground {
            self.motion_y = JUMP;
            if input.sprint {
                // A sprint jump keeps its speed by being thrown forwards too.
                let yaw = input.yaw.to_radians();
                self.motion_x -= yaw.sin() * SPRINT_JUMP;
                self.motion_z += yaw.cos() * SPRINT_JUMP;
            }
        }

        // Sneaking at an edge: 1.8 will not let you walk off the block you are standing on.
        // It shaves the step down until there is still ground under you, rather than stopping
        // you dead, which is why you can still shuffle along the very lip of a drop.
        if input.sneak && self.on_ground {
            self.creep(world);
        }

        self.move_by(world);

        // Gravity is applied after the move, which is what puts the jump apex at 1.2522.
        self.motion_y = (self.motion_y - GRAVITY) * DRAG;

        for motion in [&mut self.motion_x, &mut self.motion_z] {
            if motion.abs() < STOPPED {
                *motion = 0.0;
            }
        }
    }

    /// Trim this tick's movement so a sneaking player keeps ground beneath them.
    fn creep(&mut self, world: &impl World) {
        /// What vanilla shaves off at a time.
        const BITE: f64 = 0.05;

        let grounded = |x: f64, z: f64, body: &Body| {
            let probe = Body { x, z, ..*body };
            probe.standing_on(world, body.y)
        };
        while self.motion_x != 0.0 && !grounded(self.x + self.motion_x, self.z, self) {
            self.motion_x = if self.motion_x.abs() < BITE {
                0.0
            } else {
                self.motion_x - BITE.copysign(self.motion_x)
            };
        }
        while self.motion_z != 0.0 && !grounded(self.x, self.z + self.motion_z, self) {
            self.motion_z = if self.motion_z.abs() < BITE {
                0.0
            } else {
                self.motion_z - BITE.copysign(self.motion_z)
            };
        }
        // Both together can still leave the corner hanging, so check the pair as well.
        while self.motion_x != 0.0
            && self.motion_z != 0.0
            && !grounded(self.x + self.motion_x, self.z + self.motion_z, self)
        {
            self.motion_x = if self.motion_x.abs() < BITE {
                0.0
            } else {
                self.motion_x - BITE.copysign(self.motion_x)
            };
            self.motion_z = if self.motion_z.abs() < BITE {
                0.0
            } else {
                self.motion_z - BITE.copysign(self.motion_z)
            };
        }
    }

    /// Move by the current motion, one axis at a time so a wall stops only that direction.
    fn move_by(&mut self, world: &impl World) {
        self.y = self.step(world, 1, self.motion_y);
        self.x = self.step(world, 0, self.motion_x);
        self.z = self.step(world, 2, self.motion_z);
    }

    /// Move along one axis as far as nothing is in the way. Returns the new coordinate.
    fn step(&mut self, world: &impl World, axis: usize, amount: f64) -> f64 {
        let from = [self.x, self.y, self.z][axis];
        if amount == 0.0 {
            if axis == 1 {
                self.on_ground = self.standing_on(world, from);
            }
            return from;
        }
        let to = from + amount;
        let mut moved = [self.x, self.y, self.z];
        moved[axis] = to;
        if self.clear(world, moved) {
            if axis == 1 {
                // Falling into nothing means not on the ground any more.
                self.on_ground = amount < 0.0 && self.standing_on(world, to);
                if self.on_ground {
                    self.motion_y = 0.0;
                }
            }
            return to;
        }
        // Blocked: stop against the face of whatever was hit.
        if axis == 1 {
            if amount < 0.0 {
                self.on_ground = true;
                self.motion_y = 0.0;
                // The top of a slab is where you land on a slab. Snapping to the top of the
                // whole block instead is what made standing on anything half-height impossible.
                return self.surface_below(world, from).unwrap_or(to.floor() + 1.0);
            }
            self.motion_y = 0.0;
            return from;
        }

        // Walking into something: try stepping over it before giving up, which is how anyone
        // gets up a staircase without jumping.
        if self.on_ground {
            let mut raised = moved;
            raised[1] = self.y + STEP_HEIGHT;
            if self.clear(world, raised) {
                let probe = Body {
                    x: raised[0],
                    z: raised[2],
                    ..*self
                };
                if let Some(top) = probe.surface_below(world, raised[1]) {
                    let settled = [raised[0], top, raised[2]];
                    if top > self.y && self.clear(world, settled) {
                        self.y = top;
                        return to;
                    }
                }
            }
        }

        if axis == 0 {
            self.motion_x = 0.0;
        } else {
            self.motion_z = 0.0;
        }
        from
    }

    /// Is the player's box free of solid blocks at this position?
    fn clear(&self, world: &impl World, at: [f64; 3]) -> bool {
        const SLACK: f64 = 1e-7;
        let half = WIDTH / 2.0;
        let (px0, px1) = (at[0] - half, at[0] + half);
        let (py0, py1) = (at[1], at[1] + HEIGHT);
        let (pz0, pz1) = (at[2] - half, at[2] + half);
        let (x0, x1) = (px0.floor() as i32, px1.floor() as i32);
        // One row further down than the feet, because a fence is taller than its own block and
        // would otherwise be walked straight through at the top.
        let (y0, y1) = (py0.floor() as i32 - 1, (py1 - SLACK).floor() as i32);
        let (z0, z1) = (pz0.floor() as i32, pz1.floor() as i32);
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    for (from, to) in world.boxes(x, y, z) {
                        let bx = (f64::from(x) + f64::from(from[0]), f64::from(x) + f64::from(to[0]));
                        let by = (f64::from(y) + f64::from(from[1]), f64::from(y) + f64::from(to[1]));
                        let bz = (f64::from(z) + f64::from(from[2]), f64::from(z) + f64::from(to[2]));
                        if px0 < bx.1 - SLACK
                            && px1 > bx.0 + SLACK
                            && py0 < by.1 - SLACK
                            && py1 > by.0 + SLACK
                            && pz0 < bz.1 - SLACK
                            && pz1 > bz.0 + SLACK
                        {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    /// The highest surface at or below the player's feet that they are standing over.
    ///
    /// This is what makes half-height blocks work: the answer for a slab is the top of the slab,
    /// not the top of the block it is in.
    fn surface_below(&self, world: &impl World, feet: f64) -> Option<f64> {
        const SLACK: f64 = 1e-4;
        let half = WIDTH / 2.0;
        let (x0, x1) = ((self.x - half).floor() as i32, (self.x + half).floor() as i32);
        let (z0, z1) = ((self.z - half).floor() as i32, (self.z + half).floor() as i32);
        let mut best: Option<f64> = None;
        for y in (feet.floor() as i32 - 1)..=(feet.ceil() as i32) {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    for (from, to) in world.boxes(x, y, z) {
                        let top = f64::from(y) + f64::from(to[1]);
                        let bx = (f64::from(x) + f64::from(from[0]), f64::from(x) + f64::from(to[0]));
                        let bz = (f64::from(z) + f64::from(from[2]), f64::from(z) + f64::from(to[2]));
                        let under = self.x - half < bx.1 - 1e-7
                            && self.x + half > bx.0 + 1e-7
                            && self.z - half < bz.1 - 1e-7
                            && self.z + half > bz.0 + 1e-7;
                        if under && top <= feet + SLACK && best.is_none_or(|b| top > b) {
                            best = Some(top);
                        }
                    }
                }
            }
        }
        best
    }

    fn standing_on(&self, world: &impl World, y: f64) -> bool {
        // On the ground means a surface within a hair of the feet, whatever its height: the top
        // of a slab is as much ground as the top of a block.
        self.surface_below(world, y)
            .is_some_and(|top| (y - top).abs() < 1e-4)
    }

    /// Horizontal speed, in blocks per second.
    #[must_use]
    pub fn speed(&self) -> f64 {
        (self.motion_x * self.motion_x + self.motion_z * self.motion_z).sqrt() * TPS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A floor of whole blocks with something of a given height sitting on top of it at x >= 1.
    ///
    /// Enough to tell the three cases apart: walking onto a half-height block, walking into a
    /// full one, and walking into something taller than it looks.
    struct Ledge {
        /// How tall the thing at x >= 1 is, in blocks.
        height: f32,
    }

    impl World for Ledge {
        fn solid(&self, _x: i32, y: i32, _z: i32) -> bool {
            y < 0
        }
        fn boxes(&self, x: i32, y: i32, _z: i32) -> &[([f32; 3], [f32; 3])] {
            if y < 0 {
                return FULL_CUBE;
            }
            if y == 0 && x >= 1 {
                // Leaked once per height, which is fine for a test and keeps the signature
                // borrow-free.
                return match self.height {
                    h if h <= 0.125 => &[([0.0, 0.0, 0.0], [1.0, 0.125, 1.0])],
                    h if h <= 0.5 => &[([0.0, 0.0, 0.0], [1.0, 0.5, 1.0])],
                    h if h <= 1.0 => FULL_CUBE,
                    _ => &[([0.0, 0.0, 0.0], [1.0, 1.5, 1.0])],
                };
            }
            &[]
        }
    }

    /// Walk east for a while and report where the feet ended up.
    fn walk_east(world: &impl World, ticks: u32) -> Body {
        let mut body = Body {
            x: 0.5,
            y: 0.0,
            z: 0.5,
            on_ground: true,
            ..Body::default()
        };
        let input = Input {
            forward: 1.0,
            yaw: -90.0,
            ..Input::default()
        };
        for _ in 0..ticks {
            body.tick(input, world);
        }
        body
    }

    #[test]
    fn you_stand_on_top_of_a_slab_and_not_on_top_of_its_block() {
        let world = Ledge { height: 0.5 };
        let mut body = Body {
            x: 1.5,
            y: 2.0,
            z: 0.5,
            ..Body::default()
        };
        for _ in 0..40 {
            body.tick(Input::default(), &world);
        }
        assert!(
            (body.y - 0.5).abs() < 1e-3,
            "landed at {} rather than on the slab at 0.5",
            body.y
        );
        assert!(body.on_ground);
    }

    #[test]
    fn a_snow_layer_is_an_eighth_of_a_block_to_stand_on() {
        let world = Ledge { height: 0.125 };
        let mut body = Body {
            x: 1.5,
            y: 2.0,
            z: 0.5,
            ..Body::default()
        };
        for _ in 0..40 {
            body.tick(Input::default(), &world);
        }
        assert!((body.y - 0.125).abs() < 1e-3, "landed at {}", body.y);
    }

    #[test]
    fn walking_into_a_slab_steps_up_onto_it() {
        let world = Ledge { height: 0.5 };
        let body = walk_east(&world, 25);
        assert!(body.x > 1.5, "did not get past the slab: x={}", body.x);
        assert!(
            (body.y - 0.5).abs() < 1e-3,
            "should be standing on it at 0.5, not {}",
            body.y
        );
    }

    #[test]
    fn walking_into_a_whole_block_does_not_climb_it() {
        // Six tenths of a block is the limit, and a block is more than that.
        let world = Ledge { height: 1.0 };
        let body = walk_east(&world, 25);
        assert!(body.y < 0.01, "climbed to {} unaided", body.y);
        assert!(body.x < 1.0, "walked through it to x={}", body.x);
    }

    #[test]
    fn a_fence_is_taller_than_its_block_and_stops_you() {
        let world = Ledge { height: 1.5 };
        let body = walk_east(&world, 25);
        assert!(body.x < 1.0, "walked into the fence to x={}", body.x);
        // And jumping does not clear it either.
        let mut jumper = Body {
            x: 0.7,
            y: 0.0,
            z: 0.5,
            on_ground: true,
            ..Body::default()
        };
        let input = Input {
            forward: 1.0,
            jump: true,
            yaw: -90.0,
            ..Input::default()
        };
        for _ in 0..30 {
            jumper.tick(input, &world);
        }
        assert!(jumper.x < 1.0, "jumped the fence to x={}", jumper.x);
    }

    #[test]
    fn a_world_that_only_knows_solid_from_not_still_works() {
        // The default `boxes` is the old answer, so nothing that came before this notices.
        let floor = Floor { ground: 0 };
        assert_eq!(floor.boxes(0, -1, 0), FULL_CUBE);
        assert!(floor.boxes(0, 5, 0).is_empty());
    }

    fn standing() -> Body {
        Body {
            y: 4.0,
            on_ground: true,
            ..Body::default()
        }
    }

    /// Hold forward until the speed stops changing.
    fn run_until_steady(sprint: bool) -> f64 {
        let world = Floor { ground: 4 };
        let mut body = standing();
        let input = Input {
            forward: 1.0,
            sprint,
            ..Input::default()
        };
        for _ in 0..200 {
            body.tick(input, &world);
        }
        body.speed()
    }

    #[test]
    fn walking_settles_at_the_speed_the_game_is_known_for() {
        let speed = run_until_steady(false);
        assert!(
            (speed - 4.317).abs() < 0.001,
            "walking should be 4.317 blocks/second, got {speed}"
        );
    }

    #[test]
    fn sprinting_settles_at_5_612() {
        let speed = run_until_steady(true);
        assert!(
            (speed - 5.612).abs() < 0.001,
            "sprinting should be 5.612 blocks/second, got {speed}"
        );
    }

    #[test]
    fn a_jump_reaches_exactly_1_2522_blocks() {
        let world = Floor { ground: 4 };
        let mut body = standing();
        let jump = Input {
            jump: true,
            ..Input::default()
        };
        body.tick(jump, &world);
        let mut apex: f64 = body.y;
        for _ in 0..30 {
            body.tick(Input::default(), &world);
            apex = apex.max(body.y);
        }
        let height = apex - 4.0;
        assert!(
            (height - 1.2522).abs() < 0.0001,
            "a jump reaches 1.2522 blocks, got {height}"
        );
    }

    #[test]
    fn a_jump_lands_back_on_the_floor() {
        let world = Floor { ground: 4 };
        let mut body = standing();
        body.tick(
            Input {
                jump: true,
                ..Input::default()
            },
            &world,
        );
        for _ in 0..40 {
            body.tick(Input::default(), &world);
        }
        assert!(body.on_ground, "should have landed");
        assert!((body.y - 4.0).abs() < 1e-9, "landed at y={}", body.y);
    }

    #[test]
    fn falling_approaches_terminal_velocity() {
        let mut body = Body {
            y: 256.0,
            ..Body::default()
        };
        // The asymptote is approached by a factor of 0.98 a tick, so 300 ticks is still 0.009
        // short of it; a thousand gets there.
        for _ in 0..1000 {
            body.tick(Input::default(), &Empty);
        }
        // 0.08 * 0.98 / (1 - 0.98), downwards.
        assert!(
            (body.motion_y + 3.92).abs() < 1e-6,
            "terminal fall is -3.92 blocks/tick, got {}",
            body.motion_y
        );
    }

    #[test]
    fn a_standing_player_really_stops() {
        let world = Floor { ground: 4 };
        let mut body = standing();
        let forward = Input {
            forward: 1.0,
            ..Input::default()
        };
        for _ in 0..40 {
            body.tick(forward, &world);
        }
        assert!(body.speed() > 4.0, "was walking");
        for _ in 0..40 {
            body.tick(Input::default(), &world);
        }
        assert_eq!(body.motion_x, 0.0, "motion is dropped once it is tiny");
        assert_eq!(body.motion_z, 0.0);
    }

    /// Diagonal movement really is about 2% faster in 1.8, and it falls out of the same 0.98
    /// that puts walking at 4.317: `moveFlying` only scales input down when its magnitude is
    /// over 1, so a straight line keeps 0.98 while a diagonal (1.386) is normalised to 1.0.
    /// Worth pinning, because "fix" it and walking stops matching the real game.
    #[test]
    fn walking_diagonally_is_the_known_2_percent_faster() {
        let world = Floor { ground: 4 };
        let mut straight = standing();
        let mut diagonal = standing();
        for _ in 0..200 {
            straight.tick(
                Input {
                    forward: 1.0,
                    ..Input::default()
                },
                &world,
            );
            diagonal.tick(
                Input {
                    forward: 1.0,
                    strafe: 1.0,
                    ..Input::default()
                },
                &world,
            );
        }
        let ratio = diagonal.speed() / straight.speed();
        assert!(
            (ratio - 1.0 / 0.98).abs() < 1e-6,
            "diagonal should be exactly 1/0.98 of straight: straight {}, diagonal {}, ratio {ratio}",
            straight.speed(),
            diagonal.speed()
        );
    }

    #[test]
    fn sneaking_is_slower_than_walking() {
        let world = Floor { ground: 4 };
        let mut body = standing();
        for _ in 0..200 {
            body.tick(
                Input {
                    forward: 1.0,
                    sneak: true,
                    ..Input::default()
                },
                &world,
            );
        }
        let speed = body.speed();
        assert!(
            speed > 1.2 && speed < 1.4,
            "sneaking is ~1.3 b/s, got {speed}"
        );
    }

    /// One block to stand on, nothing around it.
    struct Pillar;
    impl World for Pillar {
        fn solid(&self, x: i32, y: i32, z: i32) -> bool {
            (x, y, z) == (0, 3, 0)
        }
    }

    #[test]
    fn walking_off_a_ledge_falls() {
        let mut body = Body {
            x: 0.5,
            y: 4.0,
            z: 0.5,
            on_ground: true,
            ..Body::default()
        };
        let east = Input {
            forward: 1.0,
            yaw: -90.0,
            ..Input::default()
        };
        for _ in 0..40 {
            body.tick(east, &Pillar);
        }
        assert!(
            body.y < 3.0,
            "should have walked off and fallen, y={}",
            body.y
        );
    }

    #[test]
    fn sneaking_at_a_ledge_keeps_you_on_it() {
        let mut body = Body {
            x: 0.5,
            y: 4.0,
            z: 0.5,
            on_ground: true,
            ..Body::default()
        };
        let east_sneaking = Input {
            forward: 1.0,
            sneak: true,
            yaw: -90.0,
            ..Input::default()
        };
        for _ in 0..80 {
            body.tick(east_sneaking, &Pillar);
        }
        assert!(body.on_ground, "should still be standing");
        assert!(
            (body.y - 4.0).abs() < 1e-9,
            "should not have fallen, y={}",
            body.y
        );
        // Shuffled out to the lip and stopped. The box is 0.6 wide and only has to overlap the
        // block, so leaning half a body over the edge is right — it is what vanilla does.
        assert!(
            body.x > 0.5 && body.x <= 1.0 + WIDTH / 2.0,
            "x={} should still be over the block",
            body.x
        );
    }

    #[test]
    fn sneaking_in_the_open_still_moves_normally() {
        let world = Floor { ground: 4 };
        let mut body = standing();
        for _ in 0..40 {
            body.tick(
                Input {
                    forward: 1.0,
                    sneak: true,
                    yaw: -90.0,
                    ..Input::default()
                },
                &world,
            );
        }
        assert!(
            body.x > 2.0,
            "sneaking over solid ground still moves: x={}",
            body.x
        );
    }

    /// Water from y=0 up to `surface`, with a floor under it.
    struct Pool {
        surface: i32,
    }
    impl World for Pool {
        fn solid(&self, _x: i32, y: i32, _z: i32) -> bool {
            y < 0
        }
        fn fluid(&self, _x: i32, y: i32, _z: i32) -> crate::blocks::Fluid {
            if y < self.surface {
                crate::blocks::Fluid::Water
            } else {
                crate::blocks::Fluid::None
            }
        }
    }

    #[test]
    fn you_sink_in_water_rather_than_falling() {
        let pool = Pool { surface: 20 };
        let mut body = Body {
            y: 10.0,
            ..Body::default()
        };
        for _ in 0..40 {
            body.tick(Input::default(), &pool);
        }
        // Terminal sink is 0.02 * 0.8 / (1 - 0.8) = 0.08 a tick, nowhere near the 3.92 of air.
        assert!(
            body.motion_y > -0.12 && body.motion_y < -0.05,
            "should sink gently, got {}",
            body.motion_y
        );
    }

    #[test]
    fn holding_jump_swims_upwards() {
        let pool = Pool { surface: 20 };
        let mut body = Body {
            y: 10.0,
            ..Body::default()
        };
        let swim = Input {
            jump: true,
            ..Input::default()
        };
        for _ in 0..40 {
            body.tick(swim, &pool);
        }
        assert!(body.y > 10.0, "should have risen, y={}", body.y);
    }

    #[test]
    fn swimming_is_slower_than_walking() {
        let pool = Pool { surface: 20 };
        let mut body = Body {
            y: 10.0,
            ..Body::default()
        };
        for _ in 0..60 {
            body.tick(
                Input {
                    forward: 1.0,
                    ..Input::default()
                },
                &pool,
            );
        }
        let speed = body.speed();
        assert!(
            speed > 0.5 && speed < 3.0,
            "swimming is a wade, got {speed} b/s"
        );
    }

    #[test]
    fn a_wall_stops_only_the_direction_it_blocks() {
        struct Wall;
        impl World for Wall {
            fn solid(&self, x: i32, y: i32, _z: i32) -> bool {
                y < 4 || x >= 2
            }
        }
        let mut body = Body {
            x: 0.0,
            y: 4.0,
            z: 0.0,
            on_ground: true,
            ..Body::default()
        };
        // Facing +x: yaw -90 points east.
        let into_wall = Input {
            forward: 1.0,
            yaw: -90.0,
            ..Input::default()
        };
        for _ in 0..100 {
            body.tick(into_wall, &Wall);
        }
        assert!(body.x < 2.0, "stopped by the wall at x={}", body.x);
        assert!(body.on_ground, "still standing");
    }
}
