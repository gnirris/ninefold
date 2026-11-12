//! The 1.8.9 wire protocol (version 47).
//!
//! Every packet on the wire is `VarInt length, VarInt id, payload`, and most fields inside the
//! payload are length-prefixed with a VarInt too, so this codec is the floor the whole client
//! stands on. Protocol 47 covers Minecraft 1.8 through 1.8.9 — servers advertise it as `47` and
//! will not talk to a client claiming anything else.
//!
//! Note for anyone arriving from a modern-protocol codebase: this is *pre-flattening*. Blocks are
//! an ID and four bits of metadata packed into a `u16`, not a block state; chunks are sent as
//! fixed arrays, not a palette. Nothing here can be shared with a 1.13+ implementation.

pub mod status;
pub mod wire;

/// Protocol version 1.8–1.8.9 servers expect in a handshake.
pub const PROTOCOL_VERSION: i32 = 47;

/// A VarInt is at most five bytes: 32 bits carried seven at a time.
pub const MAX_VARINT_LEN: usize = 5;

/// What went wrong reading a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The buffer ended mid-value. With a stream, read more and try again.
    Incomplete,
    /// A VarInt ran past five bytes, so it cannot be a 32-bit value. A desynchronised stream
    /// looks exactly like this, which is why it is an error and not a saturating read.
    VarIntTooLong,
    /// A string field was not UTF-8.
    BadUtf8,
    /// A length prefix was negative, or longer than the caller allows.
    BadLength,
}

pub type Result<T> = core::result::Result<T, Error>;

/// Reads protocol values out of a byte slice, tracking how far it has got.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// How many bytes have been consumed.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.at
    }

    /// What is left unread.
    #[must_use]
    pub fn rest(&self) -> &'a [u8] {
        &self.bytes[self.at..]
    }

    pub fn u8(&mut self) -> Result<u8> {
        let byte = *self.bytes.get(self.at).ok_or(Error::Incomplete)?;
        self.at += 1;
        Ok(byte)
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes([self.u8()?, self.u8()?]))
    }

    pub fn i64(&mut self) -> Result<i64> {
        let mut out = [0u8; 8];
        for slot in &mut out {
            *slot = self.u8()?;
        }
        Ok(i64::from_be_bytes(out))
    }

    /// A VarInt: seven bits per byte, little end first, top bit meaning "another follows".
    pub fn var_int(&mut self) -> Result<i32> {
        let mut value: u32 = 0;
        for shift in 0..MAX_VARINT_LEN {
            let byte = self.u8()?;
            value |= u32::from(byte & 0x7F) << (shift * 7);
            if byte & 0x80 == 0 {
                // Negative numbers are sent as their two's complement, so this is a bit cast.
                return Ok(value as i32);
            }
        }
        Err(Error::VarIntTooLong)
    }

    /// A VarInt length followed by that many bytes.
    pub fn bytes(&mut self, limit: usize) -> Result<&'a [u8]> {
        let len = self.var_int()?;
        let len = usize::try_from(len).map_err(|_| Error::BadLength)?;
        if len > limit {
            return Err(Error::BadLength);
        }
        let end = self.at.checked_add(len).ok_or(Error::BadLength)?;
        let slice = self.bytes.get(self.at..end).ok_or(Error::Incomplete)?;
        self.at = end;
        Ok(slice)
    }

    /// A VarInt length followed by that many bytes of UTF-8.
    ///
    /// `limit` is in bytes. The protocol limits strings by characters, so passing
    /// `max_chars * 4` is the safe conversion.
    pub fn string(&mut self, limit: usize) -> Result<&'a str> {
        core::str::from_utf8(self.bytes(limit)?).map_err(|_| Error::BadUtf8)
    }
}

/// Builds a packet body.
#[derive(Debug, Default, Clone)]
pub struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    #[must_use]
    pub const fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }

    pub fn u8(&mut self, value: u8) -> &mut Self {
        self.bytes.push(value);
        self
    }

    pub fn u16(&mut self, value: u16) -> &mut Self {
        self.bytes.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn i64(&mut self, value: i64) -> &mut Self {
        self.bytes.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn var_int(&mut self, value: i32) -> &mut Self {
        let mut rest = value as u32;
        loop {
            let byte = (rest & 0x7F) as u8;
            rest >>= 7;
            if rest == 0 {
                self.bytes.push(byte);
                return self;
            }
            self.bytes.push(byte | 0x80);
        }
    }

    pub fn bytes(&mut self, value: &[u8]) -> &mut Self {
        // A length that does not fit in an i32 cannot be sent; no caller is anywhere near it.
        self.var_int(i32::try_from(value.len()).unwrap_or(i32::MAX));
        self.bytes.extend_from_slice(value);
        self
    }

    pub fn string(&mut self, value: &str) -> &mut Self {
        self.bytes(value.as_bytes())
    }
}

/// Wraps a packet id and body in the length prefix the wire expects.
///
/// Only valid before compression is switched on; once the server sends Set Compression, packets
/// carry an extra uncompressed-length field.
#[must_use]
pub fn frame(id: i32, body: &[u8]) -> Vec<u8> {
    let mut head = Writer::new();
    head.var_int(id);
    let head = head.finish();
    let mut out = Writer::new();
    out.var_int(i32::try_from(head.len() + body.len()).unwrap_or(i32::MAX));
    let mut out = out.finish();
    out.extend_from_slice(&head);
    out.extend_from_slice(body);
    out
}

/// What the client is asking the server to switch to after the handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextState {
    Status = 1,
    Login = 2,
}

/// The first packet of any connection: id 0x00 in the handshaking state.
#[must_use]
pub fn handshake(host: &str, port: u16, next: NextState) -> Vec<u8> {
    let mut body = Writer::new();
    body.var_int(PROTOCOL_VERSION)
        .string(host)
        .u16(port)
        .var_int(next as i32);
    frame(0x00, &body.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_ints_match_the_protocol_examples() {
        // The values wiki.vg pins the format to, including the negative ones.
        for (value, bytes) in [
            (0i32, vec![0x00]),
            (1, vec![0x01]),
            (2, vec![0x02]),
            (127, vec![0x7f]),
            (128, vec![0x80, 0x01]),
            (255, vec![0xff, 0x01]),
            (25565, vec![0xdd, 0xc7, 0x01]),
            (2_097_151, vec![0xff, 0xff, 0x7f]),
            (2_147_483_647, vec![0xff, 0xff, 0xff, 0xff, 0x07]),
            (-1, vec![0xff, 0xff, 0xff, 0xff, 0x0f]),
            (-2_147_483_648, vec![0x80, 0x80, 0x80, 0x80, 0x08]),
        ] {
            let mut w = Writer::new();
            w.var_int(value);
            assert_eq!(w.finish(), bytes, "writing {value}");
            assert_eq!(Reader::new(&bytes).var_int(), Ok(value), "reading {value}");
        }
    }

    #[test]
    fn a_cut_off_var_int_asks_for_more_rather_than_guessing() {
        assert_eq!(Reader::new(&[0x80]).var_int(), Err(Error::Incomplete));
        assert_eq!(Reader::new(&[]).var_int(), Err(Error::Incomplete));
    }

    #[test]
    fn a_six_byte_var_int_is_refused() {
        // A desynchronised stream looks like this; it must not be read as a huge number.
        let runaway = [0x80, 0x80, 0x80, 0x80, 0x80, 0x01];
        assert_eq!(Reader::new(&runaway).var_int(), Err(Error::VarIntTooLong));
    }

    #[test]
    fn strings_round_trip_and_respect_their_limit() {
        let mut w = Writer::new();
        w.string("Stece");
        let bytes = w.finish();
        assert_eq!(Reader::new(&bytes).string(32).unwrap(), "Stece");
        // A string longer than the caller allows is refused, not truncated.
        assert_eq!(Reader::new(&bytes).string(2), Err(Error::BadLength));
    }

    #[test]
    fn non_utf8_is_an_error_not_a_panic() {
        let mut w = Writer::new();
        w.bytes(&[0xff, 0xfe]);
        assert_eq!(Reader::new(&w.finish()).string(8), Err(Error::BadUtf8));
    }

    #[test]
    fn handshake_is_what_a_1_8_9_server_expects() {
        let packet = handshake("localhost", 25565, NextState::Login);
        let mut r = Reader::new(&packet);
        let len = r.var_int().unwrap();
        assert_eq!(len as usize, packet.len() - r.position());
        assert_eq!(r.var_int(), Ok(0x00), "handshake is packet id 0");
        assert_eq!(r.var_int(), Ok(47), "1.8.9 is protocol 47");
        assert_eq!(r.string(255 * 4), Ok("localhost"));
        assert_eq!(r.u16(), Ok(25565));
        assert_eq!(r.var_int(), Ok(2), "next state: login");
        assert!(r.rest().is_empty(), "no trailing bytes");
    }
}
