//! ninefold with a window.
//!
//! One thread: the network is polled without blocking, the physics runs on its own 20 Hz clock
//! regardless of frame rate, and whatever is left is spent drawing. Minecraft's own loop works
//! the same way, and it keeps movement identical whether the machine draws at 30 frames or 300.
//!
//!   ninefold-client <host[:port]> [name]

mod atlas;
mod gui;
mod world;

use std::time::{Duration, Instant};

use ninefold_protocol::physics::{Body, Input, TPS};
use ninefold_protocol::play::{self, Session};
use ninefold_protocol::raycast;
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
    // The font comes out of the same place as the block textures.
    let font = assets.texture("font/ascii");
    if font.is_none() {
        println!("no font/ascii in the assets: the readout will be invisible");
    }
    let sheets = gui::sheets(assets.as_ref());
    let mut app = App::new(session, atlas, font, sheets);
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
    /// Where the player was at the end of the previous tick. The camera is drawn somewhere
    /// between this and where they are now, because 20 positions a second is visibly steppy
    /// however fast the frames come.
    previous: [f64; 3],
    /// Time owed to the physics clock, so it ticks at 20 Hz whatever the frame rate.
    owed: Duration,
    last_frame: Instant,
    /// Columns already given to the renderer, so only new or changed ones are meshed.
    meshed: std::collections::HashSet<(i32, i32)>,
    looking: bool,
    /// The block under the crosshair, if any.
    aimed: Option<raycast::Hit>,
    /// Frames drawn since `counted_from`, for the readout.
    frames: u32,
    counted_from: Instant,
    fps: f64,
    /// The font sheet, kept until the renderer exists to take it.
    font: Option<ninefold_assets::Texture>,
    /// The interface sheets, likewise.
    sheets: gui::Sheets,
    /// Window size, for laying the interface out.
    screen: (u32, u32),
    /// The readout, as F3 does in the real game.
    debug: bool,
    /// Hitboxes, as F3+B does.
    hitboxes: bool,
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
    fn new(
        session: Session,
        atlas: atlas::Atlas,
        font: Option<ninefold_assets::Texture>,
        sheets: gui::Sheets,
    ) -> Self {
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
            previous: [p.x, p.y, p.z],
            yaw: f64::from(p.yaw),
            pitch: f64::from(p.pitch),
            session,
            atlas,
            held: Held::default(),
            owed: Duration::ZERO,
            last_frame: Instant::now(),
            meshed: std::collections::HashSet::new(),
            looking: false,
            aimed: None,
            frames: 0,
            counted_from: Instant::now(),
            fps: 0.0,
            font,
            sheets,
            screen: (1280, 720),
            debug: false,
            hitboxes: false,
        }
    }

    /// Mesh the columns that need it, a few at a time.
    ///
    /// Meshing a column takes a few milliseconds, so doing every new one in a single frame is a
    /// visible freeze when a handful arrive at once. A budget per frame spreads it out: terrain
    /// fills in over a few frames instead of the window locking up.
    fn mesh_some(&mut self) {
        const PER_FRAME: usize = 2;

        // Columns the server has unloaded: drop their buffers.
        let gone: Vec<_> = self
            .meshed
            .iter()
            .copied()
            .filter(|at| !self.session.columns.contains_key(at))
            .collect();
        for at in gone {
            self.meshed.remove(&at);
            if let Some(renderer) = &mut self.renderer {
                renderer.remove_column(at);
            }
        }

        // New or changed columns, nearest first so what you are looking at fills in soonest.
        let here = (
            (self.body.x / 16.0).floor() as i32,
            (self.body.z / 16.0).floor() as i32,
        );
        let mut todo: Vec<(i32, i32)> = self
            .session
            .columns
            .keys()
            .copied()
            .filter(|at| !self.meshed.contains(at) || self.session.dirty.contains(at))
            .collect();
        todo.sort_by_key(|at| (at.0 - here.0).pow(2) + (at.1 - here.1).pow(2));

        for at in todo.into_iter().take(PER_FRAME) {
            let (vertices, indices) = world::column(&self.session, &self.atlas, at);
            if let Some(renderer) = &mut self.renderer {
                renderer.set_column(at, &vertices, &indices);
            }
            self.meshed.insert(at);
            self.session.dirty.remove(&at);
        }
    }

    fn tick_physics(&mut self) {
        self.previous = [self.body.x, self.body.y, self.body.z];
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
        self.screen = (size.width, size.height);
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
            if let Some(font) = &self.font {
                renderer.set_font(font.width, font.height, &font.rgba);
            }
            renderer.set_gui_sheets(self.sheets.width, self.sheets.height, &self.sheets.rgba);
        }
        self.window = Some(window);
        self.mesh_some();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.screen = (size.width, size.height);
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Pressed && self.looking {
                    // Looking already: a click is a swing at whatever is under the crosshair.
                    if button == winit::event::MouseButton::Left
                        && let Some(hit) = self.aimed
                    {
                        if let Err(e) = self.session.dig(hit.x, hit.y, hit.z, hit.side as u8) {
                            eprintln!("could not dig: {e}");
                        } else {
                            // Take it out locally too, rather than waiting for the server to
                            // tell us what we already know.
                            self.session.set_block(hit.x, hit.y, hit.z, 0);
                        }
                    } else if let Err(e) = self.session.swing() {
                        eprintln!("could not swing: {e}");
                    }
                }
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
                        KeyCode::F3 if down => self.debug = !self.debug,
                        KeyCode::KeyB if down && self.debug => self.hitboxes = !self.hitboxes,
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
                self.mesh_some();

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

                {
                    let eye = [self.body.x, self.body.y + EYE_HEIGHT, self.body.z];
                    let facing =
                        ninefold_render::camera::facing(self.yaw as f32, self.pitch as f32);
                    let direction = [
                        f64::from(facing[0]),
                        f64::from(facing[1]),
                        f64::from(facing[2]),
                    ];
                    let aimed = raycast::cast(&self.session, eye, direction, raycast::REACH);
                    if aimed != self.aimed {
                        self.aimed = aimed;
                        if let Some(renderer) = &mut self.renderer {
                            renderer.set_highlight(aimed.map(|h| (h.x, h.y, h.z)));
                        }
                    }
                }

                // The game's own HUD, laid out in interface units and drawn from its own art.
                {
                    let mut hud = gui::Hud::new(self.screen.0, self.screen.1);
                    hud.lay_out(self.session.held, self.session.health, self.session.food);
                    let quads = hud.finish();
                    if let Some(renderer) = &mut self.renderer {
                        renderer.set_sprites(&quads);
                    }
                }

                // The readout, and whatever has been said.
                {
                    self.frames += 1;
                    let counted = self.counted_from.elapsed();
                    if counted >= Duration::from_millis(500) {
                        self.fps = f64::from(self.frames) / counted.as_secs_f64();
                        self.frames = 0;
                        self.counted_from = Instant::now();
                    }
                    let white = [1.0, 1.0, 1.0, 0.95];
                    let dim = [0.75, 0.78, 0.85, 0.9];
                    let mut lines = if !self.debug {
                        Vec::new()
                    } else {
                        vec![
                            (format!("ninefold {}", env!("CARGO_PKG_VERSION")), white),
                            (
                                format!(
                                    "xyz {:.1} {:.1} {:.1}   chunk {} {}",
                                    self.body.x,
                                    self.body.y,
                                    self.body.z,
                                    (self.body.x / 16.0).floor() as i32,
                                    (self.body.z / 16.0).floor() as i32
                                ),
                                dim,
                            ),
                            (
                                format!(
                                    "{:.0} fps   {} chunks   {} entities",
                                    self.fps,
                                    self.meshed.len(),
                                    self.session.entities.len()
                                ),
                                dim,
                            ),
                            (
                                match self.aimed {
                                    Some(hit) => {
                                        let block =
                                            self.session.block_at(hit.x, hit.y, hit.z).unwrap_or(0);
                                        format!(
                                            "looking at {} {} {}  id {} meta {}",
                                            hit.x,
                                            hit.y,
                                            hit.z,
                                            block >> 4,
                                            block & 0xF
                                        )
                                    }
                                    None => "looking at nothing".to_owned(),
                                },
                                dim,
                            ),
                        ]
                    };
                    // The last few things said, oldest first, under the readout.
                    let said = self.session.chat.len().saturating_sub(5);
                    for line in &self.session.chat[said..] {
                        lines.push((line.clone(), [1.0, 0.95, 0.7, 0.95]));
                    }
                    if let Some(renderer) = &mut self.renderer {
                        renderer.set_text(&lines);
                    }
                }

                // Boxes round everything else in the world.
                {
                    let boxes: Vec<_> = if !self.hitboxes {
                        Vec::new()
                    } else {
                        self.session
                            .entities
                            .values()
                            .map(|e| {
                                let (w, h) = e.kind.size();
                                let colour = match e.kind {
                                    play::Kind::Player => [1.0, 1.0, 1.0, 0.9],
                                    play::Kind::Mob(_) => [1.0, 0.4, 0.3, 0.85],
                                    play::Kind::Object(_) => [0.5, 0.8, 1.0, 0.8],
                                };
                                ([e.x as f32, e.y as f32, e.z as f32], [w, h, w], colour)
                            })
                            .collect()
                    };
                    if let Some(renderer) = &mut self.renderer {
                        renderer.set_entities(&boxes);
                    }
                }

                if let Some(renderer) = &mut self.renderer {
                    // How far into the next tick we are, so the camera glides between the two
                    // positions instead of stepping twenty times a second.
                    let alpha = (self.owed.as_secs_f64() * TPS).clamp(0.0, 1.0);
                    let between = |from: f64, to: f64| from + (to - from) * alpha;
                    let eye = [
                        between(self.previous[0], self.body.x) as f32,
                        (between(self.previous[1], self.body.y) + EYE_HEIGHT) as f32,
                        between(self.previous[2], self.body.z) as f32,
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
