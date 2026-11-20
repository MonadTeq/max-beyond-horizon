//! Минимальный GIF89a-кодировщик (только std): палитра + LZW.
//!
//! Зачем свой: PNG-кодек уже есть, а для превью FLC-анимации нужен именно GIF
//! (индексированные кадры с задержкой и зацикливанием), и тащить внешний крейт
//! ради этого не хочется — формат простой:
//!
//! * глобальная таблица цветов на 256 записей (палитра первого кадра FLC);
//! * при смене палитры в кадре — **локальная** таблица (GIF её допускает),
//!   поэтому цвета кадров остаются 1:1 с индексным буфером FLC;
//! * сжатие — LZW с минимальным размером кода 8 (`clear = 256`, `EOI = 257`),
//!   подблоки по ≤255 байт;
//! * `NETSCAPE2.0` — бесконечное зацикливание (как `flicsmgr_construct` в игре).
//!
//! Кодировщик детерминирован: одинаковые кадры дают одинаковые байты, никакого
//! времени/случайности в файле нет — это важно для идемпотентности экспорта.

/// Размер таблицы цветов GIF (`2^8`).
pub const PALETTE_SIZE: usize = 256;

/// Минимальный размер LZW-кода для 8-битных индексов.
const MIN_CODE_SIZE: u32 = 8;
/// Код «сбросить словарь».
const CLEAR_CODE: u16 = 1 << MIN_CODE_SIZE; // 256
/// Код «конец данных».
const EOI_CODE: u16 = CLEAR_CODE + 1; // 257
/// Предел кодов LZW до принудительного сброса словаря.
const MAX_CODE: u16 = 4096;

/// Один кадр GIF.
pub struct GifFrame<'a> {
    /// Индексы палитры, `width * height` байт.
    pub indices: &'a [u8],
    /// Локальная палитра кадра; `None` — используется глобальная.
    pub local_palette: Option<&'a [[u8; 3]; PALETTE_SIZE]>,
    /// Задержка перед следующим кадром в сотых долях секунды.
    pub delay_cs: u16,
}

/// Писатель битов GIF: байты заполняются с младшего бита (как требует LZW GIF).
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

    /// Записать `size` бит кода (младшим битом вперёд).
    fn code(&mut self, value: u16, size: u32) {
        self.bits |= u32::from(value) << self.count;
        self.count += size;

        while self.count >= 8 {
            self.out.push((self.bits & 0xFF) as u8);
            self.bits >>= 8;
            self.count -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.out.push((self.bits & 0xFF) as u8);
        }

        self.out
    }
}

/// LZW-сжатие индексного буфера (вариант GIF: коды переменной длины).
///
/// Словарь — `prefix << 8 | byte`; при переполнении 4096 кодов словарь
/// сбрасывается `clear`-кодом, как предписывает спецификация.
fn lzw_compress(indices: &[u8]) -> Vec<u8> {
    let mut writer = BitWriter::new();
    let mut code_size = MIN_CODE_SIZE + 1;
    let mut next = EOI_CODE + 1;
    let mut dictionary: std::collections::HashMap<u32, u16> = std::collections::HashMap::new();

    writer.code(CLEAR_CODE, code_size);

    let Some(&first) = indices.first() else {
        writer.code(EOI_CODE, code_size);

        return writer.finish();
    };

    let mut prefix = u16::from(first);

    for &byte in &indices[1..] {
        let key = (u32::from(prefix) << 8) | u32::from(byte);

        if let Some(&code) = dictionary.get(&key) {
            prefix = code;
            continue;
        }

        writer.code(prefix, code_size);

        if next < MAX_CODE {
            dictionary.insert(key, next);
            next += 1;

            // Ширина кода растёт, когда СЛЕДУЮЩИЙ свободный код уже не влезает
            // в текущую ширину. Порог именно `2^size + 1`, а не `2^size`:
            // декодер заводит запись на один код позже кодера (первый код
            // после clear записи не создаёт), поэтому «ранний» порог даёт
            // испорченный поток (проверено независимым декодером Pillow).
            if next == (1 << code_size) + 1 && code_size < 12 {
                code_size += 1;
            }
        } else {
            writer.code(CLEAR_CODE, code_size);
            dictionary.clear();
            next = EOI_CODE + 1;
            code_size = MIN_CODE_SIZE + 1;
        }

        prefix = u16::from(byte);
    }

    writer.code(prefix, code_size);
    writer.code(EOI_CODE, code_size);

    writer.finish()
}

/// Нарезает поток на GIF-подблоки (`[len][len байт]…`, терминатор `0x00`).
fn push_blocks(out: &mut Vec<u8>, data: &[u8]) {
    for chunk in data.chunks(255) {
        out.push(chunk.len() as u8);
        out.extend_from_slice(chunk);
    }

    out.push(0);
}

/// Заголовок + логический дескриптор экрана + глобальная палитра + цикл.
fn push_header(
    out: &mut Vec<u8>,
    width: u16,
    height: u16,
    global_palette: &[[u8; 3]; PALETTE_SIZE],
) {
    out.extend_from_slice(b"GIF89a");
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    // GCT есть (0x80), 8 бит на канал (0x70), размер таблицы 2^8 = 256 (0x07).
    out.push(0x80 | 0x70 | 0x07);
    out.push(0); // индекс фона
    out.push(0); // соотношение сторон

    for color in global_palette {
        out.extend_from_slice(color);
    }

    // NETSCAPE2.0 — зацикливание без счётчика.
    out.extend_from_slice(&[0x21, 0xFF, 0x0B]);
    out.extend_from_slice(b"NETSCAPE2.0");
    out.extend_from_slice(&[0x03, 0x01, 0x00, 0x00, 0x00]);
}

/// Собирает GIF89a из кадров одного размера.
///
/// * `global_palette` — таблица цветов по умолчанию (обычно палитра первого
///   кадра); кадр со своей палитрой пишет локальную таблицу;
/// * пустой список кадров — валидный GIF с нулём изображений (не используется
///   экспортом, но кодировщик остаётся тотальным).
pub fn encode(
    width: u16,
    height: u16,
    global_palette: &[[u8; 3]; PALETTE_SIZE],
    frames: &[GifFrame<'_>],
) -> Vec<u8> {
    let mut out = Vec::new();

    push_header(&mut out, width, height, global_palette);

    for frame in frames {
        // Graphic Control Extension: задержка, без прозрачности.
        out.extend_from_slice(&[0x21, 0xF9, 0x04, 0x00]);
        out.extend_from_slice(&frame.delay_cs.to_le_bytes());
        out.extend_from_slice(&[0x00, 0x00]);

        // Image Descriptor (+ локальная палитра, если кадр её принёс).
        out.push(0x2C);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());

        match frame.local_palette {
            Some(palette) => {
                out.push(0x80 | 0x07); // LCT, 256 цветов

                for color in palette {
                    out.extend_from_slice(color);
                }
            }
            None => out.push(0x00),
        }

        out.push(MIN_CODE_SIZE as u8);
        push_blocks(&mut out, &lzw_compress(frame.indices));
    }

    out.push(0x3B); // trailer

    out
}

/// Задержка GIF (в сотых долях секунды) по длительности кадра FLC, мс.
///
/// GIF хранит задержку в 1/100 с, поэтому 70 мс → 7 (≈ 14.3 FPS). Значение
/// зажимается в `1..=1000`, чтобы плееры не «съедали» кадр (0 = «как можно
/// быстрее» и часто трактуется как 1/10 с).
pub fn delay_from_ms(speed_ms: u32) -> u16 {
    let centiseconds = (speed_ms + 5) / 10;

    centiseconds.clamp(1, 1000) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Обратный LZW (только для тестов): проверяем, что поток декодируется.
    struct BitReader<'a> {
        data: &'a [u8],
        pos: usize,
        bit: u32,
    }

    impl BitReader<'_> {
        fn take(&mut self, size: u32) -> u16 {
            let mut value = 0u16;

            for shift in 0..size {
                let byte = self.data[self.pos];
                let bit = (u32::from(byte) >> self.bit) & 1;

                value |= (bit as u16) << shift;

                self.bit += 1;
                if self.bit == 8 {
                    self.bit = 0;
                    self.pos += 1;
                }
            }

            value
        }
    }

    fn lzw_decompress(data: &[u8], min_code_size: u32, expected: usize) -> Vec<u8> {
        let clear = 1u16 << min_code_size;
        let eoi = clear + 1;
        // Начальная таблица — 258 записей: индексы 0..=255, clear и EOI.
        let mut table: Vec<Vec<u8>> =
            (0..=(u16::from(clear) + 1)).map(|code| vec![code as u8]).collect();
        let mut reader = BitReader {
            data,
            pos: 0,
            bit: 0,
        };
        let mut size = min_code_size + 1;
        let mut out: Vec<u8> = Vec::new();
        let mut previous: Option<u16> = None;

        loop {
            let code = reader.take(size);

            if code == clear {
                table.truncate(usize::from(clear) + 2);
                size = min_code_size + 1;
                previous = None;
                continue;
            }

            if code == eoi {
                break;
            }

            let entry = if usize::from(code) < table.len() {
                table[usize::from(code)].clone()
            } else {
                let mut entry = table[usize::from(previous.expect("код без префикса"))].clone();
                entry.push(entry[0]);

                entry
            };

            out.extend_from_slice(&entry);

            if let Some(previous) = previous {
                let mut new_entry = table[usize::from(previous)].clone();
                new_entry.push(entry[0]);

                if table.len() < 4096 {
                    table.push(new_entry);

                    if table.len() == (1 << size) as usize && size < 12 {
                        size += 1;
                    }
                }
            }

            previous = Some(code);

            if out.len() > expected + 4 {
                break;
            }
        }

        out
    }

    fn palette() -> [[u8; 3]; PALETTE_SIZE] {
        let mut palette = [[0u8; 3]; PALETTE_SIZE];

        for (index, color) in palette.iter_mut().enumerate() {
            *color = [index as u8, (index as u8).wrapping_mul(3), 255 - index as u8];
        }

        palette
    }

    #[test]
    fn lzw_roundtrips_every_byte_value() {
        let data: Vec<u8> = (0..=255u8).cycle().take(4096).collect();

        assert_eq!(lzw_decompress(&lzw_compress(&data), 8, data.len()), data);
    }

    #[test]
    fn lzw_roundtrips_a_full_frame_with_repetitions() {
        let mut data = vec![7u8; 128 * 128];

        for (index, pixel) in data.iter_mut().enumerate() {
            if index % 1000 == 0 {
                *pixel = (index % 251) as u8;
            }
        }

        assert_eq!(lzw_decompress(&lzw_compress(&data), 8, data.len()), data);
    }

    #[test]
    fn gif_has_signature_palette_and_frames() {
        let palette = palette();
        let frame_a = vec![1u8; 8 * 4];
        let frame_b = vec![2u8; 8 * 4];
        let gif = encode(
            8,
            4,
            &palette,
            &[
                GifFrame {
                    indices: &frame_a,
                    local_palette: None,
                    delay_cs: 7,
                },
                GifFrame {
                    indices: &frame_b,
                    local_palette: Some(&palette),
                    delay_cs: 7,
                },
            ],
        );

        assert_eq!(&gif[..6], b"GIF89a");
        assert_eq!(u16::from_le_bytes([gif[6], gif[7]]), 8);
        assert_eq!(u16::from_le_bytes([gif[8], gif[9]]), 4);
        assert_eq!(*gif.last().unwrap(), 0x3B, "trailer");
        // 2 кадра — два разделителя изображений (0x2C) и два GCE.
        assert!(gif.iter().filter(|byte| **byte == 0x2C).count() >= 2);
        assert!(gif.windows(11).any(|window| window == b"NETSCAPE2.0"));
    }

    #[test]
    fn delay_is_clamped_and_rounded() {
        assert_eq!(delay_from_ms(0), 1);
        assert_eq!(delay_from_ms(4), 1);
        assert_eq!(delay_from_ms(70), 7);
        assert_eq!(delay_from_ms(66), 7);
        assert_eq!(delay_from_ms(10_000), 1000);
    }
}
