//! Being in the world.
//!
//! Only what is needed to get in and stay in: the server's welcome, the position it puts you at,
//! and the two things it will drop you for ignoring — keep-alives, and confirming where you are.
//! Chunks, entities and blocks come next.

use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

use crate::chunk::{self, Column};
use crate::wire::{Connection, body};
use crate::{Reader, Writer};

// Clientbound, play state, 1.8.9.
const KEEP_ALIVE: i32 = 0x00;
const JOIN_GAME: i32 = 0x01;
const POSITION_AND_LOOK: i32 = 0x08;
const BLOCK_CHANGE: i32 = 0x23;
const MULTI_BLOCK_CHANGE: i32 = 0x22;
const CHUNK_DATA: i32 = 0x21;
const MAP_CHUNK_BULK: i32 = 0x26;
const CHAT: i32 = 0x02;
const RESPAWN: i32 = 0x07;
const UPDATE_HEALTH: i32 = 0x06;
const HELD_SLOT: i32 = 0x09;
const DISCONNECT: i32 = 0x40;
const SPAWN_PLAYER: i32 = 0x0C;
const SPAWN_MOB: i32 = 0x0F;
const SPAWN_OBJECT: i32 = 0x0E;
const DESTROY_ENTITIES: i32 = 0x13;
const ENTITY_MOVE: i32 = 0x15;
const ENTITY_LOOK_MOVE: i32 = 0x17;
const ENTITY_TELEPORT: i32 = 0x18;

// Serverbound.
const OUT_KEEP_ALIVE: i32 = 0x00;
const OUT_DIG: i32 = 0x07;
const OUT_SWING: i32 = 0x0A;
const OUT_CLIENT_STATUS: i32 = 0x16;
const OUT_POSITION_AND_LOOK: i32 = 0x06;

/// Something in the world that is not the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entity {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub kind: Kind,
}

/// Enough to decide how big something is and what colour to draw it.
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
            Kind::Mob(54) => (0.6, 1.9),       // zombie
            Kind::Mob(90) => (0.9, 0.9),       // pig
            Kind::Mob(91 | 92) => (0.9, 1.3),  // sheep, cow
            Kind::Mob(93) => (0.4, 0.7),       // chicken
            Kind::Mob(_) => (0.8, 1.4),
            Kind::Object(_) => (0.25, 0.25),
        }
    }
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
    /// What has been said, newest last.
    pub chat: Vec<String>,
    /// Hearts, out of twenty.
    pub health: f32,
    /// Drumsticks, out of twenty.
    pub food: u8,
    /// Which hotbar slot is in hand, 0..8.
    pub held: u8,
    /// Health has run out, and the server is waiting to be asked for a respawn.
    pub dead: bool,
    /// Counts every time the server has moved us itself. Anything keeping its own copy of the
    /// position — the physics, for one — has to notice this and snap to the new one, or it will
    /// carry on from where it was and argue with the server about where the player is.
    pub teleports: u32,
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
            chat: Vec::new(),
            health: 20.0,
            food: 20,
            held: 0,
            dead: false,
            teleports: 0,
        }
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
            UPDATE_HEALTH => {
                let mut r = Reader::new(&payload);
                self.health = r.f32().map_err(bad)?;
                self.food = u8::try_from(r.var_int().map_err(bad)?.clamp(0, 20)).unwrap_or(20);
                if self.health <= 0.0 {
                    self.dead = true;
                }
            }
            HELD_SLOT => {
                let slot = Reader::new(&payload).u8().map_err(bad)?;
                self.held = slot.min(8);
            }
            SPAWN_PLAYER => {
                let mut r = Reader::new(&payload);
                let entity_id = r.var_int().map_err(bad)?;
                // Sixteen bytes of UUID, which nothing here needs yet.
                for _ in 0..16 {
                    r.u8().map_err(bad)?;
                }
                let entity = Entity {
                    x: fixed(r.i32().map_err(bad)?),
                    y: fixed(r.i32().map_err(bad)?),
                    z: fixed(r.i32().map_err(bad)?),
                    yaw: angle(r.u8().map_err(bad)?),
                    kind: Kind::Player,
                };
                self.entities.insert(entity_id, entity);
            }
            SPAWN_MOB | SPAWN_OBJECT => {
                let packet = id;
                let mut r = Reader::new(&payload);
                let entity_id = r.var_int().map_err(bad)?;
                let kind = r.u8().map_err(bad)?;
                let entity = Entity {
                    x: fixed(r.i32().map_err(bad)?),
                    y: fixed(r.i32().map_err(bad)?),
                    z: fixed(r.i32().map_err(bad)?),
                    yaw: angle(r.u8().map_err(bad)?),
                    kind: if packet == SPAWN_MOB {
                        Kind::Mob(kind)
                    } else {
                        Kind::Object(kind)
                    },
                };
                self.entities.insert(entity_id, entity);
            }
            DESTROY_ENTITIES => {
                let mut r = Reader::new(&payload);
                let count = r.var_int().map_err(bad)?.max(0);
                for _ in 0..count {
                    let gone = r.var_int().map_err(bad)?;
                    self.entities.remove(&gone);
                }
            }
            ENTITY_MOVE | ENTITY_LOOK_MOVE => {
                let packet = id;
                let mut r = Reader::new(&payload);
                let entity_id = r.var_int().map_err(bad)?;
                // Steps are eighths of a block... no: thirty-seconds, same as positions.
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
                if let Some(entity) = self.entities.get_mut(&entity_id) {
                    entity.x = x;
                    entity.y = y;
                    entity.z = z;
                    entity.yaw = yaw;
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
                return Err(io::Error::other(format!("disconnected: {reason}")));
            }
            _ => {}
        }
        Ok(id)
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

    fn dig_packet(&mut self, status: u8, x: i32, y: i32, z: i32, side: u8) -> io::Result<()> {
        let at = encode_position(x, y, z);
        self.connection.send(
            OUT_DIG,
            &body(|w: &mut Writer| {
                w.u8(status).i64(at).u8(side);
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
}

/// The decoded world, as something the physics can walk on.
///
/// Anything that is not air counts as solid, which is right for a flat world and wrong for
/// flowers and grass; a proper block table comes with the renderer.
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
