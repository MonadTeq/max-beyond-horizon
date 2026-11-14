//! ENet transport handshake on channel 0 (`docs/protocol.md` §7).
//!
//! Both peers send `TP 00` right after `ENET_EVENT_TYPE_CONNECT` and validate
//! the peer's copy. A mismatch is answered with `TP 03` (reason 1/2/3) and the
//! connection is dropped:
//!
//! * **reason 1** — game/enet/protocol version mismatch;
//! * **reason 2** — session ticket mismatch (server side only);
//! * **reason 3** — game data hash mismatch (`SHA256(MAX.RES) ‖
//!   SHA256(PATCHES.RES)`, 128 lowercase hex chars).
//!
//! The layout is `type u8, game_version u32, enet_version u32,
//! protocol_version u16, ticket string(u32 len), data_hash string(u32 len)`,
//! all little-endian. `TP 03` appends `reason u8` and `reason_text string`.

use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::wire::reader::{Reader, WireError, Writer};

/// ENet wire version pinned by the C++ port (`ENET_VERSION` in `enet.h`).
pub const ENET_VERSION: u32 = 0x010312;
/// M.A.X. network protocol version (`docs/protocol.md` §7).
pub const PROTOCOL_VERSION: u16 = 0x0004;
/// Default `GAME_VERSION` of the M.A.X. Port 0.8.0 builds
/// (`(major<<16)|(minor<<8)|patch`). The value must match the server build;
/// production code should read it from configuration.
pub const DEFAULT_GAME_VERSION: u32 = 0x0000_0800;
/// TP 00 packet type.
pub const TP_HELLO: u8 = 0x00;
/// TP 03 packet type.
pub const TP_REJECT: u8 = 0x03;
/// Game data file names looked up by [`DataHash::from_data_dir`].
pub const MAX_RES_FILE: &str = "MAX.RES";
/// Patches file name looked up by [`DataHash::from_data_dir`].
pub const PATCHES_RES_FILE: &str = "PATCHES.RES";

/// Two optional SHA-256 digests whose hex concatenation is the `data_hash`
/// field. `None` contributes **nothing** to the wire value, mirroring the
/// C++ `ResourceManager_Sha256File`: a missing file yields an empty string,
/// not a hash of empty bytes (`if (!fp) return {};`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataHash {
    /// `SHA256(MAX.RES)`, or `None` when the file is absent.
    pub max_res: Option<[u8; 32]>,
    /// `SHA256(PATCHES.RES)`, or `None` when the file is absent.
    pub patches_res: Option<[u8; 32]>,
}

impl DataHash {
    /// Hash of the two input buffers (always present).
    pub fn from_bytes(max_res: &[u8], patches_res: &[u8]) -> Self {
        Self {
            max_res: Some(Sha256::digest(max_res).into()),
            patches_res: Some(Sha256::digest(patches_res).into()),
        }
    }

    /// Wraps already-computed digests.
    pub const fn from_digests(max_res: [u8; 32], patches_res: [u8; 32]) -> Self {
        Self {
            max_res: Some(max_res),
            patches_res: Some(patches_res),
        }
    }

    /// Hashes the two data files on disk. A missing file contributes
    /// **nothing** to the wire value, mirroring `ResourceManager_Sha256File`
    /// in the C++ server (`if (!fp) return {};`): the wire hash of a missing
    /// file is the zero-length string, not a hash of empty bytes.
    pub fn from_files(
        max_res: impl AsRef<Path>,
        patches_res: impl AsRef<Path>,
    ) -> io::Result<Self> {
        fn read_or_none(path: &Path) -> io::Result<Option<Vec<u8>>> {
            match std::fs::read(path) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(io::Error::new(
                    error.kind(),
                    format!("cannot read {}: {error}", path.display()),
                )),
            }
        }
        let max_res = read_or_none(max_res.as_ref())?;
        let patches_res = read_or_none(patches_res.as_ref())?;
        Ok(Self {
            max_res: max_res.as_deref().map(|bytes| Sha256::digest(bytes).into()),
            patches_res: patches_res
                .as_deref()
                .map(|bytes| Sha256::digest(bytes).into()),
        })
    }

    /// Looks up `MAX.RES` and `PATCHES.RES` in `dir` (case-insensitive
    /// fallback for lowercase file names); missing files contribute nothing.
    pub fn from_data_dir(dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir = dir.as_ref();
        let max_res = find_file_optional(dir, MAX_RES_FILE);
        let patches_res = find_file_optional(dir, PATCHES_RES_FILE);
        match (max_res, patches_res) {
            (Some(max_res), Some(patches_res)) => Self::from_files(max_res, patches_res),
            (Some(max_res), None) => Self::from_files(max_res, dir.join(PATCHES_RES_FILE)),
            (None, Some(patches_res)) => Self::from_files(dir.join(MAX_RES_FILE), patches_res),
            (None, None) => Ok(Self {
                max_res: None,
                patches_res: None,
            }),
        }
    }

    /// The wire value: `hex(SHA256(MAX.RES)) + hex(SHA256(PATCHES.RES))`,
    /// with absent files contributing nothing (64, 128 or 0 hex chars).
    pub fn to_hex(self) -> String {
        let mut out = String::with_capacity(128);
        for digest in [self.max_res, self.patches_res].into_iter().flatten() {
            for byte in digest {
                out.push(hex_digit(byte >> 4));
                out.push(hex_digit(byte & 0x0F));
            }
        }
        out
    }

    /// Parses the wire value back into digests: 128 hex chars (both files),
    /// 64 (MAX.RES only), or empty (neither).
    pub fn from_hex(value: &str) -> Result<Self, HandshakeError> {
        let bytes = value.as_bytes();
        let parse = |chunk: &[u8]| -> Result<[u8; 32], HandshakeError> {
            let mut digest = [0u8; 32];
            for (index, pair) in chunk.chunks_exact(2).enumerate() {
                let high = hex_value(pair[0])
                    .ok_or_else(|| HandshakeError::InvalidDataHash("non-hex digit".into()))?;
                let low = hex_value(pair[1])
                    .ok_or_else(|| HandshakeError::InvalidDataHash("non-hex digit".into()))?;
                digest[index] = (high << 4) | low;
            }
            Ok(digest)
        };
        match bytes.len() {
            0 => Ok(Self {
                max_res: None,
                patches_res: None,
            }),
            64 => Ok(Self {
                max_res: Some(parse(&bytes[..64])?),
                patches_res: None,
            }),
            128 => Ok(Self {
                max_res: Some(parse(&bytes[..64])?),
                patches_res: Some(parse(&bytes[64..])?),
            }),
            other => Err(HandshakeError::InvalidDataHash(format!(
                "expected 0, 64 or 128 hex characters, got {other}"
            ))),
        }
    }
}

fn find_file_optional(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.is_file() {
        return Some(exact);
    }
    let lower = dir.join(name.to_ascii_lowercase());
    if lower.is_file() {
        return Some(lower);
    }
    None
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => unreachable!("nibble out of range"),
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Reject reason of `TP 03` (`docs/protocol.md` §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// `1` — game/enet/protocol version mismatch.
    Version,
    /// `2` — session ticket mismatch.
    SessionTicket,
    /// `3` — game data hash mismatch.
    GameData,
    /// A reason byte this client does not know.
    Unknown(u8),
}

impl RejectReason {
    /// Wire value.
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Version => 1,
            Self::SessionTicket => 2,
            Self::GameData => 3,
            Self::Unknown(value) => value,
        }
    }

    /// Diagnostic text written by the C++ port.
    pub const fn text(self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::SessionTicket => "session ticket",
            Self::GameData => "game data",
            Self::Unknown(_) => "unknown",
        }
    }

    /// Maps a wire reason byte.
    pub const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Version,
            2 => Self::SessionTicket,
            3 => Self::GameData,
            other => Self::Unknown(other),
        }
    }
}

/// `TP 00` handshake payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    /// `GAME_VERSION` of the sender.
    pub game_version: u32,
    /// `ENET_VERSION` of the sender (always [`ENET_VERSION`]).
    pub enet_version: u32,
    /// M.A.X. network protocol version ([`PROTOCOL_VERSION`]).
    pub protocol_version: u16,
    /// Session ticket; empty from the server (and for open sessions).
    pub ticket: String,
    /// `SHA256(MAX.RES) ‖ SHA256(PATCHES.RES)` as 128 hex chars.
    pub data_hash: String,
}

impl Handshake {
    /// Builds a client handshake from a ticket and data hash.
    pub fn new(game_version: u32, ticket: impl Into<String>, data_hash: DataHash) -> Self {
        Self {
            game_version,
            enet_version: ENET_VERSION,
            protocol_version: PROTOCOL_VERSION,
            ticket: ticket.into(),
            data_hash: data_hash.to_hex(),
        }
    }

    /// Encodes `TP 00` including the type byte.
    pub fn encode(&self) -> Result<Vec<u8>, HandshakeError> {
        let mut w = Writer::with_capacity(64);
        w.write_u8(TP_HELLO);
        w.write_u32(self.game_version);
        w.write_u32(self.enet_version);
        w.write_u16(self.protocol_version);
        w.write_string(&self.ticket)?;
        w.write_string(&self.data_hash)?;
        Ok(w.finish())
    }

    /// Decodes `TP 00` (the type byte must be `0x00`).
    pub fn decode(bytes: &[u8]) -> Result<Self, HandshakeError> {
        let mut r = Reader::new(bytes);
        let type_byte = r.read_u8()?;
        if type_byte != TP_HELLO {
            return Err(HandshakeError::UnexpectedType {
                expected: TP_HELLO,
                found: type_byte,
            });
        }
        let handshake = Self {
            game_version: r.read_u32()?,
            enet_version: r.read_u32()?,
            protocol_version: r.read_u16()?,
            ticket: r.read_string()?,
            data_hash: r.read_string()?,
        };
        r.finish()?;
        Ok(handshake)
    }

    /// Validates the peer's handshake against this one (both sides do this).
    ///
    /// The ticket is **not** checked here: only the server compares it with
    /// its configured `server_session_ticket` (see [`validate_ticket`]).
    pub fn validate_peer(&self, peer: &Handshake) -> Result<(), RejectReason> {
        if self.game_version != peer.game_version
            || self.enet_version != peer.enet_version
            || self.protocol_version != peer.protocol_version
        {
            return Err(RejectReason::Version);
        }
        if !self.data_hash.eq_ignore_ascii_case(&peer.data_hash) {
            return Err(RejectReason::GameData);
        }
        Ok(())
    }
}

/// Server-side ticket check: an empty configured ticket accepts every client.
pub fn validate_ticket(expected: &str, received: &str) -> Result<(), RejectReason> {
    if expected.is_empty() || expected == received {
        Ok(())
    } else {
        Err(RejectReason::SessionTicket)
    }
}

/// `TP 03` rejection payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reject {
    /// Local game version of the sender.
    pub game_version: u32,
    /// Local ENet version.
    pub enet_version: u32,
    /// Local protocol version.
    pub protocol_version: u16,
    /// Rejection reason.
    pub reason: RejectReason,
    /// Diagnostic text (`"version"`, `"session ticket"`, `"game data"`).
    pub reason_text: String,
}

impl Reject {
    /// Builds a rejection from a local handshake and a reason.
    pub fn new(local: &Handshake, reason: RejectReason) -> Self {
        Self {
            game_version: local.game_version,
            enet_version: local.enet_version,
            protocol_version: local.protocol_version,
            reason,
            reason_text: reason.text().to_string(),
        }
    }

    /// Encodes `TP 03` including the type byte.
    pub fn encode(&self) -> Result<Vec<u8>, HandshakeError> {
        let mut w = Writer::with_capacity(32);
        w.write_u8(TP_REJECT);
        w.write_u32(self.game_version);
        w.write_u32(self.enet_version);
        w.write_u16(self.protocol_version);
        w.write_u8(self.reason.as_u8());
        w.write_string(&self.reason_text)?;
        Ok(w.finish())
    }

    /// Decodes `TP 03` (the type byte must be `0x03`).
    pub fn decode(bytes: &[u8]) -> Result<Self, HandshakeError> {
        let mut r = Reader::new(bytes);
        let type_byte = r.read_u8()?;
        if type_byte != TP_REJECT {
            return Err(HandshakeError::UnexpectedType {
                expected: TP_REJECT,
                found: type_byte,
            });
        }
        let reject = Self {
            game_version: r.read_u32()?,
            enet_version: r.read_u32()?,
            protocol_version: r.read_u16()?,
            reason: RejectReason::from_u8(r.read_u8()?),
            reason_text: r.read_string()?,
        };
        r.finish()?;
        Ok(reject)
    }
}

/// Either transport packet (channel 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportPacket {
    /// `TP 00`.
    Hello(Handshake),
    /// `TP 03`.
    Reject(Reject),
}

impl TransportPacket {
    /// Decodes a channel-0 frame.
    pub fn decode(bytes: &[u8]) -> Result<Self, HandshakeError> {
        match bytes.first() {
            Some(&TP_HELLO) => Ok(Self::Hello(Handshake::decode(bytes)?)),
            Some(&TP_REJECT) => Ok(Self::Reject(Reject::decode(bytes)?)),
            Some(&other) => Err(HandshakeError::UnexpectedType {
                expected: TP_HELLO,
                found: other,
            }),
            None => Err(HandshakeError::Wire(WireError::UnexpectedEof {
                offset: 0,
                need: 1,
            })),
        }
    }

    /// Encodes this packet.
    pub fn encode(&self) -> Result<Vec<u8>, HandshakeError> {
        match self {
            Self::Hello(handshake) => handshake.encode(),
            Self::Reject(reject) => reject.encode(),
        }
    }
}

/// Errors of the channel-0 handshake.
#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    /// Malformed wire data.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// A frame of another type arrived.
    #[error("expected transport packet 0x{expected:02X}, got 0x{found:02X}")]
    UnexpectedType {
        /// Expected type byte.
        expected: u8,
        /// Actual type byte.
        found: u8,
    },
    /// The peer rejected the handshake with `TP 03`.
    #[error("handshake rejected: {reason_text} (reason {reason:?})")]
    Rejected {
        /// Reject reason.
        reason: RejectReason,
        /// Diagnostic text from the peer.
        reason_text: String,
    },
    /// A data_hash string is not 128 hex characters.
    #[error("invalid data hash: {0}")]
    InvalidDataHash(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_hash() -> DataHash {
        DataHash::from_bytes(b"abc", b"")
    }

    #[test]
    fn sha256_vectors_are_the_known_ones() {
        let hash = data_hash();
        assert_eq!(
            data_hash().to_hex(),
            concat!(
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            )
        );
        assert_eq!(DataHash::from_hex(&hash.to_hex()).unwrap(), hash);
    }

    #[test]
    fn data_hash_parses_and_rejects_bad_hex() {
        assert!(DataHash::from_hex("00").is_err());
        assert!(DataHash::from_hex(&"z".repeat(128)).is_err());
    }

    #[test]
    fn data_hash_from_files_and_dir() {
        let dir = std::env::temp_dir().join(format!(
            "max_net_data_hash_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MAX_RES_FILE), b"abc").unwrap();
        std::fs::write(dir.join(PATCHES_RES_FILE), b"").unwrap();
        assert_eq!(DataHash::from_data_dir(&dir).unwrap(), data_hash());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tp00_layout_is_exact() {
        let handshake = Handshake {
            game_version: DEFAULT_GAME_VERSION,
            enet_version: ENET_VERSION,
            protocol_version: PROTOCOL_VERSION,
            ticket: "abc123".into(),
            data_hash: "deadbeef".into(),
        };
        let bytes = handshake.encode().unwrap();
        assert_eq!(
            bytes,
            [
                0x00, // type
                0x00, 0x08, 0x00, 0x00, // game_version 0.8.0
                0x12, 0x03, 0x01, 0x00, // enet_version 1.3.18
                0x04, 0x00, // protocol_version
                0x06, 0x00, 0x00, 0x00, b'a', b'b', b'c', b'1', b'2', b'3', // ticket
                0x08, 0x00, 0x00, 0x00, b'd', b'e', b'a', b'd', b'b', b'e', b'e',
                b'f', // hash
            ]
        );
        assert_eq!(Handshake::decode(&bytes).unwrap(), handshake);
    }

    #[test]
    fn tp03_layout_and_reason_mapping() {
        let reject = Reject {
            game_version: DEFAULT_GAME_VERSION,
            enet_version: ENET_VERSION,
            protocol_version: PROTOCOL_VERSION,
            reason: RejectReason::SessionTicket,
            reason_text: "session ticket".into(),
        };
        let bytes = reject.encode().unwrap();
        assert_eq!(
            bytes[..12],
            [0x03, 0x00, 0x08, 0x00, 0x00, 0x12, 0x03, 0x01, 0x00, 0x04, 0x00, 0x02]
        );
        assert_eq!(&bytes[12..16], &[14, 0, 0, 0]);
        assert_eq!(&bytes[16..], b"session ticket");
        assert_eq!(Reject::decode(&bytes).unwrap(), reject);
        assert_eq!(RejectReason::from_u8(1), RejectReason::Version);
        assert_eq!(RejectReason::from_u8(2), RejectReason::SessionTicket);
        assert_eq!(RejectReason::from_u8(3), RejectReason::GameData);
        assert_eq!(RejectReason::from_u8(9), RejectReason::Unknown(9));
    }

    #[test]
    fn transport_packet_dispatches_by_type() {
        let handshake = Handshake::new(DEFAULT_GAME_VERSION, "", data_hash());
        let hello = TransportPacket::Hello(handshake.clone());
        assert_eq!(
            TransportPacket::decode(&hello.encode().unwrap()).unwrap(),
            hello
        );
        let reject = Reject::new(&handshake, RejectReason::GameData);
        let packet = TransportPacket::Reject(reject);
        assert_eq!(
            TransportPacket::decode(&packet.encode().unwrap()).unwrap(),
            packet
        );
    }

    #[test]
    fn validation_maps_mismatches_to_reasons() {
        let local = Handshake::new(DEFAULT_GAME_VERSION, "t", data_hash());

        let mut peer = local.clone();
        assert_eq!(local.validate_peer(&peer), Ok(()));

        peer.game_version = 1;
        assert_eq!(local.validate_peer(&peer), Err(RejectReason::Version));

        let mut peer = local.clone();
        peer.enet_version = 0x010311;
        assert_eq!(local.validate_peer(&peer), Err(RejectReason::Version));

        let mut peer = local.clone();
        peer.protocol_version = 5;
        assert_eq!(local.validate_peer(&peer), Err(RejectReason::Version));

        let mut peer = local.clone();
        peer.data_hash = "00".repeat(64);
        assert_eq!(local.validate_peer(&peer), Err(RejectReason::GameData));

        // The ticket is server-side only.
        let mut peer = local.clone();
        peer.ticket = "different".into();
        assert_eq!(local.validate_peer(&peer), Ok(()));
        assert_eq!(validate_ticket("t", "t"), Ok(()));
        assert_eq!(
            validate_ticket("t", "other"),
            Err(RejectReason::SessionTicket)
        );
        assert_eq!(validate_ticket("", "anything"), Ok(()));
    }

    #[test]
    fn malformed_tp00_is_rejected() {
        let bytes = [0x00, 0x01];
        assert!(matches!(
            Handshake::decode(&bytes),
            Err(HandshakeError::Wire(WireError::UnexpectedEof { .. }))
        ));
        assert!(matches!(
            Handshake::decode(&[0x03, 0, 0, 0, 0]),
            Err(HandshakeError::UnexpectedType { .. })
        ));
    }
}
