//! max_assets — reading the resources of the original game.
//!
//! Formats (documented in `max-original-dasm/formats/`):
//! - RES/PATCHES — game data archives (header, index, records) —
//!   module [`res`];
//! - WRL — planet maps (minimap, tile-id, tiles, palette, pass table) —
//!   module [`wrl`];
//! - sprites — multi-frame RLE sprites `COMMTWR`…`WALDO` and shadows `S_*`
//!   from MAX.RES — module [`sprite`];
//! - interface images — `FRAMEPIC`/`PANELTOP`/`PANELBTM`/`MNUBTN*`
//!   (`ImageSimpleHeader`/`ImageBigHeader`) — module [`image`];
//! - mouse cursors — `Cursor_Descriptor` (simple image + animation frames) —
//!   module [`cursor`];
//! - resource markers on the map — `RAWMSK*`/`FUELMK*`/`GOLDMK*` — module
//!   [`markers`];
//! - sounds — RIFF/WAVE `*.SPW` (PCM16) next to `MAX.RES` — module [`wav`];
//! - FLC — Autodesk FLIC animations (`F_*` resources of MAX.RES — references to
//!   `*.FLC` files next to the archive) — module [`flic`];
//! - MVE — Interplay video (`INTROFLC` → `maxint.mve`): video formats `0x06`/`0x10`/`0x11`
//!   and the audio track (`ALLOC_AUDIO_BUFFERS`/`AUDIO_FRAME`) — module [`mve`];
//! - SCE/MPS — missions; FON — fonts; MSC — music, ordinary PCM16 WAV
//!   (module [`wav`], `D2` of the client);
//! - mission descriptors — `PATCHES.RES` `SC_*` (XOR-obfuscated JSON),
//!   the mission registry and mission-file resolution — module [`mission`].
//!
//! The crate parses bytes into structured data; it contains no render specifics.
//! Format errors are returned as values ([`ResError`], [`WrlError`],
//! [`SpriteError`], [`FlicError`]); there are no panics on corrupt data.

#![forbid(unsafe_code)]

pub mod cursor;
pub mod flic;
pub mod image;
pub mod markers;
pub mod mission;
pub mod mve;
pub mod res;
pub mod sprite;
pub mod wav;
pub mod wrl;

pub use cursor::{
    CursorDescriptor, CursorError, CursorSet, CURSOR_COUNT, CURSOR_HEADER, CURSOR_RESOURCES,
};
pub use flic::{
    Flic, FlicError, FlicFrame, FlicHeader, FlicPalette, FlicPlayer, FrameError,
    FLIC_CHUNK_HEADER_SIZE, FLIC_FRAME_HEADER_SIZE, FLIC_HEADER_SIZE, FLIC_PALETTE_SIZE, FLC_MAGIC,
    FLI_MAGIC, FRAME_MAGIC,
};
pub use image::{BigImage, ImageError, SimpleImage, BIG_HEADER, SIMPLE_HEADER};
pub use markers::{decode_marker, ResourceMarkers, BIG_MARKER_COUNT, SMALL_MARKER_COUNT};
pub use mission::{
    xor_decode, Media, MediaResource, MissionDescriptor, MissionRegistry, StoryBlock, TextBlock,
    DEFAULT_LANGUAGE, GENERIC_TABLE, MISSION_CATEGORY_COUNT, MISSION_RESOURCE_TAGS,
};
pub use mve::{
    Mve, MveAudioChunk, MveAudioFormat, MveDecoder, MveError, MveInfo, AUDIO_HEADER_SIZE,
    AUDIO_TRACKS, DEFAULT_AUDIO_FORMAT, DEFAULT_FRAME_DELAY_US, FILE_HEADER_SIZE,
    MAX_PENDING_AUDIO_CHUNKS, MVE_SIGNATURE, OPCODE_HEADER_SIZE,
    PALETTE_SIZE as MVE_PALETTE_SIZE, VIDEO_HEADER_SIZE,
};
pub use res::{
    parse_file_ref, ResArchive, ResEntry, ResError, ENTRY_SIZE, FIRST_DATA_OFFSET, HEADER_SIZE,
    RES_SIGNATURE, TAG_LEN,
};
pub use sprite::{
    decode, decode_shadow, decode_sprite, DecodedSprite, ShadowFrame, ShadowSheet, SpriteError,
    SpriteFrame, SpriteRow, SpriteRun, SpriteSheet, FRAME_HEADER_SIZE, MAX_FRAME_PIXELS,
    PALETTE_SIZE, ROW_DELIMITER,
};
pub use wav::{
    WavClip, WavError, WavSource, DECODE_CHUNK_BYTES, PCM_FMT_MIN_SIZE, RIFF_HEADER_SIZE,
    WAVE_FORMAT_PCM,
};
pub use wrl::{
    WrlError, WrlMap, WrlOffsets, DMO_SIGNATURE, MAX_CPP_LAYERS, PALETTE_BYTES, PALETTE_COLORS,
    PASS_TABLE_SURFACES, SIGNATURE_LEN, STANDARD_HEIGHT, STANDARD_WIDTH, TILE_PIXELS, TILE_SIZE,
    WRL_SIGNATURE,
};

/// Parses a RES archive from bytes and validates its structure.
///
/// Compatible wrapper over [`ResArchive::parse`] (there used to be a stub here).
/// Use [`ResArchive`] directly to access the records.
pub fn parse_res(data: &[u8]) -> anyhow::Result<()> {
    ResArchive::parse(data)?;

    Ok(())
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
