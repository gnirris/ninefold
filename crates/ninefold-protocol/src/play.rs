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
const CHUNK_DATA: i32 = 0x21;
const MAP_CHUNK_BULK: i32 = 0x26;
const DISCONNECT: i32 = 0x40;

// Serverbound.
const OUT_KEEP_ALIVE: i32 = 0x00;
const OUT_POSITION_AND_LOOK: i32 = 0x06;

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
                // Answering this is what tells the server the teleport took.
                self.send_position()?;
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
        self.block_at(x, y, z).is_some_and(|block| block >> 4 != 0)
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

fn bad(e: crate::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}"))
}
