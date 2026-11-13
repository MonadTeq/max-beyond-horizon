//! max_assets — чтение ресурсов оригинальной игры.
//!
//! Форматы (задокументированы в `original/MAX/RE/formats/`):
//! - RES/PATCHES — архивы игровых данных (индексы, записи);
//! - WRL/SCE/MPS — карты и миссии;
//! - SPW — спрайты (палитрованные тайлы/юниты);
//! - FLC — анимации; FON — шрифты; MSC/MVE — музыка/видео.
//!
//! Крейт парсит байты в структурированные данные; рендер-специфику не содержит.

/// Разбор архива RES (заглушка — реализация в фазе RE3/R1).
pub fn parse_res(_data: &[u8]) -> anyhow::Result<()> {
    anyhow::bail!("RES parsing lands in phase RE3/R1")
}

#[cfg(test)]
mod tests {
    #[test]
    fn assets_smoke() {
        assert!(crate::parse_res(b"").is_err());
    }
}
