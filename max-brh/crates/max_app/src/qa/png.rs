//! PNG codec of QA layer 3: no external dependencies (std only).
//!
//! Why a custom codec: golden frames must be generated and compared in
//! `rust:1.94.0-bookworm` **without network** and without `image`/`png`/`flate2` in
//! `Cargo.lock` — otherwise `cargo test` in CI pulls new crates, and the frames
//! themselves (640×480 × 3 bytes = 900 KB) do not fit into git.
//!
//! What is here:
//!
//! * [`Image`] — RGB frame (8 bits per channel);
//! * [`encode`] / [`decode`] — PNG: `IHDR` + `PLTE` (for ≤256 colors) +
//!   `IDAT` ([`zlib_compress`]: deflate fixed Huffman + LZ77) + `IEND`;
//! * [`compare`] — per-pixel comparison of two frames with a per-channel threshold;
//! * [`write`] / [`read`] — file wrappers.
//!
//! Limitations (deliberate, described in `max-brh-qa/scripts/README.md`):
//! the decoder understands **stored** and **fixed Huffman** deflate blocks — exactly those
//! that [`encode`] writes. Dynamic Huffman (the typical output of `libpng`,
//! `optipng`, etc.) is rejected with a clear error, so golden frames are
//! regenerated only via `qa_run`, not with third-party tools.
//!
//! The frame format is the same as `RgbaBuffer` (RGBA, alpha = 255), but in PNG
//! RGB (or a palette index) is written: the alpha in application frames is always
//! opaque. This saves both the file and git: 8 screens of `visual-screens.qa`
//! take ~870 KiB instead of ~7 MB of raw frames.

const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Maximum supported frame size per side.
pub const MAX_DIMENSION: u32 = 8192;

/// RGB frame (8 bits per channel, rows top to bottom).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 3` bytes of RGB.
    pub rgb: Vec<u8>,
}

impl Image {
    /// Black frame.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            rgb: vec![0; width as usize * height as usize * 3],
        }
    }

    /// Frame from an RGBA buffer (`RgbaBuffer::pixels`): alpha is dropped.
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

    /// Number of pixels.
    pub fn pixels(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Pixel `(x, y)`.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let offset = (y as usize * self.width as usize + x as usize) * 3;

        Some([self.rgb[offset], self.rgb[offset + 1], self.rgb[offset + 2]])
    }

    /// Sets a pixel (outside the frame — does nothing).
    pub fn set_pixel(&mut self, x: u32, y: u32, color: [u8; 3]) {
        if x >= self.width || y >= self.height {
            return;
        }

        let offset = (y as usize * self.width as usize + x as usize) * 3;
        self.rgb[offset..offset + 3].copy_from_slice(&color);
    }
}

// ---------------------------------------------------------------------------
// Checksums
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

/// CRC32 (IEEE, as in PNG).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;

    for byte in data {
        crc = CRC_TABLE[((crc ^ u32::from(*byte)) & 0xFF) as usize] ^ (crc >> 8);
    }

    crc ^ 0xFFFF_FFFF
}

/// Adler-32 (as in zlib).
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
// Deflate: encoding (fixed Huffman + LZ77)
// ---------------------------------------------------------------------------

/// Base lengths of codes 257…285 (`RFC 1951` §3.2.5).
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
/// Extra bits of the length codes.
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// Base distances of codes 0…29.
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
/// Extra bits of the distance codes.
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Match length → (symbol, number of extra bits, extra bits value).
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

/// Distance → (code, number of extra bits, extra bits value).
fn distance_code(distance: u16) -> (u16, u8, u16) {
    for index in (0..DIST_BASE.len()).rev() {
        if distance >= DIST_BASE[index] {
            return (index as u16, DIST_EXTRA[index], distance - DIST_BASE[index]);
        }
    }

    (0, 0, distance.saturating_sub(1))
}

/// Fixed literal/length tree code (`RFC 1951` §3.2.6).
fn fixed_literal(symbol: u16) -> (u32, u32) {
    match symbol {
        0..=143 => (0x30 + u32::from(symbol), 8),
        144..=255 => (0x190 + u32::from(symbol) - 144, 9),
        256..=279 => (u32::from(symbol) - 256, 7),
        _ => (0xC0 + u32::from(symbol) - 280, 8),
    }
}

/// Bit writer: bytes are filled from the least significant bit, Huffman codes
/// are sent starting from the most significant (as deflate requires).
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

    /// Write `count` bits of the value `value` (least significant bit first).
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

    /// Write a Huffman code: the code bits go from most to least significant.
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

/// deflate in a single block with a fixed tree (BTYPE = 01).
///
/// Greedy LZ77 over 3-byte matches: a hash chain of up to
/// [`MAX_CHAIN`] candidates, a 32 KiB window, matches up to 258 bytes. This is
/// enough for 640×480 frames (see the measurements in `encode_indexed`).
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

            // Positions inside a match also go into the hash chain, otherwise
            // the following matches are searched only by the "heads" of the strings.
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

/// zlib stream (`0x78 0x01` + deflate + Adler-32).
fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 4 + 64);
    out.push(0x78);
    out.push(0x01);
    out.extend_from_slice(&deflate_fixed(data));
    out.extend_from_slice(&adler32(data).to_be_bytes());

    out
}

// ---------------------------------------------------------------------------
// Inflate: decoding (stored + fixed Huffman)
// ---------------------------------------------------------------------------

/// deflate bit reader.
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

    /// `count` bits least significant bit first (extra bits of lengths/distances).
    fn le_bits(&mut self, count: u32) -> Result<u32, String> {
        let mut value = 0u32;

        for shift in 0..count {
            value |= self.bit()? << shift;
        }

        Ok(value)
    }

    /// `count` bits most significant bit first (Huffman codes).
    fn huff(&mut self, count: u32) -> Result<u32, String> {
        let mut value = 0u32;

        for _ in 0..count {
            value = (value << 1) | self.bit()?;
        }

        Ok(value)
    }

    /// Byte boundary alignment (before a stored block).
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

/// Symbol of the fixed literal/length tree.
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
// PNG: encoding
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

/// PNG bytes of the frame.
///
/// If the frame has at most 256 distinct colors (and M.A.X. frames are 8-bit
/// palette graphics), an **indexed** PNG is written (color type 3): one
/// byte per pixel instead of three. Otherwise — truecolor (color type 2).
pub fn encode(image: &Image) -> Vec<u8> {
    match palette_index(image) {
        Some((palette, indices)) => encode_indexed(image, &palette, &indices),
        None => encode_truecolor(image),
    }
}

/// Palette (≤256 colors) and pixel indices; `None` — more than 256 colors.
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
    ihdr.extend_from_slice(&[8, color_type, 0, 0, 0]); // 8 bit, no interlace
    push_chunk(out, b"IHDR", &ihdr);
}

fn encode_indexed(image: &Image, palette: &[[u8; 3]], indices: &[u8]) -> Vec<u8> {
    let stride = image.width as usize;
    let mut raw = Vec::with_capacity((stride + 1) * image.height as usize);

    // Row filter — None (0): for QA frames it gives the smallest PNG.
    // Measurement on 8 screens of `visual-screens.qa` (this encoder, filters):
    // None 866 KiB; "the cheapest of 0/1/2" 891 KiB; all five 1.2 MiB.
    // Reason: the deflate here is LZ77 over 3-byte matches, and raw
    // palette index rows give long repeats, while filters
    // turn them into high-entropy residuals.
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

    // See the comment in `encode_indexed`: filter 0 measures better than the rest.
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
// PNG: decoding
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

/// Parses PNG bytes into an RGB frame.
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
// Files and comparison
// ---------------------------------------------------------------------------

/// Writes the frame to a PNG file.
pub fn write(path: impl AsRef<std::path::Path>, image: &Image) -> Result<(), String> {
    let path = path.as_ref();
    let bytes = encode(image);

    std::fs::write(path, &bytes)
        .map_err(|error| format!("png: запись {}: {error}", path.display()))
}

/// Reads a PNG file.
pub fn read(path: impl AsRef<std::path::Path>) -> Result<Image, String> {
    let path = path.as_ref();
    let bytes =
        std::fs::read(path).map_err(|error| format!("png: чтение {}: {error}", path.display()))?;

    decode(&bytes).map_err(|error| format!("png: {}: {error}", path.display()))
}

/// Difference of two frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameDiff {
    /// Total pixels.
    pub total: usize,
    /// Pixels where at least one channel differs by more than the threshold.
    pub changed: usize,
    /// Maximum per-channel delta.
    pub max_delta: u8,
    /// Difference map (RGB): white background, changed pixels — red.
    pub image: Image,
}

impl FrameDiff {
    /// Fraction of changed pixels (`0.0..=1.0`).
    pub fn changed_ratio(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }

        self.changed as f64 / self.total as f64
    }
}

/// Per-pixel comparison: a pixel "changed" if `max|Δchannel| > threshold`.
///
/// The frames must be the same size.
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
#[path = "png_tests.rs"]
mod tests;
