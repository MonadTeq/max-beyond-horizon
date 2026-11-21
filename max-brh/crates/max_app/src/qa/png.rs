//! PNG-кодек слоя 3 QA: без внешних зависимостей (только std).
//!
//! Зачем свой кодек: golden-кадры должны генерироваться и сверяться в
//! `rust:1.94.0-bookworm` **без сети** и без `image`/`png`/`flate2` в
//! `Cargo.lock` — иначе `cargo test` в CI тянет новые крейты, а сами кадры
//! (640×480 × 3 байта = 900 КБ) не влезают в git.
//!
//! Что здесь есть:
//!
//! * [`Image`] — RGB-кадр (8 бит на канал);
//! * [`encode`] / [`decode`] — PNG: `IHDR` + `PLTE` (для ≤256 цветов) +
//!   `IDAT` ([`zlib_compress`]: deflate fixed Huffman + LZ77) + `IEND`;
//! * [`compare`] — попиксельная сверка двух кадров с порогом по каналу;
//! * [`write`] / [`read`] — файловые обёртки.
//!
//! Ограничения (осознанные, описаны в `max-brh-qa/scripts/README.md`):
//! декодер понимает **stored** и **fixed Huffman** блоки deflate — ровно те,
//! что пишет [`encode`]. Динамический Huffman (типичный вывод `libpng`,
//! `optipng` и т.п.) отклоняется с внятной ошибкой, поэтому golden-кадры
//! перегенерируются только через `qa_run`, а не сторонними утилитами.
//!
//! Формат кадра — тот же, что у `RgbaBuffer` (RGBA, альфа = 255), но в PNG
//! пишется RGB (или индекс палитры): альфа в кадрах приложения всегда
//! непрозрачная. Это экономит и файл, и git: 8 экранов `visual-screens.qa`
//! занимают ~870 КиБ вместо ~7 МБ сырых кадров.

const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Максимальный поддерживаемый размер кадра по каждой стороне.
pub const MAX_DIMENSION: u32 = 8192;

/// RGB-кадр (8 бит на канал, строки сверху вниз).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Ширина в пикселях.
    pub width: u32,
    /// Высота в пикселях.
    pub height: u32,
    /// `width * height * 3` байт RGB.
    pub rgb: Vec<u8>,
}

impl Image {
    /// Чёрный кадр.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            rgb: vec![0; width as usize * height as usize * 3],
        }
    }

    /// Кадр из RGBA-буфера (`RgbaBuffer::pixels`): альфа отбрасывается.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Self {
        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);

        for pixel in rgba.chunks_exact(4) {
            rgb.extend_from_slice(&pixel[..3]);
        }

        Self {
            width,
            height,
            rgb,
        }
    }

    /// Число пикселей.
    pub fn pixels(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Пиксель `(x, y)`.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let offset = (y as usize * self.width as usize + x as usize) * 3;

        Some([self.rgb[offset], self.rgb[offset + 1], self.rgb[offset + 2]])
    }

    /// Устанавливает пиксель (вне кадра — ничего не делает).
    pub fn set_pixel(&mut self, x: u32, y: u32, color: [u8; 3]) {
        if x >= self.width || y >= self.height {
            return;
        }

        let offset = (y as usize * self.width as usize + x as usize) * 3;
        self.rgb[offset..offset + 3].copy_from_slice(&color);
    }
}

// ---------------------------------------------------------------------------
// Контрольные суммы
// ---------------------------------------------------------------------------

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0usize;

    while index < 256 {
        let mut value = index as u32;

        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                0xEDB8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }

        table[index] = value;
        index += 1;
    }

    table
}

const CRC_TABLE: [u32; 256] = crc_table();

/// CRC32 (IEEE, как в PNG).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;

    for byte in data {
        crc = CRC_TABLE[((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
    }

    crc ^ 0xFFFF_FFFF
}

/// Adler-32 (как в zlib).
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;

    let mut a = 1u32;
    let mut b = 0u32;

    for byte in data {
        a = (a + u32::from(*byte)) % MOD;
        b = (b + a) % MOD;
    }

    (b << 16) | a
}

// ---------------------------------------------------------------------------
// Deflate: кодирование (fixed Huffman + LZ77)
// ---------------------------------------------------------------------------

/// Базовые длины кодов 257…285 (`RFC 1951` §3.2.5).
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
/// Дополнительные биты кодов длин.
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// Базовые расстояния кодов 0…29.
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
/// Дополнительные биты кодов расстояний.
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Длина совпадения → (символ, число доп. бит, значение доп. бит).
fn length_code(length: u16) -> (u16, u8, u16) {
    for index in (0..LENGTH_BASE.len()).rev() {
        if length >= LENGTH_BASE[index] {
            return (
                257 + index as u16,
                LENGTH_EXTRA[index],
                length - LENGTH_BASE[index],
            );
        }
    }

    (257, 0, 0)
}

/// Расстояние → (код, число доп. бит, значение доп. бит).
fn distance_code(distance: u16) -> (u16, u8, u16) {
    for index in (0..DIST_BASE.len()).rev() {
        if distance >= DIST_BASE[index] {
            return (index as u16, DIST_EXTRA[index], distance - DIST_BASE[index]);
        }
    }

    (0, 0, distance.saturating_sub(1))
}

/// Код фиксированного дерева литералов/длин (`RFC 1951` §3.2.6).
fn fixed_literal(symbol: u16) -> (u32, u32) {
    match symbol {
        0..=143 => (0x30 + u32::from(symbol), 8),
        144..=255 => (0x190 + u32::from(symbol) - 144, 9),
        256..=279 => (u32::from(symbol) - 256, 7),
        _ => (0xC0 + u32::from(symbol) - 280, 8),
    }
}

/// Писатель битов: байты заполняются с младшего бита, коды Хаффмана
/// отправляются начиная со старшего (как требует deflate).
struct BitWriter {
    out: Vec<u8>,
    bits: u32,
    count: u32,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            bits: 0,
            count: 0,
        }
    }

    /// Записать `count` бит значения `value` (младший бит — первым).
    fn put(&mut self, value: u32, count: u32) {
        debug_assert!(count < 32);

        if count == 0 {
            return;
        }

        self.bits |= (value & ((1u32 << count) - 1)) << self.count;
        self.count += count;

        while self.count >= 8 {
            self.out.push((self.bits & 0xFF) as u8);
            self.bits >>= 8;
            self.count -= 8;
        }
    }

    /// Записать код Хаффмана: биты кода идут от старшего к младшему.
    fn put_code(&mut self, code: u32, length: u32) {
        let mut reversed = 0u32;
        let mut value = code;
        let mut remaining = length;

        while remaining > 0 {
            reversed = (reversed << 1) | (value & 1);
            value >>= 1;
            remaining -= 1;
        }

        self.put(reversed, length);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.out.push((self.bits & 0xFF) as u8);
        }

        self.out
    }
}

/// deflate одним блоком с фиксированным деревом (BTYPE = 01).
///
/// Жадный LZ77 по 3-байтовым совпадениям: цепочка хэша до
/// [`MAX_CHAIN`] кандидатов, окно 32 КиБ, совпадения до 258 байт. Этого
/// достаточно для кадров 640×480 (см. замеры в `encode_indexed`).
fn deflate_fixed(data: &[u8]) -> Vec<u8> {
    const WINDOW: usize = 32_768;
    const MIN_MATCH: usize = 3;
    const MAX_MATCH: usize = 258;
    const MAX_CHAIN: usize = 256;
    const NICE_LENGTH: usize = 258;
    const HASH_SIZE: usize = 1 << 15;

    let mut writer = BitWriter::new();
    writer.put(1, 1); // BFINAL
    writer.put(1, 2); // BTYPE = 01 (fixed Huffman)

    let hash_at = |index: usize| -> usize {
        let a = usize::from(data[index]);
        let b = usize::from(data[index + 1]);
        let c = usize::from(data[index + 2]);

        ((a << 10) ^ (b << 5) ^ c) & (HASH_SIZE - 1)
    };

    let mut head = vec![usize::MAX; HASH_SIZE];
    let mut prev = vec![usize::MAX; data.len()];
    let mut index = 0usize;

    while index < data.len() {
        let mut best_length = 0usize;
        let mut best_distance = 0usize;

        if index + MIN_MATCH <= data.len() {
            let hash = hash_at(index);
            let limit = index.saturating_sub(WINDOW);
            let max_length = (data.len() - index).min(MAX_MATCH);
            let mut candidate = head[hash];
            let mut chain = 0usize;

            while candidate != usize::MAX && candidate >= limit && chain < MAX_CHAIN {
                let mut length = 0usize;

                while length < max_length && data[candidate + length] == data[index + length] {
                    length += 1;
                }

                if length >= MIN_MATCH && length > best_length {
                    best_length = length;
                    best_distance = index - candidate;

                    if length >= NICE_LENGTH {
                        break;
                    }
                }

                candidate = prev[candidate];
                chain += 1;
            }

            prev[index] = head[hash];
            head[hash] = index;
        }

        if best_length >= MIN_MATCH {
            let (symbol, extra_bits, extra_value) = length_code(best_length as u16);
            let (code, code_length) = fixed_literal(symbol);
            writer.put_code(code, code_length);
            writer.put(u32::from(extra_value), u32::from(extra_bits));

            let (distance, distance_bits, distance_value) = distance_code(best_distance as u16);
            writer.put_code(u32::from(distance), 5);
            writer.put(u32::from(distance_value), u32::from(distance_bits));

            // Позиции внутри совпадения тоже попадают в цепочку хэшей, иначе
            // следующие совпадения ищутся только по «головам» строк.
            for offset in 1..best_length {
                let position = index + offset;

                if position + MIN_MATCH <= data.len() {
                    let hash = hash_at(position);
                    prev[position] = head[hash];
                    head[hash] = position;
                }
            }

            index += best_length;
        } else {
            let (code, length) = fixed_literal(u16::from(data[index]));
            writer.put_code(code, length);
            index += 1;
        }
    }

    let (end_code, end_length) = fixed_literal(256);
    writer.put_code(end_code, end_length);

    writer.finish()
}

/// zlib-поток (`0x78 0x01` + deflate + Adler-32).
fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 4 + 64);
    out.push(0x78);
    out.push(0x01);
    out.extend_from_slice(&deflate_fixed(data));
    out.extend_from_slice(&adler32(data).to_be_bytes());

    out
}

// ---------------------------------------------------------------------------
// Inflate: декодирование (stored + fixed Huffman)
// ---------------------------------------------------------------------------

/// Читатель битов deflate.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            bit: 0,
        }
    }

    fn bit(&mut self) -> Result<u32, String> {
        let byte = *self
            .data
            .get(self.pos)
            .ok_or_else(|| "deflate: поток оборван".to_string())?;
        let value = (u32::from(byte) >> self.bit) & 1;

        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.pos += 1;
        }

        Ok(value)
    }

    /// `count` бит младшим битом вперёд (доп. биты длин/расстояний).
    fn le_bits(&mut self, count: u32) -> Result<u32, String> {
        let mut value = 0u32;

        for shift in 0..count {
            value |= self.bit()? << shift;
        }

        Ok(value)
    }

    /// `count` бит старшим битом вперёд (коды Хаффмана).
    fn huff(&mut self, count: u32) -> Result<u32, String> {
        let mut value = 0u32;

        for _ in 0..count {
            value = (value << 1) | self.bit()?;
        }

        Ok(value)
    }

    /// Выравнивание на границу байта (перед stored-блоком).
    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }

    fn byte(&mut self) -> Result<u8, String> {
        let byte = *self
            .data
            .get(self.pos)
            .ok_or_else(|| "deflate: поток оборван".to_string())?;
        self.pos += 1;

        Ok(byte)
    }
}

/// Символ фиксированного дерева литералов/длин.
fn fixed_symbol(reader: &mut BitReader<'_>) -> Result<u16, String> {
    let first = reader.huff(7)?;

    if first <= 0x17 {
        return Ok((256 + first) as u16);
    }

    let second = (first << 1) | reader.huff(1)?;

    if (0x30..=0xBF).contains(&second) {
        return Ok((second - 0x30) as u16);
    }

    if (0xC0..=0xC7).contains(&second) {
        return Ok((280 + second - 0xC0) as u16);
    }

    let third = (second << 1) | reader.huff(1)?;

    if (0x190..=0x1FF).contains(&third) {
        return Ok((144 + third - 0x190) as u16);
    }

    Err(format!(
        "deflate: недопустимый код фиксированного дерева {third:#05x}"
    ))
}

fn inflate(zlib: &[u8]) -> Result<Vec<u8>, String> {
    if zlib.len() < 6 {
        return Err("zlib: поток короче заголовка".to_string());
    }

    let cmf = zlib[0];
    let flg = zlib[1];

    if cmf & 0x0F != 8 {
        return Err(format!(
            "zlib: метод сжатия {} (ожидался deflate 8)",
            cmf & 0x0F
        ));
    }
    if (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 {
        return Err("zlib: неверная контрольная сумма заголовка".to_string());
    }
    if flg & 0x20 != 0 {
        return Err("zlib: preset dictionary не поддерживается".to_string());
    }

    let body = &zlib[2..zlib.len() - 4];
    let mut reader = BitReader::new(body);
    let mut out: Vec<u8> = Vec::new();

    loop {
        let last = reader.le_bits(1)? == 1;
        let kind = reader.le_bits(2)?;

        match kind {
            0 => {
                reader.align();
                let length = u32::from(reader.byte()?) | (u32::from(reader.byte()?) << 8);
                let inverse = u32::from(reader.byte()?) | (u32::from(reader.byte()?) << 8);

                if length != (!inverse & 0xFFFF) {
                    return Err("deflate: stored-блок с неверной длиной".to_string());
                }

                for _ in 0..length {
                    out.push(reader.byte()?);
                }
            }
            1 => loop {
                let symbol = fixed_symbol(&mut reader)?;

                match symbol {
                    0..=255 => out.push(symbol as u8),
                    256 => break,
                    257..=285 => {
                        let index = usize::from(symbol - 257);
                        let length = usize::from(LENGTH_BASE[index])
                            + reader.le_bits(u32::from(LENGTH_EXTRA[index]))? as usize;

                        let code = reader.huff(5)? as usize;
                        if code >= DIST_BASE.len() {
                            return Err(format!("deflate: код расстояния {code}"));
                        }
                        let distance = usize::from(DIST_BASE[code])
                            + reader.le_bits(u32::from(DIST_EXTRA[code]))? as usize;

                        if distance == 0 || distance > out.len() {
                            return Err(format!(
                                "deflate: расстояние {distance} больше выхода {}",
                                out.len()
                            ));
                        }

                        let start = out.len() - distance;
                        for offset in 0..length {
                            let byte = out[start + offset];
                            out.push(byte);
                        }
                    }
                    other => return Err(format!("deflate: символ {other} вне алфавита")),
                }
            },
            2 => {
                return Err(
                    "png: динамический Huffman не поддерживается — перегенерируйте кадр через qa_run"
                        .to_string(),
                )
            }
            _ => return Err("deflate: недопустимый тип блока (11)".to_string()),
        }

        if last {
            break;
        }
    }

    let expected = u32::from_be_bytes([
        zlib[zlib.len() - 4],
        zlib[zlib.len() - 3],
        zlib[zlib.len() - 2],
        zlib[zlib.len() - 1],
    ]);

    if adler32(&out) != expected {
        return Err("zlib: не совпала контрольная сумма данных".to_string());
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// PNG: кодирование
// ---------------------------------------------------------------------------

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);

    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// PNG-байты кадра.
///
/// Если в кадре не больше 256 различных цветов (а кадры M.A.X. — 8-битная
/// палитровая графика), пишется **индексированный** PNG (color type 3): один
/// байт на пиксель вместо трёх. Иначе — truecolor (color type 2).
pub fn encode(image: &Image) -> Vec<u8> {
    match palette_index(image) {
        Some((palette, indices)) => encode_indexed(image, &palette, &indices),
        None => encode_truecolor(image),
    }
}

/// Палитра (≤256 цветов) и индексы пикселей; `None` — цветов больше 256.
fn palette_index(image: &Image) -> Option<(Vec<[u8; 3]>, Vec<u8>)> {
    let mut lookup: std::collections::HashMap<[u8; 3], u8> = std::collections::HashMap::new();
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut indices = Vec::with_capacity(image.pixels());

    for pixel in image.rgb.chunks_exact(3) {
        let color = [pixel[0], pixel[1], pixel[2]];

        let index = match lookup.get(&color) {
            Some(index) => *index,
            None => {
                if palette.len() == 256 {
                    return None;
                }

                let index = palette.len() as u8;
                palette.push(color);
                lookup.insert(color, index);
                index
            }
        };

        indices.push(index);
    }

    Some((palette, indices))
}

fn encode_header(out: &mut Vec<u8>, width: u32, height: u32, color_type: u8) {
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, color_type, 0, 0, 0]); // 8 бит, без interlace
    push_chunk(out, b"IHDR", &ihdr);
}

fn encode_indexed(image: &Image, palette: &[[u8; 3]], indices: &[u8]) -> Vec<u8> {
    let stride = image.width as usize;
    let mut raw = Vec::with_capacity((stride + 1) * image.height as usize);

    // Фильтр строки — None (0): для кадров QA он даёт самый маленький PNG.
    // Замер на 8 экранах `visual-screens.qa` (этот кодировщик, фильтры):
    // None 866 КиБ; «самый дешёвый из 0/1/2» 891 КиБ; все пять 1.2 МиБ.
    // Причина: deflate здесь — LZ77 по 3-байтовым совпадениям, и сырые
    // строки палитровых индексов дают длинные повторы, а фильтры
    // превращают их в высокоэнтропийные остатки.
    for y in 0..image.height as usize {
        raw.push(0);
        raw.extend_from_slice(&indices[y * stride..(y + 1) * stride]);
    }

    let compressed = zlib_compress(&raw);

    let mut out = Vec::with_capacity(compressed.len() + palette.len() * 3 + 128);
    out.extend_from_slice(&SIGNATURE);
    encode_header(&mut out, image.width, image.height, 3);

    let mut plte = Vec::with_capacity(palette.len() * 3);
    for color in palette {
        plte.extend_from_slice(color);
    }
    push_chunk(&mut out, b"PLTE", &plte);
    push_chunk(&mut out, b"IDAT", &compressed);
    push_chunk(&mut out, b"IEND", &[]);

    out
}

fn encode_truecolor(image: &Image) -> Vec<u8> {
    let stride = image.width as usize * 3;
    let mut raw = Vec::with_capacity((stride + 1) * image.height as usize);

    // См. комментарий в `encode_indexed`: фильтр 0 меряется лучше остальных.
    for y in 0..image.height as usize {
        raw.push(0);
        raw.extend_from_slice(&image.rgb[y * stride..(y + 1) * stride]);
    }

    let compressed = zlib_compress(&raw);

    let mut out = Vec::with_capacity(compressed.len() + 128);
    out.extend_from_slice(&SIGNATURE);
    encode_header(&mut out, image.width, image.height, 2);
    push_chunk(&mut out, b"IDAT", &compressed);
    push_chunk(&mut out, b"IEND", &[]);

    out
}

// ---------------------------------------------------------------------------
// PNG: декодирование
// ---------------------------------------------------------------------------

fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let p = i32::from(left) + i32::from(up) - i32::from(up_left);
    let pa = (p - i32::from(left)).abs();
    let pb = (p - i32::from(up)).abs();
    let pc = (p - i32::from(up_left)).abs();

    if pa <= pb && pa <= pc {
        left
    } else if pb <= pc {
        up
    } else {
        up_left
    }
}

/// Разбирает PNG-байты в RGB-кадр.
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    if bytes.len() < SIGNATURE.len() || bytes[..SIGNATURE.len()] != SIGNATURE {
        return Err("png: нет сигнатуры PNG".to_string());
    }

    let mut pos = SIGNATURE.len();
    let mut width = 0u32;
    let mut height = 0u32;
    let mut channels = 0usize;
    let mut idat = Vec::new();
    let mut seen_header = false;
    let mut palette: Vec<[u8; 3]> = Vec::new();

    while pos + 8 <= bytes.len() {
        let length = u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
            as usize;
        let kind: [u8; 4] = [bytes[pos + 4], bytes[pos + 5], bytes[pos + 6], bytes[pos + 7]];

        if pos + 12 + length > bytes.len() {
            return Err("png: чанк выходит за пределы файла".to_string());
        }

        let data = &bytes[pos + 8..pos + 8 + length];
        let stored_crc = u32::from_be_bytes([
            bytes[pos + 8 + length],
            bytes[pos + 9 + length],
            bytes[pos + 10 + length],
            bytes[pos + 11 + length],
        ]);

        let mut crc_input = Vec::with_capacity(4 + length);
        crc_input.extend_from_slice(&kind);
        crc_input.extend_from_slice(data);

        if crc32(&crc_input) != stored_crc {
            return Err(format!(
                "png: CRC чанка {} не совпал",
                String::from_utf8_lossy(&kind)
            ));
        }

        match &kind {
            b"IHDR" => {
                if data.len() < 13 {
                    return Err("png: короткий IHDR".to_string());
                }

                width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                let depth = data[8];
                let color_type = data[9];
                let interlace = data[12];

                if depth != 8 {
                    return Err(format!("png: битовая глубина {depth} (поддержка — 8)"));
                }
                channels = match color_type {
                    2 => 3,
                    3 => 1,
                    6 => 4,
                    other => {
                        return Err(format!("png: тип цвета {other} (поддержка — 2, 3 и 6)"))
                    }
                };
                if interlace != 0 {
                    return Err("png: interlace не поддерживается".to_string());
                }
                if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
                    return Err(format!("png: недопустимый размер {width}×{height}"));
                }

                seen_header = true;
            }
            b"PLTE" => {
                for chunk in data.chunks_exact(3) {
                    palette.push([chunk[0], chunk[1], chunk[2]]);
                }
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }

        pos += 12 + length;
    }

    if !seen_header {
        return Err("png: нет IHDR".to_string());
    }
    if idat.is_empty() {
        return Err("png: нет IDAT".to_string());
    }
    if channels == 1 && palette.is_empty() {
        return Err("png: индексированный кадр без PLTE".to_string());
    }

    let raw = inflate(&idat)?;
    let stride = width as usize * channels;
    let expected = (stride + 1) * height as usize;

    if raw.len() < expected {
        return Err(format!(
            "png: данных {} байт, ожидалось {expected}",
            raw.len()
        ));
    }

    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    let mut previous = vec![0u8; stride];

    for y in 0..height as usize {
        let offset = y * (stride + 1);
        let filter = raw[offset];
        let row = &raw[offset + 1..offset + 1 + stride];
        let mut current = vec![0u8; stride];

        for index in 0..stride {
            let left = if index >= channels {
                current[index - channels]
            } else {
                0
            };
            let up = previous[index];
            let up_left = if index >= channels {
                previous[index - channels]
            } else {
                0
            };

            current[index] = match filter {
                0 => row[index],
                1 => row[index].wrapping_add(left),
                2 => row[index].wrapping_add(up),
                3 => {
                    let average = (u16::from(left) + u16::from(up)) / 2;
                    row[index].wrapping_add(average as u8)
                }
                4 => row[index].wrapping_add(paeth(left, up, up_left)),
                other => return Err(format!("png: неизвестный фильтр строки {other}")),
            };
        }

        for pixel in 0..width as usize {
            let base = pixel * channels;

            if channels == 1 {
                let index = usize::from(current[base]);
                let color = palette
                    .get(index)
                    .copied()
                    .ok_or_else(|| format!("png: индекс палитры {index} вне PLTE"))?;
                rgb.extend_from_slice(&color);
            } else {
                rgb.extend_from_slice(&current[base..base + 3]);
            }
        }

        previous = current;
    }

    Ok(Image {
        width,
        height,
        rgb,
    })
}

// ---------------------------------------------------------------------------
// Файлы и сверка
// ---------------------------------------------------------------------------

/// Записывает кадр в PNG-файл.
pub fn write(path: impl AsRef<std::path::Path>, image: &Image) -> Result<(), String> {
    let path = path.as_ref();
    let bytes = encode(image);

    std::fs::write(path, &bytes)
        .map_err(|error| format!("png: запись {}: {error}", path.display()))
}

/// Читает PNG-файл.
pub fn read(path: impl AsRef<std::path::Path>) -> Result<Image, String> {
    let path = path.as_ref();
    let bytes =
        std::fs::read(path).map_err(|error| format!("png: чтение {}: {error}", path.display()))?;

    decode(&bytes).map_err(|error| format!("png: {}: {error}", path.display()))
}

/// Разница двух кадров.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameDiff {
    /// Пикселей всего.
    pub total: usize,
    /// Пикселей, у которых хотя бы один канал отличается больше порога.
    pub changed: usize,
    /// Максимальная дельта по каналу.
    pub max_delta: u8,
    /// Карта различий (RGB): фон белый, изменённые пиксели — красные.
    pub image: Image,
}

impl FrameDiff {
    /// Доля изменённых пикселей (`0.0..=1.0`).
    pub fn changed_ratio(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }

        self.changed as f64 / self.total as f64
    }
}

/// Попиксельная сверка: пиксель «изменился», если `max|Δканал| > threshold`.
///
/// Кадры обязаны быть одного размера.
pub fn compare(expected: &Image, actual: &Image, threshold: u8) -> Result<FrameDiff, String> {
    if expected.width != actual.width || expected.height != actual.height {
        return Err(format!(
            "png: размеры кадров не совпадают: {}×{} и {}×{}",
            expected.width, expected.height, actual.width, actual.height
        ));
    }

    let mut image = Image::new(expected.width, expected.height);
    image.rgb.fill(0xFF);

    let mut changed = 0usize;
    let mut max_delta = 0u8;

    for y in 0..expected.height {
        for x in 0..expected.width {
            let left = expected.pixel(x, y).unwrap_or([0, 0, 0]);
            let right = actual.pixel(x, y).unwrap_or([0, 0, 0]);
            let mut delta = 0u8;

            for channel in 0..3 {
                let difference = left[channel].abs_diff(right[channel]);
                delta = delta.max(difference);
            }

            if delta > threshold {
                changed += 1;
                max_delta = max_delta.max(delta);
                image.set_pixel(x, y, [0xC0, 0x20, 0x20]);
            }
        }
    }

    Ok(FrameDiff {
        total: expected.pixels(),
        changed,
        max_delta,
        image,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(width: u32, height: u32) -> Image {
        let mut image = Image::new(width, height);

        for y in 0..height {
            for x in 0..width {
                let value = ((x * 7 + y * 13) % 251) as u8;
                image.set_pixel(
                    x,
                    y,
                    [value, value.wrapping_mul(3), value.wrapping_add(y as u8)],
                );
            }
        }

        image
    }

    #[test]
    fn png_roundtrip_preserves_every_pixel() {
        let image = sample(160, 120);
        let bytes = encode(&image);

        assert_eq!(&bytes[..8], &SIGNATURE);
        assert_eq!(&bytes[12..16], b"IHDR");

        let decoded = decode(&bytes).expect("PNG разбирается");

        assert_eq!(decoded, image);
    }

    #[test]
    fn palette_png_is_used_for_few_colors_and_roundtrips() {
        // 8-битная графика: 4 цвета → индексированный PNG (color type 3).
        let palette = [[10u8, 20, 30], [40, 50, 60], [255, 255, 255], [0, 0, 0]];
        let mut image = Image::new(64, 48);

        for y in 0..48 {
            for x in 0..64 {
                image.set_pixel(x, y, palette[((x / 8 + y / 8) % 4) as usize]);
            }
        }

        let bytes = encode(&image);

        assert_eq!(bytes[25], 3, "color type 3 (индексированный)");
        assert!(
            bytes.windows(4).any(|window| window == b"PLTE"),
            "есть палитра"
        );
        assert_eq!(decode(&bytes).expect("разбирается"), image);
        assert!(
            bytes.len() < 2_000,
            "четырёхцветный кадр 64×48: {} байт",
            bytes.len()
        );
    }

    #[test]
    fn many_colors_fall_back_to_truecolor() {
        let image = sample(64, 64);
        let bytes = encode(&image);

        assert_eq!(bytes[25], 2, "color type 2 (truecolor)");
        assert_eq!(decode(&bytes).expect("разбирается"), image);
    }

    #[test]
    fn flat_frame_compresses_well() {
        let mut image = Image::new(640, 480);
        image.rgb.fill(0x40);

        let bytes = encode(&image);

        assert!(
            bytes.len() < 8_000,
            "однотонный кадр 640×480: {} байт",
            bytes.len()
        );
        assert_eq!(decode(&bytes).expect("разбирается"), image);
    }

    #[test]
    fn crc_and_adler_catch_corruption() {
        let image = sample(32, 24);
        let mut bytes = encode(&image);

        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;

        assert!(decode(&bytes).is_err(), "порча IEND ловится CRC");
    }

    #[test]
    fn compare_counts_only_pixels_above_the_threshold() {
        let expected = Image::new(10, 10);
        let mut actual = expected.clone();

        actual.set_pixel(0, 0, [30, 0, 0]); // ровно порог — не изменение
        actual.set_pixel(1, 0, [31, 0, 0]); // выше порога

        let diff = compare(&expected, &actual, 30).expect("сверка");

        assert_eq!(diff.total, 100);
        assert_eq!(diff.changed, 1);
        assert_eq!(diff.max_delta, 31);
        assert_eq!(diff.image.pixel(1, 0), Some([0xC0, 0x20, 0x20]));
        assert_eq!(diff.image.pixel(0, 0), Some([0xFF, 0xFF, 0xFF]));
    }

    #[test]
    fn compare_rejects_different_sizes() {
        let left = Image::new(4, 4);
        let right = Image::new(5, 4);

        assert!(compare(&left, &right, 30).is_err());
    }

    #[test]
    fn rgba_conversion_drops_alpha() {
        let rgba = [1u8, 2, 3, 255, 4, 5, 6, 0];
        let image = Image::from_rgba(2, 1, &rgba);

        assert_eq!(image.rgb, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn adler32_matches_the_reference_value() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn png_files_roundtrip_through_the_filesystem() {
        let dir = std::env::temp_dir().join(format!("max_png_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp");
        let path = dir.join("frame.png");

        let image = sample(64, 48);
        write(&path, &image).expect("запись");
        assert_eq!(read(&path).expect("чтение"), image);

        std::fs::remove_dir_all(dir).ok();
    }
}
