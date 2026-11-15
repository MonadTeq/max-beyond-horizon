//! RES / PATCHES.RES — архивы ресурсов M.A.X.
//!
//! Формат описан в `max-original-dasm/formats/res.md`:
//!
//! ```text
//! +0x00  "RES0"                  — сигнатура (4 байта, без NUL)
//! +0x04  u32 LE table_offset     — смещение таблицы записей
//! +0x08  u32 LE table_size       — размер таблицы в байтах (кратен 16)
//! +0x0C  данные ресурсов подряд  — без сжатия и выравнивания
//! ...    res_index[table_size/16]— 16-байтные записи таблицы
//! ```
//!
//! Запись таблицы: `tag[8]` (C-строка: всё после первого `0x00` — мусор) +
//! `u32 LE data_offset` + `u32 LE data_size`.
//!
//! Особенности, унаследованные от эталонного загрузчика (`formats/res.md` §7):
//! * тег читается как C-строка в фиксированном 8-байтном поле и обрезается по
//!   первому NUL (`"ILOGO\0TM"` — это `"ILOGO"`); тег ровно из 8 байт допустим;
//! * таблица читается строго `table_offset..table_offset + table_size`: за ней
//!   в PATCHES.RES лежат 16 байт мусора (обрывок 85-й записи, §1.4);
//! * при дубликатах тега выигрывает первая запись (как в оригинальном
//!   загрузчике, §3);
//! * полезная нагрузка непрозрачна: контейнер её не сжимает и не разбирает.
//!
//! Парсер не паникует на битых данных: структурные ошибки возвращаются как
//! [`ResError`], а некритичные расхождения (например, запись, заходящая за
//! начало таблицы) собираются в [`ResArchive::warnings`].

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Сигнатура заголовка RES (`"RES0"`), 4 байта без NUL.
pub const RES_SIGNATURE: [u8; 4] = *b"RES0";
/// Размер заголовка (`sizeof(res_header)`).
pub const HEADER_SIZE: usize = 12;
/// Размер записи таблицы (`sizeof(res_index)`).
pub const ENTRY_SIZE: usize = 16;
/// Длина поля тега в записи.
pub const TAG_LEN: usize = 8;
/// Смещение данных первого ресурса: сразу после заголовка (§1.1).
pub const FIRST_DATA_OFFSET: u32 = HEADER_SIZE as u32;

/// Одна запись таблицы ресурсов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResEntry {
    /// Порядковый номер записи в таблице (0-based).
    pub index: usize,
    /// Тег как C-строка: 8 байт поля, обрезанные по первому NUL.
    pub name: String,
    /// Смещение данных от начала файла.
    pub offset: u32,
    /// Размер данных в байтах.
    pub size: u32,
}

impl ResEntry {
    /// Конец данных записи (`offset + size`) как `u64` — без переполнения.
    #[inline]
    pub fn end(&self) -> u64 {
        u64::from(self.offset) + u64::from(self.size)
    }

    /// Пустая запись (`data_size == 0`).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Размер данных в байтах как `usize`.
    #[inline]
    pub fn len(&self) -> usize {
        self.size as usize
    }
}

/// Ошибка разбора/чтения RES-архива.
#[derive(Debug)]
pub enum ResError {
    /// Не удалось прочитать файл целиком.
    Io {
        /// Путь, который читался.
        path: PathBuf,
        /// Причина от ОС.
        source: io::Error,
    },
    /// Файл короче 12-байтного заголовка.
    TooShort {
        /// Фактическая длина файла.
        len: usize,
        /// Минимально необходимая длина.
        needed: usize,
    },
    /// Первые 4 байта — не `"RES0"`.
    BadSignature {
        /// Фактическая сигнатура.
        found: [u8; 4],
    },
    /// Таблица начинается раньше конца заголовка.
    TableBeforeHeader {
        /// Смещение таблицы из заголовка.
        offset: u32,
    },
    /// Таблица выходит за конец файла.
    TableOutOfBounds {
        /// Смещение таблицы из заголовка.
        offset: u32,
        /// Размер таблицы из заголовка.
        size: u32,
        /// Фактическая длина файла.
        file_len: usize,
    },
    /// Данные записи выходят за конец файла.
    ResourceOutOfBounds {
        /// Порядковый номер записи.
        index: usize,
        /// Тег записи.
        name: String,
        /// Смещение данных.
        offset: u32,
        /// Размер данных.
        size: u32,
        /// Фактическая длина файла.
        file_len: usize,
    },
    /// Ресурс с таким тегом не найден.
    NotFound {
        /// Искомый тег.
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

/// Разобранный RES-архив.
///
/// Байты файла хранятся целиком, поэтому [`ResArchive::read`] возвращает срез
/// без копирования, а [`ResArchive::extract_all`] — уже собственные `Vec<u8>`.
#[derive(Clone)]
pub struct ResArchive {
    data: Vec<u8>,
    table_offset: u32,
    table_size: u32,
    entries: Vec<ResEntry>,
    /// Тег -> индекс первой записи с этим тегом (дубликаты не перезаписывают).
    by_name: BTreeMap<String, usize>,
    warnings: Vec<String>,
}

impl ResArchive {
    /// Разбирает архив из байтов (копирует срез во владение).
    pub fn parse(data: &[u8]) -> Result<Self, ResError> {
        Self::from_vec(data.to_vec())
    }

    /// Разбирает архив из уже имеющегося `Vec<u8>` (без копирования данных).
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

        // Таблица гарантированно лежит внутри файла, читаем ровно count записей.
        for i in 0..count {
            let pos = table_start + i * ENTRY_SIZE;
            let tag_raw = &data[pos..pos + TAG_LEN];
            let name = decode_tag(tag_raw);
            let offset = u32::from_le_bytes([
                data[pos + 8],
                data[pos + 9],
                data[pos + 10],
                data[pos + 11],
            ]);
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

            // Мягкие проверки, как в эталонном Python-парсере: реальные файлы
            // им удовлетворяют, а «странный» архив всё равно можно осмотреть.
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

    /// Читает и разбирает архив из файла.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ResError> {
        let path = path.as_ref();
        let data = fs::read(path).map_err(|source| ResError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        Self::from_vec(data)
    }

    /// Сырые байты файла целиком.
    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Длина файла в байтах.
    #[inline]
    pub fn file_size(&self) -> usize {
        self.data.len()
    }

    /// Смещение таблицы записей из заголовка.
    #[inline]
    pub fn table_offset(&self) -> u32 {
        self.table_offset
    }

    /// Размер таблицы записей из заголовка (в байтах).
    #[inline]
    pub fn table_size(&self) -> u32 {
        self.table_size
    }

    /// Все записи таблицы в файловом порядке (включая дубликаты тегов).
    #[inline]
    pub fn list(&self) -> &[ResEntry] {
        &self.entries
    }

    /// Количество записей таблицы.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// В таблице нет записей.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Первая запись с данным тегом (дубликаты разрешаются «первый выигрывает»).
    #[inline]
    pub fn entry(&self, name: &str) -> Option<&ResEntry> {
        self.by_name.get(name).map(|&index| &self.entries[index])
    }

    /// Имя файла из тела ресурса-ссылки (`TANK1` → `"tank1.spw"`).
    ///
    /// В MAX.RES лежат 721 такая ссылка (`max-original-dasm/formats/spw.md`
    /// §1): тело ресурса — ASCIIZ-имя файла рядом с `MAX.RES`. Для обычных
    /// ресурсов (спрайты, карты) возвращается [`None`].
    pub fn file_reference(&self, name: &str) -> Option<&str> {
        let bytes = self.read(name).ok()?;

        parse_file_ref(bytes)
    }

    /// Байты ресурса по тегу.
    ///
    /// Тег сравнивается как C-строка (без учёта мусора после NUL), но с учётом
    /// регистра — как в эталонном парсере.
    pub fn read(&self, name: &str) -> Result<&[u8], ResError> {
        let index = *self
            .by_name
            .get(name)
            .ok_or_else(|| ResError::NotFound {
                name: name.to_owned(),
            })?;

        self.read_entry(index)
    }

    /// Распаковывает все ресурсы в память: тег -> байты.
    ///
    /// Дубликаты тегов пропускаются (первая запись выигрывает, как в
    /// оригинальном загрузчике). Порядок ключей — отсортированный (`BTreeMap`).
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

    /// Некритичные расхождения с форматом (пусто для эталонных файлов).
    #[inline]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn read_entry(&self, index: usize) -> Result<&[u8], ResError> {
        // `index` приходит только из `entries`, поэтому запись существует.
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

/// Декодирует 8-байтное поле тега как C-строку в Latin-1.
///
/// Байты после первого NUL — мусор утилиты сборки патча (`"ILOGO\0TM"`).
/// Latin-1 выбран потому же, что и в эталонном Python-парсере: так любой байт
/// превращается в символ без потерь и без ошибок UTF-8.
fn decode_tag(raw: &[u8]) -> String {
    raw.iter()
        .take_while(|&&byte| byte != 0)
        .map(|&byte| byte as char)
        .collect()
}

/// Имя файла из тела ресурса-ссылки: ASCIIZ без пути.
///
/// Ресурсы-ссылки (`TANK1` → `"tank1.spw\0"`, `SNOW_1` → `"snow_1.w\0"`)
/// хранят в теле обычную C-строку. Возвращается срез **без** завершающего NUL
/// и только если имя безопасно для склейки с каталогом: ASCII-буквы/цифры,
/// `.`, `_`, `-`, без `/`, `\`, `:` и `..`. Всё остальное (спрайт, карта,
/// битые байты) — [`None`], чтобы вызывающий не принял данные за имя файла.
pub fn parse_file_ref(bytes: &[u8]) -> Option<&str> {
    let end = bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len());
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

    // `..` и абсолютные пути (двоеточие) отсекаются проверкой выше, но
    // страхуемся явно: имя склеивается с каталогом игры.
    if name.contains("..") {
        return None;
    }

    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Собирает минимальный валидный архив из пар (тег, данные).
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
        // Второй архив: тег с NUL и мусором после него.
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

        assert_eq!(ResArchive::parse(&bytes).unwrap().read("ABCDEFGH").unwrap(), b"x");
        assert_eq!(
            ResArchive::parse(&with_garbage).unwrap().read("ILOGO").unwrap(),
            b"y"
        );
    }

    #[test]
    fn non_multiple_table_size_warns_but_parses() {
        let mut bytes = build_archive(&[("A", b"1")]);
        let table_size = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);

        // Приписываем 3 «лишних» байта к таблице: count = size / 16.
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

        // Таблица за концом файла.
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

        // data_size = u32::MAX для единственной записи.
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
        assert_eq!(archive.file_reference("MISSING"), None, "нет тега — нет ссылки");

        assert_eq!(parse_file_ref(b"tank1.spw\0"), Some("tank1.spw"));
        assert_eq!(parse_file_ref(b""), None);
        assert_eq!(parse_file_ref(b"snow_1.w\0garbage"), Some("snow_1.w"));
    }

    /// Реальный `MAX.RES`: сотни ASCIIZ-ссылок на `*.spw` и другие файлы.
    ///
    /// Проверка ловит расхождение с форматом (`spw.md` §1: 607 ссылок на
    /// `.spw`, 721 всего). Если игровых данных нет, тест пропускается.
    #[test]
    fn real_max_res_exposes_ascii_file_references() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../max-original/MAX.RES");

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
            // Ссылки крошечные; спрайты/карты отбрасываем до чтения.
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
        assert!(other >= 20, "ссылки .wrl/.flc/.msc/.mve тоже читаются: {other}");
    }
}
