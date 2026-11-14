//! max_assets — чтение ресурсов оригинальной игры.
//!
//! Форматы (задокументированы в `max-original-dasm/formats/`):
//! - RES/PATCHES — архивы игровых данных (заголовок, индекс, записи) —
//!   модуль [`res`];
//! - WRL — карты планет (миникарта, tile-id, тайлы, палитра, pass-таблица) —
//!   модуль [`wrl`];
//! - SCE/MPS — миссии; SPW — спрайты; FLC — анимации; FON — шрифты;
//!   MSC/MVE — музыка/видео (пока не разбираются).
//!
//! Крейт парсит байты в структурированные данные; рендер-специфику не содержит.
//! Ошибки формата возвращаются значениями ([`ResError`], [`WrlError`]),
//! паники на битых данных нет.

#![forbid(unsafe_code)]

pub mod res;
pub mod wrl;

pub use res::{
    ResArchive, ResEntry, ResError, ENTRY_SIZE, FIRST_DATA_OFFSET, HEADER_SIZE, RES_SIGNATURE,
    TAG_LEN,
};
pub use wrl::{
    WrlError, WrlMap, WrlOffsets, DMO_SIGNATURE, MAX_CPP_LAYERS, PALETTE_BYTES, PALETTE_COLORS,
    PASS_TABLE_SURFACES, SIGNATURE_LEN, STANDARD_HEIGHT, STANDARD_WIDTH, TILE_PIXELS, TILE_SIZE,
    WRL_SIGNATURE,
};

/// Разбирает RES-архив из байтов и проверяет его структуру.
///
/// Совместимая обёртка над [`ResArchive::parse`] (раньше здесь была заглушка).
/// Для доступа к записям используйте [`ResArchive`] напрямую.
pub fn parse_res(data: &[u8]) -> anyhow::Result<()> {
    ResArchive::parse(data)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_smoke() {
        assert!(parse_res(b"").is_err());
        assert!(WrlMap::parse(b"").is_err());
    }
}
