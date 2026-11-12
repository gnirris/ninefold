//! ninefold with a window.
//!
//! One thread: the network is polled without blocking, the physics runs on its own 20 Hz clock
//! regardless of frame rate, and whatever is left is spent drawing. Minecraft's own loop works
//! the same way, and it keeps movement identical whether the machine draws at 30 frames or 300.
//!
//!   ninefold-client <host[:port]> [name]

mod atlas;
mod world;

use std::time::{Duration, Instant};

use ninefold_protocol::physics::{Body, Input, TPS};
use ninefold_protocol::play::{self, Session};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

/// Where the camera sits above the player's feet.
const EYE_HEIGHT: f64 = 1.62;
/// Degrees of turn per pixel of mouse movement.
const SENSITIVITY: f64 = 0.15;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let target = args.next().unwrap_or_else(|| "127.0.0.1:25565".to_owned());
    let name = args.next().unwrap_or_else(|| "ninefold".to_owned());
    let (host, port) = match target.rsplit_once(':') {
        Some((host, port)) => (host.to_owned(), port.parse().unwrap_or(25565)),
        None => (target.clone(), 25565),
    };

    println!("connecting to {host}:{port} as {name}…");
    let (mut connection, profile) = ninefold_protocol::login::login(&host, port, &name)?;
    let joined = play::read_join(&mut connection)?;
    println!(
        "logged in as {} — {} world, gamemode {}",
        profile.username, joined.level_type, joined.gamemode
    );
    let mut session = Session::new(connection);
    session.settle(Duration::from_secs(15))?;
    println!(
        "placed at x={:.1} y={:.1} z={:.1}, {} chunks",
        session.position.x,
        session.position.y,
        session.position.z,
        session.columns.len()
    );

    let assets = ninefold_assets::pick(None);
    println!("textures: {}", assets.describe());

    let atlas = atlas::Atlas::build(assets.as_ref());
    println!("atlas: {}x{} pixels", atlas.width, atlas.height);

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(session, atlas);
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct App {
    window: Option<std::sync::Arc<Window>>,
    renderer: Option<ninefold_render::Renderer>,
    session: Session,
    atlas: atlas::Atlas,
    body: Body,
    held: Held,
    yaw: f64,
    pitch: f64,
    /// Time owed to the physics clock, so it ticks at 20 Hz whatever the frame rate.
    owed: Duration,
    last_frame: Instant,
    chunks_when_meshed: usize,
    looking: bool,
}

#[derive(Default)]
struct Held {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    jump: bool,
    sneak: bool,
    sprint: bool,
}

impl App {
    fn new(session: Session, atlas: atlas::Atlas) -> Self {
        let p = session.position;
        Self {
            window: None,
            renderer: None,
            body: Body {
                x: p.x,
                y: p.y,
                z: p.z,
                on_ground: true,
                ..Body::default()
            },
            yaw: f64::from(p.yaw),
            pitch: f64::from(p.pitch),
            session,
            atlas,
            held: Held::default(),
            owed: Duration::ZERO,
            last_frame: Instant::now(),
            chunks_when_meshed: usize::MAX,
            looking: false,
        }
    }

    /// Rebuild the geometry, but only when the world has actually changed.
    fn remesh_if_needed(&mut self) {
        if self.session.columns.len() == self.chunks_when_meshed {
            return;
        }
        self.chunks_when_meshed = self.session.columns.len();
        let (vertices, indices) = world::geometry(&self.session, &self.atlas);
        if let Some(renderer) = &mut self.renderer {
            renderer.set_geometry(&vertices, &indices);
        }
        println!(
            "meshed {} chunks: {} vertices",
            self.chunks_when_meshed,
            vertices.len()
        );
    }

    fn tick_physics(&mut self) {
        let input = Input {
            forward: f64::from(i8::from(self.held.forward) - i8::from(self.held.back)),
            strafe: f64::from(i8::from(self.held.left) - i8::from(self.held.right)),
            jump: self.held.jump,
            sprint: self.held.sprint && self.held.forward,
            sneak: self.held.sneak,
            yaw: self.yaw,
        };
        self.body.tick(input, &self.session);
        let p = &mut self.session.position;
        p.x = self.body.x;
        p.y = self.body.y;
        p.z = self.body.z;
        p.yaw = self.yaw as f32;
        p.pitch = self.pitch as f32;
        p.on_ground = self.body.on_ground;
        // Dropped if the connection has gone; the next pump reports it properly.
        let _ = self.session.send_position();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("ninefold")
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720));
        let Ok(window) = event_loop.create_window(attributes) else {
            eprintln!("could not open a window");
            event_loop.exit();
            return;
        };
        let window = std::sync::Arc::new(window);
        let size = window.inner_size();
        match ninefold_render::Renderer::new(window.clone(), size.width, size.height) {
            Ok(renderer) => self.renderer = Some(renderer),
            Err(e) => {
                eprintln!("no renderer: {e}");
                event_loop.exit();
                return;
            }
        }
        if let Some(renderer) = &mut self.renderer {
            renderer.set_atlas(self.atlas.width, self.atlas.height, &self.atlas.rgba);
        }
        self.window = Some(window);
        self.remesh_if_needed();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::MouseInput { state, .. } => {
                if state == ElementState::Pressed
                    && !self.looking
                    && let Some(window) = &self.window
                {
                    {
                        // Confined rather than locked: locked is not supported everywhere.
                        let _ = window
                            .set_cursor_grab(CursorGrabMode::Locked)
                            .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
                        window.set_cursor_visible(false);
                        self.looking = true;
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let down = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = event.physical_key {
                    match code {
                        KeyCode::KeyW => self.held.forward = down,
                        KeyCode::KeyS => self.held.back = down,
                        KeyCode::KeyA => self.held.left = down,
                        KeyCode::KeyD => self.held.right = down,
                        KeyCode::Space => self.held.jump = down,
                        KeyCode::ShiftLeft => self.held.sneak = down,
                        KeyCode::ControlLeft => self.held.sprint = down,
                        KeyCode::Escape if down => {
                            if let Some(window) = &self.window {
                                let _ = window.set_cursor_grab(CursorGrabMode::None);
                                window.set_cursor_visible(true);
                            }
                            self.looking = false;
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                // Whatever has arrived from the server.
                if let Err(e) = self.session.pump() {
                    eprintln!("connection lost: {e}");
                    event_loop.exit();
                    return;
                }
                self.remesh_if_needed();

                // Physics on its own clock, so movement does not depend on frame rate.
                let tick = Duration::from_secs_f64(1.0 / TPS);
                self.owed += self.last_frame.elapsed();
                self.last_frame = Instant::now();
                // Never run away: after a stall, drop the backlog rather than fast-forwarding.
                self.owed = self.owed.min(tick * 5);
                while self.owed >= tick {
                    self.owed -= tick;
                    self.tick_physics();
                }

                if let Some(renderer) = &mut self.renderer {
                    let eye = [
                        self.body.x as f32,
                        (self.body.y + EYE_HEIGHT) as f32,
                        self.body.z as f32,
                    ];
                    let direction =
                        ninefold_render::camera::facing(self.yaw as f32, self.pitch as f32);
                    renderer.draw(eye, direction);
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event
            && self.looking
        {
            self.yaw += delta.0 * SENSITIVITY;
            // Straight up and straight down, and no further: past vertical the view flips.
            self.pitch = (self.pitch + delta.1 * SENSITIVITY).clamp(-89.9, 89.9);
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}
