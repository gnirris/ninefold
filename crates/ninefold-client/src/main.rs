//! ninefold with a window.
//!
//! One thread: the network is polled without blocking, the physics runs on its own 20 Hz clock
//! regardless of frame rate, and whatever is left is spent drawing. Minecraft's own loop works
//! the same way, and it keeps movement identical whether the machine draws at 30 frames or 300.
//!
//! The one thing that is not on this thread is connecting, because a server that takes ten
//! seconds to answer would otherwise be ten seconds of a frozen window.
//!
//!   ninefold-client <host[:port]> [name] [--join]
//!
//! `--join` skips the title screen and connects straight away, which is what a launcher wants
//! and what a test wants; without it the client starts where the game starts.

mod atlas;
mod font;
mod gui;
mod items;
mod models;
mod screens;
mod sound;
mod world;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ninefold_protocol::physics::{Body, Input, TPS};
use ninefold_protocol::play::{self, HOTBAR_START, Session};
use ninefold_protocol::raycast;
use ninefold_protocol::wire::Connection;
use ninefold_render::Vertex;
use screens::{Action, Screen};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

/// Where the camera sits above the player's feet.
const EYE_HEIGHT: f64 = 1.62;
/// Degrees of turn per pixel of mouse movement.
const SENSITIVITY: f64 = 0.15;
/// What the desktop knows this window as, matching `ninefold.desktop`.
const APP_ID: &str = "ninefold";
/// How much of the way an entity is moved towards where the server says it is, per frame.
///
/// The server sends an entity's position twenty times a second at best, and often less; drawing
/// it exactly where the last packet put it makes everything else in the world stutter even when
/// the player's own movement is smooth.
const FOLLOW: f64 = 0.35;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let straight_in = args.iter().any(|a| a == "--join");
    let mut plain = args.iter().filter(|a| !a.starts_with("--"));
    let target = plain
        .next()
        .cloned()
        .unwrap_or_else(|| "127.0.0.1:25565".to_owned());
    let name = plain.next().cloned().unwrap_or_else(|| "ninefold".to_owned());
    let (host, port) = match target.rsplit_once(':') {
        Some((host, port)) => (host.to_owned(), port.parse().unwrap_or(25565)),
        None => (target.clone(), 25565),
    };

    let assets = ninefold_assets::pick(None);
    println!("textures: {}", assets.describe());
    let atlas = atlas::Atlas::build(assets.as_ref());
    println!("atlas: {}x{} pixels", atlas.width, atlas.height);
    let items = items::Items::build(assets.as_ref());
    println!("items: {} pictures", items.len());
    let sheet = assets.texture("font/ascii");
    if sheet.is_none() {
        println!("no font/ascii in the assets: there will be no text");
    }
    let font = font::Font::measure(sheet.as_ref());
    if sheet.is_some() {
        println!("font: measured off the game's own sheet");
    }
    let sheets = gui::sheets(assets.as_ref());
    let mobs = mob_sheet(assets.as_ref());
    let audio = sound::Audio::open();
    if audio.working() {
        println!("sound: {}", audio.describe());
    } else {
        println!("sound: silent — {}", audio.describe());
    }
    if items.is_empty() {
        println!("no item pictures in the assets: slots will show blocks only");
    }

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(
        Art {
            atlas,
            items,
            font,
            font_sheet: sheet,
            sheets,
            mobs,
        },
        Server {
            host,
            port,
            name,
            shown: target,
            straight_in,
        },
        audio,
    );
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Every picture the client needs, loaded once before the window opens.
struct Art {
    atlas: atlas::Atlas,
    items: items::Items,
    font: font::Font,
    /// The font's own picture, kept until the renderer exists to take it.
    font_sheet: Option<ninefold_assets::Texture>,
    sheets: gui::Sheets,
    mobs: Mobs,
}

/// Where we are trying to connect to.
struct Server {
    host: String,
    port: u16,
    name: String,
    /// What to call it on the title screen.
    shown: String,
    /// Skip the title screen entirely.
    straight_in: bool,
}

/// Every mob texture in one sheet, with where each one landed.
struct Mobs {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    placed: HashMap<&'static str, models::Placed>,
}

/// Pack the mob textures into a grid. They are all 64 wide and at most 64 tall.
fn mob_sheet(assets: &dyn ninefold_assets::AssetSource) -> Mobs {
    const CELL: u32 = 64;
    const ACROSS: u32 = 4;
    let names = models::textures();
    let down = (u32::try_from(names.len()).unwrap_or(1)).div_ceil(ACROSS).max(1);
    let (width, height) = (ACROSS * CELL, down * CELL);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let mut placed = HashMap::new();
    for (index, name) in names.iter().enumerate() {
        let index = index as u32;
        let (ox, oy) = ((index % ACROSS) * CELL, (index / ACROSS) * CELL);
        let Some(texture) = assets.texture(name) else {
            continue;
        };
        for row in 0..texture.height.min(CELL) {
            let src = (row * texture.width * 4) as usize;
            let dst = (((oy + row) * width + ox) * 4) as usize;
            let run = (texture.width.min(CELL) * 4) as usize;
            if src + run <= texture.rgba.len() && dst + run <= rgba.len() {
                rgba[dst..dst + run].copy_from_slice(&texture.rgba[src..src + run]);
            }
        }
        placed.insert(
            *name,
            models::Placed {
                at: [ox as f32, oy as f32],
                sheet: [width as f32, height as f32],
            },
        );
    }
    Mobs {
        width,
        height,
        rgba,
        placed,
    }
}

/// What the connecting thread sends back.
type Arrival = Result<(Connection, play::Joined), String>;

struct App {
    window: Option<std::sync::Arc<Window>>,
    renderer: Option<ninefold_render::Renderer>,
    art: Art,
    server: Server,
    audio: sound::Audio,

    /// Which screen is in front of the world, and the world itself once there is one.
    screen: Screen,
    session: Option<Session>,
    /// The thread that is trying to connect, if one is.
    connecting: Option<mpsc::Receiver<Arrival>>,
    since: Instant,
    /// Why the connection ended, for the screen that says so.
    failure: String,

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
    meshed: HashSet<(i32, i32)>,
    /// Where each entity is being drawn, as against where the server last said it is.
    smoothed: HashMap<i32, [f64; 3]>,
    looking: bool,
    /// Where the mouse is, in window pixels, for the menus.
    mouse: (f64, f64),
    /// What can be clicked on the screen in front of the world.
    hits: Vec<screens::Hit>,
    /// What is being typed into the chat box.
    typed: String,
    /// The block under the crosshair, if any.
    aimed: Option<raycast::Hit>,
    /// Frames drawn since `counted_from`, for the readout.
    frames: u32,
    counted_from: Instant,
    fps: f64,
    /// Window size, for laying the interface out.
    size: (u32, u32),
    /// The readout, as F3 does in the real game.
    debug: bool,
    /// Hitboxes, as F3+B does.
    hitboxes: bool,
    /// The block being broken and how long it still needs.
    breaking: Option<(raycast::Hit, f32)>,
    /// Is the left button down?
    swinging: bool,
    /// Ticks spent breaking, so the tool taps rather than hums.
    taps: u32,
    /// The teleport count the physics has already followed.
    followed_teleports: u32,
    /// Whether the "we are in" line has been printed for this connection.
    announced: bool,
    /// What the server has been told about crouching and running.
    told_sneaking: bool,
    told_sprinting: bool,
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
    fn new(art: Art, server: Server, audio: sound::Audio) -> Self {
        Self {
            window: None,
            renderer: None,
            art,
            server,
            audio,
            screen: Screen::Title,
            session: None,
            connecting: None,
            since: Instant::now(),
            failure: String::new(),
            body: Body::default(),
            previous: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            held: Held::default(),
            owed: Duration::ZERO,
            last_frame: Instant::now(),
            meshed: HashSet::new(),
            smoothed: HashMap::new(),
            looking: false,
            mouse: (0.0, 0.0),
            hits: Vec::new(),
            typed: String::new(),
            aimed: None,
            frames: 0,
            counted_from: Instant::now(),
            fps: 0.0,
            size: (1280, 720),
            debug: false,
            hitboxes: false,
            breaking: None,
            swinging: false,
            taps: 0,
            followed_teleports: 0,
            announced: false,
            told_sneaking: false,
            told_sprinting: false,
        }
    }

    /// Start connecting, on a thread of its own.
    fn connect(&mut self) {
        if self.connecting.is_some() {
            return;
        }
        let (host, port, name) = (
            self.server.host.clone(),
            self.server.port,
            self.server.name.clone(),
        );
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = ninefold_protocol::login::login(&host, port, &name)
                .and_then(|(mut connection, _profile)| {
                    play::read_join(&mut connection).map(|joined| (connection, joined))
                })
                .map_err(|e| e.to_string());
            // Nobody to tell means the window has gone, which is not an error.
            let _ = send.send(outcome);
        });
        self.connecting = Some(receive);
        self.screen = Screen::Connecting;
        self.since = Instant::now();
    }

    /// Take whatever the connecting thread has to say.
    fn settle_connection(&mut self) {
        let Some(receive) = &self.connecting else {
            return;
        };
        match receive.try_recv() {
            Ok(Ok((connection, joined))) => {
                println!(
                    "logged in — {} world, gamemode {}",
                    joined.level_type, joined.gamemode
                );
                self.session = Some(Session::joined(connection, &joined));
                self.connecting = None;
                self.screen = Screen::None;
                self.followed_teleports = 0;
                self.announced = false;
                self.meshed.clear();
                self.smoothed.clear();
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_mobs(&[], &[]);
                }
                self.grab(true);
            }
            Ok(Err(why)) => {
                self.failure = why;
                self.connecting = None;
                self.screen = Screen::Lost;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.failure = "the connection attempt stopped without saying why".to_owned();
                self.connecting = None;
                self.screen = Screen::Lost;
            }
        }
    }

    /// Put the world away and go back to the title screen.
    fn disconnect(&mut self, why: Option<String>) {
        self.session = None;
        self.connecting = None;
        self.breaking = None;
        self.swinging = false;
        for at in self.meshed.drain() {
            if let Some(renderer) = &mut self.renderer {
                renderer.remove_column(at);
            }
        }
        self.smoothed.clear();
        if let Some(renderer) = &mut self.renderer {
            renderer.set_mobs(&[], &[]);
            renderer.set_highlight(None);
            renderer.set_boxes(&[]);
        }
        self.grab(false);
        match why {
            Some(why) => {
                self.failure = why;
                self.screen = Screen::Lost;
            }
            None => self.screen = Screen::Title,
        }
    }

    /// Hold the mouse, or let it go.
    fn grab(&mut self, hold: bool) {
        self.looking = hold;
        let Some(window) = &self.window else {
            return;
        };
        if hold {
            // Confined rather than locked: locked is not supported everywhere.
            let _ = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
            window.set_cursor_visible(false);
        } else {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
        }
    }

    /// Mesh the columns that need it, a few at a time.
    ///
    /// Meshing a column takes a few milliseconds, so doing every new one in a single frame is a
    /// visible freeze when a handful arrive at once. A budget per frame spreads it out: terrain
    /// fills in over a few frames instead of the window locking up.
    fn mesh_some(&mut self) {
        const PER_FRAME: usize = 2;
        let Some(session) = &mut self.session else {
            return;
        };

        // Columns the server has unloaded: drop their buffers.
        let gone: Vec<_> = self
            .meshed
            .iter()
            .copied()
            .filter(|at| !session.columns.contains_key(at))
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
        let mut todo: Vec<(i32, i32)> = session
            .columns
            .keys()
            .copied()
            .filter(|at| !self.meshed.contains(at) || session.dirty.contains(at))
            .collect();
        todo.sort_by_key(|at| (at.0 - here.0).pow(2) + (at.1 - here.1).pow(2));

        for at in todo.into_iter().take(PER_FRAME) {
            let (vertices, indices) = world::column(session, &self.art.atlas, at);
            if let Some(renderer) = &mut self.renderer {
                renderer.set_column(at, &vertices, &indices);
            }
            self.meshed.insert(at);
            session.dirty.remove(&at);
        }
    }

    /// Keep breaking whatever is under the crosshair, if the button is down.
    ///
    /// A 1.8 server times the dig itself and ignores a "finished" that comes too soon, so this
    /// has to wait out the block's own break time rather than firing both packets at once.
    fn tick_breaking(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(hit) = self.aimed.filter(|_| self.swinging) else {
            // Nothing aimed at, or the button is up.
            if let Some((hit, _)) = self.breaking.take() {
                let _ = session.dig_cancel(hit.x, hit.y, hit.z, hit.side as u8);
            }
            return;
        };

        // Looking at something else now: start again on the new block.
        if self
            .breaking
            .is_none_or(|(old, _)| (old.x, old.y, old.z) != (hit.x, hit.y, hit.z))
        {
            if let Some((old, _)) = self.breaking.take() {
                let _ = session.dig_cancel(old.x, old.y, old.z, old.side as u8);
            }
            let block = session.block_at(hit.x, hit.y, hit.z).unwrap_or(0);
            // Creative takes everything out in one hit, as it does in the real game.
            let needs = if session.creative() {
                0.0
            } else {
                atlas::break_seconds(block)
            };
            if let Err(e) = session.dig_start(hit.x, hit.y, hit.z, hit.side as u8) {
                eprintln!("could not start digging: {e}");
                return;
            }
            self.breaking = Some((hit, needs));
            self.taps = 0;
        }

        let Some((hit, left)) = &mut self.breaking else {
            return;
        };
        *left -= 1.0 / TPS as f32;
        if *left > 0.0 {
            let _ = session.swing();
            self.taps += 1;
            // A tap every quarter second, which is roughly vanilla's own cadence.
            if self.taps.is_multiple_of(5) {
                let block = session.block_at(hit.x, hit.y, hit.z).unwrap_or(0);
                self.audio.digging(block);
            }
            return;
        }
        // Done: tell the server, and take it out here too rather than waiting to be told what
        // we already know.
        let (x, y, z, side) = (hit.x, hit.y, hit.z, hit.side as u8);
        self.breaking = None;
        let block = session.block_at(x, y, z).unwrap_or(0);
        if let Err(e) = session.dig_finish(x, y, z, side) {
            eprintln!("could not finish digging: {e}");
            return;
        }
        session.set_block(x, y, z, 0);
        let ear = [self.body.x, self.body.y + EYE_HEIGHT, self.body.z];
        let at = [f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(z) + 0.5];
        self.audio.broke(block, at, ear, self.yaw as f32);
    }

    /// Put the held block against whatever is being looked at.
    fn place(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(hit) = self.aimed else {
            // Nothing in reach: using the item is still a thing you can do.
            let _ = session.use_held();
            return;
        };
        let held = session.held_item();
        let (nx, ny, nz) = hit.against();
        // The middle of the face, which is what the server uses to decide which half of a slab
        // or which way a stair faces. Without a hit point from the raycast this is the best
        // guess, and for a full block it makes no difference at all.
        if let Err(e) = session.place((hit.x, hit.y, hit.z), hit.side as u8, (8, 8, 8)) {
            eprintln!("could not place: {e}");
            return;
        }
        let _ = session.swing();
        // Show it immediately rather than waiting a round trip, the way vanilla does.
        if let Some(block) = held.and_then(ninefold_protocol::item::Slot::as_block) {
            // Never inside our own feet or head.
            let standing = (
                self.body.x.floor() as i32,
                self.body.y.floor() as i32,
                self.body.z.floor() as i32,
            );
            if (nx, ny, nz) != standing && (nx, ny, nz) != (standing.0, standing.1 + 1, standing.2)
            {
                session.set_block(nx, ny, nz, block);
                let ear = [self.body.x, self.body.y + EYE_HEIGHT, self.body.z];
                let at = [
                    f64::from(nx) + 0.5,
                    f64::from(ny) + 0.5,
                    f64::from(nz) + 0.5,
                ];
                self.audio.placed(block, at, ear, self.yaw as f32);
            }
        }
    }

    fn tick_physics(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        self.previous = [self.body.x, self.body.y, self.body.z];
        if session.dead {
            // Nothing to send and nothing to move: the server is waiting to be asked for a
            // respawn, and anything else it would ignore.
            return;
        }
        // A shove from the server — an explosion, a mob's hit — happened to us, not to our idea
        // of ourselves, so it goes straight onto the motion.
        if let Some((vx, vy, vz)) = session.pushed.take() {
            self.body.motion_x += vx;
            self.body.motion_y += vy;
            self.body.motion_z += vz;
        }
        // While a menu is open the player stands still, but the world keeps going.
        let moving = !self.screen.modal();
        let input = Input {
            forward: if moving {
                f64::from(i8::from(self.held.forward) - i8::from(self.held.back))
            } else {
                0.0
            },
            strafe: if moving {
                f64::from(i8::from(self.held.left) - i8::from(self.held.right))
            } else {
                0.0
            },
            jump: moving && self.held.jump,
            sprint: moving && self.held.sprint && self.held.forward,
            sneak: moving && self.held.sneak,
            yaw: self.yaw,
        };
        let was = (self.body.x, self.body.z);
        self.body.tick(input, session);
        let p = &mut session.position;
        p.x = self.body.x;
        p.y = self.body.y;
        p.z = self.body.z;
        p.yaw = self.yaw as f32;
        p.pitch = self.pitch as f32;
        p.on_ground = self.body.on_ground;
        // Dropped if the connection has gone; the next pump reports it properly.
        let _ = session.send_position();

        // Crouching and sprinting are states the server tracks, not things it works out: other
        // players never see you crouch unless it is told, and its anti-cheat does not expect
        // you to be moving at sprinting speed either.
        if input.sneak != self.told_sneaking {
            self.told_sneaking = input.sneak;
            let _ = session.entity_action(if input.sneak { 0 } else { 1 });
        }
        if input.sprint != self.told_sprinting {
            self.told_sprinting = input.sprint;
            let _ = session.entity_action(if input.sprint { 3 } else { 4 });
        }

        let moved = ((self.body.x - was.0).powi(2) + (self.body.z - was.1).powi(2)).sqrt();
        let under = session
            .block_at(
                self.body.x.floor() as i32,
                (self.body.y - 0.2).floor() as i32,
                self.body.z.floor() as i32,
            )
            .unwrap_or(0);
        let on_ground = self.body.on_ground;
        self.audio.walking(moved, under, !on_ground);
    }

    /// Everything the server has told us to play, and anything we noticed ourselves.
    fn tick_sounds(&mut self) {
        let ear = [self.body.x, self.body.y + EYE_HEIGHT, self.body.z];
        let yaw = self.yaw as f32;
        let (queued, hurt) = match self.session.as_mut() {
            Some(session) => (
                std::mem::take(&mut session.sounds),
                std::mem::take(&mut session.hurt),
            ),
            None => return,
        };
        for sound in queued {
            self.audio.at(
                &sound.name,
                (&[sound.x, sound.y, sound.z], sound.volume, sound.pitch),
                (&ear, yaw),
            );
        }
        if hurt {
            self.audio.here("game.player.hurt", 0.7, 1.0);
        }
    }

    /// Build the mesh for everything alive, and the boxes for everything that has no model.
    fn build_entities(&mut self) -> Vec<([f32; 3], [f32; 3], [f32; 4])> {
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut boxes = Vec::new();
        let Some(session) = &self.session else {
            return boxes;
        };

        // Anything the server has forgotten should stop being drawn.
        self.smoothed.retain(|id, _| session.entities.contains_key(id));
        for (id, entity) in &session.entities {
            let target = [entity.x, entity.y, entity.z];
            let drawn = self.smoothed.entry(*id).or_insert(target);
            for axis in 0..3 {
                // A big jump is a teleport, not movement: go straight there.
                if (target[axis] - drawn[axis]).abs() > 4.0 {
                    drawn[axis] = target[axis];
                } else {
                    drawn[axis] += (target[axis] - drawn[axis]) * FOLLOW;
                }
            }
            let at = [drawn[0] as f32, drawn[1] as f32, drawn[2] as f32];
            let light = f32::from(session.light_at(
                drawn[0].floor() as i32,
                (drawn[1] + 0.5).floor() as i32,
                drawn[2].floor() as i32,
            )) / 15.0;

            match models::model_for(entity.kind) {
                Some(model) => {
                    if let Some(placed) = self.art.mobs.placed.get(model.texture) {
                        models::build(
                            model,
                            *placed,
                            at,
                            entity.head_yaw,
                            light.max(0.25),
                            (&mut vertices, &mut indices),
                        );
                    }
                }
                None => {
                    // A dropped item or an arrow: a small box is better than nothing at all.
                    let (w, h) = entity.kind.size();
                    boxes.push((at, [w, h, w], [0.6, 0.85, 1.0, 0.8]));
                }
            }
            if self.hitboxes {
                let (w, h) = entity.kind.size();
                boxes.push((at, [w, h, w], [1.0, 1.0, 1.0, 0.9]));
            }
        }
        if let Some(renderer) = &mut self.renderer {
            renderer.set_mobs(&vertices, &indices);
        }
        boxes
    }

    /// What F3 shows.
    fn readout(&self) -> Vec<String> {
        let mut lines = vec![format!("ninefold {}", env!("CARGO_PKG_VERSION"))];
        lines.push(format!(
            "{:.0} fps   {} chunks meshed   sound: {}",
            self.fps,
            self.meshed.len(),
            self.audio.describe()
        ));
        let Some(session) = &self.session else {
            return lines;
        };
        lines.push(format!(
            "xyz {:.2} {:.2} {:.2}   chunk {} {}",
            self.body.x,
            self.body.y,
            self.body.z,
            (self.body.x / 16.0).floor() as i32,
            (self.body.z / 16.0).floor() as i32
        ));
        lines.push(format!(
            "facing {:.1}  pitch {:.1}  on ground {}  entities {}",
            self.yaw.rem_euclid(360.0),
            self.pitch,
            self.body.on_ground,
            session.entities.len()
        ));
        lines.push(format!(
            "biome {}  time {}  light {:.2}",
            session.biome_at(self.body.x.floor() as i32, self.body.z.floor() as i32),
            session.day_time,
            session.sky_brightness()
        ));
        lines.push(match self.aimed {
            Some(hit) => {
                let block = session.block_at(hit.x, hit.y, hit.z).unwrap_or(0);
                format!(
                    "looking at {} {} {}  id {} meta {}  {:?}",
                    hit.x,
                    hit.y,
                    hit.z,
                    block >> 4,
                    block & 0xF,
                    hit.side
                )
            }
            None => "looking at nothing".to_owned(),
        });
        if let Some(stack) = session.held_item() {
            lines.push(format!(
                "holding {} x{}",
                items::name_of(stack.id, stack.damage),
                stack.count
            ));
        }
        lines
    }

    /// Draw whatever screen is in front, and remember what can be clicked.
    fn build_interface(&mut self) {
        let mut ui = gui::Ui::new(self.size.0, self.size.1);
        let mouse = ui.at(self.mouse.0, self.mouse.1);
        let art = gui::SlotArt {
            atlas: &self.art.atlas,
            items: &self.art.items,
            font: &self.art.font,
            light: 1.0,
        };

        self.hits = match self.screen {
            Screen::Title => screens::title(&mut ui, &self.art.font, mouse, &self.server.shown),
            Screen::Connecting => {
                screens::connecting(
                    &mut ui,
                    &self.art.font,
                    &self.server.shown,
                    self.since.elapsed().as_secs_f32(),
                );
                Vec::new()
            }
            Screen::Lost => {
                let why = self.failure.clone();
                screens::lost(&mut ui, &self.art.font, mouse, &why)
            }
            Screen::Pause => {
                self.hud(&mut ui, art);
                screens::pause(&mut ui, &self.art.font, mouse)
            }
            Screen::Inventory => {
                self.hud(&mut ui, art);
                let (slots, carried) = match &self.session {
                    Some(session) => (session.inventory.clone(), session.carried),
                    None => (Vec::new(), None),
                };
                screens::inventory(&mut ui, art, (&slots, carried), mouse)
            }
            Screen::None | Screen::Chat => {
                self.hud(&mut ui, art);
                let dead = self.session.as_ref().is_some_and(|s| s.dead);
                if dead {
                    screens::died(&mut ui, &self.art.font, mouse)
                } else {
                    Vec::new()
                }
            }
        };

        if self.debug {
            screens::debug(&mut ui, &self.art.font, &self.readout());
        }

        if let Some(renderer) = &mut self.renderer {
            renderer.set_sprites(&ui.sheet);
            renderer.set_item_quads(&ui.items);
            renderer.set_block_icons(&ui.blocks);
            renderer.set_text_quads(&ui.text);
        }
    }

    /// The hotbar, the hearts and the chat, which are drawn under every in-game screen.
    fn hud(&self, ui: &mut gui::Ui, art: gui::SlotArt<'_>) {
        let Some(session) = &self.session else {
            return;
        };
        let mut hotbar = [None; 9];
        for (slot, into) in hotbar.iter_mut().enumerate() {
            *into = session
                .inventory
                .get(HOTBAR_START + slot)
                .copied()
                .flatten();
        }
        ui.hud(
            art,
            &gui::Hud {
                held: session.held,
                health: session.health,
                food: session.food,
                air: 300,
                experience: session.experience,
                level: session.level,
                hotbar,
            },
        );
        let typing = if self.screen == Screen::Chat {
            Some(self.typed.as_str())
        } else {
            None
        };
        // The log is only worth showing if there is something in it or something being typed.
        if !session.chat.is_empty() || typing.is_some() {
            screens::chat(ui, &self.art.font, &session.chat, typing);
        }
    }

    /// Something on a menu was clicked.
    fn clicked(&mut self, event_loop: &ActiveEventLoop) {
        // Only to turn the mouse position into interface units, which is what the hits are in.
        let (mx, my) = gui::Ui::new(self.size.0, self.size.1).at(self.mouse.0, self.mouse.1);
        let Some(hit) = self.hits.iter().find(|hit| hit.covers(mx, my)).copied() else {
            return;
        };
        self.audio.here("random.click", 0.3, 1.0);
        match hit.action {
            Action::Connect => self.connect(),
            Action::Resume => {
                self.screen = Screen::None;
                self.grab(true);
            }
            Action::Disconnect => self.disconnect(None),
            Action::Respawn => {
                if let Some(session) = self.session.as_mut()
                    && let Err(e) = session.respawn()
                {
                    eprintln!("could not respawn: {e}");
                }
            }
            Action::Quit => event_loop.exit(),
            Action::Slot(index) => self.click_slot(index),
        }
    }

    /// Pick a stack up, or put it down.
    ///
    /// The server is the one that decides what actually happened; this moves the stacks locally
    /// as well so the window does not lag a round trip behind the mouse.
    fn click_slot(&mut self, index: usize) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let slot = i16::try_from(index).unwrap_or(-1);
        if slot < 0 {
            return;
        }
        if let Err(e) = session.click_slot(slot, 0, 0) {
            eprintln!("could not click a slot: {e}");
            return;
        }
        let under = session.inventory.get(index).copied().flatten();
        let carried = session.carried;
        match (under, carried) {
            // Picking up.
            (Some(stack), None) => {
                session.carried = Some(stack);
                session.inventory[index] = None;
            }
            // Putting down into an empty slot.
            (None, Some(stack)) => {
                session.inventory[index] = Some(stack);
                session.carried = None;
            }
            // Swapping, or stacking — which the server will correct if it disagrees.
            (Some(there), Some(holding)) => {
                session.inventory[index] = Some(holding);
                session.carried = Some(there);
            }
            (None, None) => {}
        }
    }

    /// A key, in the game rather than in a menu.
    fn play_key(&mut self, code: KeyCode, down: bool) {
        let dead = self.session.as_ref().is_some_and(|s| s.dead);
        match code {
            // Before the plain Space arm, or the guard never gets a look in.
            KeyCode::Space if down && dead => {
                if let Some(session) = self.session.as_mut()
                    && let Err(e) = session.respawn()
                {
                    eprintln!("could not respawn: {e}");
                }
            }
            KeyCode::KeyW => self.held.forward = down,
            KeyCode::KeyS => self.held.back = down,
            KeyCode::KeyA => self.held.left = down,
            KeyCode::KeyD => self.held.right = down,
            KeyCode::Space => self.held.jump = down,
            KeyCode::ShiftLeft => self.held.sneak = down,
            KeyCode::ControlLeft => self.held.sprint = down,
            KeyCode::F3 if down => self.debug = !self.debug,
            KeyCode::KeyB if down && self.debug => self.hitboxes = !self.hitboxes,
            KeyCode::KeyE if down => {
                self.screen = Screen::Inventory;
                self.grab(false);
            }
            KeyCode::KeyT if down => {
                self.screen = Screen::Chat;
                self.typed.clear();
            }
            KeyCode::Slash if down => {
                self.screen = Screen::Chat;
                self.typed = "/".to_owned();
            }
            KeyCode::KeyQ if down => {
                if let Some(session) = self.session.as_mut() {
                    let _ = session.drop_held(false);
                }
            }
            KeyCode::Escape if down => {
                self.screen = Screen::Pause;
                self.swinging = false;
                self.grab(false);
            }
            _ => {
                if down
                    && let Some(slot) = hotbar_key(code)
                    && let Some(session) = self.session.as_mut()
                {
                    let _ = session.select(slot);
                }
            }
        }
    }

    /// A key while the chat box is open.
    fn chat_key(&mut self, code: KeyCode, text: Option<&str>) {
        match code {
            KeyCode::Escape => {
                self.screen = Screen::None;
                self.typed.clear();
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                let line = std::mem::take(&mut self.typed);
                if !line.trim().is_empty()
                    && let Some(session) = self.session.as_mut()
                    && let Err(e) = session.say(line.trim())
                {
                    eprintln!("could not say that: {e}");
                }
                self.screen = Screen::None;
            }
            KeyCode::Backspace => {
                self.typed.pop();
            }
            _ => {
                // Whatever the keyboard layout says this key produces, which is the only way to
                // get a question mark out of a keyboard that is not American.
                if let Some(text) = text {
                    for character in text.chars() {
                        if !character.is_control() && self.typed.chars().count() < 100 {
                            self.typed.push(character);
                        }
                    }
                }
            }
        }
    }
}

/// Which hotbar slot a number key is.
fn hotbar_key(code: KeyCode) -> Option<u8> {
    match code {
        KeyCode::Digit1 => Some(0),
        KeyCode::Digit2 => Some(1),
        KeyCode::Digit3 => Some(2),
        KeyCode::Digit4 => Some(3),
        KeyCode::Digit5 => Some(4),
        KeyCode::Digit6 => Some(5),
        KeyCode::Digit7 => Some(6),
        KeyCode::Digit8 => Some(7),
        KeyCode::Digit9 => Some(8),
        _ => None,
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
        // The desktop matches a window to its launcher entry by this name, not by the title, so
        // without it the taskbar shows a blank icon and the entry's own icon is never used.
        #[cfg(all(unix, not(target_os = "macos")))]
        let attributes = {
            use winit::platform::wayland::WindowAttributesExtWayland;
            use winit::platform::x11::WindowAttributesExtX11;
            WindowAttributesExtX11::with_name(
                WindowAttributesExtWayland::with_name(attributes, APP_ID, APP_ID),
                APP_ID,
                APP_ID,
            )
        };
        let Ok(window) = event_loop.create_window(attributes) else {
            eprintln!("could not open a window");
            event_loop.exit();
            return;
        };
        let window = std::sync::Arc::new(window);
        let size = window.inner_size();
        self.size = (size.width, size.height);
        match ninefold_render::Renderer::new(window.clone(), size.width, size.height) {
            Ok(renderer) => self.renderer = Some(renderer),
            Err(e) => {
                eprintln!("no renderer: {e}");
                event_loop.exit();
                return;
            }
        }
        if let Some(renderer) = &mut self.renderer {
            let art = &self.art;
            renderer.set_atlas(art.atlas.width, art.atlas.height, &art.atlas.rgba);
            if let Some(font) = &art.font_sheet {
                renderer.set_font(font.width, font.height, &font.rgba);
            }
            renderer.set_gui_sheets(art.sheets.width, art.sheets.height, &art.sheets.rgba);
            renderer.set_item_sheet(art.items.width, art.items.height, &art.items.rgba);
            renderer.set_mob_sheet(art.mobs.width, art.mobs.height, &art.mobs.rgba);
        }
        self.window = Some(window);
        if self.server.straight_in {
            self.connect();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.size = (size.width, size.height);
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse = (position.x, position.y);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scrolled = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(position) => position.y as f32 / 40.0,
                };
                if self.screen == Screen::None
                    && scrolled.abs() > 0.1
                    && let Some(session) = self.session.as_mut()
                {
                    // Scrolling up goes left along the bar, as it does in the game.
                    let step = if scrolled > 0.0 { 8 } else { 1 };
                    let slot = (session.held + step) % 9;
                    let _ = session.select(slot);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                if self.screen.modal() {
                    if pressed && button == MouseButton::Left {
                        self.clicked(event_loop);
                    }
                } else if self.session.as_ref().is_some_and(|s| s.dead) {
                    // Dead: the only thing a click can do is press the respawn button.
                    if pressed && button == MouseButton::Left {
                        self.clicked(event_loop);
                    }
                } else if !self.looking {
                    // Clicking back into the window takes the mouse again rather than acting.
                    if pressed {
                        self.grab(true);
                    }
                } else {
                    match button {
                        MouseButton::Left => {
                            self.swinging = pressed;
                            if !pressed
                                && let Some((hit, _)) = self.breaking.take()
                                && let Some(session) = self.session.as_mut()
                                && let Err(e) =
                                    session.dig_cancel(hit.x, hit.y, hit.z, hit.side as u8)
                            {
                                eprintln!("could not stop digging: {e}");
                            }
                        }
                        MouseButton::Right if pressed => self.place(),
                        _ => {}
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let down = event.state == ElementState::Pressed;
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                match self.screen {
                    Screen::Chat if down => {
                        let text = event.text.as_ref().map(|t| t.as_str());
                        self.chat_key(code, text);
                    }
                    Screen::Chat => {}
                    Screen::None => self.play_key(code, down),
                    Screen::Inventory if down => {
                        if matches!(code, KeyCode::Escape | KeyCode::KeyE) {
                            self.screen = Screen::None;
                            self.grab(true);
                            if let Some(session) = self.session.as_mut() {
                                let _ = session.close_window();
                            }
                        }
                    }
                    Screen::Pause if down => {
                        if code == KeyCode::Escape {
                            self.screen = Screen::None;
                            self.grab(true);
                        }
                    }
                    Screen::Title | Screen::Lost if down => {
                        if code == KeyCode::Escape {
                            event_loop.exit();
                        } else if matches!(code, KeyCode::Enter | KeyCode::NumpadEnter) {
                            self.connect();
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event
            && self.looking
            && !self.screen.modal()
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

impl App {
    /// One frame: take what has arrived, move the world on, and draw it.
    fn frame(&mut self) {
        self.settle_connection();

        if let Some(session) = self.session.as_mut()
            && let Err(e) = session.pump()
        {
            let why = e.to_string();
            eprintln!("connection lost: {why}");
            self.disconnect(Some(why));
        }

        // The server has put us somewhere — respawned, teleported, pulled back after a move it
        // did not like. Whatever the physics thought is now wrong.
        if let Some(session) = &self.session
            && session.teleports != self.followed_teleports
        {
            self.followed_teleports = session.teleports;
            let p = session.position;
            self.body.x = p.x;
            self.body.y = p.y;
            self.body.z = p.z;
            self.body.motion_x = 0.0;
            self.body.motion_y = 0.0;
            self.body.motion_z = 0.0;
            self.body.on_ground = p.on_ground;
            self.previous = [p.x, p.y, p.z];
            let starting_over = session.columns.is_empty();
            if starting_over {
                // Chunks for the new place have not arrived; meshing starts over.
                for at in self.meshed.drain() {
                    if let Some(renderer) = &mut self.renderer {
                        renderer.remove_column(at);
                    }
                }
            }
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
            self.tick_breaking();
        }
        self.tick_sounds();

        // What is under the crosshair.
        if self.session.is_some() && !self.screen.modal() {
            let eye = [self.body.x, self.body.y + EYE_HEIGHT, self.body.z];
            let facing = ninefold_render::camera::facing(self.yaw as f32, self.pitch as f32);
            let direction = [
                f64::from(facing[0]),
                f64::from(facing[1]),
                f64::from(facing[2]),
            ];
            let aimed = self
                .session
                .as_ref()
                .and_then(|session| raycast::cast(session, eye, direction, raycast::REACH));
            if aimed != self.aimed {
                self.aimed = aimed;
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_highlight(aimed.map(|h| (h.x, h.y, h.z)));
                }
            }
        } else if self.aimed.is_some() {
            self.aimed = None;
            if let Some(renderer) = &mut self.renderer {
                renderer.set_highlight(None);
            }
        }

        let boxes = self.build_entities();
        if let Some(renderer) = &mut self.renderer {
            renderer.set_boxes(&boxes);
        }

        // One line, once, so a run with no window to look at still says whether it worked.
        if !self.announced
            && self.meshed.len() >= 4
            && let Some(session) = &self.session
        {
            self.announced = true;
            println!(
                "in the world at {:.1} {:.1} {:.1} — {} chunks, {} entities, daylight {:.2}",
                self.body.x,
                self.body.y,
                self.body.z,
                self.meshed.len(),
                session.entities.len(),
                session.sky_brightness()
            );
        }

        self.frames += 1;
        let counted = self.counted_from.elapsed();
        if counted >= Duration::from_millis(500) {
            self.fps = f64::from(self.frames) / counted.as_secs_f64();
            self.frames = 0;
            self.counted_from = Instant::now();
        }
        self.build_interface();

        let daylight = self
            .session
            .as_ref()
            .map_or(1.0, ninefold_protocol::play::Session::sky_brightness);
        if let Some(renderer) = &mut self.renderer {
            renderer.set_daylight(daylight);
            // How far into the next tick we are, so the camera glides between the two positions
            // instead of stepping twenty times a second.
            let alpha = (self.owed.as_secs_f64() * TPS).clamp(0.0, 1.0);
            let between = |from: f64, to: f64| from + (to - from) * alpha;
            let eye = [
                between(self.previous[0], self.body.x) as f32,
                (between(self.previous[1], self.body.y) + EYE_HEIGHT) as f32,
                between(self.previous[2], self.body.z) as f32,
            ];
            let direction = ninefold_render::camera::facing(self.yaw as f32, self.pitch as f32);
            renderer.draw(eye, direction);
        }
    }
}
