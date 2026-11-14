//! Мультикадровые спрайты M.A.X. — тела RES-ресурсов с тегами `COMMTWR`…`WALDO`
//! и `S_*` (тени).
//!
//! Формат описан в `max-original-dasm/formats/spw.md` (важно: файлы `*.SPW` в
//! каталоге игры — это RIFF/WAVE-звуки, а не спрайты, §1):
//!
//! ```text
//! ImageMultiHeader:
//! +0x00  u16 image_count
//! +0x02  u32 frame_offset[image_count]   — смещения кадров ОТ НАЧАЛА СПРАЙТА
//!
//! ImageMultiFrameHeader (по frame_offset[i]):
//! +0x00  i16 width, i16 height, i16 hotx, i16 hoty
//! +0x08  u32 row_offset[height]          — смещения строк ОТ НАЧАЛА СПРАЙТА
//!
//! строка спрайта: {u8 skip, u8 count, u8 pixels[count]}*, 0xFF — конец строки
//! строка тени:    {u8 skip, u8 count}*,                     0xFF — конец строки
//! ```
//!
//! Ключевые свойства формата, воспроизведённые здесь (§4, §9):
//! * прозрачность **структурная**: пропущенные пиксели в файле отсутствуют,
//!   поэтому [`SpriteFrame::pixels`] хранит `None` на их месте;
//! * сумма `skip + count` по прогонам строки может быть меньше `width` —
//!   хвост строки прозрачен и в файле не записан (`TANK` кадр 0: 39 из 44, §4.2);
//! * байт `0xFF` — терминатор строки только в позиции счётчика `skip`; внутри
//!   поля пикселей он допустим и читается как обычный индекс палитры (§4.1);
//! * прогон с `count == 0` валиден («только пропуск»);
//! * в кадрах тени пиксельных байтов нет вообще: прогон — пара счётчиков
//!   `{skip, count}`, поэтому тень декодируется в маску затемнения (§4.3);
//! * спрайты не имеют ни сигнатуры, ни флагов — «тень или нет» определяется
//!   только тегом ресурса, поэтому режим задаётся параметром [`decode`].
//!
//! Декодер не паникует на битых данных: структурные ошибки возвращаются как
//! [`SpriteError`], чтение за границы ресурса невозможно (§9.13).

use std::fmt;

/// Размер заголовка спрайта: `u16 image_count`.
pub const HEADER_SIZE: usize = 2;
/// Размер заголовка кадра: 4 × `i16` (width, height, hotx, hoty).
pub const FRAME_HEADER_SIZE: usize = 8;
/// Терминатор строки (`row_delimiter` в `Gfx_DecodeSprite`).
pub const ROW_DELIMITER: u8 = 0xFF;
/// Сколько цветов в палитре (индекс пикселя — `u8`).
pub const PALETTE_SIZE: usize = 256;
/// Верхний предел пикселей одного кадра: защита от OOM на битом заголовке.
pub const MAX_FRAME_PIXELS: usize = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Ошибки
// ---------------------------------------------------------------------------

/// Ошибка разбора тела спрайта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpriteError {
    /// Тело короче двухбайтного `image_count`.
    TooShort {
        /// Фактическая длина тела ресурса.
        len: usize,
        /// Минимально необходимая длина.
        needed: usize,
    },
    /// `image_count == 0` — у спрайта нет ни одного кадра (§9.13).
    ZeroImages,
    /// Таблица смещений кадров не помещается в тело ресурса.
    OffsetTableTruncated {
        /// `image_count` из заголовка.
        image_count: u16,
        /// Требуемый конец таблицы (в байтах от начала спрайта).
        needed: usize,
        /// Фактическая длина тела.
        len: usize,
    },
    /// `frame_offset[i]` указывает за пределы тела ресурса.
    FrameOutOfBounds {
        /// Индекс кадра.
        frame: usize,
        /// Смещение кадра из таблицы.
        offset: u32,
        /// Фактическая длина тела.
        len: usize,
    },
    /// Заголовок кадра или таблица смещений строк не помещаются в тело.
    FrameTruncated {
        /// Индекс кадра.
        frame: usize,
        /// Смещение кадра.
        offset: u32,
        /// Требуемый конец заголовка кадра вместе с таблицей строк.
        needed: usize,
        /// Фактическая длина тела.
        len: usize,
    },
    /// Размеры кадра неположительны (`width <= 0` или `height <= 0`).
    BadDimensions {
        /// Индекс кадра.
        frame: usize,
        /// `width` из заголовка кадра.
        width: i16,
        /// `height` из заголовка кадра.
        height: i16,
    },
    /// Кадр требует больше пикселей, чем [`MAX_FRAME_PIXELS`] — заголовок битый.
    FrameTooLarge {
        /// Индекс кадра.
        frame: usize,
        /// `width` из заголовка кадра.
        width: u16,
        /// `height` из заголовка кадра.
        height: u16,
    },
    /// `row_offset[y]` указывает за пределы тела ресурса.
    RowOutOfBounds {
        /// Индекс кадра.
        frame: usize,
        /// Индекс строки.
        row: usize,
        /// Смещение строки из таблицы.
        offset: u32,
        /// Фактическая длина тела.
        len: usize,
    },
    /// Строка не завершилась терминатором `0xFF` до конца тела ресурса.
    RowUnterminated {
        /// Индекс кадра.
        frame: usize,
        /// Индекс строки.
        row: usize,
        /// Смещение строки.
        offset: u32,
        /// Фактическая длина тела.
        len: usize,
    },
    /// Пара счётчиков `{skip, count}` или байт терминатора обрывается концом тела.
    RunHeaderTruncated {
        /// Индекс кадра.
        frame: usize,
        /// Индекс строки.
        row: usize,
        /// Смещение чтения.
        offset: usize,
        /// Фактическая длина тела.
        len: usize,
    },
    /// Поле пикселей прогона обрывается концом тела ресурса.
    PixelPayloadTruncated {
        /// Индекс кадра.
        frame: usize,
        /// Индекс строки.
        row: usize,
        /// Смещение чтения.
        offset: usize,
        /// Фактическая длина тела.
        len: usize,
    },
    /// Сумма `skip + count` по прогонам строки превышает `width` (§4.2).
    RowOverrun {
        /// Индекс кадра.
        frame: usize,
        /// Индекс строки.
        row: usize,
        /// Накопленная позиция по x после прогона.
        x: u32,
        /// `width` кадра.
        width: u16,
    },
}

impl fmt::Display for SpriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len, needed } => write!(
                f,
                "спрайт слишком короткий: {len} байт (нужно минимум {needed})"
            ),
            Self::ZeroImages => write!(f, "image_count = 0: в спрайте нет кадров"),
            Self::OffsetTableTruncated {
                image_count,
                needed,
                len,
            } => write!(
                f,
                "таблица смещений кадров ({image_count} записей) требует {needed} байт, тело {len}"
            ),
            Self::FrameOutOfBounds { frame, offset, len } => write!(
                f,
                "кадр {frame}: смещение 0x{offset:X} за концом спрайта (0x{len:X})"
            ),
            Self::FrameTruncated {
                frame,
                offset,
                needed,
                len,
            } => write!(
                f,
                "кадр {frame} (0x{offset:X}): заголовок со строками требует {needed} байт, тело {len}"
            ),
            Self::BadDimensions {
                frame,
                width,
                height,
            } => write!(
                f,
                "кадр {frame}: неположительные размеры {width}×{height}"
            ),
            Self::FrameTooLarge {
                frame,
                width,
                height,
            } => write!(
                f,
                "кадр {frame}: размеры {width}×{height} превышают предел в {MAX_FRAME_PIXELS} пикселей"
            ),
            Self::RowOutOfBounds {
                frame,
                row,
                offset,
                len,
            } => write!(
                f,
                "кадр {frame} строка {row}: смещение 0x{offset:X} за концом спрайта (0x{len:X})"
            ),
            Self::RowUnterminated {
                frame,
                row,
                offset,
                len,
            } => write!(
                f,
                "кадр {frame} строка {row} (0x{offset:X}): нет терминатора 0xFF до конца тела (0x{len:X})"
            ),
            Self::RunHeaderTruncated {
                frame,
                row,
                offset,
                len,
            } => write!(
                f,
                "кадр {frame} строка {row}: пара счётчиков на 0x{offset:X} обрывается концом тела (0x{len:X})"
            ),
            Self::PixelPayloadTruncated {
                frame,
                row,
                offset,
                len,
            } => write!(
                f,
                "кадр {frame} строка {row}: пиксели прогона на 0x{offset:X} обрываются концом тела (0x{len:X})"
            ),
            Self::RowOverrun {
                frame,
                row,
                x,
                width,
            } => write!(
                f,
                "кадр {frame} строка {row}: прогоны выходят за width={width} (x={x})"
            ),
        }
    }
}

impl std::error::Error for SpriteError {}

// ---------------------------------------------------------------------------
// Структуры данных
// ---------------------------------------------------------------------------

/// Один прогон строки в том виде, в каком он записан в файле.
///
/// У обычного спрайта `pixels.len() == count`, у строки тени `pixels` пуст:
/// там за парой счётчиков байтов пикселей нет.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteRun {
    /// `u8 skip` — сколько прозрачных пикселей пропустить.
    pub skip: u8,
    /// `u8 count` — сколько пикселей (для тени — затемняемых) идёт дальше.
    pub count: u8,
    /// Индексы палитры прогона; пусто для теней.
    pub pixels: Vec<u8>,
}

impl SpriteRun {
    /// Прогон не содержит пиксельных байтов (строка тени).
    #[inline]
    pub fn is_shadow(&self) -> bool {
        self.pixels.is_empty()
    }

    /// Сколько пикселей строки покрывает прогон (`skip + count`).
    #[inline]
    pub fn span(&self) -> u32 {
        u32::from(self.skip) + u32::from(self.count)
    }
}

/// Одна строка кадра: последовательность прогонов до терминатора `0xFF`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpriteRow {
    /// Прогоны в файловом порядке.
    pub runs: Vec<SpriteRun>,
    /// Сумма `skip + count` по прогонам; может быть меньше `width` (§4.2).
    pub painted: u32,
}

impl SpriteRow {
    /// Сколько прогонов с непустым полем пикселей.
    pub fn painted_runs(&self) -> usize {
        self.runs.iter().filter(|run| run.count > 0).count()
    }
}

/// Кадр обычного (не теневого) спрайта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteFrame {
    /// Ширина кадра в пикселях.
    pub width: u16,
    /// Высота кадра в пикселях.
    pub height: u16,
    /// Горячая точка (якорь) по x.
    pub hotx: i16,
    /// Горячая точка (якорь) по y.
    pub hoty: i16,
    /// Разобранные строки — как в файле (потерянных данных нет).
    pub rows: Vec<SpriteRow>,
    /// Пиксели кадра `width * height`, row-major: `None` — прозрачность.
    pub pixels: Vec<Option<u8>>,
}

impl SpriteFrame {
    /// Пиксель по координатам кадра; `None` за границей и на прозрачности.
    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> Option<u8> {
        if x >= u32::from(self.width) || y >= u32::from(self.height) {
            return None;
        }

        self.pixels[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// Строка кадра по индексу.
    #[inline]
    pub fn row(&self, y: usize) -> Option<&SpriteRow> {
        self.rows.get(y)
    }

    /// Число непрозрачных пикселей кадра.
    pub fn opaque_pixels(&self) -> usize {
        self.pixels.iter().filter(|pixel| pixel.is_some()).count()
    }

    /// Прямоугольник кадра с учётом горячей точки в координатах «якоря»:
    /// `ulx = point.x - hotx`, `uly = point.y - hoty` (§5.1 при `divisor = 2`).
    pub fn bounds_for_anchor(&self, x: i32, y: i32) -> (i32, i32, i32, i32) {
        let ulx = x - i32::from(self.hotx);
        let uly = y - i32::from(self.hoty);

        (
            ulx,
            uly,
            ulx + i32::from(self.width),
            uly + i32::from(self.height),
        )
    }
}

/// Мультикадровый спрайт: `u16 image_count` + кадры подряд.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteSheet {
    /// `image_count` из заголовка.
    pub image_count: u16,
    /// Кадры в порядке `frame_offset[]` (`len() == image_count`).
    pub frames: Vec<SpriteFrame>,
}

impl SpriteSheet {
    /// Кадр по индексу (`GetSpriteFrame`).
    #[inline]
    pub fn frame(&self, index: usize) -> Option<&SpriteFrame> {
        self.frames.get(index)
    }

    /// Число кадров (совпадает с `image_count`).
    #[inline]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Кадров нет (невозможно для успешно разобранного спрайта).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Кадр по индексу с обёрткой вокруг модуля 8 — раскладка кадров юнита
    /// «фаза × 8 + угол» (§5.3): `image_base + phase * 8 + angle`.
    ///
    /// Возвращает `None`, если итоговый индекс выходит за `image_count`.
    #[inline]
    pub fn directional_frame(
        &self,
        image_base: usize,
        phase: usize,
        angle: usize,
    ) -> Option<&SpriteFrame> {
        self.frames.get(image_base + phase * 8 + angle)
    }
}

/// Кадр тени: маска затемнения вместо пикселей (§4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowFrame {
    /// Ширина кадра в пикселях.
    pub width: u16,
    /// Высота кадра в пикселях.
    pub height: u16,
    /// Горячая точка (якорь) по x.
    pub hotx: i16,
    /// Горячая точка (якорь) по y.
    pub hoty: i16,
    /// Разобранные строки: пары `{skip, count}` без пиксельных байтов.
    pub rows: Vec<SpriteRow>,
    /// Маска `width * height`, row-major: `true` — затемнить пиксель карты.
    pub mask: Vec<bool>,
}

impl ShadowFrame {
    /// Затемняется ли пиксель кадра по координатам.
    #[inline]
    pub fn darkened(&self, x: u32, y: u32) -> bool {
        if x >= u32::from(self.width) || y >= u32::from(self.height) {
            return false;
        }

        self.mask[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// Число затемняемых пикселей кадра.
    pub fn darkened_pixels(&self) -> usize {
        self.mask.iter().filter(|&&cell| cell).count()
    }

    /// Прямоугольник тени с учётом горячей точки (как [`SpriteFrame::bounds_for_anchor`]).
    pub fn bounds_for_anchor(&self, x: i32, y: i32) -> (i32, i32, i32, i32) {
        let ulx = x - i32::from(self.hotx);
        let uly = y - i32::from(self.hoty);

        (
            ulx,
            uly,
            ulx + i32::from(self.width),
            uly + i32::from(self.height),
        )
    }
}

/// Мультикадровая тень (`S_*`-ресурс).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowSheet {
    /// `image_count` из заголовка.
    pub image_count: u16,
    /// Кадры в порядке `frame_offset[]`.
    pub frames: Vec<ShadowFrame>,
}

impl ShadowSheet {
    /// Кадр по индексу.
    #[inline]
    pub fn frame(&self, index: usize) -> Option<&ShadowFrame> {
        self.frames.get(index)
    }

    /// Число кадров.
    #[inline]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Кадров нет (невозможно для успешно разобранной тени).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

/// Результат [`decode`]: спрайт либо тень — режим задаётся вызывающим.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedSprite {
    /// Обычный спрайт: пиксели-индексы палитры.
    Sprite(SpriteSheet),
    /// Тень: маска затемнения.
    Shadow(ShadowSheet),
}

impl DecodedSprite {
    /// Спрайт, если тело декодировано в обычном режиме.
    pub fn as_sprite(&self) -> Option<&SpriteSheet> {
        match self {
            Self::Sprite(sheet) => Some(sheet),
            Self::Shadow(_) => None,
        }
    }

    /// Тень, если тело декодировано в режиме тени.
    pub fn as_shadow(&self) -> Option<&ShadowSheet> {
        match self {
            Self::Sprite(_) => None,
            Self::Shadow(sheet) => Some(sheet),
        }
    }

    /// Тело декодировано как тень.
    pub fn is_shadow(&self) -> bool {
        matches!(self, Self::Shadow(_))
    }
}

// ---------------------------------------------------------------------------
// Декодер
// ---------------------------------------------------------------------------

/// Декодирует тело спрайта. `shadow = true` читает строки как пары
/// `{skip, count}` без пиксельных байтов (теги `S_*`, §4.3).
///
/// Режим нельзя определить по содержимому (§3.4), поэтому он приходит извне.
pub fn decode(bytes: &[u8], shadow: bool) -> Result<DecodedSprite, SpriteError> {
    if bytes.len() < HEADER_SIZE {
        return Err(SpriteError::TooShort {
            len: bytes.len(),
            needed: HEADER_SIZE,
        });
    }

    let image_count = u16::from_le_bytes([bytes[0], bytes[1]]);

    if image_count == 0 {
        return Err(SpriteError::ZeroImages);
    }

    let table_end = HEADER_SIZE + 4 * image_count as usize;

    if bytes.len() < table_end {
        return Err(SpriteError::OffsetTableTruncated {
            image_count,
            needed: table_end,
            len: bytes.len(),
        });
    }

    let frames = if shadow {
        let mut frames = Vec::with_capacity(image_count as usize);

        for index in 0..image_count as usize {
            frames.push(decode_frame(
                bytes,
                index,
                frame_offset(bytes, index),
                true,
            )?);
        }

        DecodedSprite::Shadow(ShadowSheet {
            image_count,
            frames: frames
                .into_iter()
                .map(ShadowFrame::from_raw)
                .collect::<Vec<ShadowFrame>>(),
        })
    } else {
        let mut frames = Vec::with_capacity(image_count as usize);

        for index in 0..image_count as usize {
            frames.push(sprite_frame_from_raw(decode_frame(
                bytes,
                index,
                frame_offset(bytes, index),
                false,
            )?));
        }

        DecodedSprite::Sprite(SpriteSheet {
            image_count,
            frames,
        })
    };

    Ok(frames)
}

/// Декодирует тело обычного спрайта (теги `COMMTWR`…`WALDO`).
pub fn decode_sprite(bytes: &[u8]) -> Result<SpriteSheet, SpriteError> {
    match decode(bytes, false)? {
        DecodedSprite::Sprite(sheet) => Ok(sheet),
        DecodedSprite::Shadow(_) => unreachable!("decode(false) всегда даёт Sprite"),
    }
}

/// Декодирует тело тени (теги `S_*`).
pub fn decode_shadow(bytes: &[u8]) -> Result<ShadowSheet, SpriteError> {
    match decode(bytes, true)? {
        DecodedSprite::Shadow(sheet) => Ok(sheet),
        DecodedSprite::Sprite(_) => unreachable!("decode(true) всегда даёт Shadow"),
    }
}

#[inline]
fn frame_offset(bytes: &[u8], index: usize) -> u32 {
    let pos = HEADER_SIZE + 4 * index;

    u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
}

/// Сырой (ещё не разложенный в `pixels`/`mask`) кадр.
struct RawFrame {
    width: u16,
    height: u16,
    hotx: i16,
    hoty: i16,
    rows: Vec<SpriteRow>,
}

impl ShadowFrame {
    fn from_raw(raw: RawFrame) -> Self {
        let width = raw.width as usize;
        let height = raw.height as usize;
        let mut mask = vec![false; width * height];

        for (y, row) in raw.rows.iter().enumerate() {
            let mut x = 0usize;

            for run in &row.runs {
                x += usize::from(run.skip);

                for i in 0..usize::from(run.count) {
                    let index = y * width + x + i;

                    if let Some(cell) = mask.get_mut(index) {
                        *cell = true;
                    }
                }

                x += usize::from(run.count);
            }
        }

        Self {
            width: raw.width,
            height: raw.height,
            hotx: raw.hotx,
            hoty: raw.hoty,
            rows: raw.rows,
            mask,
        }
    }
}

fn sprite_frame_from_raw(raw: RawFrame) -> SpriteFrame {
    let width = raw.width as usize;
    let height = raw.height as usize;
    let mut pixels = vec![None; width * height];

    for (y, row) in raw.rows.iter().enumerate() {
        let mut x = 0usize;

        for run in &row.runs {
            x += usize::from(run.skip);

            for &pixel in &run.pixels {
                let index = y * width + x;

                if let Some(cell) = pixels.get_mut(index) {
                    *cell = Some(pixel);
                }

                x += 1;
            }
        }
    }

    SpriteFrame {
        width: raw.width,
        height: raw.height,
        hotx: raw.hotx,
        hoty: raw.hoty,
        rows: raw.rows,
        pixels,
    }
}

/// Разбирает один кадр: заголовок, таблицу строк и сами строки.
fn decode_frame(
    bytes: &[u8],
    index: usize,
    offset: u32,
    shadow: bool,
) -> Result<RawFrame, SpriteError> {
    let start = offset as usize;

    if start + FRAME_HEADER_SIZE > bytes.len() {
        return Err(SpriteError::FrameOutOfBounds {
            frame: index,
            offset,
            len: bytes.len(),
        });
    }

    let width = i16::from_le_bytes([bytes[start], bytes[start + 1]]);
    let height = i16::from_le_bytes([bytes[start + 2], bytes[start + 3]]);
    let hotx = i16::from_le_bytes([bytes[start + 4], bytes[start + 5]]);
    let hoty = i16::from_le_bytes([bytes[start + 6], bytes[start + 7]]);

    if width <= 0 || height <= 0 {
        return Err(SpriteError::BadDimensions {
            frame: index,
            width,
            height,
        });
    }

    let width = width as u16;
    let height = height as u16;
    let pixels = usize::from(width) * usize::from(height);

    if pixels > MAX_FRAME_PIXELS {
        return Err(SpriteError::FrameTooLarge {
            frame: index,
            width,
            height,
        });
    }

    let header_end = start + FRAME_HEADER_SIZE + 4 * height as usize;

    if header_end > bytes.len() {
        return Err(SpriteError::FrameTruncated {
            frame: index,
            offset,
            needed: header_end,
            len: bytes.len(),
        });
    }

    let mut rows = Vec::with_capacity(height as usize);

    for y in 0..height as usize {
        let pos = start + FRAME_HEADER_SIZE + 4 * y;
        let row_offset =
            u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);

        if row_offset as usize >= bytes.len() {
            return Err(SpriteError::RowOutOfBounds {
                frame: index,
                row: y,
                offset: row_offset,
                len: bytes.len(),
            });
        }

        rows.push(decode_row(bytes, index, y, row_offset, width, shadow)?);
    }

    Ok(RawFrame {
        width,
        height,
        hotx,
        hoty,
        rows,
    })
}

/// Разбирает строку до терминатора `0xFF` (§4.1 / §4.3).
fn decode_row(
    bytes: &[u8],
    frame: usize,
    row: usize,
    row_offset: u32,
    width: u16,
    shadow: bool,
) -> Result<SpriteRow, SpriteError> {
    let len = bytes.len();
    let mut pos = row_offset as usize;
    let mut x = 0u32;
    let mut runs = Vec::new();

    loop {
        if pos >= len {
            return Err(SpriteError::RowUnterminated {
                frame,
                row,
                offset: row_offset,
                len,
            });
        }

        if bytes[pos] == ROW_DELIMITER {
            break;
        }

        if pos + 1 >= len {
            return Err(SpriteError::RunHeaderTruncated {
                frame,
                row,
                offset: pos,
                len,
            });
        }

        let skip = bytes[pos];
        let count = bytes[pos + 1];

        pos += 2;
        x += u32::from(skip);

        let pixels = if shadow {
            Vec::new()
        } else {
            let end = pos + usize::from(count);

            if end > len {
                return Err(SpriteError::PixelPayloadTruncated {
                    frame,
                    row,
                    offset: pos,
                    len,
                });
            }

            let pixels = bytes[pos..end].to_vec();

            pos = end;

            pixels
        };

        x += u32::from(count);

        if x > u32::from(width) {
            return Err(SpriteError::RowOverrun {
                frame,
                row,
                x,
                width,
            });
        }

        runs.push(SpriteRun {
            skip,
            count,
            pixels,
        });
    }

    Ok(SpriteRow { runs, painted: x })
}

// ---------------------------------------------------------------------------
// Тесты
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Собирает тело спрайта из кадров вида `(width, height, hotx, hoty, rows)`,
    /// где `rows` — уже закодированные строки (байты вместе с терминатором).
    fn build_sprite(frames: &[(u16, u16, i16, i16, Vec<Vec<u8>>)]) -> Vec<u8> {
        let image_count = frames.len() as u16;
        let mut out = Vec::new();

        out.extend_from_slice(&image_count.to_le_bytes());
        // Заглушки смещений кадров, заполняются ниже.
        out.extend(std::iter::repeat_n(0u8, 4 * frames.len()));

        for (index, (width, height, hotx, hoty, rows)) in frames.iter().enumerate() {
            assert_eq!(rows.len(), *height as usize);

            let frame_offset = out.len() as u32;
            out[HEADER_SIZE + 4 * index..HEADER_SIZE + 4 * index + 4]
                .copy_from_slice(&frame_offset.to_le_bytes());

            out.extend_from_slice(&width.to_le_bytes());
            out.extend_from_slice(&height.to_le_bytes());
            out.extend_from_slice(&hotx.to_le_bytes());
            out.extend_from_slice(&hoty.to_le_bytes());

            let row_table_start = out.len();
            out.extend(std::iter::repeat_n(0u8, 4 * rows.len()));

            for (y, row) in rows.iter().enumerate() {
                let row_offset = out.len() as u32;
                out[row_table_start + 4 * y..row_table_start + 4 * y + 4]
                    .copy_from_slice(&row_offset.to_le_bytes());
                out.extend_from_slice(row);
            }
        }

        out
    }

    /// `{skip, count, pixels[]}`.
    fn run(skip: u8, pixels: &[u8]) -> Vec<u8> {
        let mut out = vec![skip, pixels.len() as u8];

        out.extend_from_slice(pixels);
        out
    }

    #[test]
    fn decodes_rows_with_transparent_tail_and_hot_spot() {
        // width = 6, row = {skip 1, count 2, [A, B]}, {skip 1, count 2, [C, D]} -> x = 6? нет: 1+2+1+2 = 6.
        let mut row = run(1, &[0xAA, 0xBB]);

        row.extend(run(1, &[0xCC, 0xDD]));
        row.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(6, 1, 3, 4, vec![row])]);
        let sheet = decode_sprite(&bytes).unwrap();

        assert_eq!(sheet.image_count, 1);
        assert_eq!(sheet.len(), 1);

        let frame = sheet.frame(0).unwrap();

        assert_eq!(
            (frame.width, frame.height, frame.hotx, frame.hoty),
            (6, 1, 3, 4)
        );
        assert_eq!(
            frame.pixels,
            vec![None, Some(0xAA), Some(0xBB), None, Some(0xCC), Some(0xDD)]
        );
        assert_eq!(frame.opaque_pixels(), 4);
        assert_eq!(frame.rows[0].painted, 6);
        // height = 1, hot = (3, 4), точка (10, 20) -> кадр (7, 16)..(13, 17).
        assert_eq!(frame.bounds_for_anchor(10, 20), (7, 16, 13, 17));
    }

    #[test]
    fn tail_shorter_than_width_is_transparent() {
        // Реальный edge-кейс TANK §3.1: сумма счётчиков меньше width.
        let mut row = run(1, &[0x11]);

        row.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(5, 1, 0, 0, vec![row])]);
        let frame = &decode_sprite(&bytes).unwrap().frames[0];

        assert_eq!(frame.pixels, vec![None, Some(0x11), None, None, None]);
        assert_eq!(frame.rows[0].painted, 2);
    }

    #[test]
    fn ff_inside_pixel_payload_is_a_palette_index() {
        // 0xFF допустим внутри поля пикселей: декодер читает ровно count байт.
        let mut row = run(0, &[ROW_DELIMITER, 0x00, ROW_DELIMITER]);

        row.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(3, 1, 0, 0, vec![row])]);
        let frame = &decode_sprite(&bytes).unwrap().frames[0];

        assert_eq!(frame.pixels, vec![Some(0xFF), Some(0x00), Some(0xFF)]);
    }

    #[test]
    fn zero_count_run_only_skips() {
        let mut row = run(2, &[]);

        row.extend(run(1, &[0x42]));
        row.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(4, 1, 0, 0, vec![row])]);
        let frame = &decode_sprite(&bytes).unwrap().frames[0];

        assert_eq!(frame.pixels, vec![None, None, None, Some(0x42)]);
        assert_eq!(frame.rows[0].runs.len(), 2);
        assert_eq!(frame.rows[0].runs[0].span(), 2);
    }

    #[test]
    fn shadow_rows_are_pairs_of_counters() {
        // S_TANK §3.2: {4, 5}, {25, 5}, 0xFF при width = 49.
        let mut row = vec![4, 5, 25, 5];

        row.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(49, 1, 20, 23, vec![row])]);
        let sheet = decode_shadow(&bytes).unwrap();
        let frame = sheet.frame(0).unwrap();

        assert_eq!(frame.width, 49);
        assert_eq!((frame.hotx, frame.hoty), (20, 23));
        assert_eq!(frame.darkened_pixels(), 10);
        // {4, 5}: x = 4..=8; {25, 5}: x = 4 + 5 + 25 = 34..=38.
        assert!(frame.darkened(4, 0));
        assert!(frame.darkened(8, 0));
        assert!(frame.darkened(34, 0));
        assert!(frame.darkened(38, 0));
        assert!(!frame.darkened(3, 0));
        assert!(!frame.darkened(9, 0));
        assert!(!frame.darkened(33, 0));
        assert!(!frame.darkened(39, 0));
        assert!(!frame.darkened(48, 0));
        assert!(frame.rows[0].runs.iter().all(SpriteRun::is_shadow));
        assert!(frame.rows[0].runs.iter().all(|run| run.pixels.is_empty()));
    }

    #[test]
    fn multi_frame_offsets_are_absolute() {
        let mut first = run(0, &[1, 2]);

        first.push(ROW_DELIMITER);

        let mut second = run(0, &[3]);

        second.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(2, 1, 0, 0, vec![first]), (1, 1, 7, 8, vec![second])]);
        let sheet = decode_sprite(&bytes).unwrap();

        assert_eq!(sheet.len(), 2);
        assert_eq!(sheet.frames[0].pixels, vec![Some(1), Some(2)]);
        assert_eq!(sheet.frames[1].pixels, vec![Some(3)]);
        assert_eq!((sheet.frames[1].hotx, sheet.frames[1].hoty), (7, 8));

        // Кадры юнитов раскладываются «фаза × 8 + угол» (§5.3).
        assert!(sheet.directional_frame(0, 0, 0).is_some());
        assert!(sheet.directional_frame(0, 1, 0).is_none());
    }

    #[test]
    fn decode_returns_the_requested_mode() {
        let mut row = run(0, &[9, 9]);

        row.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(2, 1, 0, 0, vec![row])]);

        assert!(decode(&bytes, false).unwrap().as_sprite().is_some());
        assert!(!decode(&bytes, false).unwrap().is_shadow());
        assert_eq!(
            decode(&bytes, false)
                .unwrap()
                .as_sprite()
                .unwrap()
                .image_count,
            1
        );

        // Та же ширина, но строка тени: пары {0, 2} без пиксельных байтов.
        let mut shadow_row = vec![0, 2];

        shadow_row.push(ROW_DELIMITER);

        let shadow_bytes = build_sprite(&[(2, 1, 0, 0, vec![shadow_row])]);
        let shadow = decode(&shadow_bytes, true).unwrap();

        assert!(shadow.is_shadow());
        assert!(shadow.as_sprite().is_none());
        assert_eq!(
            shadow
                .as_shadow()
                .unwrap()
                .frame(0)
                .unwrap()
                .darkened_pixels(),
            2
        );
    }

    #[test]
    fn structural_errors_do_not_panic() {
        assert!(matches!(
            decode_sprite(b""),
            Err(SpriteError::TooShort { .. })
        ));
        assert!(matches!(
            decode_sprite(&0u16.to_le_bytes()),
            Err(SpriteError::ZeroImages)
        ));

        // image_count = 5, а таблица обрезана.
        let mut truncated = vec![5, 0, 0, 0];

        assert!(matches!(
            decode_sprite(&truncated),
            Err(SpriteError::OffsetTableTruncated { .. })
        ));

        // Кадр за концом тела.
        truncated = vec![1, 0, 0xFF, 0x00, 0x00, 0x00];

        assert!(matches!(
            decode_sprite(&truncated),
            Err(SpriteError::FrameOutOfBounds { .. })
        ));

        // Нулевая ширина кадра.
        let zero_width = build_sprite(&[(0, 1, 0, 0, vec![vec![ROW_DELIMITER]])]);

        assert!(matches!(
            decode_sprite(&zero_width),
            Err(SpriteError::BadDimensions { .. })
        ));

        // Строка без терминатора: доходит до конца тела.
        // height = 1, прогон {skip 0, count 1, 0xAA} и ни одного 0xFF в конце.
        let broken = build_sprite(&[(1, 1, 0, 0, vec![vec![0u8, 1, 0xAA]])]);

        assert!(matches!(
            decode_sprite(&broken),
            Err(SpriteError::RowUnterminated { .. })
        ));

        // Строка выходит за width.
        let mut overrun = run(3, &[1, 2, 3]);

        overrun.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(5, 1, 0, 0, vec![overrun])]);

        assert!(matches!(
            decode_sprite(&bytes),
            Err(SpriteError::RowOverrun { .. })
        ));

        // Обрезанное поле пикселей.
        let mut clipped = vec![0u8, 5, 1, 2]; // count = 5, а байтов только 2

        clipped.push(ROW_DELIMITER);

        let bytes = build_sprite(&[(5, 1, 0, 0, vec![clipped])]);

        assert!(matches!(
            decode_sprite(&bytes),
            Err(SpriteError::PixelPayloadTruncated { .. })
        ));
    }
}
