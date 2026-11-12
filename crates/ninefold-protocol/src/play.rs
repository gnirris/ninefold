//! Being in the world.
//!
//! One `Session` owns the connection and everything the server has told us: where we are, what
//! the chunks hold, what else is alive, what is in our hands. Packets arrive in any order and
//! most of them are a correction to something we already believed, so the rule throughout is
//! that the server is right and anything the client thinks follows from it.

use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

use crate::chunk::{self, Column};
use crate::item::{self, Slot};
use crate::wire::{Connection, body};
use crate::{Reader, Writer};

// Clientbound, play state, 1.8.9.
const KEEP_ALIVE: i32 = 0x00;
const JOIN_GAME: i32 = 0x01;
const CHAT: i32 = 0x02;
const TIME_UPDATE: i32 = 0x03;
const ENTITY_EQUIPMENT: i32 = 0x04;
const SPAWN_POSITION: i32 = 0x05;
const UPDATE_HEALTH: i32 = 0x06;
const RESPAWN: i32 = 0x07;
const POSITION_AND_LOOK: i32 = 0x08;
const HELD_SLOT: i32 = 0x09;
const SPAWN_PLAYER: i32 = 0x0C;
const SPAWN_OBJECT: i32 = 0x0E;
const SPAWN_MOB: i32 = 0x0F;
const ENTITY_VELOCITY: i32 = 0x12;
const DESTROY_ENTITIES: i32 = 0x13;
const ENTITY_MOVE: i32 = 0x15;
const ENTITY_LOOK: i32 = 0x16;
const ENTITY_LOOK_MOVE: i32 = 0x17;
const ENTITY_TELEPORT: i32 = 0x18;
const ENTITY_HEAD_LOOK: i32 = 0x19;
const ENTITY_STATUS: i32 = 0x1A;
const CHUNK_DATA: i32 = 0x21;
const MULTI_BLOCK_CHANGE: i32 = 0x22;
const BLOCK_CHANGE: i32 = 0x23;
const MAP_CHUNK_BULK: i32 = 0x26;
const SET_EXPERIENCE: i32 = 0x1F;
const SOUND_EFFECT: i32 = 0x29;
const GAME_STATE: i32 = 0x2B;
const OPEN_WINDOW: i32 = 0x2D;
const CLOSE_WINDOW: i32 = 0x2E;
const SET_SLOT: i32 = 0x2F;
const WINDOW_ITEMS: i32 = 0x30;
const CONFIRM_TRANSACTION: i32 = 0x32;
const PLAYER_LIST_ITEM: i32 = 0x38;
const PLAYER_ABILITIES: i32 = 0x39;
const DISCONNECT: i32 = 0x40;

// Serverbound.
const OUT_KEEP_ALIVE: i32 = 0x00;
const OUT_CHAT: i32 = 0x01;
const OUT_POSITION_AND_LOOK: i32 = 0x06;
const OUT_DIG: i32 = 0x07;
const OUT_PLACE: i32 = 0x08;
const OUT_HELD_SLOT: i32 = 0x09;
const OUT_SWING: i32 = 0x0A;
const OUT_ENTITY_ACTION: i32 = 0x0B;
const OUT_CLOSE_WINDOW: i32 = 0x0D;
const OUT_CLICK_WINDOW: i32 = 0x0E;
const OUT_CONFIRM_TRANSACTION: i32 = 0x0F;
const OUT_CREATIVE_SLOT: i32 = 0x10;
const OUT_CLIENT_STATUS: i32 = 0x16;

/// Slots in the player's own window: craft output, craft grid, armour, main, hotbar.
pub const INVENTORY_SLOTS: usize = 45;
/// Where the hotbar starts in that window.
pub const HOTBAR_START: usize = 36;

/// Something in the world that is not the player.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    /// Where the head is pointing, which for a player is what you see them look at.
    pub head_yaw: f32,
    pub kind: Kind,
    /// A player's name, once the tab list has mentioned them.
    pub name: Option<String>,
    /// What they are holding, for drawing something in their hand later.
    pub held: Option<Slot>,
}

/// Enough to decide how big something is and which model to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Player,
    /// A living thing, by its 1.8 type number.
    Mob(u8),
    /// A dropped item, an arrow, a minecart and so on.
    Object(u8),
}

impl Kind {
    /// Width and height in blocks. Close enough to 1.8's hitboxes for the common ones.
    #[must_use]
    pub const fn size(self) -> (f32, f32) {
        match self {
            Kind::Player => (0.6, 1.8),
            Kind::Mob(50 | 51) => (0.65, 1.8), // creeper, skeleton
            Kind::Mob(54 | 57) => (0.6, 1.9),  // zombie, zombie pigman
            Kind::Mob(90) => (0.9, 0.9),       // pig
            Kind::Mob(91 | 92 | 96) => (0.9, 1.3), // sheep, cow, mooshroom
            Kind::Mob(93) => (0.4, 0.7),       // chicken
            Kind::Mob(52 | 59) => (1.4, 0.9),  // spiders
            Kind::Mob(120) => (0.6, 1.8),      // villager
            Kind::Mob(_) => (0.8, 1.4),
            Kind::Object(_) => (0.25, 0.25),
        }
    }
}

/// Something the server asked to be heard.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    /// The event name, as in `random.pop` or `mob.pig.say`.
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub volume: f32,
    pub pitch: f32,
}

/// Positions arrive as fixed point: a block is 32.
fn fixed(value: i32) -> f64 {
    f64::from(value) / 32.0
}

/// And angles as a byte of a full turn.
fn angle(value: u8) -> f32 {
    f32::from(value) * 360.0 / 256.0
}

/// What the server says when you arrive.
#[derive(Debug, Clone)]
pub struct Joined {
    pub entity_id: i32,
    pub gamemode: u8,
    pub dimension: i8,
    pub difficulty: u8,
    pub max_players: u8,
    pub level_type: String,
}

/// Where the player is.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

/// A connection that is in the world.
pub struct Session {
    pub connection: Connection,
    pub position: Position,
    /// Set once the server has told us where we are. Until then it does not want to hear from us.
    pub placed: bool,
    pub chunks_seen: u32,
    pub keep_alives: u32,
    /// Decoded columns, by chunk coordinate.
    pub columns: HashMap<(i32, i32), Column>,
    /// Columns whose blocks have changed since they were last drawn.
    pub dirty: std::collections::HashSet<(i32, i32)>,
    /// Everything else in the world, by the server's entity id.
    pub entities: HashMap<i32, Entity>,
    /// Our own entity id, so we can tell our own packets from everyone else's.
    pub entity_id: i32,
    /// What has been said, newest last.
    pub chat: Vec<String>,
    /// Hearts, out of twenty.
    pub health: f32,
    /// Drumsticks, out of twenty.
    pub food: u8,
    /// Which hotbar slot is in hand, 0..8.
    pub held: u8,
    /// Our own window: 0 craft output, 1..4 grid, 5..8 armour, 9..35 main, 36..44 hotbar.
    pub inventory: Vec<Option<Slot>>,
    /// Whatever the mouse is carrying inside an open window.
    pub carried: Option<Slot>,
    /// 0 survival, 1 creative, 2 adventure, 3 spectator.
    pub gamemode: u8,
    /// Set by the abilities packet: in creative, breaking is instant and flight is allowed.
    pub may_fly: bool,
    pub flying: bool,
    /// How far along the experience bar is, 0..1, and what level that is.
    pub experience: f32,
    pub level: i32,
    /// Ticks since the world began, and where in the day it is, 0..24000.
    pub world_age: i64,
    pub day_time: i64,
    /// Health has run out, and the server is waiting to be asked for a respawn.
    pub dead: bool,
    /// Counts every time the server has moved us itself. Anything keeping its own copy of the
    /// position — the physics, for one — has to notice this and snap to the new one, or it will
    /// carry on from where it was and argue with the server about where the player is.
    pub teleports: u32,
    /// Sounds the server asked for, oldest first. Drained by whoever can play them.
    pub sounds: Vec<Sound>,
    /// A shove the server applied to us — an explosion, a mob's hit — for the physics to take.
    pub pushed: Option<(f64, f64, f64)>,
    /// Set when the server says we were hurt, so the client can flash and make a noise.
    pub hurt: bool,
    /// Names from the tab list, by UUID, so a spawning player can be labelled.
    names: HashMap<[u8; 16], String>,
    /// The window the server has opened over the inventory, if any.
    pub window: u8,
    /// Rolling counter for window clicks, which the server echoes back.
    actions: i16,
}

impl Session {
    #[must_use]
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            position: Position::default(),
            placed: false,
            chunks_seen: 0,
            keep_alives: 0,
            columns: HashMap::new(),
            dirty: std::collections::HashSet::new(),
            entities: HashMap::new(),
            entity_id: 0,
            chat: Vec::new(),
            health: 20.0,
            food: 20,
            held: 0,
            inventory: vec![None; INVENTORY_SLOTS],
            carried: None,
            gamemode: 0,
            may_fly: false,
            flying: false,
            experience: 0.0,
            level: 0,
            world_age: 0,
            day_time: 6000,
            dead: false,
            teleports: 0,
            sounds: Vec::new(),
            pushed: None,
            hurt: false,
            names: HashMap::new(),
            window: 0,
            actions: 0,
        }
    }

    /// The same, with what the Join Game packet said already filled in.
    #[must_use]
    pub fn joined(connection: Connection, joined: &Joined) -> Self {
        let mut session = Self::new(connection);
        session.entity_id = joined.entity_id;
        session.gamemode = joined.gamemode & 0x7;
        session
    }

    /// Creative breaks blocks in one hit and may fly.
    #[must_use]
    pub const fn creative(&self) -> bool {
        self.gamemode == 1
    }

    /// What is in our hand.
    #[must_use]
    pub fn held_item(&self) -> Option<Slot> {
        self.inventory
            .get(HOTBAR_START + self.held as usize)
            .copied()
            .flatten()
    }

    /// Read one packet and do whatever it demands. Returns the packet id, for callers that want
    /// to watch the traffic.
    pub fn step(&mut self) -> io::Result<i32> {
        let (id, payload) = self.connection.recv()?;
        match id {
            KEEP_ALIVE => {
                // The server drops a client that stops answering these.
                let token = Reader::new(&payload).var_int().map_err(bad)?;
                self.keep_alives += 1;
                self.connection.send(
                    OUT_KEEP_ALIVE,
                    &body(|w| {
                        w.var_int(token);
                    }),
                )?;
            }
            POSITION_AND_LOOK => {
                let mut r = Reader::new(&payload);
                let (x, y, z) = (
                    r.f64().map_err(bad)?,
                    r.f64().map_err(bad)?,
                    r.f64().map_err(bad)?,
                );
                let (yaw, pitch) = (r.f32().map_err(bad)?, r.f32().map_err(bad)?);
                // Each bit says that field is relative to where we already are.
                let flags = r.u8().map_err(bad)?;
                let p = &mut self.position;
                p.x = if flags & 0x01 != 0 { p.x + x } else { x };
                p.y = if flags & 0x02 != 0 { p.y + y } else { y };
                p.z = if flags & 0x04 != 0 { p.z + z } else { z };
                p.yaw = if flags & 0x08 != 0 { p.yaw + yaw } else { yaw };
                p.pitch = if flags & 0x10 != 0 {
                    p.pitch + pitch
                } else {
                    pitch
                };
                self.placed = true;
                self.teleports += 1;
                // Answering this is what tells the server the teleport took.
                self.send_position()?;
            }
            BLOCK_CHANGE => {
                let mut r = Reader::new(&payload);
                let (x, y, z) = decode_position(r.i64().map_err(bad)?);
                let block = u16::try_from(r.var_int().map_err(bad)?).unwrap_or(0);
                self.set_block(x, y, z, block);
            }
            MULTI_BLOCK_CHANGE => {
                let mut r = Reader::new(&payload);
                let (cx, cz) = (r.i32().map_err(bad)?, r.i32().map_err(bad)?);
                let count = r.var_int().map_err(bad)?.max(0);
                for _ in 0..count {
                    // Packed into two bytes: x and z as nibbles, then y.
                    let packed = r.u16().map_err(bad)?;
                    let (x, z, y) = (
                        i32::from(packed >> 12),
                        i32::from((packed >> 8) & 0xF),
                        i32::from(packed & 0xFF),
                    );
                    let block = u16::try_from(r.var_int().map_err(bad)?).unwrap_or(0);
                    self.set_block(cx * 16 + x, y, cz * 16 + z, block);
                }
            }
            RESPAWN => {
                // A respawn can be into another dimension, so nothing already loaded is worth
                // keeping: the chunks and everything in them belong to the world being left.
                let mut r = Reader::new(&payload);
                let _dimension = r.i32().map_err(bad)?;
                let _difficulty = r.u8().map_err(bad)?;
                if let Ok(mode) = r.u8() {
                    self.gamemode = mode & 0x7;
                }
                self.columns.clear();
                self.dirty.clear();
                self.entities.clear();
                self.chunks_seen = 0;
                self.placed = false;
                self.dead = false;
                self.health = 20.0;
            }
            CHAT => {
                let mut r = Reader::new(&payload);
                let json = r.string(64 * 1024).map_err(bad)?;
                let line = crate::status::chat_text(json);
                if !line.trim().is_empty() {
                    self.chat.push(line);
                    // Only the last few are ever shown; the rest is just memory.
                    if self.chat.len() > 64 {
                        self.chat.drain(..self.chat.len() - 64);
                    }
                }
            }
            TIME_UPDATE => {
                let mut r = Reader::new(&payload);
                self.world_age = r.i64().map_err(bad)?;
                // A negative time means the server has frozen it; the magnitude is still the time.
                self.day_time = r.i64().map_err(bad)?.abs() % 24_000;
            }
            UPDATE_HEALTH => {
                let mut r = Reader::new(&payload);
                let was = self.health;
                self.health = r.f32().map_err(bad)?;
                self.food = u8::try_from(r.var_int().map_err(bad)?.clamp(0, 20)).unwrap_or(20);
                if self.health < was {
                    self.hurt = true;
                }
                if self.health <= 0.0 {
                    self.dead = true;
                }
            }
            HELD_SLOT => {
                let slot = Reader::new(&payload).u8().map_err(bad)?;
                self.held = slot.min(8);
            }
            SET_EXPERIENCE => {
                let mut r = Reader::new(&payload);
                self.experience = r.f32().map_err(bad)?.clamp(0.0, 1.0);
                self.level = r.var_int().map_err(bad)?;
            }
            SPAWN_POSITION => {}
            PLAYER_ABILITIES => {
                let flags = Reader::new(&payload).u8().map_err(bad)?;
                self.flying = flags & 0x02 != 0;
                self.may_fly = flags & 0x04 != 0;
            }
            GAME_STATE => {
                let mut r = Reader::new(&payload);
                let reason = r.u8().map_err(bad)?;
                let value = r.f32().map_err(bad)?;
                if reason == 3 {
                    self.gamemode = (value as u8) & 0x7;
                }
            }
            PLAYER_LIST_ITEM => {
                self.read_player_list(&payload)?;
            }
            SPAWN_PLAYER => {
                let mut r = Reader::new(&payload);
                let entity_id = r.var_int().map_err(bad)?;
                let mut uuid = [0u8; 16];
                for byte in &mut uuid {
                    *byte = r.u8().map_err(bad)?;
                }
                let x = fixed(r.i32().map_err(bad)?);
                let y = fixed(r.i32().map_err(bad)?);
                let z = fixed(r.i32().map_err(bad)?);
                let yaw = angle(r.u8().map_err(bad)?);
                let pitch = angle(r.u8().map_err(bad)?);
                let entity = Entity {
                    x,
                    y,
                    z,
                    yaw,
                    pitch,
                    head_yaw: yaw,
                    kind: Kind::Player,
                    name: self.names.get(&uuid).cloned(),
                    held: None,
                };
                self.entities.insert(entity_id, entity);
            }
            SPAWN_MOB | SPAWN_OBJECT => {
                let packet = id;
                let mut r = Reader::new(&payload);
                let entity_id = r.var_int().map_err(bad)?;
                let kind = r.u8().map_err(bad)?;
                let x = fixed(r.i32().map_err(bad)?);
                let y = fixed(r.i32().map_err(bad)?);
                let z = fixed(r.i32().map_err(bad)?);
                let yaw = angle(r.u8().map_err(bad)?);
                let pitch = angle(r.u8().map_err(bad)?);
                let entity = Entity {
                    x,
                    y,
                    z,
                    yaw,
                    pitch,
                    head_yaw: yaw,
                    kind: if packet == SPAWN_MOB {
                        Kind::Mob(kind)
                    } else {
                        Kind::Object(kind)
                    },
                    name: None,
                    held: None,
                };
                self.entities.insert(entity_id, entity);
            }
            ENTITY_EQUIPMENT => {
                let mut r = Reader::new(&payload);
                let entity_id = r.var_int().map_err(bad)?;
                let slot = r.i16().map_err(bad)?;
                let stack = item::read(&mut r).map_err(bad)?;
                // Slot 0 is the hand; 1..4 are armour, which nothing draws yet.
                if slot == 0
                    && let Some(entity) = self.entities.get_mut(&entity_id)
                {
                    entity.held = stack;
                }
            }
            DESTROY_ENTITIES => {
                let mut r = Reader::new(&payload);
                let count = r.var_int().map_err(bad)?.max(0);
                for _ in 0..count {
                    let gone = r.var_int().map_err(bad)?;
                    self.entities.remove(&gone);
                }
            }
            ENTITY_VELOCITY => {
                let mut r = Reader::new(&payload);
                let entity_id = r.var_int().map_err(bad)?;
                // Velocity is in 1/8000 of a block per tick.
                let shove = |v: i16| f64::from(v) / 8000.0;
                let (vx, vy, vz) = (
                    shove(r.i16().map_err(bad)?),
                    shove(r.i16().map_err(bad)?),
                    shove(r.i16().map_err(bad)?),
                );
                if entity_id == self.entity_id {
                    self.pushed = Some((vx, vy, vz));
                }
            }
            ENTITY_MOVE | ENTITY_LOOK_MOVE => {
                let packet = id;
                let mut r = Reader::new(&payload);
                let entity_id = r.var_int().map_err(bad)?;
                // Steps are thirty-seconds of a block, same as positions.
                let (dx, dy, dz) = (
                    r.i8().map_err(bad)?,
                    r.i8().map_err(bad)?,
                    r.i8().map_err(bad)?,
                );
                if let Some(entity) = self.entities.get_mut(&entity_id) {
                    entity.x += f64::from(dx) / 32.0;
                    entity.y += f64::from(dy) / 32.0;
                    entity.z += f64::from(dz) / 32.0;
                    if packet == ENTITY_LOOK_MOVE {
                        entity.yaw = angle(r.u8().map_err(bad)?);
                        entity.pitch = angle(r.u8().map_err(bad)?);
                    }
                }
            }
            ENTITY_LOOK => {
                let mut r = Reader::new(&payload);
                let entity_id = r.var_int().map_err(bad)?;
                let yaw = angle(r.u8().map_err(bad)?);
                let pitch = angle(r.u8().map_err(bad)?);
                if let Some(entity) = self.entities.get_mut(&entity_id) {
                    entity.yaw = yaw;
                    entity.pitch = pitch;
                }
            }
            ENTITY_HEAD_LOOK => {
                let mut r = Reader::new(&payload);
                let entity_id = r.var_int().map_err(bad)?;
                let yaw = angle(r.u8().map_err(bad)?);
                if let Some(entity) = self.entities.get_mut(&entity_id) {
                    entity.head_yaw = yaw;
                }
            }
            ENTITY_STATUS => {
                let mut r = Reader::new(&payload);
                let entity_id = r.i32().map_err(bad)?;
                let status = r.u8().map_err(bad)?;
                // 2 is "took a hit", which is what makes the sound and the red flash.
                if status == 2 {
                    if entity_id == self.entity_id {
                        self.hurt = true;
                    } else if let Some(entity) = self.entities.get(&entity_id) {
                        let name = hurt_sound(entity.kind);
                        let (x, y, z) = (entity.x, entity.y, entity.z);
                        self.push_sound(Sound {
                            name: name.to_owned(),
                            x,
                            y,
                            z,
                            volume: 1.0,
                            pitch: 1.0,
                        });
                    }
                }
            }
            ENTITY_TELEPORT => {
                let mut r = Reader::new(&payload);
                let entity_id = r.var_int().map_err(bad)?;
                let (x, y, z) = (
                    fixed(r.i32().map_err(bad)?),
                    fixed(r.i32().map_err(bad)?),
                    fixed(r.i32().map_err(bad)?),
                );
                let yaw = angle(r.u8().map_err(bad)?);
                let pitch = angle(r.u8().map_err(bad)?);
                if let Some(entity) = self.entities.get_mut(&entity_id) {
                    entity.x = x;
                    entity.y = y;
                    entity.z = z;
                    entity.yaw = yaw;
                    entity.pitch = pitch;
                }
            }
            SOUND_EFFECT => {
                let mut r = Reader::new(&payload);
                let name = r.string(1024).map_err(bad)?.to_owned();
                // Positions here are in eighths of a block.
                let at = |v: i32| f64::from(v) / 8.0;
                let (x, y, z) = (
                    at(r.i32().map_err(bad)?),
                    at(r.i32().map_err(bad)?),
                    at(r.i32().map_err(bad)?),
                );
                let volume = r.f32().map_err(bad)?;
                // Pitch is a byte of 63 per whole step.
                let pitch = f32::from(r.u8().map_err(bad)?) / 63.0;
                self.push_sound(Sound {
                    name,
                    x,
                    y,
                    z,
                    volume,
                    pitch,
                });
            }
            SET_SLOT => {
                let mut r = Reader::new(&payload);
                let window = r.i8().map_err(bad)?;
                let slot = r.i16().map_err(bad)?;
                let stack = item::read(&mut r).map_err(bad)?;
                if window == 0
                    && let Ok(index) = usize::try_from(slot)
                    && index < self.inventory.len()
                {
                    self.inventory[index] = stack;
                }
            }
            WINDOW_ITEMS => {
                let mut r = Reader::new(&payload);
                let window = r.u8().map_err(bad)?;
                let count = r.i16().map_err(bad)?.max(0);
                for index in 0..count as usize {
                    let stack = item::read(&mut r).map_err(bad)?;
                    if window == 0 && index < self.inventory.len() {
                        self.inventory[index] = stack;
                    }
                }
            }
            OPEN_WINDOW => {
                self.window = Reader::new(&payload).u8().map_err(bad)?;
            }
            CLOSE_WINDOW => {
                self.window = 0;
                self.carried = None;
            }
            CONFIRM_TRANSACTION => {
                let mut r = Reader::new(&payload);
                let window = r.u8().map_err(bad)?;
                let action = r.i16().map_err(bad)?;
                let accepted = r.bool().map_err(bad)?;
                // A refused click has to be acknowledged or the server stops accepting any.
                if !accepted {
                    self.connection.send(
                        OUT_CONFIRM_TRANSACTION,
                        &body(|w: &mut Writer| {
                            w.u8(window).i16(action).bool(true);
                        }),
                    )?;
                }
            }
            CHUNK_DATA => {
                self.chunks_seen += 1;
                match chunk::read_chunk_data(&payload).map_err(bad)? {
                    Some(column) => {
                        self.columns.insert((column.x, column.z), column);
                    }
                    // A full column with no sections means the server is unloading it.
                    None => {
                        let mut r = Reader::new(&payload);
                        let (x, z) = (r.i32().map_err(bad)?, r.i32().map_err(bad)?);
                        self.columns.remove(&(x, z));
                    }
                }
            }
            MAP_CHUNK_BULK => {
                for column in chunk::read_map_chunk_bulk(&payload).map_err(bad)? {
                    self.chunks_seen += 1;
                    self.columns.insert((column.x, column.z), column);
                }
            }
            DISCONNECT => {
                let reason = Reader::new(&payload)
                    .string(64 * 1024)
                    .map_err(bad)?
                    .to_owned();
                return Err(io::Error::other(format!(
                    "disconnected: {}",
                    crate::status::chat_text(&reason)
                )));
            }
            _ => {}
        }
        Ok(id)
    }

    /// Names out of the tab list, so a player who spawns can be labelled.
    fn read_player_list(&mut self, payload: &[u8]) -> io::Result<()> {
        let mut r = Reader::new(payload);
        let action = r.var_int().map_err(bad)?;
        let count = r.var_int().map_err(bad)?.clamp(0, 1024);
        for _ in 0..count {
            let mut uuid = [0u8; 16];
            for byte in &mut uuid {
                *byte = r.u8().map_err(bad)?;
            }
            match action {
                // Added: name, properties, gamemode, ping, optional display name.
                0 => {
                    let name = r.string(64).map_err(bad)?.to_owned();
                    let properties = r.var_int().map_err(bad)?.clamp(0, 64);
                    for _ in 0..properties {
                        r.string(64 * 1024).map_err(bad)?;
                        r.string(64 * 1024).map_err(bad)?;
                        if r.bool().map_err(bad)? {
                            r.string(64 * 1024).map_err(bad)?;
                        }
                    }
                    r.var_int().map_err(bad)?;
                    r.var_int().map_err(bad)?;
                    if r.bool().map_err(bad)? {
                        r.string(64 * 1024).map_err(bad)?;
                    }
                    self.names.insert(uuid, name);
                }
                // Gamemode, ping: one VarInt each.
                1 | 2 => {
                    r.var_int().map_err(bad)?;
                }
                // Display name.
                3 => {
                    if r.bool().map_err(bad)? {
                        r.string(64 * 1024).map_err(bad)?;
                    }
                }
                // Removed.
                4 => {
                    self.names.remove(&uuid);
                }
                _ => return Ok(()),
            }
        }
        Ok(())
    }

    fn push_sound(&mut self, sound: Sound) {
        self.sounds.push(sound);
        // Nobody is going to hear a backlog, and an unread queue is just a leak.
        if self.sounds.len() > 32 {
            self.sounds.remove(0);
        }
    }

    /// Tell the server where we are. Ignored until it has placed us.
    pub fn send_position(&mut self) -> io::Result<()> {
        if !self.placed {
            return Ok(());
        }
        let p = self.position;
        self.connection.send(
            OUT_POSITION_AND_LOOK,
            &body(|w: &mut Writer| {
                w.f64(p.x)
                    .f64(p.y)
                    .f64(p.z)
                    .f32(p.yaw)
                    .f32(p.pitch)
                    .bool(p.on_ground);
            }),
        )
    }

    /// Walk, in blocks, and tell the server. 1.8 rejects a step longer than 10 blocks as flying.
    pub fn walk(&mut self, dx: f64, dz: f64) -> io::Result<()> {
        self.position.x += dx;
        self.position.z += dz;
        self.position.on_ground = true;
        self.send_position()
    }

    /// Read packets until the world has arrived or `how_long` runs out.
    pub fn settle(&mut self, how_long: Duration) -> io::Result<()> {
        let until = Instant::now() + how_long;
        while Instant::now() < until {
            self.step()?;
            if self.placed && self.chunks_seen > 0 {
                return Ok(());
            }
        }
        Ok(())
    }
}

impl Session {
    /// The block at a world position, if its chunk has arrived. `id << 4 | metadata`.
    #[must_use]
    pub fn block_at(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        if !(0..256).contains(&y) {
            return None;
        }
        // Floor division: chunk -1 covers x -16..-1.
        let column = self.columns.get(&(x.div_euclid(16), z.div_euclid(16)))?;
        Some(column.block_at(
            x.rem_euclid(16) as usize,
            y as usize,
            z.rem_euclid(16) as usize,
        ))
    }

    /// Which biome a column is, at a world position. 1 (plains) where nothing is known.
    #[must_use]
    pub fn biome_at(&self, x: i32, z: i32) -> u8 {
        self.columns
            .get(&(x.div_euclid(16), z.div_euclid(16)))
            .and_then(|column| {
                let at = (z.rem_euclid(16) * 16 + x.rem_euclid(16)) as usize;
                column.biomes.as_ref().and_then(|b| b.get(at).copied())
            })
            .unwrap_or(1)
    }

    /// Tell the server digging has started.
    ///
    /// A 1.8 server times the dig itself against the block's hardness and throws away a
    /// "finished" that arrives too early — so these are three separate things, not one.
    pub fn dig_start(&mut self, x: i32, y: i32, z: i32, side: u8) -> io::Result<()> {
        self.dig_packet(0, x, y, z, side)
    }

    /// Stop without breaking it.
    pub fn dig_cancel(&mut self, x: i32, y: i32, z: i32, side: u8) -> io::Result<()> {
        self.dig_packet(1, x, y, z, side)
    }

    /// The block has had its time: take it out.
    pub fn dig_finish(&mut self, x: i32, y: i32, z: i32, side: u8) -> io::Result<()> {
        self.dig_packet(2, x, y, z, side)
    }

    /// Throw away what is in hand. Status 3 is one, 4 is the stack.
    pub fn drop_held(&mut self, whole_stack: bool) -> io::Result<()> {
        self.dig_packet(if whole_stack { 3 } else { 4 }, 0, 0, 0, 255)
    }

    fn dig_packet(&mut self, status: u8, x: i32, y: i32, z: i32, side: u8) -> io::Result<()> {
        let at = encode_position(x, y, z);
        self.connection.send(
            OUT_DIG,
            &body(|w: &mut Writer| {
                w.u8(status).i64(at).u8(side);
            }),
        )
    }

    /// Put the held item against a block face.
    ///
    /// `side` is the face that was hit and `cursor` is where on that face, in sixteenths — the
    /// server uses both to decide which way a stair faces or which half a slab fills.
    pub fn place(
        &mut self,
        at: (i32, i32, i32),
        side: u8,
        cursor: (u8, u8, u8),
    ) -> io::Result<()> {
        let held = self.held_item();
        let position = encode_position(at.0, at.1, at.2);
        self.connection.send(
            OUT_PLACE,
            &body(|w: &mut Writer| {
                w.i64(position).u8(side);
                item::write(w, held);
                w.u8(cursor.0).u8(cursor.1).u8(cursor.2);
            }),
        )
    }

    /// Use what is in hand without aiming at anything: eating, drawing a bow, throwing.
    pub fn use_held(&mut self) -> io::Result<()> {
        let held = self.held_item();
        self.connection.send(
            OUT_PLACE,
            &body(|w: &mut Writer| {
                // The position every client sends for "in the air", and face 255 with it.
                w.i64(-1).u8(255);
                item::write(w, held);
                w.u8(0).u8(0).u8(0);
            }),
        )
    }

    /// Change hotbar slot, 0..8.
    pub fn select(&mut self, slot: u8) -> io::Result<()> {
        self.held = slot.min(8);
        let slot = i16::from(self.held);
        self.connection.send(
            OUT_HELD_SLOT,
            &body(|w: &mut Writer| {
                w.i16(slot);
            }),
        )
    }

    /// Say something, or run a command if it starts with a slash.
    pub fn say(&mut self, line: &str) -> io::Result<()> {
        // 1.8 cuts anything longer off and kicks a client that sends more.
        let line: String = line.chars().take(100).collect();
        self.connection.send(
            OUT_CHAT,
            &body(|w: &mut Writer| {
                w.string(&line);
            }),
        )
    }

    /// Sneaking, sprinting and the rest: the server needs telling, or other players never see
    /// you crouch and its anti-cheat does not expect you to be fast.
    pub fn entity_action(&mut self, action: u8) -> io::Result<()> {
        let id = self.entity_id;
        self.connection.send(
            OUT_ENTITY_ACTION,
            &body(|w: &mut Writer| {
                w.var_int(id).u8(action).var_int(0);
            }),
        )
    }

    /// Click a slot in the open window. `button` is 0 left, 1 right; `mode` 0 is an ordinary
    /// click and 1 is shift-click.
    pub fn click_slot(&mut self, slot: i16, button: u8, mode: u8) -> io::Result<()> {
        let window = self.window;
        self.actions = self.actions.wrapping_add(1);
        let action = self.actions;
        let under = usize::try_from(slot)
            .ok()
            .and_then(|i| self.inventory.get(i).copied())
            .flatten();
        self.connection.send(
            OUT_CLICK_WINDOW,
            &body(|w: &mut Writer| {
                w.u8(window).i16(slot).u8(button).i16(action).u8(mode);
                item::write(w, under);
            }),
        )
    }

    /// Shut whatever window is open.
    pub fn close_window(&mut self) -> io::Result<()> {
        let window = self.window;
        self.window = 0;
        self.carried = None;
        self.connection.send(
            OUT_CLOSE_WINDOW,
            &body(|w: &mut Writer| {
                w.u8(window);
            }),
        )
    }

    /// Put something in a slot out of nowhere. Creative only; the server refuses it otherwise.
    pub fn creative_slot(&mut self, slot: i16, stack: Option<Slot>) -> io::Result<()> {
        if let Ok(index) = usize::try_from(slot)
            && index < self.inventory.len()
        {
            self.inventory[index] = stack;
        }
        self.connection.send(
            OUT_CREATIVE_SLOT,
            &body(|w: &mut Writer| {
                w.i16(slot);
                item::write(w, stack);
            }),
        )
    }

    /// Come back to life where the server puts you.
    pub fn respawn(&mut self) -> io::Result<()> {
        self.dead = false;
        self.connection.send(
            OUT_CLIENT_STATUS,
            &body(|w: &mut Writer| {
                w.var_int(0);
            }),
        )
    }

    /// Swing the arm, so other players see something happen.
    pub fn swing(&mut self) -> io::Result<()> {
        self.connection.send(OUT_SWING, &[])
    }

    /// Change one block, and note that its column needs drawing again.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, block: u16) {
        if !(0..256).contains(&y) {
            return;
        }
        let at = (x.div_euclid(16), z.div_euclid(16));
        if let Some(column) = self.columns.get_mut(&at) {
            column.set_block(
                x.rem_euclid(16) as usize,
                y as usize,
                z.rem_euclid(16) as usize,
                block,
            );
            self.dirty.insert(at);
            // A block on a chunk edge changes what its neighbour can see through.
            if x.rem_euclid(16) == 0 {
                self.dirty.insert((at.0 - 1, at.1));
            }
            if x.rem_euclid(16) == 15 {
                self.dirty.insert((at.0 + 1, at.1));
            }
            if z.rem_euclid(16) == 0 {
                self.dirty.insert((at.0, at.1 - 1));
            }
            if z.rem_euclid(16) == 15 {
                self.dirty.insert((at.0, at.1 + 1));
            }
        }
    }

    /// How bright it is at a world position, 0..15.
    #[must_use]
    pub fn light_at(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..256).contains(&y) {
            return 15;
        }
        self.columns
            .get(&(x.div_euclid(16), z.div_euclid(16)))
            .map_or(15, |column| {
                column.light_at(
                    x.rem_euclid(16) as usize,
                    y as usize,
                    z.rem_euclid(16) as usize,
                )
            })
    }

    /// The block the player is standing on.
    #[must_use]
    pub fn block_below(&self) -> Option<u16> {
        let p = self.position;
        self.block_at(
            p.x.floor() as i32,
            p.y.floor() as i32 - 1,
            p.z.floor() as i32,
        )
    }

    /// How much of the day has passed, 0..1, with 0 at dawn.
    #[must_use]
    pub fn time_of_day(&self) -> f32 {
        self.day_time as f32 / 24_000.0
    }

    /// How bright the sky is, 0.2 at midnight to 1.0 at noon.
    ///
    /// This is vanilla's own curve: the sun's height through the day, clamped so that most of the
    /// day is full brightness and the change happens over dusk and dawn, with night never
    /// reaching black.
    #[must_use]
    pub fn sky_brightness(&self) -> f32 {
        // Day time 0 is sunrise, so noon (6000) is a quarter of the way round.
        let turn = (self.time_of_day() - 0.25).rem_euclid(1.0);
        let height = (turn * std::f32::consts::TAU).cos();
        let dark = 1.0 - (height * 2.0 + 0.5).clamp(0.0, 1.0);
        (1.0 - dark) * 0.8 + 0.2
    }
}

/// What a mob says when it is hit.
const fn hurt_sound(kind: Kind) -> &'static str {
    match kind {
        Kind::Player => "game.player.hurt",
        Kind::Mob(50) => "mob.creeper.say",
        Kind::Mob(51) => "mob.skeleton.hurt",
        Kind::Mob(54) => "mob.zombie.hurt",
        Kind::Mob(90) => "mob.pig.say",
        Kind::Mob(91) => "mob.sheep.say",
        Kind::Mob(92 | 96) => "mob.cow.hurt",
        Kind::Mob(93) => "mob.chicken.hurt",
        Kind::Mob(95) => "mob.wolf.hurt",
        Kind::Mob(_) => "game.neutral.hurt",
        Kind::Object(_) => "random.break",
    }
}

/// The decoded world, as something the physics can walk on.
impl crate::physics::World for Session {
    fn solid(&self, x: i32, y: i32, z: i32) -> bool {
        self.block_at(x, y, z).is_some_and(crate::blocks::solid)
    }

    fn fluid(&self, x: i32, y: i32, z: i32) -> crate::blocks::Fluid {
        self.block_at(x, y, z)
            .map_or(crate::blocks::Fluid::None, crate::blocks::fluid)
    }
}

impl Session {
    /// Read whatever has already arrived, and stop as soon as nothing has. Keeps the tick loop
    /// to time instead of blocking on the next packet.
    pub fn pump(&mut self) -> io::Result<()> {
        self.connection
            .set_read_timeout(Some(Duration::from_millis(1)))?;
        let outcome = loop {
            match self.step() {
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    break Ok(());
                }
                Err(e) => break Err(e),
            }
        };
        self.connection
            .set_read_timeout(Some(Duration::from_secs(10)))?;
        outcome
    }
}

/// Read the Join Game that always comes first in the play state.
pub fn read_join(connection: &mut Connection) -> io::Result<Joined> {
    loop {
        let (id, payload) = connection.recv()?;
        if id == DISCONNECT {
            let reason = Reader::new(&payload)
                .string(64 * 1024)
                .map_err(bad)?
                .to_owned();
            return Err(io::Error::other(format!("disconnected: {reason}")));
        }
        if id != JOIN_GAME {
            continue;
        }
        let mut r = Reader::new(&payload);
        return Ok(Joined {
            entity_id: r.i32().map_err(bad)?,
            gamemode: r.u8().map_err(bad)?,
            dimension: r.i8().map_err(bad)?,
            difficulty: r.u8().map_err(bad)?,
            max_players: r.u8().map_err(bad)?,
            level_type: r.string(64).map_err(bad)?.to_owned(),
        });
    }
}

/// Pack a block position the way 1.8 wants it.
#[must_use]
pub fn encode_position(x: i32, y: i32, z: i32) -> i64 {
    ((i64::from(x) & 0x3FF_FFFF) << 38)
        | ((i64::from(y) & 0xFFF) << 26)
        | (i64::from(z) & 0x3FF_FFFF)
}

/// 1.8 packs a block position into one long: 26 bits of x, 12 of y, 26 of z, all signed.
fn decode_position(value: i64) -> (i32, i32, i32) {
    let x = (value >> 38) as i32;
    let y = ((value >> 26) & 0xFFF) as i32;
    // Shifting left then arithmetic-right sign-extends the low 26 bits.
    let z = ((value << 38) >> 38) as i32;
    (x, y, z)
}

fn bad(e: crate::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}"))
}

#[cfg(test)]
mod tests {
    use super::decode_position;

    #[test]
    fn encoding_a_position_and_decoding_it_gives_the_same_block() {
        for (x, y, z) in [(0, 0, 0), (1, 70, -3), (-30_000_000, 255, 30_000_000)] {
            assert_eq!(
                super::decode_position(super::encode_position(x, y, z)),
                (x, y, z)
            );
        }
    }

    #[test]
    fn a_packed_position_comes_back_out() {
        // The encoding the server uses: x << 38 | (y & 0xFFF) << 26 | (z & 0x3FFFFFF).
        for (x, y, z) in [
            (0, 0, 0),
            (1, 2, 3),
            (-1, 255, -1),
            (30_000_000, 64, -30_000_000),
        ] {
            let packed =
                (i64::from(x) << 38) | ((i64::from(y) & 0xFFF) << 26) | (i64::from(z) & 0x3FF_FFFF);
            assert_eq!(decode_position(packed), (x, y, z), "round trip {x},{y},{z}");
        }
    }
}
