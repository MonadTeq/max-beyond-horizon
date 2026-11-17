//! Декодер видео Interplay MVE (`INTROFLC` → `maxint.mve`).
//!
//! Формат — контейнер Interplay MVE (M.A.X. играет интро через
//! `movie_run(INTROFLC, …)` → `mvelib32.c`). Разбор:
//!
//! ```text
//! файл: 20-байтная подпись "Interplay MVE File\x1A\0",
//!       u16 magic 0x001A, u16 version 0x0100, u16 checksum
//!       (checksum == !version + 0x1234);
//! далее записи (chunk): u16 payload_size, u16 type
//!       type 0 init_audio, 1 audio_only, 2 init_video, 3 video,
//!       4 shutdown, 5 end;
//! внутри записи — поток опкодов: u16 size, u8 opcode, u8 version, data.
//! ```
//!
//! Поддержаны опкоды, нужные интро: `CREATE_TIMER` (2), `INIT_VIDEO_BUFFERS`
//! (5), `SET_PALETTE` (12), `SET_DECODING_MAP` (15), `VIDEO_DATA_11` (17),
//! аудио/неизвестные опкоды пропускаются. Видео — 8-битные индексированные
//! кадры, нарезанные на блоки 8×8; карта декодирования — по нибблу на блок
//! (младший ниббл — левый блок, старший — правый). Кодек блоков — точный порт
//! `nfPkDecomp` из `max-cpp-port/src/mvelib32.c` (0x0 — копия прошлого кадра,
//! 0x1 — позапрошлого, 0x2/0x3 — векторы движения, 0x4/0x5 — копия с малым
//! сдвигом, 0x7…0xF — шаблоны 2/4/16/64 цветов, 0x6 в ресурсах M.A.X. не
//! встречается и оставлен no-op, как в FFmpeg). Флаг `flags & 1` кадра
//! переключает двойной буфер (как `nfAdvance`), поэтому кадр N полностью
//! определяется кадрами N−1/N−2 — декодер детерминирован.
//!
//! Аудио не декодируется: звуковые опкоды читаются и пропускаются (как и
//! `sndAdd` без аудиодрайвера). Субтитры `INTROSUB` тоже не накладываются —
//! это отдельный слой `moviesubtitle.cpp`.
//!
//! Проверка: `tests/mve_assets.rs` сверяет метаданные реального `MAXINT.MVE`
//! (432×320, 4391 кадр, 15 FPS) и FNV-1a хеши кадров 0/50/100 с эталоном,
//! снятым C-обвязкой над `nfPkDecomp` оракула.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

/// Подпись файла (19 байт; в файле за ней идёт NUL).
pub const MVE_SIGNATURE: &[u8; 19] = b"Interplay MVE File\x1a";
/// Размер заголовка файла: подпись + magic + version + checksum.
pub const FILE_HEADER_SIZE: usize = 26;
/// Размер заголовка опкода: `u16 size`, `u8 opcode`, `u8 version`.
pub const OPCODE_HEADER_SIZE: usize = 4;
/// Заголовок видео-опкода `VIDEO_DATA_11`: 7 × `u16` (unused×2, x, y, w, h, flags).
pub const VIDEO_HEADER_SIZE: usize = 14;
/// Сколько цветов в палитре.
pub const PALETTE_SIZE: usize = 256;
/// Задержка кадра по умолчанию (15 FPS), если `CREATE_TIMER` не встретился.
pub const DEFAULT_FRAME_DELAY_US: u32 = 66_667;
/// Блок кодека: 8×8 пикселей.
pub const BLOCK_SIZE: usize = 8;

/// Тип записи контейнера.
const CHUNK_INIT_AUDIO: u16 = 0;
const CHUNK_AUDIO_ONLY: u16 = 1;
const CHUNK_INIT_VIDEO: u16 = 2;
const CHUNK_VIDEO: u16 = 3;
const CHUNK_SHUTDOWN: u16 = 4;
const CHUNK_END: u16 = 5;

/// Опкоды.
const OP_END_OF_STREAM: u8 = 0x00;
const OP_END_OF_CHUNK: u8 = 0x01;
const OP_CREATE_TIMER: u8 = 0x02;
const OP_INIT_VIDEO_BUFFERS: u8 = 0x05;
const OP_VIDEO_DATA_06: u8 = 0x06;
const OP_SET_PALETTE: u8 = 0x0C;
const OP_SET_DECODING_MAP: u8 = 0x0F;
const OP_VIDEO_DATA_10: u8 = 0x10;
const OP_VIDEO_DATA_11: u8 = 0x11;

// ---------------------------------------------------------------------------
// Ошибки
// ---------------------------------------------------------------------------

/// Ошибка разбора/декодирования MVE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MveError {
    /// Файл короче заголовка.
    TooShort {
        /// Фактическая длина.
        len: usize,
        /// Минимально необходимая.
        needed: usize,
    },
    /// Нет подписи `Interplay MVE File`.
    BadSignature,
    /// Magic/version/checksum не совпали.
    BadHeader,
    /// Запись контейнера выходит за конец файла.
    TruncatedChunk {
        /// Смещение записи.
        offset: usize,
        /// Требуемый конец.
        needed: usize,
        /// Фактическая длина.
        len: usize,
    },
    /// Опкод выходит за конец записи.
    TruncatedOpcode {
        /// Смещение опкода.
        offset: usize,
        /// Требуемый конец.
        needed: usize,
        /// Фактическая длина.
        len: usize,
    },
    /// Первый видео-опкод встретился до карты декодирования.
    MissingDecodingMap {
        /// Индекс кадра.
        frame: usize,
    },
    /// Размеры/координаты кадра не помещаются в буфер.
    BadFrameRect {
        /// Индекс кадра.
        frame: usize,
        /// Смещение по x (в пикселях).
        x: u16,
        /// Смещение по y (в пикселях).
        y: u16,
        /// Ширина (в пикселях).
        width: u16,
        /// Высота (в пикселях).
        height: u16,
    },
    /// Карта декодирования короче `width*height/2` байт.
    BadDecodingMap {
        /// Индекс кадра.
        frame: usize,
        /// Минимально необходимая длина.
        needed: usize,
        /// Фактическая длина.
        len: usize,
    },
    /// Видео-опкод короче 14-байтного заголовка.
    TruncatedVideoHeader {
        /// Индекс кадра.
        frame: usize,
        /// Фактическая длина.
        len: usize,
    },
    /// Поток блока обрывается.
    TruncatedVideo {
        /// Индекс кадра.
        frame: usize,
    },
    /// Формат видео `0x06`/`0x10` не поддержан (в ресурсах M.A.X. — `0x11`).
    UnsupportedVideoFormat {
        /// Формат опкода.
        format: u8,
    },
    /// Неизвестный код блока (в ресурсах не встречается).
    UnsupportedBlock {
        /// Индекс кадра.
        frame: usize,
        /// Код блока.
        opcode: u8,
    },
    /// В файле нет видео (нет `INIT_VIDEO_BUFFERS`/`VIDEO_DATA`).
    NoVideo,
}

impl fmt::Display for MveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len, needed } => write!(
                f,
                "MVE слишком короткий: {len} байт (нужно минимум {needed})"
            ),
            Self::BadSignature => write!(f, "нет подписи Interplay MVE"),
            Self::BadHeader => write!(f, "magic/version/checksum MVE не совпали"),
            Self::TruncatedChunk {
                offset,
                needed,
                len,
            } => write!(
                f,
                "запись на 0x{offset:X} требует {needed} байт, файл {len}"
            ),
            Self::TruncatedOpcode {
                offset,
                needed,
                len,
            } => write!(
                f,
                "опкод на 0x{offset:X} требует {needed} байт, запись {len}"
            ),
            Self::MissingDecodingMap { frame } => {
                write!(f, "кадр {frame}: видео без карты декодирования")
            }
            Self::BadFrameRect {
                frame,
                x,
                y,
                width,
                height,
            } => write!(
                f,
                "кадр {frame}: прямоугольник ({x},{y}) {width}×{height} вне буфера"
            ),
            Self::BadDecodingMap { frame, needed, len } => write!(
                f,
                "кадр {frame}: карта декодирования {len} байт (нужно {needed})"
            ),
            Self::TruncatedVideoHeader { frame, len } => write!(
                f,
                "кадр {frame}: видео-опкод {len} байт (нужен заголовок 14)"
            ),
            Self::TruncatedVideo { frame } => {
                write!(f, "кадр {frame}: поток блоков обрывается")
            }
            Self::UnsupportedVideoFormat { format } => {
                write!(f, "формат видео 0x{format:02X} не поддержан (нужен 0x11)")
            }
            Self::UnsupportedBlock { frame, opcode } => {
                write!(f, "кадр {frame}: неизвестный код блока {opcode}")
            }
            Self::NoVideo => write!(f, "в MVE нет видео"),
        }
    }
}

impl std::error::Error for MveError {}

// ---------------------------------------------------------------------------
// Метаданные
// ---------------------------------------------------------------------------

/// Паспорт MVE-файла.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MveInfo {
    /// Ширина кадра в пикселях (`width_blocks * 8`).
    pub width: u16,
    /// Высота кадра в пикселях (`height_blocks * fqty * 8`).
    pub height: u16,
    /// Число видео-кадров (`VIDEO_DATA_11`).
    pub frame_count: usize,
    /// Длительность кадра в микросекундах (`rate * divider`).
    pub frame_delay_us: u32,
    /// Формат видео (в `maxint.mve` — `0x11`).
    pub video_format: u8,
}

impl MveInfo {
    /// Длительность кадра в секундах.
    pub fn frame_delay_seconds(&self) -> f32 {
        if self.frame_delay_us == 0 {
            DEFAULT_FRAME_DELAY_US as f32 / 1_000_000.0
        } else {
            self.frame_delay_us as f32 / 1_000_000.0
        }
    }

    /// Кадров в секунду (для логов/тестов).
    pub fn frames_per_second(&self) -> f32 {
        1.0 / self.frame_delay_seconds()
    }
}

// ---------------------------------------------------------------------------
// Разбор файла
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct PaletteUpdate {
    start: u16,
    count: u16,
    /// Абсолютное смещение 3·count байт 6-битной палитры.
    offset: usize,
}

#[derive(Debug, Clone, Copy)]
struct FrameHeader {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    flags: u16,
}

#[derive(Debug, Clone, Copy)]
struct FrameSpec {
    map_offset: usize,
    map_len: usize,
    video_offset: usize,
    video_len: usize,
    header: FrameHeader,
}

#[derive(Debug, Clone, Copy)]
enum Event {
    Palette(PaletteUpdate),
    Frame(FrameSpec),
}

#[derive(Debug)]
struct Inner {
    data: Arc<[u8]>,
    events: Vec<Event>,
    info: MveInfo,
}

/// Разобранный MVE-файл: метаданные + индексированные кадры.
///
/// Сам файл (`Arc<[u8]>`) разделяется между клонами и декодерами; кадры
/// декодируются по требованию [`MveDecoder`], поэтому 71 МБ `maxint.mve` не
/// превращаются в 600 МБ распакованных кадров.
#[derive(Debug, Clone)]
pub struct Mve {
    inner: Arc<Inner>,
}

impl Mve {
    /// Читает и разбирает MVE-файл.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MveError> {
        let bytes = std::fs::read(path.as_ref()).map_err(|_| MveError::TooShort {
            len: 0,
            needed: FILE_HEADER_SIZE,
        })?;

        Self::parse(bytes)
    }

    /// Разбирает MVE из байтов.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, MveError> {
        if bytes.len() < FILE_HEADER_SIZE {
            return Err(MveError::TooShort {
                len: bytes.len(),
                needed: FILE_HEADER_SIZE,
            });
        }

        if &bytes[..MVE_SIGNATURE.len()] != MVE_SIGNATURE {
            return Err(MveError::BadSignature);
        }

        let magic = u16::from_le_bytes([bytes[20], bytes[21]]);
        let version = u16::from_le_bytes([bytes[22], bytes[23]]);
        let checksum = u16::from_le_bytes([bytes[24], bytes[25]]);

        if magic != 0x001A || version != 0x0100 || checksum != expected_checksum(version) {
            return Err(MveError::BadHeader);
        }

        let data: Arc<[u8]> = Arc::from(bytes);
        let mut events = Vec::new();
        let mut width = 0u16;
        let mut height = 0u16;
        let mut frame_count = 0usize;
        let mut frame_delay_us = 0u32;
        let mut video_format = 0u8;
        let mut map: Option<(usize, usize)> = None;
        let mut offset = FILE_HEADER_SIZE;
        let mut end_of_stream = false;

        while offset + 4 <= data.len() {
            let size = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
            let kind = u16::from_le_bytes([data[offset + 2], data[offset + 3]]);
            let payload_start = offset + 4;
            let payload_end = payload_start
                .checked_add(size)
                .ok_or(MveError::TruncatedChunk {
                    offset,
                    needed: payload_start,
                    len: data.len(),
                })?;

            if size == 0 && kind == CHUNK_END {
                break;
            }

            if payload_end > data.len() {
                return Err(MveError::TruncatedChunk {
                    offset,
                    needed: payload_end,
                    len: data.len(),
                });
            }

            if kind == CHUNK_SHUTDOWN {
                break;
            }

            if !matches!(
                kind,
                CHUNK_INIT_AUDIO | CHUNK_AUDIO_ONLY | CHUNK_INIT_VIDEO | CHUNK_VIDEO
            ) {
                // Неизвестный тип записи: пропускаем целиком (как FFmpeg CHUNK_BAD).
                offset = payload_end;
                continue;
            }

            let mut pos = payload_start;

            while pos + OPCODE_HEADER_SIZE <= payload_end {
                let opcode_size = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                let opcode = data[pos + 2];
                let data_start = pos + OPCODE_HEADER_SIZE;
                let data_end =
                    data_start
                        .checked_add(opcode_size)
                        .ok_or(MveError::TruncatedOpcode {
                            offset: pos,
                            needed: data_start,
                            len: payload_end,
                        })?;

                if data_end > payload_end {
                    return Err(MveError::TruncatedOpcode {
                        offset: pos,
                        needed: data_end,
                        len: payload_end,
                    });
                }

                match opcode {
                    OP_END_OF_STREAM => {
                        end_of_stream = true;
                        break;
                    }
                    OP_END_OF_CHUNK => break,
                    OP_CREATE_TIMER => {
                        if opcode_size >= 6 {
                            let rate = u32::from_le_bytes([
                                data[data_start],
                                data[data_start + 1],
                                data[data_start + 2],
                                data[data_start + 3],
                            ]);
                            let divider =
                                u16::from_le_bytes([data[data_start + 4], data[data_start + 5]]);
                            frame_delay_us = rate.saturating_mul(u32::from(divider));
                        }
                    }
                    OP_INIT_VIDEO_BUFFERS => {
                        if opcode_size >= 6 {
                            let width_blocks =
                                u16::from_le_bytes([data[data_start], data[data_start + 1]]);
                            let height_blocks =
                                u16::from_le_bytes([data[data_start + 2], data[data_start + 3]]);
                            let fqty = if opcode_size >= 8 {
                                u16::from_le_bytes([data[data_start + 4], data[data_start + 5]])
                            } else {
                                1
                            };
                            let fqty = fqty.max(1);

                            width = width_blocks.saturating_mul(8);
                            height = height_blocks.saturating_mul(fqty).saturating_mul(8);
                        }
                    }
                    OP_SET_PALETTE => {
                        if opcode_size >= 4 {
                            let start =
                                u16::from_le_bytes([data[data_start], data[data_start + 1]]);
                            let count =
                                u16::from_le_bytes([data[data_start + 2], data[data_start + 3]]);
                            let available = (data_end - data_start - 4) / 3;

                            if available > 0 {
                                events.push(Event::Palette(PaletteUpdate {
                                    start,
                                    count: count.min(available as u16),
                                    offset: data_start + 4,
                                }));
                            }
                        }
                    }
                    OP_SET_DECODING_MAP => {
                        map = Some((data_start, opcode_size));
                    }
                    OP_VIDEO_DATA_11 | OP_VIDEO_DATA_10 | OP_VIDEO_DATA_06 => {
                        if opcode_size < VIDEO_HEADER_SIZE {
                            return Err(MveError::TruncatedVideoHeader {
                                frame: frame_count,
                                len: opcode_size,
                            });
                        }

                        if opcode != OP_VIDEO_DATA_11 {
                            video_format = opcode;
                            // Форматы 0x10/0x06 в ресурсах M.A.X. не встречаются.
                            // Запоминаем кадр, чтобы сообщить об ошибке при декодировании.
                            let (map_offset, map_len) = map.unwrap_or((data_start, 0));
                            events.push(Event::Frame(FrameSpec {
                                map_offset,
                                map_len,
                                video_offset: data_start,
                                video_len: opcode_size,
                                header: parse_frame_header(&data, data_start),
                            }));
                            frame_count += 1;
                            pos = data_end;
                            continue;
                        }

                        let Some((map_offset, map_len)) = map else {
                            return Err(MveError::MissingDecodingMap { frame: frame_count });
                        };

                        events.push(Event::Frame(FrameSpec {
                            map_offset,
                            map_len,
                            video_offset: data_start,
                            video_len: opcode_size,
                            header: parse_frame_header(&data, data_start),
                        }));
                        frame_count += 1;
                        video_format = OP_VIDEO_DATA_11;
                    }
                    _ => {}
                }

                pos = data_end;
            }

            if end_of_stream {
                break;
            }

            offset = payload_end;
        }

        if width == 0 || height == 0 || frame_count == 0 {
            return Err(MveError::NoVideo);
        }

        if frame_delay_us == 0 {
            frame_delay_us = DEFAULT_FRAME_DELAY_US;
        }

        Ok(Self {
            inner: Arc::new(Inner {
                data,
                events,
                info: MveInfo {
                    width,
                    height,
                    frame_count,
                    frame_delay_us,
                    video_format,
                },
            }),
        })
    }

    /// Метаданные файла.
    pub fn info(&self) -> MveInfo {
        self.inner.info
    }

    /// Создаёт декодер с нулевыми кадрами и начальной палитрой
    /// (`movie_init_palette`: всё чёрное, индекс 255 = 50/50/50).
    pub fn decoder(&self) -> MveDecoder {
        let width = usize::from(self.inner.info.width);
        let height = usize::from(self.inner.info.height);
        let mut palette = [[0u8; 3]; PALETTE_SIZE];

        palette[PALETTE_SIZE - 1] = [50, 50, 50];

        MveDecoder {
            inner: Arc::clone(&self.inner),
            event: 0,
            frame_index: 0,
            cur: vec![0u8; width * height],
            prv: vec![0u8; width * height],
            palette,
            finished: false,
        }
    }
}

fn expected_checksum(version: u16) -> u16 {
    (!version).wrapping_add(0x1234)
}

fn parse_frame_header(data: &[u8], offset: usize) -> FrameHeader {
    let word = |index: usize| -> u16 {
        u16::from_le_bytes([data[offset + index * 2], data[offset + index * 2 + 1]])
    };

    FrameHeader {
        x: word(2),
        y: word(3),
        width: word(4),
        height: word(5),
        flags: word(6),
    }
}

// ---------------------------------------------------------------------------
// Декодер кадров
// ---------------------------------------------------------------------------

/// Покадровый декодер MVE.
///
/// Кадры дельта-кодированы, поэтому их нужно декодировать последовательно от
/// нулевого. `next_frame()` возвращает индексированный кадр
/// (`width * height` байт) и продвигается вперёд; палитра — [`Self::palette`].
#[derive(Debug, Clone)]
pub struct MveDecoder {
    inner: Arc<Inner>,
    event: usize,
    frame_index: usize,
    /// Буфер, в который пишется текущий кадр.
    cur: Vec<u8>,
    /// Предыдущий показанный кадр (для копирования блоков).
    prv: Vec<u8>,
    palette: [[u8; 3]; PALETTE_SIZE],
    finished: bool,
}

impl MveDecoder {
    /// Метаданные.
    pub fn info(&self) -> MveInfo {
        self.inner.info
    }

    /// Текущая палитра (8-битные компоненты; обновляется опкодами `SET_PALETTE`).
    pub fn palette(&self) -> &[[u8; 3]; PALETTE_SIZE] {
        &self.palette
    }

    /// Сколько кадров уже декодировано.
    pub fn frame_index(&self) -> usize {
        self.frame_index
    }

    /// Декодер дошёл до конца файла.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Декодирует следующий кадр. `Ok(None)` — файл закончился.
    pub fn next_frame(&mut self) -> Result<Option<&[u8]>, MveError> {
        loop {
            if self.event >= self.inner.events.len() {
                self.finished = true;

                return Ok(None);
            }

            let event = self.inner.events[self.event];

            self.event += 1;

            match event {
                Event::Palette(update) => self.apply_palette(update),
                Event::Frame(frame) => {
                    let inner = Arc::clone(&self.inner);

                    self.decode_frame(&inner, frame)?;

                    return Ok(Some(&self.cur));
                }
            }
        }
    }

    fn apply_palette(&mut self, update: PaletteUpdate) {
        let data = &self.inner.data;

        for index in 0..usize::from(update.count) {
            let color = usize::from(update.start) + index;

            if color >= PALETTE_SIZE {
                break;
            }

            let offset = update.offset + index * 3;

            if offset + 2 >= data.len() {
                break;
            }

            // 6-битная VGA-палитра → 8-битные компоненты, как `movie_cb_set_palette`.
            self.palette[color] = [
                data[offset].saturating_mul(4),
                data[offset + 1].saturating_mul(4),
                data[offset + 2].saturating_mul(4),
            ];
        }
    }

    fn decode_frame(&mut self, inner: &Inner, frame: FrameSpec) -> Result<(), MveError> {
        let info = inner.info;
        let width = usize::from(info.width);
        let height = usize::from(info.height);
        let header = frame.header;

        if info.video_format != OP_VIDEO_DATA_11 {
            return Err(MveError::UnsupportedVideoFormat {
                format: info.video_format,
            });
        }

        if header.width == 0 || header.height == 0 {
            return Err(MveError::BadFrameRect {
                frame: self.frame_index,
                x: header.x,
                y: header.y,
                width: header.width,
                height: header.height,
            });
        }

        let frame_width = usize::from(header.width) * BLOCK_SIZE;
        let frame_height = usize::from(header.height) * BLOCK_SIZE;

        if usize::from(header.x) + frame_width > width
            || usize::from(header.y) + frame_height > height
        {
            return Err(MveError::BadFrameRect {
                frame: self.frame_index,
                x: header.x,
                y: header.y,
                width: header.width,
                height: header.height,
            });
        }

        let map_end = frame.map_offset + frame.map_len;
        let video_end = frame.video_offset + frame.video_len;

        if map_end > inner.data.len() || video_end > inner.data.len() {
            return Err(MveError::TruncatedVideo {
                frame: self.frame_index,
            });
        }

        let map = &inner.data[frame.map_offset..map_end];
        let video = &inner.data[frame.video_offset..video_end];

        if video.len() < VIDEO_HEADER_SIZE {
            return Err(MveError::TruncatedVideoHeader {
                frame: self.frame_index,
                len: video.len(),
            });
        }

        let needed = usize::from(header.width) * usize::from(header.height) / 2;

        if map.len() < needed {
            return Err(MveError::BadDecodingMap {
                frame: self.frame_index,
                needed,
                len: map.len(),
            });
        }

        if header.flags & 0x01 != 0 {
            std::mem::swap(&mut self.cur, &mut self.prv);
        }

        let mut cursor = VideoCursor::new(&video[VIDEO_HEADER_SIZE..], self.frame_index);
        let mut pattern = PatternScratch::new();
        let mut map_pos = 0usize;

        for block_y in 0..usize::from(header.height) {
            for pair in 0..usize::from(header.width) / 2 {
                let byte = map[map_pos];

                map_pos += 1;

                for (offset, opcode) in [(0usize, byte & 0x0F), (1usize, byte >> 4)] {
                    let block_x = pair * 2 + offset;

                    decode_block(
                        opcode,
                        self.frame_index,
                        block_x,
                        block_y,
                        &header,
                        width,
                        &mut self.cur,
                        &self.prv,
                        &mut cursor,
                        &mut pattern,
                    )?;
                }
            }
        }

        self.frame_index += 1;

        Ok(())
    }
}

/// Курсор по сжатому потоку блоков кадра.
struct VideoCursor<'a> {
    data: &'a [u8],
    pos: usize,
    frame: usize,
}

impl<'a> VideoCursor<'a> {
    fn new(data: &'a [u8], frame: usize) -> Self {
        Self {
            data,
            pos: 0,
            frame,
        }
    }

    fn byte(&mut self) -> Result<u8, MveError> {
        let value = *self
            .data
            .get(self.pos)
            .ok_or(MveError::TruncatedVideo { frame: self.frame })?;

        self.pos += 1;

        Ok(value)
    }

    fn block(&mut self) -> Result<[u8; 64], MveError> {
        let mut block = [0u8; 64];

        for pixel in &mut block {
            *pixel = self.byte()?;
        }

        Ok(block)
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_block(
    opcode: u8,
    frame: usize,
    block_x: usize,
    block_y: usize,
    header: &FrameHeader,
    stride: usize,
    cur: &mut [u8],
    prv: &[u8],
    cursor: &mut VideoCursor<'_>,
    pattern: &mut PatternScratch,
) -> Result<(), MveError> {
    let dst = block_offset(header, block_x, block_y, stride);

    // Шаблонные блоки 0x7…0xA портированы из `nfFillPattern*` оракула
    // (таблицы `nf_pk_lut_4/5/6`), иначе порядок пар в некоторых ветках
    // отличается от `nfPkDecomp` на пару пикселей.
    if (0x7..=0xA).contains(&opcode) {
        return decode_pattern_block(opcode, frame, dst, stride, cur, cursor, pattern);
    }

    match opcode {
        // Копия блока из предыдущего кадра.
        0x0 => {
            copy_block(cur, dst, prv, dst as isize, stride);
        }
        // Блок остаётся из позапрошлого кадра (двойной буфер уже содержит его).
        0x1 => {}
        // Вектор движения из позапрошлого кадра (положительный).
        0x2 => {
            let byte = cursor.byte()?;
            let (dx, dy) = motion_vector(byte);
            let src = dst as isize + dy * stride as isize + dx;

            copy_within_block(cur, dst, src, stride);
        }
        // Вектор движения из текущего кадра (отрицательный).
        0x3 => {
            let byte = cursor.byte()?;
            let (dx, dy) = motion_vector(byte);
            let src = dst as isize - dy * stride as isize - dx;

            copy_within_block(cur, dst, src, stride);
        }
        // Копия из предыдущего кадра со сдвигом −8..7 по обеим осям.
        0x4 => {
            let byte = cursor.byte()?;
            let dx = isize::from(byte & 0x0F) - 8;
            let dy = isize::from(byte >> 4) - 8;
            let src = dst as isize + dy * stride as isize + dx;

            copy_block(cur, dst, prv, src, stride);
        }
        // Копия из предыдущего кадра с расширенным диапазоном (i8, i8).
        0x5 => {
            let dx = cursor.byte()? as i8 as isize;
            let dy = cursor.byte()? as i8 as isize;
            let src = dst as isize + dy * stride as isize + dx;

            copy_block(cur, dst, prv, src, stride);
        }
        // В M.A.X. не встречается; FFmpeg тоже не трогает буфер.
        0x6 => {}
        0xB => {
            let block = cursor.block()?;

            write_block(cur, dst, stride, &block);
        }
        0xC => {
            let mut block = [0u8; 64];

            for y in (0..8).step_by(2) {
                for x in (0..8).step_by(2) {
                    let value = cursor.byte()?;

                    block[y * 8 + x] = value;
                    block[y * 8 + x + 1] = value;
                    block[(y + 1) * 8 + x] = value;
                    block[(y + 1) * 8 + x + 1] = value;
                }
            }

            write_block(cur, dst, stride, &block);
        }
        0xD => {
            let mut block = [0u8; 64];
            let mut colors = [0u8; 2];

            for y in 0..8 {
                if y % 4 == 0 {
                    colors[0] = cursor.byte()?;
                    colors[1] = cursor.byte()?;
                }

                for x in 0..4 {
                    block[y * 8 + x] = colors[0];
                    block[y * 8 + 4 + x] = colors[1];
                }
            }

            write_block(cur, dst, stride, &block);
        }
        0xE => {
            let value = cursor.byte()?;
            let block = [value; 64];

            write_block(cur, dst, stride, &block);
        }
        0xF => {
            let mut block = [0u8; 64];
            let sample = [cursor.byte()?, cursor.byte()?];

            for y in 0..8 {
                for x in (0..8).step_by(2) {
                    block[y * 8 + x] = sample[y & 1];
                    block[y * 8 + x + 1] = sample[1 - (y & 1)];
                }
            }

            write_block(cur, dst, stride, &block);
        }
        other => {
            return Err(MveError::UnsupportedBlock {
                frame,
                opcode: other,
            });
        }
    }

    Ok(())
}

const NF_PK_LUT_4: [u32; 16] = [
    0xC3C3C3C3, 0xC3C3C1C3, 0xC3C3C3C1, 0xC3C3C1C1, 0xC1C3C3C3, 0xC1C3C1C3, 0xC1C3C3C1, 0xC1C3C1C1,
    0xC3C1C3C3, 0xC3C1C1C3, 0xC3C1C3C1, 0xC3C1C1C1, 0xC1C1C3C3, 0xC1C1C1C3, 0xC1C1C3C1, 0xC1C1C1C1,
];

const NF_PK_LUT_5: [u32; 256] = [
    0xC3C3C3C3, 0xC3C3C2C3, 0xC3C3C1C3, 0xC3C3C5C3, 0xC3C3C3C2, 0xC3C3C2C2, 0xC3C3C1C2, 0xC3C3C5C2,
    0xC3C3C3C1, 0xC3C3C2C1, 0xC3C3C1C1, 0xC3C3C5C1, 0xC3C3C3C5, 0xC3C3C2C5, 0xC3C3C1C5, 0xC3C3C5C5,
    0xC2C3C3C3, 0xC2C3C2C3, 0xC2C3C1C3, 0xC2C3C5C3, 0xC2C3C3C2, 0xC2C3C2C2, 0xC2C3C1C2, 0xC2C3C5C2,
    0xC2C3C3C1, 0xC2C3C2C1, 0xC2C3C1C1, 0xC2C3C5C1, 0xC2C3C3C5, 0xC2C3C2C5, 0xC2C3C1C5, 0xC2C3C5C5,
    0xC1C3C3C3, 0xC1C3C2C3, 0xC1C3C1C3, 0xC1C3C5C3, 0xC1C3C3C2, 0xC1C3C2C2, 0xC1C3C1C2, 0xC1C3C5C2,
    0xC1C3C3C1, 0xC1C3C2C1, 0xC1C3C1C1, 0xC1C3C5C1, 0xC1C3C3C5, 0xC1C3C2C5, 0xC1C3C1C5, 0xC1C3C5C5,
    0xC5C3C3C3, 0xC5C3C2C3, 0xC5C3C1C3, 0xC5C3C5C3, 0xC5C3C3C2, 0xC5C3C2C2, 0xC5C3C1C2, 0xC5C3C5C2,
    0xC5C3C3C1, 0xC5C3C2C1, 0xC5C3C1C1, 0xC5C3C5C1, 0xC5C3C3C5, 0xC5C3C2C5, 0xC5C3C1C5, 0xC5C3C5C5,
    0xC3C2C3C3, 0xC3C2C2C3, 0xC3C2C1C3, 0xC3C2C5C3, 0xC3C2C3C2, 0xC3C2C2C2, 0xC3C2C1C2, 0xC3C2C5C2,
    0xC3C2C3C1, 0xC3C2C2C1, 0xC3C2C1C1, 0xC3C2C5C1, 0xC3C2C3C5, 0xC3C2C2C5, 0xC3C2C1C5, 0xC3C2C5C5,
    0xC2C2C3C3, 0xC2C2C2C3, 0xC2C2C1C3, 0xC2C2C5C3, 0xC2C2C3C2, 0xC2C2C2C2, 0xC2C2C1C2, 0xC2C2C5C2,
    0xC2C2C3C1, 0xC2C2C2C1, 0xC2C2C1C1, 0xC2C2C5C1, 0xC2C2C3C5, 0xC2C2C2C5, 0xC2C2C1C5, 0xC2C2C5C5,
    0xC1C2C3C3, 0xC1C2C2C3, 0xC1C2C1C3, 0xC1C2C5C3, 0xC1C2C3C2, 0xC1C2C2C2, 0xC1C2C1C2, 0xC1C2C5C2,
    0xC1C2C3C1, 0xC1C2C2C1, 0xC1C2C1C1, 0xC1C2C5C1, 0xC1C2C3C5, 0xC1C2C2C5, 0xC1C2C1C5, 0xC1C2C5C5,
    0xC5C2C3C3, 0xC5C2C2C3, 0xC5C2C1C3, 0xC5C2C5C3, 0xC5C2C3C2, 0xC5C2C2C2, 0xC5C2C1C2, 0xC5C2C5C2,
    0xC5C2C3C1, 0xC5C2C2C1, 0xC5C2C1C1, 0xC5C2C5C1, 0xC5C2C3C5, 0xC5C2C2C5, 0xC5C2C1C5, 0xC5C2C5C5,
    0xC3C1C3C3, 0xC3C1C2C3, 0xC3C1C1C3, 0xC3C1C5C3, 0xC3C1C3C2, 0xC3C1C2C2, 0xC3C1C1C2, 0xC3C1C5C2,
    0xC3C1C3C1, 0xC3C1C2C1, 0xC3C1C1C1, 0xC3C1C5C1, 0xC3C1C3C5, 0xC3C1C2C5, 0xC3C1C1C5, 0xC3C1C5C5,
    0xC2C1C3C3, 0xC2C1C2C3, 0xC2C1C1C3, 0xC2C1C5C3, 0xC2C1C3C2, 0xC2C1C2C2, 0xC2C1C1C2, 0xC2C1C5C2,
    0xC2C1C3C1, 0xC2C1C2C1, 0xC2C1C1C1, 0xC2C1C5C1, 0xC2C1C3C5, 0xC2C1C2C5, 0xC2C1C1C5, 0xC2C1C5C5,
    0xC1C1C3C3, 0xC1C1C2C3, 0xC1C1C1C3, 0xC1C1C5C3, 0xC1C1C3C2, 0xC1C1C2C2, 0xC1C1C1C2, 0xC1C1C5C2,
    0xC1C1C3C1, 0xC1C1C2C1, 0xC1C1C1C1, 0xC1C1C5C1, 0xC1C1C3C5, 0xC1C1C2C5, 0xC1C1C1C5, 0xC1C1C5C5,
    0xC5C1C3C3, 0xC5C1C2C3, 0xC5C1C1C3, 0xC5C1C5C3, 0xC5C1C3C2, 0xC5C1C2C2, 0xC5C1C1C2, 0xC5C1C5C2,
    0xC5C1C3C1, 0xC5C1C2C1, 0xC5C1C1C1, 0xC5C1C5C1, 0xC5C1C3C5, 0xC5C1C2C5, 0xC5C1C1C5, 0xC5C1C5C5,
    0xC3C5C3C3, 0xC3C5C2C3, 0xC3C5C1C3, 0xC3C5C5C3, 0xC3C5C3C2, 0xC3C5C2C2, 0xC3C5C1C2, 0xC3C5C5C2,
    0xC3C5C3C1, 0xC3C5C2C1, 0xC3C5C1C1, 0xC3C5C5C1, 0xC3C5C3C5, 0xC3C5C2C5, 0xC3C5C1C5, 0xC3C5C5C5,
    0xC2C5C3C3, 0xC2C5C2C3, 0xC2C5C1C3, 0xC2C5C5C3, 0xC2C5C3C2, 0xC2C5C2C2, 0xC2C5C1C2, 0xC2C5C5C2,
    0xC2C5C3C1, 0xC2C5C2C1, 0xC2C5C1C1, 0xC2C5C5C1, 0xC2C5C3C5, 0xC2C5C2C5, 0xC2C5C1C5, 0xC2C5C5C5,
    0xC1C5C3C3, 0xC1C5C2C3, 0xC1C5C1C3, 0xC1C5C5C3, 0xC1C5C3C2, 0xC1C5C2C2, 0xC1C5C1C2, 0xC1C5C5C2,
    0xC1C5C3C1, 0xC1C5C2C1, 0xC1C5C1C1, 0xC1C5C5C1, 0xC1C5C3C5, 0xC1C5C2C5, 0xC1C5C1C5, 0xC1C5C5C5,
    0xC5C5C3C3, 0xC5C5C2C3, 0xC5C5C1C3, 0xC5C5C5C3, 0xC5C5C3C2, 0xC5C5C2C2, 0xC5C5C1C2, 0xC5C5C5C2,
    0xC5C5C3C1, 0xC5C5C2C1, 0xC5C5C1C1, 0xC5C5C5C1, 0xC5C5C3C5, 0xC5C5C2C5, 0xC5C5C1C5, 0xC5C5C5C5,
];

const NF_PK_LUT_6: [u32; 256] = [
    0xE3C3E3C3, 0xE3C7E3C3, 0xE3C1E3C3, 0xE3C5E3C3, 0xE7C3E3C3, 0xE7C7E3C3, 0xE7C1E3C3, 0xE7C5E3C3,
    0xE1C3E3C3, 0xE1C7E3C3, 0xE1C1E3C3, 0xE1C5E3C3, 0xE5C3E3C3, 0xE5C7E3C3, 0xE5C1E3C3, 0xE5C5E3C3,
    0xE3C3E3C7, 0xE3C7E3C7, 0xE3C1E3C7, 0xE3C5E3C7, 0xE7C3E3C7, 0xE7C7E3C7, 0xE7C1E3C7, 0xE7C5E3C7,
    0xE1C3E3C7, 0xE1C7E3C7, 0xE1C1E3C7, 0xE1C5E3C7, 0xE5C3E3C7, 0xE5C7E3C7, 0xE5C1E3C7, 0xE5C5E3C7,
    0xE3C3E3C1, 0xE3C7E3C1, 0xE3C1E3C1, 0xE3C5E3C1, 0xE7C3E3C1, 0xE7C7E3C1, 0xE7C1E3C1, 0xE7C5E3C1,
    0xE1C3E3C1, 0xE1C7E3C1, 0xE1C1E3C1, 0xE1C5E3C1, 0xE5C3E3C1, 0xE5C7E3C1, 0xE5C1E3C1, 0xE5C5E3C1,
    0xE3C3E3C5, 0xE3C7E3C5, 0xE3C1E3C5, 0xE3C5E3C5, 0xE7C3E3C5, 0xE7C7E3C5, 0xE7C1E3C5, 0xE7C5E3C5,
    0xE1C3E3C5, 0xE1C7E3C5, 0xE1C1E3C5, 0xE1C5E3C5, 0xE5C3E3C5, 0xE5C7E3C5, 0xE5C1E3C5, 0xE5C5E3C5,
    0xE3C3E7C3, 0xE3C7E7C3, 0xE3C1E7C3, 0xE3C5E7C3, 0xE7C3E7C3, 0xE7C7E7C3, 0xE7C1E7C3, 0xE7C5E7C3,
    0xE1C3E7C3, 0xE1C7E7C3, 0xE1C1E7C3, 0xE1C5E7C3, 0xE5C3E7C3, 0xE5C7E7C3, 0xE5C1E7C3, 0xE5C5E7C3,
    0xE3C3E7C7, 0xE3C7E7C7, 0xE3C1E7C7, 0xE3C5E7C7, 0xE7C3E7C7, 0xE7C7E7C7, 0xE7C1E7C7, 0xE7C5E7C7,
    0xE1C3E7C7, 0xE1C7E7C7, 0xE1C1E7C7, 0xE1C5E7C7, 0xE5C3E7C7, 0xE5C7E7C7, 0xE5C1E7C7, 0xE5C5E7C7,
    0xE3C3E7C1, 0xE3C7E7C1, 0xE3C1E7C1, 0xE3C5E7C1, 0xE7C3E7C1, 0xE7C7E7C1, 0xE7C1E7C1, 0xE7C5E7C1,
    0xE1C3E7C1, 0xE1C7E7C1, 0xE1C1E7C1, 0xE1C5E7C1, 0xE5C3E7C1, 0xE5C7E7C1, 0xE5C1E7C1, 0xE5C5E7C1,
    0xE3C3E7C5, 0xE3C7E7C5, 0xE3C1E7C5, 0xE3C5E7C5, 0xE7C3E7C5, 0xE7C7E7C5, 0xE7C1E7C5, 0xE7C5E7C5,
    0xE1C3E7C5, 0xE1C7E7C5, 0xE1C1E7C5, 0xE1C5E7C5, 0xE5C3E7C5, 0xE5C7E7C5, 0xE5C1E7C5, 0xE5C5E7C5,
    0xE3C3E1C3, 0xE3C7E1C3, 0xE3C1E1C3, 0xE3C5E1C3, 0xE7C3E1C3, 0xE7C7E1C3, 0xE7C1E1C3, 0xE7C5E1C3,
    0xE1C3E1C3, 0xE1C7E1C3, 0xE1C1E1C3, 0xE1C5E1C3, 0xE5C3E1C3, 0xE5C7E1C3, 0xE5C1E1C3, 0xE5C5E1C3,
    0xE3C3E1C7, 0xE3C7E1C7, 0xE3C1E1C7, 0xE3C5E1C7, 0xE7C3E1C7, 0xE7C7E1C7, 0xE7C1E1C7, 0xE7C5E1C7,
    0xE1C3E1C7, 0xE1C7E1C7, 0xE1C1E1C7, 0xE1C5E1C7, 0xE5C3E1C7, 0xE5C7E1C7, 0xE5C1E1C7, 0xE5C5E1C7,
    0xE3C3E1C1, 0xE3C7E1C1, 0xE3C1E1C1, 0xE3C5E1C1, 0xE7C3E1C1, 0xE7C7E1C1, 0xE7C1E1C1, 0xE7C5E1C1,
    0xE1C3E1C1, 0xE1C7E1C1, 0xE1C1E1C1, 0xE1C5E1C1, 0xE5C3E1C1, 0xE5C7E1C1, 0xE5C1E1C1, 0xE5C5E1C1,
    0xE3C3E1C5, 0xE3C7E1C5, 0xE3C1E1C5, 0xE3C5E1C5, 0xE7C3E1C5, 0xE7C7E1C5, 0xE7C1E1C5, 0xE7C5E1C5,
    0xE1C3E1C5, 0xE1C7E1C5, 0xE1C1E1C5, 0xE1C5E1C5, 0xE5C3E1C5, 0xE5C7E1C5, 0xE5C1E1C5, 0xE5C5E1C5,
    0xE3C3E5C3, 0xE3C7E5C3, 0xE3C1E5C3, 0xE3C5E5C3, 0xE7C3E5C3, 0xE7C7E5C3, 0xE7C1E5C3, 0xE7C5E5C3,
    0xE1C3E5C3, 0xE1C7E5C3, 0xE1C1E5C3, 0xE1C5E5C3, 0xE5C3E5C3, 0xE5C7E5C3, 0xE5C1E5C3, 0xE5C5E5C3,
    0xE3C3E5C7, 0xE3C7E5C7, 0xE3C1E5C7, 0xE3C5E5C7, 0xE7C3E5C7, 0xE7C7E5C7, 0xE7C1E5C7, 0xE7C5E5C7,
    0xE1C3E5C7, 0xE1C7E5C7, 0xE1C1E5C7, 0xE1C5E5C7, 0xE5C3E5C7, 0xE5C7E5C7, 0xE5C1E5C7, 0xE5C5E5C7,
    0xE3C3E5C1, 0xE3C7E5C1, 0xE3C1E5C1, 0xE3C5E5C1, 0xE7C3E5C1, 0xE7C7E5C1, 0xE7C1E5C1, 0xE7C5E5C1,
    0xE1C3E5C1, 0xE1C7E5C1, 0xE1C1E5C1, 0xE1C5E5C1, 0xE5C3E5C1, 0xE5C7E5C1, 0xE5C1E5C1, 0xE5C5E5C1,
    0xE3C3E5C5, 0xE3C7E5C5, 0xE3C1E5C5, 0xE3C5E5C5, 0xE7C3E5C5, 0xE7C7E5C5, 0xE7C1E5C5, 0xE7C5E5C5,
    0xE1C3E5C5, 0xE1C7E5C5, 0xE1C1E5C5, 0xE1C5E5C5, 0xE5C3E5C5, 0xE5C7E5C5, 0xE5C1E5C5, 0xE5C5E5C5,
];

/// Скрэтч шаблонных блоков: `map1/map2` как в `nfPkDecomp` (на кадр).
struct PatternScratch {
    map1: [u8; 512],
    map2: [u32; 256],
}

impl PatternScratch {
    fn new() -> Self {
        Self {
            map1: [0u8; 512],
            map2: [0u32; 256],
        }
    }
}

/// Точный порт шаблонных ветвей `nfPkDecomp` (коды блоков 0x7…0xA).
fn decode_pattern_block(
    opcode: u8,
    _frame: usize,
    mut dst: usize,
    stride: usize,
    cur: &mut [u8],
    cursor: &mut VideoCursor<'_>,
    pattern: &mut PatternScratch,
) -> Result<(), MveError> {
    let available = cursor.data.len().saturating_sub(cursor.pos);
    let take = available.min(64);
    let mut buffer = [0u8; 64];
    buffer[..take].copy_from_slice(&cursor.data[cursor.pos..cursor.pos + take]);

    // Код 0xA — построчный порт `nfPkDecomp` (в нём важна каждая
    // корректировка `dst`, а не только итоговая картинка блока).
    if opcode == 0xA {
        let result = write_case_10(
            cur,
            &mut dst,
            &buffer,
            &mut pattern.map1,
            &mut pattern.map2,
            stride,
        );

        // Ветка `src[0] > src[1]` (A) читает 24 байта, иначе (B) — 32.
        let consumed = if buffer[0] > buffer[1] { 24 } else { 32 };

        cursor.pos = (cursor.pos + consumed).min(cursor.data.len());

        return result;
    }

    match opcode {
        0x7 => {
            let src = &buffer[..];

            if src[0] > src[1] {
                for i in 0..2 {
                    let value = NF_PK_LUT_4[usize::from(src[2 + i] & 0x0F)];
                    pattern.map1[i * 8..i * 8 + 4].copy_from_slice(&value.to_le_bytes());
                    let value = NF_PK_LUT_4[usize::from(src[2 + i] >> 4)];
                    pattern.map1[4 + i * 8..4 + i * 8 + 4].copy_from_slice(&value.to_le_bytes());
                }

                pattern.map2[0xC1] = (u32::from(src[1]) << 8) | u32::from(src[1]);
                pattern.map2[0xC3] = (u32::from(src[0]) << 8) | u32::from(src[0]);

                for pair in 0..4 {
                    let base = pair * 4;
                    let word0 = (pattern.map2[usize::from(pattern.map1[base])] << 16)
                        | pattern.map2[usize::from(pattern.map1[base + 1])];
                    let word1 = (pattern.map2[usize::from(pattern.map1[base + 2])] << 16)
                        | pattern.map2[usize::from(pattern.map1[base + 3])];

                    for row in [pair * 2, pair * 2 + 1] {
                        put_u32(cur, dst + row * stride, word0);
                        put_u32(cur, dst + row * stride + 4, word1);
                    }
                }

                cursor.pos += 4;
            } else {
                fill_pattern(
                    cur,
                    dst,
                    stride,
                    src,
                    &mut pattern.map1,
                    &mut pattern.map2,
                    8,
                    0,
                    0,
                );
                cursor.pos += 10;
            }
        }
        0x8 => {
            let src = &buffer[..];

            if src[0] > src[1] {
                if src[6] > src[7] {
                    fill_pattern(
                        cur,
                        dst,
                        stride,
                        src,
                        &mut pattern.map1,
                        &mut pattern.map2,
                        4,
                        0,
                        0,
                    );
                    fill_pattern(
                        cur,
                        dst + 4 * stride,
                        stride,
                        &src[6..],
                        &mut pattern.map1,
                        &mut pattern.map2,
                        4,
                        16,
                        0,
                    );
                } else {
                    fill_pattern(
                        cur,
                        dst,
                        stride,
                        src,
                        &mut pattern.map1,
                        &mut pattern.map2,
                        4,
                        0,
                        1,
                    );
                    fill_pattern(
                        cur,
                        dst + 4,
                        stride,
                        &src[6..],
                        &mut pattern.map1,
                        &mut pattern.map2,
                        4,
                        16,
                        2,
                    );
                }

                cursor.pos += 12;
            } else {
                fill_pattern(
                    cur,
                    dst,
                    stride,
                    src,
                    &mut pattern.map1,
                    &mut pattern.map2,
                    2,
                    0,
                    2,
                );
                fill_pattern(
                    cur,
                    dst + 4 * stride,
                    stride,
                    &src[4..],
                    &mut pattern.map1,
                    &mut pattern.map2,
                    2,
                    8,
                    2,
                );
                fill_pattern(
                    cur,
                    dst + 4,
                    stride,
                    &src[8..],
                    &mut pattern.map1,
                    &mut pattern.map2,
                    2,
                    16,
                    2,
                );
                fill_pattern(
                    cur,
                    dst + 4 * stride + 4,
                    stride,
                    &src[12..],
                    &mut pattern.map1,
                    &mut pattern.map2,
                    2,
                    24,
                    2,
                );
                cursor.pos += 16;
            }
        }
        0x9 => {
            let src = &buffer[..];

            if src[0] > src[1] {
                if src[2] > src[3] {
                    fill_pattern11(src, &mut pattern.map1, &mut pattern.map2, 8, 0);

                    for i in 0..4 {
                        fill_pattern12(
                            cur,
                            dst + i * 2 * stride,
                            &pattern.map1,
                            &pattern.map2,
                            i * 8,
                            0,
                        );
                        fill_pattern12(
                            cur,
                            dst + (i * 2 + 1) * stride,
                            &pattern.map1,
                            &pattern.map2,
                            i * 8,
                            0,
                        );
                    }

                    cursor.pos += 12;
                } else {
                    fill_pattern11(src, &mut pattern.map1, &mut pattern.map2, 8, 1);

                    for i in 0..8 {
                        fill_pattern12(
                            cur,
                            dst + i * stride,
                            &pattern.map1,
                            &pattern.map2,
                            i * 4,
                            1,
                        );
                    }

                    cursor.pos += 12;
                }
            } else if src[2] > src[3] {
                fill_pattern11(src, &mut pattern.map1, &mut pattern.map2, 8, 1);

                for i in 0..4 {
                    fill_pattern12(
                        cur,
                        dst + i * 2 * stride,
                        &pattern.map1,
                        &pattern.map2,
                        i * 4,
                        1,
                    );
                    fill_pattern12(
                        cur,
                        dst + (i * 2 + 1) * stride,
                        &pattern.map1,
                        &pattern.map2,
                        i * 4,
                        1,
                    );
                }

                cursor.pos += 8;
            } else {
                fill_pattern11(src, &mut pattern.map1, &mut pattern.map2, 16, 0);

                for i in 0..8 {
                    fill_pattern12(
                        cur,
                        dst + i * stride,
                        &pattern.map1,
                        &pattern.map2,
                        i * 8,
                        0,
                    );
                }

                cursor.pos += 20;
            }
        }
        0xA => {
            let src = &buffer[..];

            if src[0] > src[1] {
                for i in 0..8 {
                    let value = NF_PK_LUT_6[usize::from(src[4 + i])];
                    pattern.map1[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
                for i in 0..8 {
                    let value = NF_PK_LUT_6[usize::from(src[16 + i])];
                    pattern.map1[32 + i * 4..32 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }

                set_colors4(&mut pattern.map2, &src[0..4]);

                if src[12] > src[13] {
                    for i in 0..4 {
                        fill_pattern12(
                            cur,
                            dst + i * stride,
                            &pattern.map1,
                            &pattern.map2,
                            i * 8,
                            0,
                        );
                    }

                    set_colors4(&mut pattern.map2, &src[12..16]);

                    for i in 0..4 {
                        fill_pattern12(
                            cur,
                            dst + (4 + i) * stride,
                            &pattern.map1,
                            &pattern.map2,
                            32 + i * 8,
                            0,
                        );
                    }
                } else {
                    // Левая половина — полные 8-пиксельные строки.
                    for i in 0..4 {
                        fill_pattern12(
                            cur,
                            dst + i * 2 * stride,
                            &pattern.map1,
                            &pattern.map2,
                            i * 8,
                            0,
                        );
                        fill_pattern12(
                            cur,
                            dst + (i * 2 + 1) * stride,
                            &pattern.map1,
                            &pattern.map2,
                            i * 8,
                            0,
                        );
                    }

                    // Правая половина — 4-пиксельные слова поверх правых колонок.
                    set_colors4(&mut pattern.map2, &src[12..16]);

                    for i in 0..4 {
                        put_lut6_word(
                            cur,
                            dst + i * 2 * stride + 4,
                            &pattern.map1,
                            &pattern.map2,
                            32 + i * 8,
                        );
                        put_lut6_word(
                            cur,
                            dst + (i * 2 + 1) * stride + 4,
                            &pattern.map1,
                            &pattern.map2,
                            32 + i * 8 + 4,
                        );
                    }
                }

                cursor.pos += 24;
            } else {
                for i in 0..4 {
                    let value = NF_PK_LUT_6[usize::from(src[4 + i])];
                    pattern.map1[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
                for i in 0..4 {
                    let value = NF_PK_LUT_6[usize::from(src[12 + i])];
                    pattern.map1[16 + i * 4..16 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
                for i in 0..4 {
                    let value = NF_PK_LUT_6[usize::from(src[20 + i])];
                    pattern.map1[32 + i * 4..32 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
                for i in 0..4 {
                    let value = NF_PK_LUT_6[usize::from(src[28 + i])];
                    pattern.map1[48 + i * 4..48 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }

                // Four 4x4 quadrants, one u32 per row.
                set_colors4(&mut pattern.map2, &src[0..4]);

                for i in 0..2 {
                    put_lut6_word(
                        cur,
                        dst + i * 2 * stride,
                        &pattern.map1,
                        &pattern.map2,
                        i * 8,
                    );
                    put_lut6_word(
                        cur,
                        dst + (i * 2 + 1) * stride,
                        &pattern.map1,
                        &pattern.map2,
                        i * 8 + 4,
                    );
                }

                set_colors4(&mut pattern.map2, &src[8..12]);

                for i in 0..2 {
                    put_lut6_word(
                        cur,
                        dst + (4 + i * 2) * stride,
                        &pattern.map1,
                        &pattern.map2,
                        16 + i * 8,
                    );
                    put_lut6_word(
                        cur,
                        dst + (4 + i * 2 + 1) * stride,
                        &pattern.map1,
                        &pattern.map2,
                        16 + i * 8 + 4,
                    );
                }

                set_colors4(&mut pattern.map2, &src[16..20]);

                for i in 0..2 {
                    put_lut6_word(
                        cur,
                        dst + i * 2 * stride + 4,
                        &pattern.map1,
                        &pattern.map2,
                        32 + i * 8,
                    );
                    put_lut6_word(
                        cur,
                        dst + (i * 2 + 1) * stride + 4,
                        &pattern.map1,
                        &pattern.map2,
                        32 + i * 8 + 4,
                    );
                }

                set_colors4(&mut pattern.map2, &src[24..28]);

                for i in 0..2 {
                    put_lut6_word(
                        cur,
                        dst + (4 + i * 2) * stride + 4,
                        &pattern.map1,
                        &pattern.map2,
                        48 + i * 8,
                    );
                    put_lut6_word(
                        cur,
                        dst + (4 + i * 2 + 1) * stride + 4,
                        &pattern.map1,
                        &pattern.map2,
                        48 + i * 8 + 4,
                    );
                }

                cursor.pos += 32;
            }
        }
        _ => unreachable!("decode_pattern_block: 0x7..=0xA"),
    }

    cursor.pos = cursor.pos.min(cursor.data.len());

    Ok(())
}

/// `nfFillPattern`: 2-цветный шаблон (`run` итераций по 1/2 строки).
#[allow(clippy::too_many_arguments)]
fn fill_pattern(
    buf: &mut [u8],
    dst: usize,
    stride: usize,
    src: &[u8],
    map1: &mut [u8],
    map2: &mut [u32],
    run: usize,
    offset: usize,
    mode: usize,
) {
    for i in 0..run {
        let value = NF_PK_LUT_5[usize::from(src[2 + i])];

        map1[offset + i * 4..offset + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }

    map2[0xC1] = (u32::from(src[1]) << 8) | u32::from(src[0]);
    map2[0xC2] = (u32::from(src[0]) << 8) | u32::from(src[1]);
    map2[0xC3] = (u32::from(src[0]) << 8) | u32::from(src[0]);
    map2[0xC5] = (u32::from(src[1]) << 8) | u32::from(src[1]);

    let mut row = dst;

    for i in 0..run {
        let index = offset + i * 4;

        match mode {
            2 => {
                put_u32(
                    buf,
                    row,
                    (map2[usize::from(map1[index])] << 16) | map2[usize::from(map1[index + 1])],
                );
                row += stride;
                put_u32(
                    buf,
                    row,
                    (map2[usize::from(map1[index + 2])] << 16) | map2[usize::from(map1[index + 3])],
                );
                row += stride;
            }
            1 => {
                put_u32(
                    buf,
                    row,
                    (map2[usize::from(map1[index + 2])] << 16) | map2[usize::from(map1[index + 1])],
                );
                row += stride;
                put_u32(
                    buf,
                    row,
                    (map2[usize::from(map1[index + 2])] << 16) | map2[usize::from(map1[index + 3])],
                );
                row += stride;
            }
            _ => {
                put_u32(
                    buf,
                    row,
                    (map2[usize::from(map1[index])] << 16) | map2[usize::from(map1[index + 1])],
                );
                put_u32(
                    buf,
                    row + 4,
                    (map2[usize::from(map1[index + 2])] << 16) | map2[usize::from(map1[index + 3])],
                );
                row += stride;
            }
        }
    }
}

/// `nfFillPattern11`: 4-цветный шаблон, заполняет `map1`.
fn fill_pattern11(src: &[u8], map1: &mut [u8], map2: &mut [u32], stride: usize, mode: usize) {
    for i in 0..stride {
        let mut value = NF_PK_LUT_6[usize::from(src[4 + i])];

        if mode != 0 {
            value = value.swap_bytes();
        }

        map1[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }

    map2[0xC1] = u32::from(src[2]);
    map2[0xC3] = u32::from(src[0]);
    map2[0xC5] = u32::from(src[3]);
    map2[0xC7] = u32::from(src[1]);
    map2[0xE1] = u32::from(src[2]);
    map2[0xE3] = u32::from(src[0]);
    map2[0xE5] = u32::from(src[3]);
    map2[0xE7] = u32::from(src[1]);
}

/// `nfFillPattern12`: 4-цветная строка/две строки по `map1`.
fn fill_pattern12(
    buf: &mut [u8],
    dst: usize,
    map1: &[u8],
    map2: &[u32],
    index: usize,
    mode: usize,
) {
    let at = |offset: usize| -> u32 { map2[usize::from(map1[index + offset])] };

    match mode {
        0 => {
            put_u32(
                buf,
                dst,
                (at(0) << 16) | (at(1) << 24) | at(2) | (at(3) << 8),
            );
            put_u32(
                buf,
                dst + 4,
                (at(4) << 16) | (at(5) << 24) | at(6) | (at(7) << 8),
            );
        }
        1 => {
            put_u32(
                buf,
                dst,
                (at(0) << 24) | (at(0) << 16) | (at(1) << 8) | at(1),
            );
            put_u32(
                buf,
                dst + 4,
                (at(2) << 24) | (at(2) << 16) | (at(3) << 8) | at(3),
            );
        }
        _ => unreachable!("fill_pattern12 mode"),
    }
}

/// Одно 32-битное слово (4 пикселя) из `map1/map2` (ветка 0xA).
fn put_lut6_word(buf: &mut [u8], dst: usize, map1: &[u8], map2: &[u32], index: usize) {
    let at = |offset: usize| -> u32 { map2[usize::from(map1[index + offset])] };
    let word = (at(0) << 16) | (at(1) << 24) | at(2) | (at(3) << 8);

    put_u32(buf, dst, word);
}

fn put_u32(buf: &mut [u8], offset: usize, value: u32) {
    if offset + 4 <= buf.len() {
        buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}

/// Точный построчный порт ветки `case 10` из `nfPkDecomp`.
///
/// В отличие от остальных шаблонов здесь важна не только картинка 8×8, но и
/// порядок записи/сдвиги `dst`: ветка сначала пишет полные строки, а затем
/// накладывает правую половину. Повторяем C-код дословно.
fn write_case_10(
    cur: &mut [u8],
    dst: &mut usize,
    src: &[u8],
    map1: &mut [u8; 512],
    map2: &mut [u32; 256],
    stride: usize,
) -> Result<(), MveError> {
    let back_right = stride * 7 - 8;
    let mut position = *dst as isize;

    let put = |cur: &mut [u8], at: isize, value: u32| {
        if at >= 0 {
            put_u32(cur, at as usize, value);
        }
    };
    let word = |map1: &[u8; 512], map2: &[u32; 256], index: usize| -> u32 {
        (map2[usize::from(map1[index])] << 16)
            | (map2[usize::from(map1[index + 1])] << 24)
            | map2[usize::from(map1[index + 2])]
            | (map2[usize::from(map1[index + 3])] << 8)
    };

    if src[0] > src[1] {
        for i in 0..8 {
            let value = NF_PK_LUT_6[usize::from(src[4 + i])];
            map1[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for i in 0..8 {
            let value = NF_PK_LUT_6[usize::from(src[16 + i])];
            map1[32 + i * 4..32 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }

        set_colors4(map2, &src[0..4]);

        if src[12] > src[13] {
            for i in 0..4 {
                put(cur, position, word(map1, map2, i * 8));
                put(cur, position + 4, word(map1, map2, i * 8 + 4));
                position += stride as isize;
            }

            set_colors4(map2, &src[12..16]);

            for i in 0..4 {
                put(cur, position, word(map1, map2, 32 + i * 8));
                put(cur, position + 4, word(map1, map2, 32 + i * 8 + 4));
                position += stride as isize;
            }

            position -= stride as isize;
            position -= back_right as isize;
        } else {
            for i in 0..4 {
                put(cur, position, word(map1, map2, i * 8));
                position += stride as isize;
                put(cur, position, word(map1, map2, i * 8 + 4));
                position += stride as isize;
            }

            position -= (stride * 8 - 4) as isize;

            set_colors4(map2, &src[12..16]);

            for i in 0..4 {
                put(cur, position, word(map1, map2, 32 + i * 8));
                position += stride as isize;
                put(cur, position, word(map1, map2, 32 + i * 8 + 4));
                position += stride as isize;
            }

            position -= stride as isize;
            position -= 4;
            position -= back_right as isize;
        }
    } else {
        for i in 0..4 {
            let value = NF_PK_LUT_6[usize::from(src[4 + i])];
            map1[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for i in 0..4 {
            let value = NF_PK_LUT_6[usize::from(src[12 + i])];
            map1[16 + i * 4..16 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for i in 0..4 {
            let value = NF_PK_LUT_6[usize::from(src[20 + i])];
            map1[32 + i * 4..32 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for i in 0..4 {
            let value = NF_PK_LUT_6[usize::from(src[28 + i])];
            map1[48 + i * 4..48 + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }

        set_colors4(map2, &src[0..4]);

        for i in 0..2 {
            put(cur, position, word(map1, map2, i * 8));
            position += stride as isize;
            put(cur, position, word(map1, map2, i * 8 + 4));
            position += stride as isize;
        }

        set_colors4(map2, &src[8..12]);

        for i in 0..2 {
            put(cur, position, word(map1, map2, 16 + i * 8));
            position += stride as isize;
            put(cur, position, word(map1, map2, 16 + i * 8 + 4));
            position += stride as isize;
        }

        position -= (stride * 8 - 4) as isize;

        set_colors4(map2, &src[16..20]);

        for i in 0..2 {
            put(cur, position, word(map1, map2, 32 + i * 8));
            position += stride as isize;
            put(cur, position, word(map1, map2, 32 + i * 8 + 4));
            position += stride as isize;
        }

        set_colors4(map2, &src[24..28]);

        for i in 0..2 {
            put(cur, position, word(map1, map2, 48 + i * 8));
            position += stride as isize;
            put(cur, position, word(map1, map2, 48 + i * 8 + 4));
            position += stride as isize;
        }

        position -= stride as isize;
        position -= 4;
        position -= back_right as isize;
    }

    *dst = position.max(0) as usize;

    Ok(())
}

fn set_colors4(map2: &mut [u32; 256], src: &[u8]) {
    map2[0xC1] = u32::from(src[2]);
    map2[0xC3] = u32::from(src[0]);
    map2[0xC5] = u32::from(src[3]);
    map2[0xC7] = u32::from(src[1]);
    map2[0xE1] = u32::from(src[2]);
    map2[0xE3] = u32::from(src[0]);
    map2[0xE5] = u32::from(src[3]);
    map2[0xE7] = u32::from(src[1]);
}

/// Смещение блока в буфере кадра.
#[inline]
fn block_offset(header: &FrameHeader, block_x: usize, block_y: usize, stride: usize) -> usize {
    let x = usize::from(header.x) + block_x * BLOCK_SIZE;
    let y = usize::from(header.y) + block_y * BLOCK_SIZE;

    y * stride + x
}

/// Расшифровка байта вектора движения (`nf_pk_lut_3`): (dx, dy).
///
/// Таблица оракула раскладывается в ту же формулу, что и в FFmpeg:
/// `B < 56` — положительный вектор `(8 + B % 7, B / 7)`, иначе —
/// `(-14 + (B - 56) % 29, 8 + (B - 56) / 29)`.
#[inline]
fn motion_vector(byte: u8) -> (isize, isize) {
    let value = isize::from(byte);

    if value < 56 {
        (8 + value % 7, value / 7)
    } else {
        (-14 + (value - 56) % 29, 8 + (value - 56) / 29)
    }
}

/// Копирует 8×8 из `src` по линейному смещению `src_offset` в `dst` по `dst_offset`.
fn copy_block(dst: &mut [u8], dst_offset: usize, src: &[u8], src_offset: isize, stride: usize) {
    let mut block = [0u8; 64];

    for row in 0..8 {
        for column in 0..8 {
            let source = src_offset + (row * stride + column) as isize;

            block[row * 8 + column] = if source >= 0 && (source as usize) < src.len() {
                src[source as usize]
            } else {
                0
            };
        }
    }

    write_block(dst, dst_offset, stride, &block);
}

/// Копирует 8×8 внутри одного буфера (чтение во временный блок).
fn copy_within_block(dst: &mut [u8], dst_offset: usize, src_offset: isize, stride: usize) {
    let mut block = [0u8; 64];

    for row in 0..8 {
        for column in 0..8 {
            let source = src_offset + (row * stride + column) as isize;

            block[row * 8 + column] = if source >= 0 && (source as usize) < dst.len() {
                dst[source as usize]
            } else {
                0
            };
        }
    }

    write_block(dst, dst_offset, stride, &block);
}

fn write_block(dst: &mut [u8], dst_offset: usize, stride: usize, block: &[u8; 64]) {
    for row in 0..8 {
        let offset = dst_offset + row * stride;

        if offset + 8 <= dst.len() {
            dst[offset..offset + 8].copy_from_slice(&block[row * 8..row * 8 + 8]);
        }
    }
}

// ---------------------------------------------------------------------------
// Тесты
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fnv1a(data: &[u8]) -> u32 {
        let mut hash = 2_166_136_261u32;

        for &byte in data {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(16_777_619);
        }

        hash
    }

    fn real_mve_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original/MAXINT.MVE")
    }

    #[test]
    fn rejects_bad_header() {
        assert!(matches!(
            Mve::parse(vec![0u8; 4]),
            Err(MveError::TooShort { .. })
        ));
        assert!(matches!(
            Mve::parse(vec![0u8; FILE_HEADER_SIZE]),
            Err(MveError::BadSignature)
        ));

        let mut bytes = vec![0u8; FILE_HEADER_SIZE];
        bytes[..MVE_SIGNATURE.len()].copy_from_slice(MVE_SIGNATURE);
        bytes[20..22].copy_from_slice(&0x001Au16.to_le_bytes());
        bytes[22..24].copy_from_slice(&0x0100u16.to_le_bytes());
        bytes[24..26].copy_from_slice(&0u16.to_le_bytes());

        assert!(matches!(Mve::parse(bytes), Err(MveError::BadHeader)));
    }

    #[test]
    fn real_maxint_metadata_and_reference_frames() {
        let path = real_mve_path();

        if !path.is_file() {
            return;
        }

        let mve = Mve::open(&path).expect("MAXINT.MVE");
        let info = mve.info();

        assert_eq!(info.width, 432);
        assert_eq!(info.height, 320);
        assert_eq!(info.frame_count, 4391);
        assert_eq!(info.frame_delay_us, 66_664);
        assert_eq!(info.video_format, OP_VIDEO_DATA_11);
        assert!((info.frames_per_second() - 15.0).abs() < 0.01);

        let mut decoder = mve.decoder();
        let mut hashes = Vec::new();

        for index in 0..=100 {
            let frame = decoder
                .next_frame()
                .unwrap_or_else(|error| panic!("кадр {index}: {error}"))
                .unwrap_or_else(|| panic!("кадр {index} отсутствует"));

            assert_eq!(frame.len(), 432 * 320);

            if index == 0 {
                // Первый кадр — тёмно-синий фон (индекс 4), не чёрный.
                assert!(frame.iter().any(|&pixel| pixel != 0));
                assert!(frame.iter().all(|&pixel| pixel == 4));
            }

            if matches!(index, 0 | 50 | 100) {
                hashes.push((index, fnv1a(frame)));
            }
        }

        assert_eq!(
            hashes,
            vec![(0, 0x74CD_BDC5), (50, 0x22EB_1E36), (100, 0x158D_8E0D)],
            "кадры 0/50/100 должны совпасть с эталоном nfPkDecomp"
        );

        // Первая палитра (`SET_PALETTE` в init-записи): индекс 4 — (4,0,4);
        // 255 остаётся 50/50/50 (`movie_init_palette`). Палитры меняются по
        // ходу фильма (13 `SET_PALETTE` в разных местах), поэтому здесь
        // проверяется именно палитра первых кадров.
        assert_eq!(decoder.palette()[4], [4, 0, 4]);
        assert_eq!(decoder.palette()[255], [50, 50, 50]);
        assert_eq!(decoder.frame_index(), 101);
    }

    #[test]
    fn real_maxint_reaches_the_end() {
        let path = real_mve_path();

        if !path.is_file() {
            return;
        }

        // Полный проход по 4391 кадрам — быстрая проверка индексации/финала
        // (без проверки пикселей каждого кадра): декодер не падает и выдаёт
        // ровно `frame_count` кадров.
        let mve = Mve::open(&path).unwrap();
        let mut decoder = mve.decoder();
        let mut count = 0usize;

        while decoder.next_frame().unwrap().is_some() {
            count += 1;
        }

        assert_eq!(count, mve.info().frame_count);
        assert!(decoder.is_finished());
        // К концу фильма применены все 13 `SET_PALETTE`: индекс 4 — (0,4,44).
        assert_eq!(decoder.palette()[4], [0, 4, 44]);
    }
}
