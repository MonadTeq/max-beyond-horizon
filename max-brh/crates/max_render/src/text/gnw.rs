//! Шрифты GNW оригинала: `GNW_TEXT_FONT_1/2/5` из `FONT1.FON`/`FONT2.FON`/
//! `FONT5.FON`.
//!
//! # Откуда данные
//!
//! C++-порт рисует текст через FreeType из TTF-ресурсов `FONT_1`/`FONT_2`/
//! `FONT_5` (`PATCHES.RES`), а те TTF были сконвертированы из **растровых
//! шрифтов GNW** `FONT1.FON`/`FONT2.FON`/`FONT5.FON` (файлы рядом с `MAX.RES`).
//! Оригинальная игра рисовала именно эти битмапы. Проверено инструментом
//! [`tools/font_vectors`](../../../../tools/font_vectors): для ASCII оба пути
//! (FreeType по TTF и декодер `.FON`) дают **побайтово одинаковые** битмапы
//! строк `M.A.X.`, `Hits 15/15`, `End Turn`, `SURVEY STATUS COLORS`, `044-034`
//! для всех трёх шрифтов.
//!
//! # Формат `.FON` (GNW)
//!
//! ```text
//! +0x00  u32 version        (0x00000100)
//! +0x04  u32 height         16 / 9 / 10 — высота ячейки глифа
//! +0x08  u32                0
//! +0x0C  u8[16]             подпись/резерв (0xFF,0xFF,0xF0,0x05,…)
//! +0x1C  255 × { u32 width; u32 offset }   глифы символов 1…255
//! +0x81C (2068)            битмапы: строки по ceil(width/8) байт, MSB-first,
//!                          бит 1 = «чернила»; offset — от начала этой зоны
//! ```
//!
//! Глиф символа 0 — пустой, шириной 0, лежит в первых `height` байтах зоны
//! битмапов (поэтому первый `offset` равен `height`). Сумма размеров глифов
//! равна длине файла — декодер проверяет это и не паникует на битых данных.
//!
//! # Метрики
//!
//! * ширина глифа = `advance` (проверено по `hmtx` TTF: совпадает для всех
//!   ASCII-символов `FONT_2`/`FONT_5` и для 91 из 95 у `FONT_1`; у `,`/`@`/`Z`/
//!   `}` расходятся на 1 px — там TTF-хинтинг, см. `vectors.txt`);
//! * высота строки — `height` (`Text_GetHeight`), межстрочный интервал в
//!   `Text_TextBox` — `height + 1`;
//! * `Text_TextLine` рисует тень/обводку вправо-вниз: для каждого пикселя
//!   текста `(x+1, y+1)` красится в `outline`, если там тоже текст, иначе в
//!   `shadow`. Это воспроизводит [`draw_text_gnw_shadowed`].

use std::sync::OnceLock;

use crate::buffer::{PixelRect, Rgba, RgbaBuffer};

/// Заголовок `.FON` занимает 28 байт.
pub const GNW_HEADER: usize = 28;
/// Глифов в таблице (символы `1..=255`; нулевой — пустой).
pub const GNW_GLYPHS: usize = 255;
/// Размер записи глифа: `width` + `offset`.
pub const GNW_RECORD: usize = 8;
/// Начало зоны битмапов: 28 + 255·8.
pub const GNW_BITMAP_BASE: usize = GNW_HEADER + GNW_GLYPHS * GNW_RECORD;
/// Ширина глифа, которым заменяется отсутствующий символ.
pub const GNW_FALLBACK: char = '?';

/// Оригинальный `.FON`, вкомпилированный в `max_render` (из `max-original/`).
const FONT_1_BYTES: &[u8] = include_bytes!("../../data/gnw_font_1.fnt");
/// Оригинальный `.FON` `FONT2.FON` (9 px, только заглавные).
const FONT_2_BYTES: &[u8] = include_bytes!("../../data/gnw_font_2.fnt");
/// Оригинальный `.FON` `FONT5.FON` (10 px).
const FONT_5_BYTES: &[u8] = include_bytes!("../../data/gnw_font_5.fnt");

/// Какой из шрифтов GNW нужен.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GnwFontId {
    /// `GNW_TEXT_FONT_1` — `FONT1.FON`, 16 px (заголовки/меню).
    Font1,
    /// `GNW_TEXT_FONT_2` — `FONT2.FON`, 9 px (подписи кнопок, статистика).
    Font2,
    /// `GNW_TEXT_FONT_5` — `FONT5.FON`, 10 px (End Turn, счётчики, координаты).
    Font5,
}

impl GnwFontId {
    /// Все шрифты, которые портированы.
    pub const ALL: [Self; 3] = [Self::Font1, Self::Font2, Self::Font5];

    /// Высота ячейки глифа (`Text_GetHeight`).
    pub const fn height(self) -> i32 {
        match self {
            Self::Font1 => 16,
            Self::Font2 => 9,
            Self::Font5 => 10,
        }
    }

    /// Исходные байты `.FON`.
    pub const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Font1 => FONT_1_BYTES,
            Self::Font2 => FONT_2_BYTES,
            Self::Font5 => FONT_5_BYTES,
        }
    }

    /// Имя файла оригинала (для диагностики).
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Font1 => "FONT1.FON",
            Self::Font2 => "FONT2.FON",
            Self::Font5 => "FONT5.FON",
        }
    }

    /// Разобранный шрифт (декодируется один раз на процесс).
    pub fn font(self) -> &'static GnwFont {
        static FONT_1: OnceLock<GnwFont> = OnceLock::new();
        static FONT_2: OnceLock<GnwFont> = OnceLock::new();
        static FONT_5: OnceLock<GnwFont> = OnceLock::new();

        let cell = match self {
            Self::Font1 => &FONT_1,
            Self::Font2 => &FONT_2,
            Self::Font5 => &FONT_5,
        };

        cell.get_or_init(|| {
            GnwFont::decode(self.bytes()).unwrap_or_else(|error| {
                panic!("встроенный {} не разобрался: {error}", self.file_name())
            })
        })
    }
}

/// Ошибка разбора `.FON`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GnwFontError {
    /// Файл короче заголовка и таблицы глифов.
    TooShort {
        /// Фактическая длина.
        len: usize,
        /// Минимально необходимая.
        needed: usize,
    },
    /// Высота шрифта нулевая или неразумная.
    BadHeight {
        /// Значение из заголовка.
        height: u32,
    },
    /// Запись глифа противоречит последовательной раскладке битмапов.
    BadRecord {
        /// Номер глифа (1-based).
        glyph: usize,
        /// Ширина из записи.
        width: u32,
        /// Смещение из записи.
        offset: u32,
        /// Ожидаемое смещение.
        expected: u32,
    },
    /// Битмапы выходят за конец файла.
    Truncated {
        /// Нужно байт от начала файла.
        needed: usize,
        /// Есть байт.
        len: usize,
    },
}

impl std::fmt::Display for GnwFontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort { len, needed } => {
                write!(f, "шрифт GNW обрезан: {len} байт (нужно минимум {needed})")
            }
            Self::BadHeight { height } => write!(f, "некорректная высота шрифта GNW: {height}"),
            Self::BadRecord {
                glyph,
                width,
                offset,
                expected,
            } => write!(
                f,
                "глиф {glyph}: ширина {width}, смещение {offset} (ожидалось {expected})"
            ),
            Self::Truncated { needed, len } => {
                write!(f, "битмапы шрифта GNW выходят за файл: нужно {needed}, есть {len}")
            }
        }
    }
}

impl std::error::Error for GnwFontError {}

/// Метрики одного глифа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GnwGlyph {
    /// Ширина в пикселях (= advance).
    pub width: u16,
    /// Высота строки глифа (= высота шрифта).
    pub height: u16,
    /// Байт на строку битмапа (`ceil(width/8)`).
    pub stride: u16,
    /// Смещение битмапа от [`GnwFont::bitmap_base`].
    pub offset: u32,
    /// Размер битмапа (`stride * height`).
    pub size: u32,
}

/// Разобранный растровый шрифт GNW.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GnwFont {
    height: u16,
    glyphs: Vec<GnwGlyph>,
    bitmap: Vec<u8>,
}

impl GnwFont {
    /// Разбирает `.FON` GNW.
    ///
    /// Проверяет, что записи глифов идут подряд (`offset` равен накопленному
    /// размеру) и что битмапы не выходят за файл. Формат — см. модульную
    /// документацию.
    pub fn decode(bytes: &[u8]) -> Result<Self, GnwFontError> {
        if bytes.len() < GNW_BITMAP_BASE {
            return Err(GnwFontError::TooShort {
                len: bytes.len(),
                needed: GNW_BITMAP_BASE,
            });
        }

        let height = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if height == 0 || height > 64 {
            return Err(GnwFontError::BadHeight { height });
        }

        let mut glyphs = Vec::with_capacity(GNW_GLYPHS + 1);
        // Глиф 0 — пустой (в файле он занимает `height` байт в начале зоны).
        glyphs.push(GnwGlyph {
            width: 0,
            height: height as u16,
            stride: 0,
            offset: 0,
            size: height,
        });

        let mut cursor = height;

        for index in 0..GNW_GLYPHS {
            let record = GNW_HEADER + index * GNW_RECORD;
            let width = u32::from_le_bytes([
                bytes[record],
                bytes[record + 1],
                bytes[record + 2],
                bytes[record + 3],
            ]);
            let offset = u32::from_le_bytes([
                bytes[record + 4],
                bytes[record + 5],
                bytes[record + 6],
                bytes[record + 7],
            ]);

            if width > u32::from(u16::MAX) || offset != cursor {
                return Err(GnwFontError::BadRecord {
                    glyph: index + 1,
                    width,
                    offset,
                    expected: cursor,
                });
            }

            let stride = width.div_ceil(8);
            let size = stride * height;
            glyphs.push(GnwGlyph {
                width: width as u16,
                height: height as u16,
                stride: stride as u16,
                offset,
                size,
            });
            cursor += size;
        }

        let needed = GNW_BITMAP_BASE + cursor as usize;
        if needed > bytes.len() {
            return Err(GnwFontError::Truncated {
                needed,
                len: bytes.len(),
            });
        }

        Ok(Self {
            height: height as u16,
            glyphs,
            bitmap: bytes[GNW_BITMAP_BASE..needed].to_vec(),
        })
    }

    /// Высота ячейки глифа (`Text_GetHeight`).
    pub fn height(&self) -> i32 {
        i32::from(self.height)
    }

    /// Межстрочный интервал оригинала (`Text_TextBox`/`Text_TextLine`: height+1).
    pub fn line_height(&self) -> i32 {
        i32::from(self.height) + 1
    }

    /// Число глифов в таблице (256, включая пустой нулевой).
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// Начало зоны битмапов (для тестов/диагностики).
    pub fn bitmap_base(&self) -> usize {
        GNW_BITMAP_BASE
    }

    /// Метрики символа; вне Latin-1 — глиф [`GNW_FALLBACK`].
    pub fn glyph(&self, ch: char) -> GnwGlyph {
        let code = ch as usize;
        let index = if code <= GNW_GLYPHS {
            code
        } else {
            GNW_FALLBACK as usize
        };

        self.glyphs[index]
    }

    /// Битмап символа (строки по `stride` байт, MSB-first).
    pub fn glyph_bitmap(&self, ch: char) -> &[u8] {
        let glyph = self.glyph(ch);
        let start = glyph.offset as usize;
        &self.bitmap[start..start + glyph.size as usize]
    }

    /// Пиксель глифа (`true` — «чернила»).
    pub fn glyph_ink(&self, ch: char, x: i32, y: i32) -> bool {
        let glyph = self.glyph(ch);

        if x < 0 || y < 0 || x >= i32::from(glyph.width) || y >= i32::from(glyph.height) {
            return false;
        }

        let row = y as usize * usize::from(glyph.stride);
        let byte = self.glyph_bitmap(ch)[row + x as usize / 8];

        byte & (0x80 >> (x as usize % 8)) != 0
    }

    /// Ширина символа (advance).
    pub fn char_width(&self, ch: char) -> i32 {
        i32::from(self.glyph(ch).width)
    }

    /// Ширина строки в пикселях (`Text_GetWidth`).
    pub fn text_width(&self, text: &str) -> i32 {
        text.chars().map(|ch| self.char_width(ch)).sum()
    }
}

/// Рисует строку шрифтом GNW в `(x, y)` (левый верх ячейки).
///
/// Возвращает ширину строки в пикселях. Рисование обрезается границами
/// буфера; вне Latin-1 используется `?`.
pub fn draw_text_gnw(
    buffer: &mut RgbaBuffer,
    x: i32,
    y: i32,
    font: &GnwFont,
    text: &str,
    color: Rgba,
) -> i32 {
    let mut pen_x = x;

    for ch in text.chars() {
        let glyph = font.glyph(ch);
        let bitmap = font.glyph_bitmap(ch);
        let width = i32::from(glyph.width);

        if pen_x >= buffer.width() as i32 {
            // Пен только растёт вправо: дальше ничего не будет видно.
            break;
        }

        for row in 0..i32::from(glyph.height) {
            let pixel_y = y + row;

            if pixel_y < 0 || pixel_y >= buffer.height() as i32 {
                continue;
            }

            let start = row as usize * usize::from(glyph.stride);

            for column in 0..width {
                let byte = bitmap[start + column as usize / 8];

                if byte & (0x80 >> (column as usize % 8)) == 0 {
                    continue;
                }

                buffer.set(pen_x + column, pixel_y, color);
            }
        }

        pen_x += width;
    }

    font.text_width(text)
}

/// То же, что [`draw_text_gnw`], но с тенью/обводкой как `Text_TextLine`.
///
/// Для каждого «чернильного» пикселя `(x, y)` оригинал красит `(x+1, y+1)`:
/// `outline`, если там тоже текст, иначе `shadow`. Порядок обхода — снизу
/// вверх, поэтому ранее перекрашенный пиксель уже не считается текстом (как
/// в C++, где цикл читает и пишет один буфер).
#[allow(clippy::too_many_arguments)]
pub fn draw_text_gnw_shadowed(
    buffer: &mut RgbaBuffer,
    x: i32,
    y: i32,
    font: &GnwFont,
    text: &str,
    base: Rgba,
    outline: Rgba,
    shadow: Rgba,
) -> i32 {
    let width = font.text_width(text) + 1;
    let height = font.height() + 1;
    let stride = width.max(1) as usize;

    // 0 — пусто, 1 — base, 2 — outline, 3 — shadow.
    let mut mask = vec![0u8; stride * height.max(1) as usize];
    let mut pen_x = 0i32;

    for ch in text.chars() {
        let glyph = font.glyph(ch);
        let bitmap = font.glyph_bitmap(ch);

        for row in 0..i32::from(glyph.height) {
            let start = row as usize * usize::from(glyph.stride);

            for column in 0..i32::from(glyph.width) {
                let byte = bitmap[start + column as usize / 8];

                if byte & (0x80 >> (column as usize % 8)) == 0 {
                    continue;
                }

                let px = pen_x + column;

                if px >= 0 && px < width && row < height {
                    mask[row as usize * stride + px as usize] = 1;
                }
            }
        }

        pen_x += i32::from(glyph.width);
    }

    for row in (0..height - 1).rev() {
        for column in 0..width - 1 {
            let index = row as usize * stride + column as usize;

            if mask[index] != 1 {
                continue;
            }

            let target = index + stride + 1;

            mask[target] = if mask[target] == 1 { 2 } else { 3 };
        }
    }

    for row in 0..height {
        for column in 0..width {
            let value = mask[row as usize * stride + column as usize];

            if value == 0 {
                continue;
            }

            let color = match value {
                1 => base,
                2 => outline,
                _ => shadow,
            };

            buffer.set(x + column, y + row, color);
        }
    }

    font.text_width(text)
}

/// Рисует одну строку по центру прямоугольника (как `Text_TextBox` с
/// `horizontal_align`, `vertical_align`).
///
/// Возвращает нарисованную ширину строки.
pub fn draw_text_gnw_centered(
    buffer: &mut RgbaBuffer,
    rect: PixelRect,
    font: &GnwFont,
    text: &str,
    color: Rgba,
) -> i32 {
    let text_width = font.text_width(text);
    let x = rect.ulx + (rect.width - (text_width + 1)).max(0) / 2;
    let y = rect.uly + ((rect.height + 1) - font.height()).max(0) / 2;

    draw_text_gnw(buffer, x, y, font, text, color)
}

/// Центрированная строка с тенью (`Text_TextLine` + `Button::SetCaption`).
#[allow(clippy::too_many_arguments)]
pub fn draw_text_gnw_centered_shadowed(
    buffer: &mut RgbaBuffer,
    rect: PixelRect,
    font: &GnwFont,
    text: &str,
    base: Rgba,
    outline: Rgba,
    shadow: Rgba,
) -> i32 {
    let text_width = font.text_width(text);
    let x = rect.ulx + (rect.width - (text_width + 1)).max(0) / 2;
    let y = rect.uly + ((rect.height + 1) - font.height()).max(0) / 2;

    draw_text_gnw_shadowed(buffer, x, y, font, text, base, outline, shadow)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Разбор эталонных рендеров, сгенерированных C++-инструментом.
    fn reference_render() -> Vec<(&'static str, String, usize, Vec<&'static str>)> {
        let text = include_str!("../../../../tools/font_vectors/render.txt");
        let mut out = Vec::new();
        let mut lines = text.lines();

        while let Some(line) = lines.next() {
            if !line.starts_with("render ") {
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            let font = parts[1];
            let string = parts[3..parts.len() - 2].join(" ");
            let string = string.trim_matches('"').to_string();
            let width: usize = parts[parts.len() - 2]
                .trim_start_matches("width=")
                .parse()
                .unwrap();
            let height: usize = parts[parts.len() - 1]
                .trim_start_matches("height=")
                .parse()
                .unwrap();
            let mut rows = Vec::new();

            for _ in 0..height {
                let row = lines.next().expect("строка эталона");
                rows.push(row.trim_start());
            }

            assert_eq!(rows[0].len(), width);
            out.push((font, string, width, rows));
        }

        out
    }

    fn font_by_label(label: &str) -> &'static GnwFont {
        match label {
            "font1" => GnwFontId::Font1.font(),
            "font2" => GnwFontId::Font2.font(),
            "font5" => GnwFontId::Font5.font(),
            other => panic!("неизвестный шрифт эталона: {other}"),
        }
    }

    #[test]
    fn embedded_fonts_decode_with_original_heights() {
        for id in GnwFontId::ALL {
            let font = id.font();
            assert_eq!(font.height(), id.height());
            assert_eq!(font.line_height(), id.height() + 1);
            assert_eq!(font.glyph_count(), 256);
            assert_eq!(font.bitmap_base(), 2068);
        }
    }

    #[test]
    fn glyph_metric_and_advance_match_original_tables() {
        let font1 = GnwFontId::Font1.font();
        let font5 = GnwFontId::Font5.font();

        // Ширины из FONT1.FON/vectors.txt (совпадают с hmtx TTF).
        assert_eq!(font1.char_width('A'), 13);
        assert_eq!(font1.char_width('B'), 10);
        assert_eq!(font1.char_width('M'), 13);
        assert_eq!(font1.char_width(' '), 12);
        assert_eq!(font1.text_width("M.A.X."), 45);

        assert_eq!(font5.char_width('H'), 9);
        assert_eq!(font5.char_width('i'), 4);
        assert_eq!(font5.char_width('0'), 7);
        assert_eq!(font5.char_width(' '), 7);
        assert_eq!(font5.text_width("End Turn"), 59);
    }

    #[test]
    fn collision_and_control_characters_use_the_fallback() {
        let font = GnwFontId::Font5.font();
        let fallback = font.glyph(GNW_FALLBACK);
        let empty = font.glyph('\u{0}');

        assert_eq!(font.glyph('Ж'), fallback);
        assert_eq!(empty.width, 0, "нулевой символ — пустой глиф");
        assert_eq!(empty.size, u32::from(empty.height));
        assert_eq!(font.glyph('Ж').width, fallback.width);
        assert_eq!(font.char_width('Ж'), i32::from(fallback.width));
    }

    #[test]
    fn font2_is_caps_only_like_the_original() {
        let font = GnwFontId::Font2.font();
        let upper = font.glyph_bitmap('A').to_vec();

        // FONT2.FON хранит в нижнем регистре те же заглавные глифы.
        assert_eq!(font.glyph_bitmap('a'), upper.as_slice());
        assert_eq!(font.glyph('a').width, font.glyph('A').width);
    }

    /// Сильный тест: каждая строка, отрисованная нашим декодером, совпадает
    /// пиксель-в-пиксель с эталоном C++-инструмента (FreeType-путь порта,
    /// который, в свою очередь, совпал с `.FON`).
    #[test]
    fn rendering_matches_the_cpp_reference_bitmaps() {
        let references = reference_render();

        assert!(references.len() >= 10, "эталон содержит все три шрифта");

        for (label, string, width, rows) in references {
            let font = font_by_label(label);
            let height = rows.len();
            let mut buffer = RgbaBuffer::filled(width as u32, height as u32, Rgba::rgb(0, 0, 0));

            draw_text_gnw(&mut buffer, 0, 0, font, &string, Rgba::WHITE);

            for (y, row) in rows.iter().enumerate() {
                for (x, expected) in row.chars().enumerate() {
                    let ink = buffer.get(x as i32, y as i32) == Some(Rgba::WHITE);

                    assert_eq!(
                        ink,
                        expected == '#',
                        "{label} {string:?} ({x},{y}): эталон {expected:?}"
                    );
                }
            }
        }
    }

    /// Декодер встроенных шрифтов совпадает с независимым разбором реальных
    /// `FONT*.FON` (если данные игры есть рядом с репозиторием).
    #[test]
    fn embedded_fonts_match_the_original_files() {
        for id in GnwFontId::ALL {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../max-original")
                .join(id.file_name());

            if !path.is_file() {
                continue;
            }

            let bytes = std::fs::read(&path).expect("FONT*.FON");
            let decoded = GnwFont::decode(&bytes).expect("шрифт разбирается");
            let embedded = id.font();

            assert_eq!(decoded.height(), embedded.height());

            for ch in 1u8..=255 {
                let ch = char::from(ch);

                assert_eq!(
                    decoded.glyph_bitmap(ch),
                    embedded.glyph_bitmap(ch),
                    "{}: глиф {ch:?}",
                    id.file_name()
                );
            }
        }
    }

    #[test]
    fn decoder_rejects_truncated_and_misaligned_files() {
        let bytes = GnwFontId::Font5.bytes();

        assert!(matches!(
            GnwFont::decode(&bytes[..100]),
            Err(GnwFontError::TooShort { .. })
        ));

        let mut broken = bytes.to_vec();
        broken[32..36].copy_from_slice(&999u32.to_le_bytes());
        assert!(matches!(
            GnwFont::decode(&broken),
            Err(GnwFontError::BadRecord { .. })
        ));

        let mut short = bytes.to_vec();
        short.truncate(bytes.len() - 1);
        assert!(matches!(
            GnwFont::decode(&short),
            Err(GnwFontError::Truncated { .. })
        ));
    }

    #[test]
    fn drawing_clips_without_panicking() {
        let font = GnwFontId::Font5.font();
        let mut buffer = RgbaBuffer::new(16, 12);

        draw_text_gnw(&mut buffer, -50, -50, font, "M.A.X.", Rgba::WHITE);
        draw_text_gnw(&mut buffer, 100, 100, font, "M.A.X.", Rgba::WHITE);
        draw_text_gnw(&mut buffer, 0, 0, font, "", Rgba::WHITE);
        draw_text_gnw_shadowed(
            &mut buffer,
            -2,
            8,
            font,
            "End Turn",
            Rgba::WHITE,
            Rgba::rgb(1, 2, 3),
            Rgba::rgb(4, 5, 6),
        );

        assert!(buffer.pixels().chunks_exact(4).any(|pixel| pixel[3] != 0));
    }

    #[test]
    fn shadowed_text_adds_outline_and_shadow_pixels() {
        let font = GnwFontId::Font5.font();
        let text = "H";
        let width = font.text_width(text) + 2;
        let height = font.height() + 2;
        let base = Rgba::rgb(200, 200, 200);
        let outline = Rgba::rgb(10, 20, 30);
        let shadow = Rgba::rgb(40, 50, 60);
        let mut plain = RgbaBuffer::new(width as u32, height as u32);
        let mut shadowed = RgbaBuffer::new(width as u32, height as u32);

        draw_text_gnw(&mut plain, 0, 0, font, text, base);
        draw_text_gnw_shadowed(&mut shadowed, 0, 0, font, text, base, outline, shadow);

        let count = |buffer: &RgbaBuffer, color: Rgba| {
            buffer
                .pixels()
                .chunks_exact(4)
                .filter(|pixel| pixel[0] == color.r && pixel[1] == color.g && pixel[2] == color.b)
                .count()
        };

        assert!(count(&shadowed, outline) > 0, "есть обводка");
        assert!(count(&shadowed, shadow) > 0, "есть тень");
        assert!(count(&shadowed, base) > 0, "есть сам текст");
        assert!(
            count(&shadowed, base) + count(&shadowed, outline) + count(&shadowed, shadow)
                > count(&plain, base),
            "тень расширяет область текста"
        );
    }

    #[test]
    fn centered_text_stays_inside_the_rect() {
        let font = GnwFontId::Font2.font();
        let mut buffer = RgbaBuffer::new(80, 20);
        let rect = PixelRect::new(10, 4, 60, 12);

        draw_text_gnw_centered(&mut buffer, rect, font, "Survey", Rgba::WHITE);

        let painted: Vec<(i32, i32)> = (0..20)
            .flat_map(|y| (0..80).map(move |x| (x, y)))
            .filter(|&(x, y)| buffer.get(x, y) == Some(Rgba::WHITE))
            .collect();

        assert!(!painted.is_empty());
        assert!(painted.iter().all(|&(x, y)| {
            x >= rect.ulx && x < rect.lrx() && y >= rect.uly && y < rect.lry()
        }));
    }
}
