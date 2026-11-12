//! Logging in.
//!
//! Offline servers (`online-mode=false`) take a name and let you in. Online servers answer the
//! login start with an encryption request, which means an RSA exchange and a session check
//! against Mojang; that is not here yet, and a server asking for it is reported rather than
//! fumbled.
//!
//! Somewhere in the middle the server usually sends Set Compression, after which every packet is
//! framed differently — so this has to be the thing that reads it, before anything else does.

use std::io;

use crate::wire::{Connection, body};
use crate::{NextState, Reader, handshake};

/// Who the server says you are, once it has let you in.
#[derive(Debug, Clone)]
pub struct Profile {
    pub uuid: String,
    pub username: String,
    /// The compression threshold the server set, if it set one.
    pub compression: Option<usize>,
}

const LOGIN_DISCONNECT: i32 = 0x00;
const ENCRYPTION_REQUEST: i32 = 0x01;
const LOGIN_SUCCESS: i32 = 0x02;
const SET_COMPRESSION: i32 = 0x03;

/// Connect and log in. The returned connection is in the play state, ready for Join Game.
pub fn login(host: &str, port: u16, username: &str) -> io::Result<(Connection, Profile)> {
    let mut c = Connection::connect(host, port)?;
    c.send_raw(&handshake(host, port, NextState::Login))?;
    c.send(
        0x00,
        &body(|w| {
            w.string(username);
        }),
    )?;

    let mut compression = None;
    loop {
        let (id, payload) = c.recv()?;
        match id {
            SET_COMPRESSION => {
                let threshold = Reader::new(&payload).var_int().map_err(bad)?;
                // A negative threshold means "turn it off again", which servers do send.
                if let Ok(threshold) = usize::try_from(threshold) {
                    c.set_compression(threshold);
                    compression = Some(threshold);
                }
            }
            LOGIN_SUCCESS => {
                let mut r = Reader::new(&payload);
                let uuid = r.string(36 * 4).map_err(bad)?.to_owned();
                let username = r.string(16 * 4).map_err(bad)?.to_owned();
                return Ok((
                    c,
                    Profile {
                        uuid,
                        username,
                        compression,
                    },
                ));
            }
            LOGIN_DISCONNECT => {
                let reason = Reader::new(&payload)
                    .string(64 * 1024)
                    .map_err(bad)?
                    .to_owned();
                return Err(io::Error::other(format!("the server refused us: {reason}")));
            }
            ENCRYPTION_REQUEST => {
                return Err(io::Error::other(
                    "this server is in online mode, which needs the Mojang session handshake \
                     (not implemented yet) — try a server with online-mode=false",
                ));
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected packet {other:#04x} during login"),
                ));
            }
        }
    }
}

fn bad(e: crate::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}"))
}
