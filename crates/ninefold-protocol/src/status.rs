//! The server list ping: what a server says about itself before you log in.
//!
//! Three packets, no authentication and no compression, which makes it the smallest complete
//! exchange in the protocol — and the first thing worth getting right, because it proves the
//! handshake, the framing and the length-prefixed strings all agree with a real server.

use std::io;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::wire::{Connection, body};
use crate::{NextState, handshake};

/// What a server reports about itself.
#[derive(Debug, Clone)]
pub struct Status {
    /// The version name the server shows, e.g. "1.8.9" or "Paper 1.8.8".
    pub version: String,
    /// The protocol it speaks. 47 is 1.8–1.8.9; anything else will refuse a 1.8.9 client.
    pub protocol: i32,
    pub players_online: u32,
    pub players_max: u32,
    /// The message of the day, with colour codes stripped.
    pub motd: String,
    /// How long the server took to answer a ping.
    pub latency: std::time::Duration,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    version: RawVersion,
    #[serde(default)]
    players: RawPlayers,
    #[serde(default)]
    description: serde_json::Value,
}

#[derive(Deserialize, Default)]
struct RawVersion {
    #[serde(default)]
    name: String,
    #[serde(default)]
    protocol: i32,
}

#[derive(Deserialize, Default)]
struct RawPlayers {
    #[serde(default)]
    online: u32,
    #[serde(default)]
    max: u32,
}

/// Ask a server what it is.
pub fn ping(host: &str, port: u16) -> io::Result<Status> {
    let mut c = Connection::connect(host, port)?;

    // Handshake, then Status Request (0x00, empty).
    c.send_raw(&handshake(host, port, NextState::Status))?;
    c.send(0x00, &[])?;

    let (id, payload) = c.recv()?;
    if id != 0x00 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected the status response, got packet {id:#04x}"),
        ));
    }
    let json = crate::Reader::new(&payload)
        .string(64 * 1024)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}")))?
        .to_owned();

    // Ping (0x01) carries any number and the server echoes it back, which is the round trip.
    let token = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let sent = Instant::now();
    c.send(
        0x01,
        &body(|w| {
            w.i64(token);
        }),
    )?;
    let (id, echo) = c.recv()?;
    let latency = sent.elapsed();
    if id != 0x01 || crate::Reader::new(&echo).i64().ok() != Some(token) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the server did not echo the ping",
        ));
    }

    let raw: Raw = serde_json::from_str(&json)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("status json: {e}")))?;
    Ok(Status {
        // Servers put colour codes in the version name too, not just the MOTD.
        version: strip_colours(&raw.version.name),
        protocol: raw.version.protocol,
        players_online: raw.players.online,
        players_max: raw.players.max,
        motd: motd_text(&raw.description),
        latency,
    })
}

/// Flatten a chat component — or a plain string — into readable text, colour codes removed.
///
/// The same shape turns up in the server list ping, in chat and in disconnect reasons.
#[must_use]
pub fn chat_text(json: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(value) => motd_text(&value),
        // Some servers send a bare string with no quotes in older paths.
        Err(_) => strip_colours(json),
    }
}

/// The description is a plain string on some servers and a chat component on others, and the
/// component may nest `extra` parts without limit. Flatten whichever arrived and drop the
/// §-colour codes.
fn motd_text(value: &serde_json::Value) -> String {
    let mut out = String::new();
    flatten(value, &mut out);
    strip_colours(&out)
}

fn flatten(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::String(s) => out.push_str(s),
        serde_json::Value::Array(items) => items.iter().for_each(|i| flatten(i, out)),
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(text)) = map.get("text") {
                out.push_str(text);
            }
            if let Some(extra) = map.get("extra") {
                flatten(extra, out);
            }
        }
        _ => {}
    }
}

/// Remove `§x` pairs. The code after `§` is one character, and a trailing `§` is dropped.
fn strip_colours(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_string_description_is_taken_as_is() {
        assert_eq!(
            motd_text(&serde_json::json!("A Minecraft Server")),
            "A Minecraft Server"
        );
    }

    #[test]
    fn a_chat_component_is_flattened_in_order() {
        let value = serde_json::json!({
            "text": "Welcome ",
            "extra": [{"text": "to "}, {"text": "the server", "extra": [{"text": "!"}]}],
        });
        assert_eq!(motd_text(&value), "Welcome to the server!");
    }

    #[test]
    fn colour_codes_are_stripped_including_a_trailing_marker() {
        assert_eq!(strip_colours("§aGreen §lBold§r plain"), "Green Bold plain");
        assert_eq!(strip_colours("ends with §"), "ends with ");
        assert_eq!(
            strip_colours("§§a"),
            "a",
            "an escaped marker eats the next char"
        );
    }

    #[test]
    fn the_version_name_is_stripped_too() {
        // Seen in the wild: play.cubecraft.net answers "§f§f§fWe support: 1.20-1.21".
        assert_eq!(
            strip_colours("§f§f§fWe support: 1.20-1.21"),
            "We support: 1.20-1.21"
        );
    }

    #[test]
    fn a_description_of_an_unexpected_shape_gives_an_empty_motd_rather_than_failing() {
        assert_eq!(motd_text(&serde_json::json!(12345)), "");
        assert_eq!(motd_text(&serde_json::Value::Null), "");
    }
}
