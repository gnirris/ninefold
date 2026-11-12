//! Packets over a socket.
//!
//! A packet on the wire is `VarInt length, VarInt id, payload`. Reading one means reading the
//! length before you know how much to read, so everything arrives into a buffer and is split off
//! once a whole packet is there — TCP will hand over half a packet, or three at once, and both
//! have to work.

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::{Error, Reader, Writer, frame};

/// Refuse a length no real packet reaches, so a desynchronised stream fails here instead of
/// trying to allocate gigabytes.
const MAX_PACKET: usize = 8 * 1024 * 1024;

const TIMEOUT: Duration = Duration::from_secs(10);

/// A connection to a server, before compression is switched on.
///
/// Once the server sends Set Compression (login, id 0x03), every packet gains an uncompressed
/// length field and this framing no longer applies. Status never compresses, so this type is
/// complete for a ping and is the starting point for login.
pub struct Connection {
    stream: TcpStream,
    /// Bytes read from the socket that have not yet become whole packets.
    buffered: Vec<u8>,
}

impl Connection {
    /// Open a connection. `host` is passed to the server in the handshake exactly as given, which
    /// is what SRV-redirected and proxied servers expect to see.
    pub fn connect(host: &str, port: u16) -> io::Result<Self> {
        let address = (host, port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address for that host"))?;
        let stream = TcpStream::connect_timeout(&address, TIMEOUT)?;
        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.set_write_timeout(Some(TIMEOUT))?;
        // Minecraft is a latency-sensitive stream of small packets; waiting to coalesce them is
        // exactly wrong.
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            buffered: Vec::new(),
        })
    }

    /// Write bytes that are already framed (the handshake builds its own).
    pub fn send_raw(&mut self, packet: &[u8]) -> io::Result<()> {
        self.stream.write_all(packet)?;
        self.stream.flush()
    }

    pub fn send(&mut self, id: i32, body: &[u8]) -> io::Result<()> {
        self.stream.write_all(&frame(id, body))?;
        self.stream.flush()
    }

    /// Block until a whole packet has arrived, and return its id and body.
    pub fn recv(&mut self) -> io::Result<(i32, Vec<u8>)> {
        loop {
            if let Some(packet) = self.take_packet()? {
                let mut r = Reader::new(&packet);
                let id = r.var_int().map_err(protocol_error)?;
                return Ok((id, r.rest().to_vec()));
            }
            let mut chunk = [0u8; 4096];
            let read = self.stream.read(&mut chunk)?;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the server closed the connection",
                ));
            }
            self.buffered.extend_from_slice(&chunk[..read]);
        }
    }

    /// Split one whole packet off the front of the buffer, if there is one.
    fn take_packet(&mut self) -> io::Result<Option<Vec<u8>>> {
        split_packet(&mut self.buffered)
    }
}

/// Take one whole packet off the front of `buffered`, leaving the rest.
///
/// `Ok(None)` means "not all here yet, read more" — the common case, since TCP hands over
/// whatever happens to have arrived.
pub fn split_packet(buffered: &mut Vec<u8>) -> io::Result<Option<Vec<u8>>> {
    let mut r = Reader::new(buffered);
    let len = match r.var_int() {
        Ok(len) => len,
        // The length prefix itself is not all here yet.
        Err(Error::Incomplete) => return Ok(None),
        Err(e) => return Err(protocol_error(e)),
    };
    let len = usize::try_from(len)
        .ok()
        .filter(|len| *len <= MAX_PACKET)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, format!("packet length {len}"))
        })?;
    let header = r.position();
    if buffered.len() < header + len {
        return Ok(None);
    }
    let packet = buffered[header..header + len].to_vec();
    buffered.drain(..header + len);
    Ok(Some(packet))
}

fn protocol_error(e: Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}"))
}

/// Build a packet body without repeating the Writer dance at every call site.
pub fn body(build: impl FnOnce(&mut Writer)) -> Vec<u8> {
    let mut w = Writer::new();
    build(&mut w);
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_packet_is_split_off_and_the_rest_left_alone() {
        let mut buf = frame(0x00, &[1, 2, 3]);
        assert_eq!(buf, vec![4, 0, 1, 2, 3], "length, id, body");
        buf.extend_from_slice(b"leftovers");

        let packet = split_packet(&mut buf).unwrap().expect("a whole packet");
        assert_eq!(packet, vec![0, 1, 2, 3], "id and body, without the length");
        assert_eq!(buf, b"leftovers", "the rest stays for the next read");
    }

    #[test]
    fn half_a_packet_waits_instead_of_guessing() {
        let whole = frame(0x00, &[1, 2, 3]);
        for cut in 0..whole.len() {
            let mut buf = whole[..cut].to_vec();
            let before = buf.clone();
            assert_eq!(split_packet(&mut buf).unwrap(), None, "cut at {cut}");
            assert_eq!(buf, before, "nothing consumed while waiting");
        }
        let mut buf = whole.clone();
        assert!(
            split_packet(&mut buf).unwrap().is_some(),
            "complete at last"
        );
    }

    #[test]
    fn three_packets_in_one_read_come_out_in_order() {
        let mut buf = frame(0x01, b"first");
        buf.extend_from_slice(&frame(0x02, b"second"));
        buf.extend_from_slice(&frame(0x03, b"third"));

        for (id, payload) in [(1u8, &b"first"[..]), (2, b"second"), (3, b"third")] {
            let packet = split_packet(&mut buf).unwrap().expect("a packet");
            assert_eq!(packet[0], id);
            assert_eq!(&packet[1..], payload);
        }
        assert!(buf.is_empty(), "nothing left over");
        assert_eq!(split_packet(&mut buf).unwrap(), None);
    }

    #[test]
    fn an_absurd_length_is_refused_rather_than_allocated() {
        // A desynchronised stream looks like a huge length; reading it must not try to reserve it.
        let mut w = Writer::new();
        w.var_int(i32::MAX);
        let mut buf = w.finish();
        buf.extend_from_slice(b"nowhere near that much");
        assert!(split_packet(&mut buf).is_err());
    }
}
