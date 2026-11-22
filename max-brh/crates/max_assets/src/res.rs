//! RES / PATCHES.RES — M.A.X. resource archives.
//!
//! The format is described in `max-original-dasm/formats/res.md`:
//!
//! ```text
//! +0x00  "RES0"                  — signature (4 bytes, no NUL)
//! +0x04  u32 LE table_offset     — offset of the record table
//! +0x08  u32 LE table_size       — table size in bytes (a multiple of 16)
//! +0x0C  resource data in sequence — without compression or alignment
//! ...    res_index[table_size/16]— 16-byte table records
//! ```
//!
//! Table record: `tag[8]` (C string: everything after the first `0x00` is garbage) +
//! `u32 LE data_offset` + `u32 LE data_size`.
//!
//! Peculiarities inherited from the reference loader (`formats/res.md` §7):
//! * the tag is read as a C string in a fixed 8-byte field and cut at the
//!   first NUL (`"ILOGO\0TM"` is `"ILOGO"`); a tag of exactly 8 bytes is valid;
//! * the table is read strictly `table_offset..table_offset + table_size`: after it
//!   PATCHES.RES has 16 bytes of garbage (a fragment of the 85th record, §1.4);
//! * with duplicate tags the first record wins (as in the original
//!   loader, §3);
//! * the payload is opaque: the container neither compresses nor parses it.
//!
//! The parser does not panic on corrupt data: structural errors are returned as
//! [`ResError`], and non-critical discrepancies (e.g. a record extending past
//! the start of the table) are collected in [`ResArchive::warnings`].

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// RES header signature (`"RES0"`), 4 bytes without NUL.
pub const RES_SIGNATURE: [u8; 4] = *b"RES0";
/// Header size (`sizeof(res_header)`).
pub const HEADER_SIZE: usize = 12;
/// Table record size (`sizeof(res_index)`).
pub const ENTRY_SIZE: usize = 16;
/// Length of the tag field in a record.
pub const TAG_LEN: usize = 8;
/// Offset of the first resource's data: right after the header (§1.1).
pub const FIRST_DATA_OFFSET: u32 = HEADER_SIZE as u32;

/// One resource table record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResEntry {
    /// Ordinal number of the record in the table (0-based).
    pub index: usize,
    /// Tag as a C string: 8 bytes of the field, cut at the first NUL.
    pub name: String,
    /// Offset of the data from the start of the file.
    pub offset: u32,
    /// Data size in bytes.
    pub size: u32,
}

impl ResEntry {
    /// End of the record data (`offset + size`) as `u64` — without overflow.
    #[inline]
    pub fn end(&self) -> u64 {
        u64::from(self.offset) + u64::from(self.size)
    }

    /// Empty record (`data_size == 0`).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Data size in bytes as `usize`.
    #[inline]
    pub fn len(&self) -> usize {
        self.size as usize
    }
}

/// RES archive parse/read error.
#[derive(Debug)]
pub enum ResError {
    /// Failed to read the file entirely.
    Io {
        /// The path that was being read.
        path: PathBuf,
        /// Reason from the OS.
        source: io::Error,
    },
    /// The file is shorter than the 12-byte header.
    TooShort {
        /// Actual file length.
        len: usize,
        /// Minimum required length.
        needed: usize,
    },
    /// The first 4 bytes are not `"RES0"`.
    BadSignature {
        /// Actual signature.
        found: [u8; 4],
    },
    /// The table starts before the end of the header.
    TableBeforeHeader {
        /// Table offset from the header.
        offset: u32,
    },
    /// The table extends beyond the end of the file.
    TableOutOfBounds {
        /// Table offset from the header.
        offset: u32,
        /// Table size from the header.
        size: u32,
        /// Actual file length.
        file_len: usize,
    },
    /// The record data extends beyond the end of the file.
    ResourceOutOfBounds {
        /// Ordinal number of the record.
        index: usize,
        /// Record tag.
        name: String,
        /// Data offset.
        offset: u32,
        /// Data size.
        size: u32,
        /// Actual file length.
        file_len: usize,
    },
    /// No resource with such a tag was found.
    NotFound {
        /// The tag being searched for.
        name: String,
    },
}

impl fmt::Display for ResError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "не удалось прочитать {}: {source}", path.display())
            }
            Self::TooShort { len, needed } => write!(
                f,
                "файл слишком короткий для заголовка RES: {len} байт (нужно минимум {needed})"
            ),
            Self::BadSignature { found } => write!(
                f,
                "некорректная сигнатура RES: ожидалось \"RES0\", получено {found:02X?}"
            ),
            Self::TableBeforeHeader { offset } => write!(
                f,
                "смещение таблицы 0x{offset:X} меньше конца заголовка (0x{HEADER_SIZE:X})"
            ),
            Self::TableOutOfBounds {
                offset,
                size,
                file_len,
            } => write!(
                f,
                "таблица выходит за конец файла: offset=0x{offset:X} size=0x{size:X}, файл {file_len} байт"
            ),
            Self::ResourceOutOfBounds {
                index,
                name,
                offset,
                size,
                file_len,
            } => write!(
                f,
                "запись {index} ({name:?}) выходит за конец файла: 0x{offset:X}+{size} > {file_len}"
            ),
            Self::NotFound { name } => write!(f, "ресурс {name:?} не найден в архиве"),
        }
    }
}

impl std::error::Error for ResError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Parsed RES archive.
///
/// The file bytes are stored whole, so [`ResArchive::read`] returns a slice
/// without copying, while [`ResArchive::extract_all`] returns its own `Vec<u8>`.
#[derive(Clone)]
pub struct ResArchive {
    data: Vec<u8>,
    table_offset: u32,
    table_size: u32,
    entries: Vec<ResEntry>,
    /// Tag -> index of the first record with that tag (duplicates do not overwrite).
    by_name: BTreeMap<String, usize>,
    warnings: Vec<String>,
}

impl ResArchive {
    /// Parses the archive from bytes (copies the slice into ownership).
    pub fn parse(data: &[u8]) -> Result<Self, ResError> {
        Self::from_vec(data.to_vec())
    }

    /// Parses the archive from an existing `Vec<u8>` (without copying the data).
    pub fn from_vec(data: Vec<u8>) -> Result<Self, ResError> {
        let file_len = data.len();

        if file_len < HEADER_SIZE {
            return Err(ResError::TooShort {
                len: file_len,
                needed: HEADER_SIZE,
            });
        }

        let mut signature = [0u8; 4];
        signature.copy_from_slice(&data[0..4]);

        if signature != RES_SIGNATURE {
            return Err(ResError::BadSignature { found: signature });
        }

        let table_offset = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let table_size = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

        if table_offset < FIRST_DATA_OFFSET {
            return Err(ResError::TableBeforeHeader {
                offset: table_offset,
            });
        }

        let table_end = u64::from(table_offset) + u64::from(table_size);

        if table_end > file_len as u64 {
            return Err(ResError::TableOutOfBounds {
                offset: table_offset,
                size: table_size,
                file_len,
            });
        }

        let mut warnings = Vec::new();

        if table_size % ENTRY_SIZE as u32 != 0 {
            warnings.push(format!(
                "размер таблицы {table_size} не кратен {ENTRY_SIZE} — лишние байты отброшены"
            ));
        }

        let count = table_size as usize / ENTRY_SIZE;
        let table_start = table_offset as usize;
        let mut entries = Vec::with_capacity(count);
        let mut by_name = BTreeMap::new();

        // The table is guaranteed to lie inside the file, so read exactly count records.
        for i in 0..count {
            let pos = table_start + i * ENTRY_SIZE;
            let tag_raw = &data[pos..pos + TAG_LEN];
            let name = decode_tag(tag_raw);
            let offset =
                u32::from_le_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]);
            let size = u32::from_le_bytes([
                data[pos + 12],
                data[pos + 13],
                data[pos + 14],
                data[pos + 15],
            ]);
            let entry = ResEntry {
                index: i,
                name: name.clone(),
                offset,
                size,
            };

            // Soft checks, like in the reference Python parser: real files
            // satisfy them, and a "strange" archive can still be inspected.
            let end = entry.end();
            if end > file_len as u64 {
                warnings.push(format!(
                    "запись {i} ({name:?}) выходит за конец файла: 0x{offset:X}+{size} > {file_len}"
                ));
            } else if end > u64::from(table_offset) {
                warnings.push(format!(
                    "запись {i} ({name:?}) заходит за начало таблицы: конец 0x{end:X} > 0x{table_offset:X}"
                ));
            }

            if !by_name.contains_key(&name) {
                by_name.insert(name, i);
            }

            entries.push(entry);
        }

        Ok(Self {
            data,
            table_offset,
            table_size,
            entries,
            by_name,
            warnings,
        })
    }

    /// Reads and parses the archive from a file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ResError> {
        let path = path.as_ref();
        let data = fs::read(path).map_err(|source| ResError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        Self::from_vec(data)
    }

    /// Raw bytes of the whole file.
    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// File length in bytes.
    #[inline]
    pub fn file_size(&self) -> usize {
        self.data.len()
    }

    /// Offset of the record table from the header.
    #[inline]
    pub fn table_offset(&self) -> u32 {
        self.table_offset
    }

    /// Size of the record table from the header (in bytes).
    #[inline]
    pub fn table_size(&self) -> u32 {
        self.table_size
    }

    /// All table records in file order (including duplicate tags).
    #[inline]
    pub fn list(&self) -> &[ResEntry] {
        &self.entries
    }

    /// Number of table records.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The table has no records.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The first record with the given tag (duplicates resolve as "first wins").
    #[inline]
    pub fn entry(&self, name: &str) -> Option<&ResEntry> {
        self.by_name.get(name).map(|&index| &self.entries[index])
    }

    /// File name from the body of a reference resource (`TANK1` → `"tank1.spw"`).
    ///
    /// MAX.RES contains 721 such references (`max-original-dasm/formats/spw.md`
    /// §1): the resource body is the ASCIIZ name of a file next to `MAX.RES`. For ordinary
    /// resources (sprites, maps) [`None`] is returned.
    pub fn file_reference(&self, name: &str) -> Option<&str> {
        let bytes = self.read(name).ok()?;

        parse_file_ref(bytes)
    }

    /// Resource bytes by tag.
    ///
    /// The tag is compared as a C string (ignoring garbage after the NUL), but case-
    /// sensitively, as in the reference parser.
    pub fn read(&self, name: &str) -> Result<&[u8], ResError> {
        let index = *self.by_name.get(name).ok_or_else(|| ResError::NotFound {
            name: name.to_owned(),
        })?;

        self.read_entry(index)
    }

    /// Unpacks all resources into memory: tag -> bytes.
    ///
    /// Duplicate tags are skipped (the first record wins, as in the
    /// original loader). The key order is sorted (`BTreeMap`).
    pub fn extract_all(&self) -> Result<BTreeMap<String, Vec<u8>>, ResError> {
        let mut out = BTreeMap::new();

        for entry in &self.entries {
            if out.contains_key(&entry.name) {
                continue;
            }

            out.insert(entry.name.clone(), self.read_entry(entry.index)?.to_vec());
        }

        Ok(out)
    }

    /// Non-critical discrepancies with the format (empty for reference files).
    #[inline]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn read_entry(&self, index: usize) -> Result<&[u8], ResError> {
        // `index` comes only from `entries`, so the record exists.
        let entry = &self.entries[index];
        let start = entry.offset as usize;
        let end = start
            .checked_add(entry.size as usize)
            .filter(|&end| end <= self.data.len());

        match end {
            Some(end) => Ok(&self.data[start..end]),
            None => Err(ResError::ResourceOutOfBounds {
                index: entry.index,
                name: entry.name.clone(),
                offset: entry.offset,
                size: entry.size,
                file_len: self.data.len(),
            }),
        }
    }
}

impl fmt::Debug for ResArchive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResArchive")
            .field("file_size", &self.data.len())
            .field("table_offset", &self.table_offset)
            .field("table_size", &self.table_size)
            .field("entries", &self.entries.len())
            .field("warnings", &self.warnings.len())
            .finish()
    }
}

/// Decodes the 8-byte tag field as a C string in Latin-1.
///
/// Bytes after the first NUL are garbage from the patch build utility (`"ILOGO\0TM"`).
/// Latin-1 was chosen for the same reason as in the reference Python parser: this way any byte
/// turns into a character losslessly and without UTF-8 errors.
fn decode_tag(raw: &[u8]) -> String {
    raw.iter()
        .take_while(|&&byte| byte != 0)
        .map(|&byte| byte as char)
        .collect()
}

/// File name from the body of a reference resource: ASCIIZ without a path.
///
/// Reference resources (`TANK1` → `"tank1.spw\0"`, `SNOW_1` → `"snow_1.w\0"`)
/// store an ordinary C string in the body. A slice is returned **without** the trailing NUL
/// and only if the name is safe to join with a directory: ASCII letters/digits,
/// `.`, `_`, `-`, without `/`, `\`, `:` and `..`. Everything else (a sprite, a map,
/// corrupt bytes) — [`None`], so that the caller does not mistake data for a file name.
pub fn parse_file_ref(bytes: &[u8]) -> Option<&str> {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    let name = &bytes[..end];

    if name.is_empty() {
        return None;
    }

    if !name
        .iter()
        .all(|&byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return None;
    }

    let name = std::str::from_utf8(name).ok()?;

    // `..` and absolute paths (a colon) are cut off by the check above, but
    // we take explicit precautions: the name is joined with the game directory.
    if name.contains("..") {
        return None;
    }

    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal valid archive from (tag, data) pairs.
    fn build_archive(items: &[(&str, &[u8])]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut table = Vec::new();
        let mut offset = FIRST_DATA_OFFSET;

        for (name, payload) in items {
            let mut tag = [0u8; TAG_LEN];
            let bytes = name.as_bytes();

            tag[..bytes.len().min(TAG_LEN)].copy_from_slice(&bytes[..bytes.len().min(TAG_LEN)]);
            table.extend_from_slice(&tag);
            table.extend_from_slice(&offset.to_le_bytes());
            table.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            offset += payload.len() as u32;
            data.extend_from_slice(payload);
        }

        let mut out = Vec::new();
        out.extend_from_slice(&RES_SIGNATURE);
        out.extend_from_slice(&(FIRST_DATA_OFFSET + data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(table.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out.extend_from_slice(&table);
        out
    }

    #[test]
    fn duplicate_tags_keep_the_first_entry() {
        let bytes = build_archive(&[("DUP", b"first"), ("DUP", b"second"), ("END", b"z")]);
        let archive = ResArchive::parse(&bytes).unwrap();

        assert_eq!(archive.len(), 3);
        assert_eq!(archive.read("DUP").unwrap(), b"first");
        assert_eq!(archive.read("END").unwrap(), b"z");

        let all = archive.extract_all().unwrap();

        assert_eq!(all.len(), 2);
        assert_eq!(all["DUP"], b"first");
        assert_eq!(all["END"], b"z");
    }

    #[test]
    fn eight_byte_and_nul_padded_tags_decode_as_c_strings() {
        let bytes = build_archive(&[("ABCDEFGH", b"x")]);
        // Second archive: a tag with a NUL and garbage after it.
        let mut with_garbage = build_archive(&[("ILOGO", b"y")]);
        let table_start = u32::from_le_bytes([
            with_garbage[4],
            with_garbage[5],
            with_garbage[6],
            with_garbage[7],
        ]) as usize;

        with_garbage[table_start + 5] = 0;
        with_garbage[table_start + 6] = b'T';
        with_garbage[table_start + 7] = b'M';

        assert_eq!(
            ResArchive::parse(&bytes).unwrap().read("ABCDEFGH").unwrap(),
            b"x"
        );
        assert_eq!(
            ResArchive::parse(&with_garbage)
                .unwrap()
                .read("ILOGO")
                .unwrap(),
            b"y"
        );
    }

    #[test]
    fn non_multiple_table_size_warns_but_parses() {
        let mut bytes = build_archive(&[("A", b"1")]);
        let table_size = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);

        // Append 3 "extra" bytes to the table: count = size / 16.
        bytes[8..12].copy_from_slice(&(table_size + 3).to_le_bytes());
        bytes.extend_from_slice(&[0u8; 3]);

        let archive = ResArchive::parse(&bytes).unwrap();

        assert_eq!(archive.len(), 1);
        assert_eq!(archive.warnings().len(), 1);
    }

    #[test]
    fn structural_errors_do_not_panic() {
        assert!(matches!(
            ResArchive::parse(b""),
            Err(ResError::TooShort { .. })
        ));
        assert!(matches!(
            ResArchive::parse(b"RES0\x00\x00\x00"),
            Err(ResError::TooShort { .. })
        ));
        assert!(matches!(
            ResArchive::parse(b"NOPE\x0C\x00\x00\x00\x00\x00\x00\x00"),
            Err(ResError::BadSignature { .. })
        ));

        // The table is beyond the end of the file.
        let mut bytes = build_archive(&[("A", b"1")]);
        bytes[4..8].copy_from_slice(&0xFFFF_0000u32.to_le_bytes());
        assert!(matches!(
            ResArchive::parse(&bytes),
            Err(ResError::TableOutOfBounds { .. })
        ));
    }

    #[test]
    fn out_of_bounds_entry_is_a_warning_and_read_error() {
        let mut bytes = build_archive(&[("A", b"1")]);
        let table_start = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;

        // data_size = u32::MAX for the single record.
        bytes[table_start + 12..table_start + 16].copy_from_slice(&u32::MAX.to_le_bytes());

        let archive = ResArchive::parse(&bytes).unwrap();

        assert_eq!(archive.warnings().len(), 1);
        assert!(matches!(
            archive.read("A"),
            Err(ResError::ResourceOutOfBounds { .. })
        ));
        assert!(archive.extract_all().is_err());
    }

    #[test]
    fn file_references_are_ascii_names_without_paths() {
        let bytes = build_archive(&[
            ("TANK1", b"tank1.spw\0"),
            ("SNOW_1", b"snow_1.w"),
            ("NO_NUL", b"menu38.spw"),
            ("BINARY", &[0x18, 0x00, 0x62, 0x00]),
            ("TRAVERSAL", b"../secret\0"),
            ("ABSOLUTE", b"C:\\game\\x.spw\0"),
            ("EMPTY", b"\0"),
        ]);
        let archive = ResArchive::parse(&bytes).unwrap();

        assert_eq!(archive.file_reference("TANK1"), Some("tank1.spw"));
        assert_eq!(archive.file_reference("SNOW_1"), Some("snow_1.w"));
        assert_eq!(
            archive.file_reference("NO_NUL"),
            Some("menu38.spw"),
            "NUL в конце не обязателен"
        );
        assert_eq!(archive.file_reference("BINARY"), None);
        assert_eq!(archive.file_reference("TRAVERSAL"), None);
        assert_eq!(archive.file_reference("ABSOLUTE"), None);
        assert_eq!(archive.file_reference("EMPTY"), None);
        assert_eq!(
            archive.file_reference("MISSING"),
            None,
            "нет тега — нет ссылки"
        );

        assert_eq!(parse_file_ref(b"tank1.spw\0"), Some("tank1.spw"));
        assert_eq!(parse_file_ref(b""), None);
        assert_eq!(parse_file_ref(b"snow_1.w\0garbage"), Some("snow_1.w"));
    }

    /// Real `MAX.RES`: hundreds of ASCIIZ references to `*.spw` and other files.
    ///
    /// The check catches a discrepancy with the format (`spw.md` §1: 607 references to
    /// `.spw`, 721 in total). If the game data is absent, the test is skipped.
    #[test]
    fn real_max_res_exposes_ascii_file_references() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original/MAX.RES");

        if !path.is_file() {
            return;
        }

        let archive = ResArchive::open(&path).expect("MAX.RES");
        assert_eq!(archive.file_reference("TANK1"), Some("tank1.spw"));
        assert_eq!(archive.file_reference("KBUY0"), Some("kbuy0.spw"));
        assert_eq!(
            archive.file_reference("TANK"),
            None,
            "спрайт TANK — не ссылка на файл"
        );

        let mut spw = 0usize;
        let mut other = 0usize;

        for entry in archive.list() {
            // The references are tiny; sprites/maps are discarded before reading.
            if entry.size == 0 || entry.size > 64 {
                continue;
            }
            if let Some(name) = archive.file_reference(&entry.name) {
                if name.ends_with(".spw") {
                    spw += 1;
                } else {
                    other += 1;
                }
            }
        }

        assert!(spw >= 600, "ожидались сотни ссылок .spw, найдено {spw}");
        assert!(
            other >= 20,
            "ссылки .wrl/.flc/.msc/.mve тоже читаются: {other}"
        );
    }
}
