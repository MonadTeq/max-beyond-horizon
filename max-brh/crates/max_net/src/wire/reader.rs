//! Byte-level primitives of the game protocol (`max-brh-docs/protocol.md` §8.1).
//!
//! The wire format is a raw little-endian field dump with no struct padding:
//! every integer is written field by field by the C++ port's `operator<<`,
//! strings carry an explicit length and there is **no** length prefix at the
//! packet level (the ENet frame length is the packet length).
//!
//! Two string flavours exist and must not be confused:
//!
//! * `std::string` — `u32 len` + raw bytes, no NUL (**packets 33/35/36/44**);
//! * `SmartString` — `u16 len` + bytes **including** the trailing NUL
//!   (**packets 09/16/18/43**).
//!
//! Fixed-size `char[N]` arrays are written as raw bytes; the C++ receiver
//! forces the last byte to NUL (`remote.cpp`, `Remote_ReadNetNode` /
//! `Remote_ReadGameSettings`), so [`WireName`] mirrors that convention.

use std::fmt;

/// Maximum accepted/emitted string length (`REMOTE_MAX_STRING_LENGTH`).
pub const MAX_STRING_LEN: usize = 4096;
/// Maximum accepted/emitted vector length (`REMOTE_MAX_VECTOR_LENGTH`).
pub const MAX_VECTOR_LEN: usize = 1024;
/// Size of the fixed `char[30]` name arrays on the wire.
pub const NAME_LEN: usize = 30;

/// Errors produced while encoding or decoding a wire packet.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    /// The packet ended before a field could be read.
    #[error("unexpected end of packet at offset {offset}: need {need} more byte(s)")]
    UnexpectedEof {
        /// Offset at which the read was attempted.
        offset: usize,
        /// Number of bytes the read wanted.
        need: usize,
    },
    /// A `std::string`/`SmartString` length exceeded the protocol limit.
    #[error("{kind} length {len} exceeds the protocol limit {max}")]
    StringTooLong {
        /// Human-readable field kind (`"string"`/`"SmartString"`).
        kind: &'static str,
        /// Length carried on the wire.
        len: usize,
        /// Protocol limit.
        max: usize,
    },
    /// A vector length exceeded the protocol limit.
    #[error("vector length {len} exceeds the protocol limit {max}")]
    VectorTooLong {
        /// Length carried on the wire.
        len: usize,
        /// Protocol limit.
        max: usize,
    },
    /// An enum/identifier value is outside its documented range.
    #[error("invalid {kind} value {value}")]
    InvalidValue {
        /// Field kind being decoded.
        kind: &'static str,
        /// Raw value from the packet.
        value: u32,
    },
    /// The packet contains bytes the decoder did not consume.
    #[error("{0} trailing byte(s) after the packet body")]
    TrailingBytes(usize),
    /// A value does not fit the target type.
    #[error("value {value} does not fit into {target}")]
    OutOfRange {
        /// Value encountered while encoding.
        value: i64,
        /// Target type name.
        target: &'static str,
    },
    /// The packet type byte is not an APPL packet (`0x50..=0x86`).
    #[error("unknown APPL packet type byte 0x{0:02X}")]
    UnknownPacket(u8),
    /// A packet is larger than [`MAX_PACKET_SIZE`](super::MAX_PACKET_SIZE).
    #[error("packet size {size} exceeds the protocol limit {max}")]
    PacketTooLarge {
        /// Encoded packet size.
        size: usize,
        /// Protocol limit.
        max: usize,
    },
    /// A decoded packet cannot be encoded back into the same wire shape.
    #[error("packet 0x{kind:02X} cannot be encoded from this variant")]
    Unencodable {
        /// APPL packet kind (`type - 0x50`).
        kind: u8,
    },
    /// A packet with optional trailing fields was filled partially.
    #[error("packet {packet} needs all optional fields set or all unset")]
    PartialFields {
        /// Packet name.
        packet: &'static str,
    },
}

/// Result alias for wire codecs.
pub type WireResult<T> = Result<T, WireError>;

/// Cursor over a packet body.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Creates a reader over `buf`.
    pub const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Current byte offset.
    pub const fn position(&self) -> usize {
        self.pos
    }

    /// Bytes left.
    pub const fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// True when the body is fully consumed.
    pub const fn is_empty(&self) -> bool {
        self.pos == self.buf.len()
    }

    /// Ensures the whole body was consumed.
    pub fn finish(self) -> WireResult<()> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(WireError::TrailingBytes(self.remaining()))
        }
    }

    fn take(&mut self, n: usize) -> WireResult<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(WireError::UnexpectedEof {
            offset: self.pos,
            need: n,
        })?;
        if end > self.buf.len() {
            return Err(WireError::UnexpectedEof {
                offset: self.pos,
                need: n,
            });
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    /// Reads `u8`.
    pub fn read_u8(&mut self) -> WireResult<u8> {
        Ok(self.take(1)?[0])
    }

    /// Reads `i8`.
    pub fn read_i8(&mut self) -> WireResult<i8> {
        Ok(self.read_u8()? as i8)
    }

    /// Reads a boolean stored as one byte (any non-zero value is `true`,
    /// matching C++ `bool` reads of raw bytes).
    pub fn read_bool(&mut self) -> WireResult<bool> {
        Ok(self.read_u8()? != 0)
    }

    /// Reads `u16` (little-endian).
    pub fn read_u16(&mut self) -> WireResult<u16> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Reads `i16` (little-endian).
    pub fn read_i16(&mut self) -> WireResult<i16> {
        Ok(self.read_u16()? as i16)
    }

    /// Reads `u32` (little-endian).
    pub fn read_u32(&mut self) -> WireResult<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads `i32` (little-endian).
    pub fn read_i32(&mut self) -> WireResult<i32> {
        Ok(self.read_u32()? as i32)
    }

    /// Reads `std::string`: `u32 len` + bytes (no NUL).
    pub fn read_string(&mut self) -> WireResult<String> {
        let len = self.read_u32()? as usize;
        if len > MAX_STRING_LEN {
            return Err(WireError::StringTooLong {
                kind: "string",
                len,
                max: MAX_STRING_LEN,
            });
        }
        let bytes = self.take(len)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    /// Reads `SmartString`: `u16 len` + bytes including a trailing NUL.
    ///
    /// The decoder is lenient about the terminator (some producers write the
    /// length without the NUL); the bytes are always treated as UTF-8.
    pub fn read_smart_string(&mut self) -> WireResult<String> {
        let len = self.read_u16()? as usize;
        if len > MAX_STRING_LEN {
            return Err(WireError::StringTooLong {
                kind: "SmartString",
                len,
                max: MAX_STRING_LEN,
            });
        }
        if len == 0 {
            return Ok(String::new());
        }
        let bytes = self.take(len)?;
        let content = bytes.strip_suffix(&[0]).unwrap_or(bytes);
        Ok(String::from_utf8_lossy(content).into_owned())
    }

    /// Reads a `u32`-prefixed vector length and validates it.
    pub fn read_vec_len(&mut self) -> WireResult<usize> {
        let len = self.read_u32()? as usize;
        if len > MAX_VECTOR_LEN {
            return Err(WireError::VectorTooLong {
                len,
                max: MAX_VECTOR_LEN,
            });
        }
        Ok(len)
    }

    /// Reads a fixed-size raw `char[N]` name array.
    pub fn read_name(&mut self) -> WireResult<WireName> {
        let bytes = self.take(NAME_LEN)?;
        let mut name = [0u8; NAME_LEN];
        name.copy_from_slice(bytes);
        // The C++ reader always forces the last byte to NUL.
        name[NAME_LEN - 1] = 0;
        Ok(WireName(name))
    }

    /// Reads `N` raw bytes into an array.
    pub fn read_array<const N: usize>(&mut self) -> WireResult<[u8; N]> {
        let bytes = self.take(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(bytes);
        Ok(out)
    }
}

/// Writer for a packet body.
#[derive(Debug, Clone, Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    /// Creates an empty writer.
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Creates a writer with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
        }
    }

    /// Number of bytes written so far.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// True when nothing has been written.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Consumes the writer and returns the encoded bytes.
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }

    /// Returns the bytes written so far.
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }

    /// Writes `u8`.
    pub fn write_u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    /// Writes `i8`.
    pub fn write_i8(&mut self, value: i8) {
        self.buf.push(value as u8);
    }

    /// Writes a boolean as one byte (`0`/`1`, as C++ `bool`).
    pub fn write_bool(&mut self, value: bool) {
        self.buf.push(u8::from(value));
    }

    /// Writes `u16` (little-endian).
    pub fn write_u16(&mut self, value: u16) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes `i16` (little-endian).
    pub fn write_i16(&mut self, value: i16) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes `u32` (little-endian).
    pub fn write_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes `i32` (little-endian).
    pub fn write_i32(&mut self, value: i32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes `std::string`: `u32 len` + bytes.
    pub fn write_string(&mut self, value: &str) -> WireResult<()> {
        if value.len() > MAX_STRING_LEN {
            return Err(WireError::StringTooLong {
                kind: "string",
                len: value.len(),
                max: MAX_STRING_LEN,
            });
        }
        self.write_u32(value.len() as u32);
        self.buf.extend_from_slice(value.as_bytes());
        Ok(())
    }

    /// Writes `SmartString`: `u16 len` + bytes + NUL.
    pub fn write_smart_string(&mut self, value: &str) -> WireResult<()> {
        let len = value.len() + 1;
        if len > MAX_STRING_LEN || len > u16::MAX as usize {
            return Err(WireError::StringTooLong {
                kind: "SmartString",
                len,
                max: MAX_STRING_LEN,
            });
        }
        self.write_u16(len as u16);
        self.buf.extend_from_slice(value.as_bytes());
        self.write_u8(0);
        Ok(())
    }

    /// Writes a `char[30]` name (up to 29 bytes, zero padded, always NUL
    /// terminated — same as the C++ receiver's convention).
    pub fn write_name(&mut self, name: &WireName) {
        self.buf.extend_from_slice(&name.0);
    }

    /// Writes raw bytes.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Writes raw bytes from a fixed array.
    pub fn write_array<const N: usize>(&mut self, bytes: &[u8; N]) {
        self.buf.extend_from_slice(bytes);
    }
}

/// Fixed `char[30]` game name.
///
/// Kept as raw bytes (not `String`) because the original game writes local
/// code-page bytes and the receiver only guarantees NUL termination. Compare
/// and display with [`WireName::as_str`] (lossy UTF-8).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WireName([u8; NAME_LEN]);

impl WireName {
    /// Empty name.
    pub const EMPTY: Self = Self([0; NAME_LEN]);

    /// Raw bytes from the wire, including NUL padding.
    pub const fn as_bytes(&self) -> &[u8; NAME_LEN] {
        &self.0
    }

    /// Decoded name up to the first NUL (lossy UTF-8).
    pub fn as_str(&self) -> std::borrow::Cow<'_, str> {
        let end = self
            .0
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(NAME_LEN);
        String::from_utf8_lossy(&self.0[..end])
    }

    /// Builds a name, truncating to 29 bytes so the last byte stays NUL.
    pub fn from_str_truncated(value: &str) -> Self {
        let mut bytes = [0u8; NAME_LEN];
        let value = value.as_bytes();
        let len = value.len().min(NAME_LEN - 1);
        bytes[..len].copy_from_slice(&value[..len]);
        Self(bytes)
    }
}

impl fmt::Debug for WireName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl fmt::Display for WireName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_str())
    }
}

impl From<&str> for WireName {
    fn from(value: &str) -> Self {
        Self::from_str_truncated(value)
    }
}

impl From<String> for WireName {
    fn from(value: String) -> Self {
        Self::from_str_truncated(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_round_trip_little_endian() {
        let mut w = Writer::new();
        w.write_u8(0xAB);
        w.write_i8(-2);
        w.write_u16(0x1234);
        w.write_i16(-300);
        w.write_u32(0xDEAD_BEEF);
        w.write_i32(-123_456);
        w.write_bool(true);
        w.write_bool(false);
        assert_eq!(
            w.as_slice(),
            &[
                0xAB, 0xFE, 0x34, 0x12, 0xD4, 0xFE, 0xEF, 0xBE, 0xAD, 0xDE, 0xC0, 0x1D, 0xFE, 0xFF,
                0x01, 0x00
            ]
        );

        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.read_u8().unwrap(), 0xAB);
        assert_eq!(r.read_i8().unwrap(), -2);
        assert_eq!(r.read_u16().unwrap(), 0x1234);
        assert_eq!(r.read_i16().unwrap(), -300);
        assert_eq!(r.read_u32().unwrap(), 0xDEAD_BEEF);
        assert_eq!(r.read_i32().unwrap(), -123_456);
        assert!(r.read_bool().unwrap());
        assert!(!r.read_bool().unwrap());
        r.finish().unwrap();
    }

    #[test]
    fn std_string_is_u32_length_without_nul() {
        let mut w = Writer::new();
        w.write_string("abc").unwrap();
        assert_eq!(w.as_slice(), &[3, 0, 0, 0, b'a', b'b', b'c']);

        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.read_string().unwrap(), "abc");
        r.finish().unwrap();
    }

    #[test]
    fn smart_string_is_u16_length_including_nul() {
        let mut w = Writer::new();
        w.write_smart_string("abc").unwrap();
        assert_eq!(w.as_slice(), &[4, 0, b'a', b'b', b'c', 0]);

        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.read_smart_string().unwrap(), "abc");
        r.finish().unwrap();
    }

    #[test]
    fn smart_string_accepts_missing_terminator_leniently() {
        let bytes = [3, 0, b'a', b'b', b'c'];
        let mut r = Reader::new(&bytes);
        assert_eq!(r.read_smart_string().unwrap(), "abc");
    }

    #[test]
    fn strings_are_bounded() {
        let too_long = u32::MAX.to_le_bytes();
        let mut r = Reader::new(&too_long);
        assert!(matches!(
            r.read_string(),
            Err(WireError::StringTooLong { .. })
        ));
        let smart = [0xFF, 0xFF];
        assert!(matches!(
            Reader::new(&smart).read_smart_string(),
            Err(WireError::StringTooLong { .. })
        ));
    }

    #[test]
    fn reads_past_end_is_an_error() {
        let mut r = Reader::new(&[1, 2]);
        assert_eq!(r.read_u16().unwrap(), 0x0201);
        assert!(matches!(
            r.read_u8(),
            Err(WireError::UnexpectedEof { offset: 2, need: 1 })
        ));
    }

    #[test]
    fn names_are_nul_terminated_and_truncated_to_29() {
        let name = WireName::from("abcdefghijklmnopqrstuvwxyz0123456789");
        assert_eq!(name.as_str(), "abcdefghijklmnopqrstuvwxyz012");
        assert_eq!(name.as_bytes()[29], 0);
        assert_eq!(WireName::EMPTY.as_str(), "");
    }

    #[test]
    fn trailing_bytes_are_detected() {
        let r = Reader::new(&[0, 1]);
        assert!(matches!(r.finish(), Err(WireError::TrailingBytes(2))));
    }
}
