//! Экспорт ресурсов M.A.X. **по каждому типу юнита** в отдельный каталог.
//!
//! Вход — данные оригинальной игры рядом с `MAX.RES`:
//!
//! * `MAX.RES` — спрайты (`COMMTWR`…`WALDO`), тени (`S_*`), портреты (`P_*`,
//!   big image 300×240), метаданные кадров (`D_*`), ссылки на FLC (`F_*`);
//! * `COLOR.PAL` — палитра спрайтов (6-битные компоненты, `×4` при отрисовке);
//! * `*.FLC` — анимации портретов (файлы, на которые ссылаются `F_*`);
//! * `PATCHES.RES` — `SC_UNITS`: маппинг тип → `sprite`/`shadow`/`portrait`/
//!   `data`/`flics_animation` (снимок уже лежит в
//!   `max-brh/crates/max_app/data/cargo_unit_data.json`, его генерирует
//!   `max-brh/tools/gen_cargo_unit_data.py`; здесь он проверяется по архиву).
//!
//! Выход — `max-brh-assets/units/<TAG>/` для всех 93 типов
//! ([`max_core::enums::UnitType::ALL`]):
//!
//! ```text
//! max-brh-assets/units/TANK/
//! ├── sprites/            кадры корпуса (frame_XXX.png) + sheet.png (лента) + sprite.json
//! ├── turret/             кадры башни (если есть у типа)
//! ├── firing/             кадры выстрела (если есть)
//! ├── connectors/         кадры коннекторов (если есть)
//! ├── shadow/             кадры тени S_*
//! ├── portrait.png        P_* 300×240 (палитра окна, как CargoMenu) или fallback
//! ├── animation/          кадры FLC → PNG + animation.gif + копия исходного .FLC
//! └── unit.json           тип/имя/флаги/источники/размеры/наличие анимации
//! ```
//!
//! Модули: [`png`] — PNG-кодек (копия `max-brh/crates/max_app/src/qa/png.rs`,
//! расширенная RGBA в `max-brh-qa-tools/max-resources-viewer`); [`gif`] — свой
//! GIF89a-кодировщик; [`render`] — растеризация спрайтов/теней/портретов;
//! [`units`] — чтение `cargo_unit_data.json` и сверка с `SC_UNITS`; [`export`] —
//! обход типов, запись файлов, `manifest.csv` и `README.md`.

#![forbid(unsafe_code)]

pub mod export;
pub mod gif;
pub mod names;
pub mod png;
pub mod render;
pub mod units;

pub use export::{export_all, ExportOptions, ExportReport, PortraitSource, UnitReport};
pub use units::{load_cargo_unit_data, read_sc_units_tokens, verify_sc_units, CargoUnit};
