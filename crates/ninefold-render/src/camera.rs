//! The view and projection matrices, written out by hand.
//!
//! A matrix library would do, but these are forty lines, they are the only maths the renderer
//! needs, and getting one of them subtly wrong is a bug you debug by staring at a wrong-looking
//! picture. Written here, they can be tested instead.

/// Column-major 4×4, the layout a GPU wants.
pub type Mat4 = [[f32; 4]; 4];

/// Multiply two matrices, `a` then `b`.
#[must_use]
pub fn multiply(a: Mat4, b: Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

/// Right-handed perspective, with depth in 0..1 the way wgpu wants it rather than the -1..1
/// OpenGL uses. Getting this wrong gives a picture that renders but has broken depth.
#[must_use]
pub fn perspective(fov_y_degrees: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y_degrees.to_radians() / 2.0).tan();
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, near * far / (near - far), 0.0],
    ]
}

/// Look from `eye` along `direction`.
#[must_use]
pub fn look_to(eye: [f32; 3], direction: [f32; 3], up: [f32; 3]) -> Mat4 {
    let f = normalise(direction);
    let s = normalise(cross(f, up));
    let u = cross(s, f);
    [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [-dot(s, eye), -dot(u, eye), dot(f, eye), 1.0],
    ]
}

/// Which way the player is facing, from the protocol's yaw and pitch in degrees.
///
/// Minecraft's yaw is 0 facing south (+z) and turns clockwise, so this is not the usual
/// spherical-coordinates formula and copying one in is how you get a camera that turns the
/// wrong way.
#[must_use]
pub fn facing(yaw_degrees: f32, pitch_degrees: f32) -> [f32; 3] {
    let yaw = yaw_degrees.to_radians();
    let pitch = pitch_degrees.to_radians();
    [
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    ]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalise(v: [f32; 3]) -> [f32; 3] {
    let length = dot(v, v).sqrt();
    if length == 0.0 {
        return v;
    }
    [v[0] / length, v[1] / length, v[2] / length]
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: Mat4 = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];

    fn apply(m: Mat4, v: [f32; 4]) -> [f32; 4] {
        let mut out = [0.0; 4];
        for (i, cell) in out.iter_mut().enumerate() {
            *cell = (0..4).map(|k| m[k][i] * v[k]).sum();
        }
        out
    }

    #[test]
    fn multiplying_by_the_identity_changes_nothing() {
        let m = perspective(70.0, 1.6, 0.1, 1000.0);
        assert_eq!(multiply(m, IDENTITY), m);
        assert_eq!(multiply(IDENTITY, m), m);
    }

    #[test]
    fn the_near_plane_lands_at_depth_zero_and_the_far_plane_at_one() {
        // wgpu wants 0..1, unlike OpenGL's -1..1. This is the test that catches that mistake.
        let m = perspective(70.0, 1.0, 0.1, 100.0);
        let near = apply(m, [0.0, 0.0, -0.1, 1.0]);
        let far = apply(m, [0.0, 0.0, -100.0, 1.0]);
        assert!(
            (near[2] / near[3]).abs() < 1e-5,
            "near should be 0, got {}",
            near[2] / near[3]
        );
        assert!(
            (far[2] / far[3] - 1.0).abs() < 1e-5,
            "far should be 1, got {}",
            far[2] / far[3]
        );
    }

    #[test]
    fn the_camera_puts_what_it_looks_at_in_the_middle() {
        let view = look_to([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
        let ahead = apply(view, [0.0, 0.0, -5.0, 1.0]);
        assert!(ahead[0].abs() < 1e-6 && ahead[1].abs() < 1e-6, "centred");
        assert!(ahead[2] < 0.0, "and in front, not behind");
    }

    #[test]
    fn yaw_zero_faces_south_the_way_minecraft_means_it() {
        let south = facing(0.0, 0.0);
        assert!((south[2] - 1.0).abs() < 1e-6, "+z is south: {south:?}");
        // Minus ninety is east, which is what the walking test used.
        let east = facing(-90.0, 0.0);
        assert!((east[0] - 1.0).abs() < 1e-6, "+x is east: {east:?}");
        // Ninety is west.
        let west = facing(90.0, 0.0);
        assert!((west[0] + 1.0).abs() < 1e-6, "-x is west: {west:?}");
    }

    #[test]
    fn looking_down_points_down() {
        let down = facing(0.0, 90.0);
        assert!(
            (down[1] + 1.0).abs() < 1e-6,
            "pitch 90 is straight down: {down:?}"
        );
        let up = facing(0.0, -90.0);
        assert!(
            (up[1] - 1.0).abs() < 1e-6,
            "pitch -90 is straight up: {up:?}"
        );
    }
}
