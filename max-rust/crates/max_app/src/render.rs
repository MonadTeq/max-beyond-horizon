//! Кадр без SDL: карта + юниты в [`RgbaBuffer`].
//!
//! Здесь собраны только **чистые** части [`max_render`]:
//! [`render_map_into`] (тайлы/поверхности/ресурсы), [`SpriteRenderer`]
//! (палитра, командные LUT, тени) и [`draw_order`] (порядок юнитов по `y`,
//! §5.4). SDL-слой (`max_render::sdl_window`) подключается исключительно в
//! `src/main.rs` под feature `sdl`, поэтому приложение и его тесты рисуют кадры
//! без окна и без дисплея.
//!
//! Если ассеты не загружены ([`App::init_assets`] не вызывался) или ресурса
//! типа нет в [`crate::assets::UNIT_SPRITES`], юнит рисуется квадратным
//! маркером командного цвета. В данных игры такой тип ровно один — `MASTER`
//! (`UNIT_TYPES_WITHOUT_SPRITE_DATA`); карта при этом всегда рисуется
//! настоящими цветами поверхности/ресурсов из [`TileStyle`].
//!
//! Общая точка отрисовки юнита — [`draw_unit_sprites`]: её используют и карта
//! (`App::draw_units`), и HUD-портрет (`panels::draw_portrait`), поэтому все
//! 93 типа рисуются одинаково (корпус, тень, башня со сдвигом).

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::map_render::{render_map_into, TileStyle};
use max_render::palette::{ColorLut, TeamColors, FULL_BRIGHTNESS_LEVEL};
use max_render::sprite_atlas::SpriteRenderer;
use max_render::terrain::PaletteRgba;
use max_render::viewport::draw_order;

use max_core::enums::{unit_flags, TeamClan, UnitOrder, UnitOrderState, UnitType};

use crate::panels::UiPalette;
use crate::App;

/// Сдвиг якоря зданий (`flags & BUILDING`) от центра клетки.
///
/// C++ `UnitInfo::SetPosition` (`unitinfo.cpp:1301`): `x = grid*64 + 32`, затем
/// для `flags & BUILDING` — `x += (GFX_MAP_TILE_SIZE / 2) - 1`, т.е. **+31**.
/// Здание 2×2 с hot spot `(58,55)` (`MININGST`) рисуется в своём 2×2-
/// прямоугольнике, а не на полклетки вверх-влево. Без этого шахта «съезжала»
/// на клетку генератора и их спрайты накладывались.
pub const BUILDING_ANCHOR_OFFSET: i32 = (64 / 2) - 1;

/// Сдвиг кадра строителя в рабочую позу (`DrawSpriteFrame(image_index + 16)`).
///
/// C++ `UnitInfo::StartBuilding` (`unitinfo.cpp:2789`) для инженера и
/// `UnitInfo::PositionInTape` (`unitinfo.cpp:4451`) для конструктора. В
/// спрайтах `ENGINEER` 32 кадра, `CONSTRCT` — 40, поэтому `angle + 16` всегда
/// попадает в блок рабочей анимации.
pub const BUILDER_WORK_FRAME_OFFSET: usize = 16;

/// Шаг рабочего цикла конструктора (`DrawSpriteFrame(image_index + 8)`).
///
/// C++ `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2856`): пока приказ
/// `ORDER_BUILD`/`ORDER_STATE_BUILD_IN_PROGRESS`, у конструктора на каждом
/// втором тике кадр сдвигается на `+8`; если `image_index + 8 >= 40`, вместо
/// этого берётся `image_index - 16` — то есть кадры идут по кругу
/// `+16 → +24 → +32 → +16`. У инженера цикл не листается: он держит `+16`.
pub const BUILDER_WORK_FRAME_STEP: usize = 8;

/// Сколько разных кадров в рабочем цикле конструктора (`+16`, `+24`, `+32`).
pub const BUILDER_WORK_FRAME_STEPS: u8 = 3;

/// Сколько секунд показывается один кадр полного цикла работающего здания.
///
/// **ПРОМЕЖУТОЧНОЕ правило** для заводов (DOS-съёмка шахту уже уточнила, см.
/// [`MINING_DRILL_FRAMES_ENABLED`]): та же скорость, что у
/// `RADAR`/`ProcessOrderSentry` — 1 кадр за 1/30 с (один тик обработки
/// приказов C++). Для шахты цикл больше не применяется: в idle корпус
/// статичен (кадр `image_base + 1` при `POWER_ON`), а видимое «мигание» дают
/// палитровые циклы ([`max_render::palette::ColorCycles`]).
pub const WORKING_BUILDING_ANIM_FRAME_SECONDS: f32 = 1.0 / 30.0;

/// Включено ли листание кадров корпуса работающего завода (ПРОМЕЖУТОЧНОЕ).
///
/// `false` возвращает статичный кадр C++: завод с очередью — `image_base`.
/// У шахты листания нет вообще (см. [`MINING_DRILL_FRAMES_ENABLED`]).
pub const WORKING_BUILDING_ANIMATION_ENABLED: bool = true;

/// Включено ли листание кадров дрели шахты (кадры `1..15`).
///
/// **Выключено сознательно:** DOS-съёмка (`max-original-dasm/formats/
/// unit-animation.md` §7.2) показала, что в idle корпус шахты всегда кадр 0
/// (включённая — `image_base + 1`), ни одного перехода `0 → 1 → … → 15` за
/// ~118 с игрового времени не было. Кадры 1..15 (положения дрели) играют,
/// вероятно, только при активной добыче (§7.4) — это требует отдельной
/// съёмки активной добычи/обработки хода. Включать их можно только после
/// такой съёмки: константа — единственная ручка правила.
pub const MINING_DRILL_FRAMES_ENABLED: bool = false;

/// Кадровые часы юнита (только для отрисовки, в снапшот/CRC не входят).
///
/// Три независимых счётчика повторяют три разных механизма C++:
///
/// * `builder_step` — `UnitsManager_ProcessOrderBuild` (`+8`, `-16` при
///   `image_index + 8 >= 40`), 0 — кадр `+16` из `StartBuilding`;
/// * `sentry_step` — `UnitsManager_ProcessOrderSentry` у юнитов с флагом
///   `ANIMATED`: каждый тик `image_index + 1`, после последнего —
///   `image_index = image_base`;
/// * `work_step` — **промежуточный** цикл кадров работающего завода:
///   завод листает блок `image_base + angle .. image_index_max`
///   (`image_index_max = image_count + image_index - 1`, как в C++
///   `UnitInfo::UnitInfo`) с той же скоростью, что `ProcessOrderSentry`.
///   C++ сам заводы по времени не анимирует, поэтому это **только
///   визуальное** дополнение; `0` — статичный кадр C++ (поэтому `dt = 0` не
///   двигает golden-кадры). Шахта этот цикл больше не использует: DOS-съёмка
///   показала статичный корпус в idle ([`MINING_DRILL_FRAMES_ENABLED`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameAnimation {
    /// Номер кадра рабочего цикла строителя (`0..BUILDER_WORK_FRAME_STEPS`).
    pub builder_step: u8,
    /// Номер тика `ProcessOrderSentry` (ANIMATED).
    pub sentry_step: u32,
    /// Номер кадра полного цикла работающего здания (`0` — статичный кадр C++).
    pub work_step: u32,
}

/// Активна ли покадровая анимация строителя (`ProcessOrderBuild`).
///
/// C++ листает кадры только у `CONSTRCT` в `ORDER_BUILD` /
/// `ORDER_STATE_BUILD_IN_PROGRESS` и только при включённых эффектах
/// (`ResourceManager_GetSettings()->GetNumericValue("effects")`). Инженер в это
/// время держит рабочую позу `+16` без цикла.
pub fn builder_animation_active(unit: &max_core::unit::UnitInfo) -> bool {
    unit.unit_type == UnitType::Constructor
        && unit.orders == UnitOrder::Build
        && unit.state == UnitOrderState::BuildInProgress
}

/// Активна ли покадровая анимация `UnitsManager_ProcessOrderSentry`.
///
/// C++ (`units_manager.cpp:3029-3041`) листает кадры **только** у юнитов с
/// флагом `ANIMATED` и только в приказе `ORDER_SENTRY`:
///
/// ```text
/// if (image_index + 1 <= image_index_max) image_index += 1;
/// else                                   image_index = image_base;
/// ```
///
/// Из зданий этот флаг есть у `RADAR` (`SC_UNITS`: `ANIMATED`), у шахт и
/// заводов его нет — их C++ по времени не анимирует.
pub fn sentry_animation_active(unit: &max_core::unit::UnitInfo) -> bool {
    unit.flags & unit_flags::ANIMATED != 0 && unit.orders == UnitOrder::Sentry
}

/// Работает ли здание, у которого порт листает кадры корпуса.
///
/// C++ не перелистывает кадры шахт/заводов по времени: у них есть кадры
/// состояния (`image_base` — выключено, `image_base + 1` — включено), которые
/// выставляются при `POWER_ON`/`POWER_DOWN`. DOS-съёмка подтвердила это и для
/// шахты (корпус в idle статичен, §7.2 `unit-animation.md`), поэтому цикл
/// остаётся только у **завода** с непустой очередью (`build_list`) —
/// промежуточное правило до съёмки работающего завода
/// ([`WORKING_BUILDING_ANIMATION_ENABLED`]).
pub fn working_building_animation_active(unit: &max_core::unit::UnitInfo) -> bool {
    // Шахта в idle не листает корпус: DOS-съёмка (§7.2) опровергла цикл
    // 0..15. Кадры дрели 1..15 включаются только константой
    // [`MINING_DRILL_FRAMES_ENABLED`] после съёмки активной добычи.
    if unit.unit_type == UnitType::MiningStation {
        return MINING_DRILL_FRAMES_ENABLED && unit.orders == UnitOrder::PowerOn;
    }

    max_core::production::is_factory(unit.unit_type)
        && unit.orders == UnitOrder::Build
        && !unit.build_list.is_empty()
}

/// Блок кадров работающего **завода**: `image_base + angle .. image_index_max`.
///
/// C++ `UnitInfo::UnitInfo` (`unitinfo.cpp:293`) задаёт
/// `image_index_max = image_count + image_index - 1`, где
/// `image_index = image_base + angle`. [`draw_unit_sprites_with_animation`]
/// передаёт сюда `frame_info.image_count` (`sentry_frames`) — это и есть
/// `image_count` C++ (`D_LRGBLD` = 2, `D_RADAR` = 16), а `image_count` —
/// длина ленты спрайта (`TRAINHAL` — 10 кадров: 2 кадра состояния + 8
/// коннекторов). Блок никогда не выходит за кадры корпуса `D_*`: кадры после
/// `image_count` — коннекторы, их нельзя рисовать на месте корпуса.
///
/// У шахты блока нет: DOS-съёмка показала статичный корпус в idle
/// ([`MINING_DRILL_FRAMES_ENABLED`]).
///
/// Возвращает `(start, len)`: первый кадр блока и длину блока (`len >= 1`).
pub fn working_building_frame_block(
    image_base: i16,
    image_count: usize,
    sentry_frames: usize,
    angle: u8,
) -> (usize, usize) {
    let last = image_count.saturating_sub(1);
    // Кадры корпуса — первые `image_count` кадров ленты (`D_*`), остальные
    // кадры ленты — частицы/коннекторы (`MININGST`: 24 кадра, из них 16 корпуса).
    let body_last = sentry_frames.max(1).min(image_count).saturating_sub(1);
    let start = usize::try_from(image_base.max(0))
        .unwrap_or(0)
        .saturating_add(usize::from(angle % 8))
        .min(last)
        .min(body_last);
    let end = start
        .saturating_add(sentry_frames.max(1) - 1)
        .min(body_last);

    (start, end - start + 1)
}

// ---------------------------------------------------------------------------
// Коннекторы зданий (`UnitInfo::RenderWithConnectors`)
// ---------------------------------------------------------------------------

/// Кадры коннекторов: бит `CONNECTOR_*` → смещение от `connector_image_base`.
///
/// Дословный порядок `UnitInfo::RenderWithConnectors` (`unitinfo.cpp:3151`):
/// сначала север/юг (по два коннектора у 2×2-здания), затем восток/запад.
/// Смещения те же, что в C++: `CONNECTOR_NORTH_LEFT → +0`,
/// `NORTH_RIGHT → +4`, `SOUTH_LEFT → +2`, `SOUTH_RIGHT → +6`,
/// `EAST_TOP → +1`, `EAST_BOTTOM → +5`, `WEST_TOP → +3`, `WEST_BOTTOM → +7`.
///
/// Именно эти кадры DOS-съёмка приняла за «8 частиц бурения» шахты
/// (`unit-animation.md` §7.1): `D_MINING.connector_image_base = 16`,
/// `connector_image_count = 8`, и в плотной базе у шахты были выставлены все
/// восемь битов. У `POWGEN` (`D_SMLBLD`: база 2, 4 коннектора) — четыре
/// «дымовых клуба», на самом деле трубы N/E/S/W.
pub const CONNECTOR_FRAMES: [(u16, usize); 8] = [
    (max_core::enums::CONNECTOR_NORTH_LEFT, 0),
    (max_core::enums::CONNECTOR_NORTH_RIGHT, 4),
    (max_core::enums::CONNECTOR_SOUTH_LEFT, 2),
    (max_core::enums::CONNECTOR_SOUTH_RIGHT, 6),
    (max_core::enums::CONNECTOR_EAST_TOP, 1),
    (max_core::enums::CONNECTOR_EAST_BOTTOM, 5),
    (max_core::enums::CONNECTOR_WEST_TOP, 3),
    (max_core::enums::CONNECTOR_WEST_BOTTOM, 7),
];

/// Индексы кадров коннекторов для маски `connectors` в порядке отрисовки C++.
///
/// Пусто, если маска нулевая, у типа нет `connector_image_base`/`count` или
/// кадр выходит за длину ленты.
pub fn connector_frame_indices(sprite: &crate::assets::LoadedSprite, mask: u16) -> Vec<usize> {
    let Some(info) = sprite.frame_info else {
        return Vec::new();
    };

    if mask == 0 || info.connector_image_count == 0 {
        return Vec::new();
    }

    let base = usize::try_from(info.connector_image_base.max(0)).unwrap_or(0);
    let count = usize::from(info.connector_image_count);
    let mut frames = Vec::new();

    for (bit, offset) in CONNECTOR_FRAMES {
        if mask & bit == 0 || offset >= count {
            continue;
        }

        let index = base + offset;

        if sprite.sheet.frame(index).is_some() {
            frames.push(index);
        }
    }

    frames
}

/// Здание команды в клетке — `Access_GetTeamBuilding` (`access.cpp:2157`).
///
/// C++ ищет первый юнит клетки с той же командой, не `ORDER_IDLE` и
/// `IsComplexMember()`; `Hash_MapHash` у зданий 2×2 регистрирует все четыре
/// клетки, поэтому запрос по любой из них находит здание.
fn team_building_at(
    state: &max_core::game::GameState,
    team: max_core::team::TeamId,
    x: i16,
    y: i16,
) -> Option<&max_core::unit::UnitInfo> {
    state
        .units_at(max_core::geo::Point::new(x, y))
        .iter()
        .filter_map(|id| state.unit(*id))
        .find(|unit| unit.team == team && unit.orders != UnitOrder::Idle && unit.is_complex_member())
}

/// Маска коннекторов здания — порт своей части `UnitsManager_UpdateConnectors`
/// (`units_manager.cpp:2051`).
///
/// C++ считает биты при постройке/перестройке комплекса и хранит их в
/// `UnitInfo::connectors` (у порта это **симуляционное** поле, сохраняемое в
/// V70 и CRC). Порт ещё не выполняет `UnitsManager_UpdateConnectors`
/// (`max_core::unit_order::update_connectors` — заглушка), поэтому у новых
/// партий маска нулевая и коннекторы не рисовались бы вовсе. Здесь та же
/// формула повторяется **только для кадра**: она не пишет в состояние и не
/// влияет на снапшот/CRC, а у зданий из сейва реальная маска всё равно
/// объединяется с вычисленной (`unit.connectors | …`).
///
/// Правила (ровно как в C++): сосед сверху `(gx, gy-1)` даёт `NORTH_LEFT`,
/// сверху-справа `(gx+1, gy-1)` — `NORTH_RIGHT`; снизу `(gx, gy+size)` —
/// `SOUTH_LEFT`, снизу-справа `(gx+1, gy+size)` — `SOUTH_RIGHT`; справа
/// `(gx+size, gy)` — `EAST_TOP`, справа-снизу `(gx+size, gy+1)` —
/// `EAST_BOTTOM`; слева `(gx-1, gy)` — `WEST_TOP`, слева-снизу
/// `(gx-1, gy+1)` — `WEST_BOTTOM`. У 1×1-зданий (`POWGEN`) есть только
/// `NORTH_LEFT`/`SOUTH_LEFT`/`EAST_TOP`/`WEST_TOP`.
pub fn connector_mask_for(
    state: &max_core::game::GameState,
    unit: &max_core::unit::UnitInfo,
) -> u16 {
    use max_core::enums::{
        CONNECTOR_EAST_BOTTOM, CONNECTOR_EAST_TOP, CONNECTOR_NORTH_LEFT, CONNECTOR_NORTH_RIGHT,
        CONNECTOR_SOUTH_LEFT, CONNECTOR_SOUTH_RIGHT, CONNECTOR_WEST_BOTTOM, CONNECTOR_WEST_TOP,
    };

    if !unit.is_complex_member() {
        return 0;
    }

    let size: i16 = if unit.flags & unit_flags::BUILDING != 0 {
        2
    } else {
        1
    };
    let (gx, gy) = (unit.grid.x, unit.grid.y);
    let team = unit.team;
    let has = |x: i16, y: i16| team_building_at(state, team, x, y).is_some();
    let mut mask = 0u16;

    if has(gx, gy - 1) {
        mask |= CONNECTOR_NORTH_LEFT;
    }
    if has(gx, gy + size) {
        mask |= CONNECTOR_SOUTH_LEFT;
    }
    if size == 2 {
        if has(gx + 1, gy - 1) {
            mask |= CONNECTOR_NORTH_RIGHT;
        }
        if has(gx + 1, gy + size) {
            mask |= CONNECTOR_SOUTH_RIGHT;
        }
    }
    if has(gx + size, gy) {
        mask |= CONNECTOR_EAST_TOP;
    }
    if size == 2 && has(gx + size, gy + 1) {
        mask |= CONNECTOR_EAST_BOTTOM;
    }
    if has(gx - 1, gy) {
        mask |= CONNECTOR_WEST_TOP;
    }
    if size == 2 && has(gx - 1, gy + 1) {
        mask |= CONNECTOR_WEST_BOTTOM;
    }

    mask
}

/// Маска, которой реально рисуются коннекторы: сохранённая ∪ вычисленная.
///
/// Сохранённая маска — из V70-сейва/симуляции (`UnitInfo::connectors`);
/// вычисленная — [`connector_mask_for`] (пока `UnitsManager_UpdateConnectors`
/// не портирован в симуляцию).
pub fn drawn_connector_mask(
    state: &max_core::game::GameState,
    unit: &max_core::unit::UnitInfo,
) -> u16 {
    unit.connectors | connector_mask_for(state, unit)
}

/// Масштаб «вырастания» из `scaler_adjust`: `2^-scaler` (`UnitInfo::RenderSprite`).
///
/// C++ `UnitInfo::RenderSprite`/`RenderShadow` (`unitinfo.cpp:3085/3050`)
/// делят размер кадра на `scaling_factor = 1 << (scaler_adjust + 1)`, то есть
/// относительно обычного кадра (`scaler_adjust = 0`, делитель 2) — `1 / 2^n`:
/// `4 → 1/16`, `3 → 1/8`, `2 → 1/4`, `1 → 1/2`, `0 → 1`. `0` — полный размер.
pub fn scaler_scale(scaler_adjust: u8) -> f32 {
    let scaler = scaler_adjust.min(4);
    1.0 / f32::from(1u16 << scaler)
}

/// База кадров корпуса с учётом клана.
///
/// C++ `UnitInfo::UnitInfo` (`unitinfo.cpp:281`): у шахты
/// `image_base = (team_clan - 1) * sizeof(uint16_t)` — 8 клановых вариантов по
/// 2 кадра (выкл/вкл). Остальные типы берут базу из `D_*` (`unit.image_base`).
/// `TeamClan::Random` (0) в max_core не разрешается в конкретный клан, поэтому
/// для него и для сейвов с нулевой базой используется клан 1 (база 0) — как
/// рисовалось раньше.
pub fn image_base_for(unit: &max_core::unit::UnitInfo, team_clan: Option<TeamClan>) -> i16 {
    if unit.unit_type == UnitType::MiningStation && unit.image_base <= 0 {
        let clan = team_clan.map(|clan| clan as i16).unwrap_or(0);
        let clan = if (1..=8).contains(&clan) { clan } else { 1 };

        (clan - 1) * 2
    } else {
        unit.image_base
    }
}

/// Индекс кадра корпуса: `image_base + angle`, а для включённого здания — +1.
///
/// C++ `UnitsManager_ProcessOrderPowerOn` (`units_manager.cpp:2945`) при
/// `ORDER_POWER_ON` рисует `DrawSpriteFrame(image_base + 1)`, `PowerDown` —
/// `DrawSpriteFrame(image_base)` (`unitinfo.cpp:5132`). Это не покадровая
/// анимация по времени: у зданий ровно два кадра состояния (выключено/
/// включено), остальные кадры — коннекторы/направления.
pub fn unit_frame_index(
    unit: &max_core::unit::UnitInfo,
    image_base: i16,
    image_count: usize,
) -> usize {
    unit_frame_index_for_angle(unit, image_base, image_count, unit.angle)
}

/// [`unit_frame_index`] с явным углом корпуса.
///
/// Спрайт юнита — `image_base + angle` (8 направлений), поэтому кадр обязан
/// выбираться по тому углу, с которым спрайт реально движется по карте
/// (`App::visual_angle`), а не только по симуляционному: визуальный маршрут
/// отстаёт от симуляции, и иначе кадр поворачивался бы раньше клетки поворота.
///
/// Во время стройки C++ рисует рабочую позу строителя:
/// `DrawSpriteFrame(image_index + 16)` (`unitinfo.cpp:2789` для инженера и
/// `4451` для конструктора), а `UnitsManager_ProcessOrderBuild` у конструктора
/// дополнительно перелистывает `+8` (`units_manager.cpp:2856-2868`). Спрайты
/// `ENGINEER` (32 кадра) и `CONSTRCT` (40) содержат эти кадры, поэтому здесь
/// добавляется ровно `+16`, пока приказ — `ORDER_BUILD`, состояние —
/// `ORDER_STATE_BUILD_IN_PROGRESS`. После достройки (`ORDER_AWAIT`) кадр снова
/// базовый: C++ возвращает `DrawSpriteFrame(image_index - 16)`.
pub fn unit_frame_index_for_angle(
    unit: &max_core::unit::UnitInfo,
    image_base: i16,
    image_count: usize,
    angle: u8,
) -> usize {
    let last = image_count.saturating_sub(1);
    let base = usize::try_from(image_base.max(0)).unwrap_or(0);
    let mut index = base.saturating_add(usize::from(angle % 8));

    if unit.orders == UnitOrder::PowerOn && index < last {
        index += 1;
    }

    if matches!(unit.unit_type, UnitType::Engineer | UnitType::Constructor)
        && unit.orders == UnitOrder::Build
        && unit.state == UnitOrderState::BuildInProgress
    {
        index = index.saturating_add(BUILDER_WORK_FRAME_OFFSET);
    }

    index.min(last)
}

/// Кадр юнита с учётом покадровой анимации строителя (`build_step`).
///
/// `build_step` — номер кадра рабочего цикла конструктора (0, 1, 2) из
/// [`crate::App::build_animation_step`]; `0` — кадр начала стройки `+16`, как
/// `DrawSpriteFrame(image_index + 16)` в C++. Дальше повторяется цикл C++
/// `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2856`):
///
/// ```text
/// index = index + 8;                    // DrawSpriteFrame(image_index + 8)
/// if index + 8 >= 40 { index -= 16; }   // кадры 40+ не влезают в CONSTRCT
/// ```
///
/// Точная формула C++ применяется к текущему индексу (а не к `angle+16+8n`),
/// поэтому последовательность заворачивается ровно как в оригинале:
/// `16 → 24 → 32 → 16` (для угла 0). У `ENGINEER` цикла нет — кадр `+16`
/// постоянный, как `DrawSpriteFrame(image_index + 16)` в `StartBuilding`.
pub fn unit_frame_index_for_animation(
    unit: &max_core::unit::UnitInfo,
    image_base: i16,
    image_count: usize,
    angle: u8,
    build_step: u8,
) -> usize {
    unit_frame_index_with_animation(
        unit,
        image_base,
        image_count,
        image_count,
        angle,
        FrameAnimation {
            builder_step: build_step,
            ..FrameAnimation::default()
        },
        true,
    )
}

/// Кадр юнита со всеми покадровыми анимациями C++ и промежуточным циклом.
///
/// `image_count` — число кадров спрайта корпуса (граница индекса);
/// `sentry_frames` — `Unit::GetFrameInfo().image_count` (`D_*`), то есть
/// сколько кадров занимает блок анимации. У `RADAR` в спрайте 20 кадров, но
/// `D_RADAR.image_count = 16` (последние 4 — коннекторы), и C++
/// `image_index_max` считает именно по `FrameInfo` — поэтому анимация обязана
/// зацикливаться на 16, а не на 20.
///
/// Порядок как в C++:
///
/// 1. статичный кадр [`unit_frame_index_for_angle`] (`image_base + angle`,
///    `+1` при `POWER_ON`, `+16` рабочая поза строителя);
/// 2. цикл конструктора `+8` / `-16` ([`builder_animation_active`]);
/// 3. `ProcessOrderSentry` для `ANIMATED` (`sentry_step`, сброс на
///    `image_base` после последнего кадра);
/// 4. **промежуточный** цикл кадров работающего **завода** (`work_step`, см.
///    [`working_building_frame_block`] и
///    [`WORKING_BUILDING_ANIMATION_ENABLED`]).
///
/// `effects = false` отключает п. 3–4, как `settings["effects"]` в C++.
///
/// Пункт 4 — **временное** правило до съёмки работающего завода: блок C++
/// `image_base + angle .. image_index_max` листается по кругу со скоростью
/// `RADAR` (1 кадр за 1/30 с). Шахта его не использует: DOS-съёмка показала
/// статичный корпус в idle ([`MINING_DRILL_FRAMES_ENABLED`]).
/// `work_step = 0` возвращает статичный кадр C++ (включённая шахта — `+1`),
/// поэтому `dt = 0` не двигает golden-кадры.
#[allow(clippy::too_many_arguments)]
pub fn unit_frame_index_with_animation(
    unit: &max_core::unit::UnitInfo,
    image_base: i16,
    image_count: usize,
    sentry_frames: usize,
    angle: u8,
    animation: FrameAnimation,
    effects: bool,
) -> usize {
    let last = image_count.saturating_sub(1);
    let mut index = unit_frame_index_for_angle(unit, image_base, image_count, angle);

    if builder_animation_active(unit) {
        for _ in 0..animation.builder_step % BUILDER_WORK_FRAME_STEPS {
            index = if index + BUILDER_WORK_FRAME_STEP >= image_count {
                index.saturating_sub(BUILDER_WORK_FRAME_OFFSET)
            } else {
                index + BUILDER_WORK_FRAME_STEP
            };
        }
    }

    if effects && sentry_animation_active(unit) {
        let base = usize::try_from(image_base.max(0)).unwrap_or(0);
        let frames = sentry_frames.max(1);

        // C++: `image_index = image_base` после последнего кадра, т.е. цикл
        // идёт по блоку `image_base .. image_base + image_count - 1`.
        index = base + (animation.sentry_step as usize % frames);
    } else if effects
        && WORKING_BUILDING_ANIMATION_ENABLED
        && working_building_animation_active(unit)
    {
        // ПРОМЕЖУТОЧНОЕ правило: полный цикл кадров работающего здания.
        // C++ `image_index_max = image_count + image_index - 1`; шаг 0 —
        // статичный кадр C++ (фаза `POWER_ON`/`image_base`), дальше по кругу.
        let (start, len) =
            working_building_frame_block(image_base, image_count, sentry_frames, angle);
        let phase = index.saturating_sub(start).min(len - 1);
        index = start + (phase + animation.work_step as usize) % len;
    }

    index.min(last)
}

/// Мировая точка якоря спрайта: центр визуальной клетки, для зданий — +31.
///
/// C++ `UnitInfo::SetPosition` (`unitinfo.cpp:1301`) и `Render`
/// (`unitinfo.cpp:3138`) рисуют по `unit.x/unit.y`; у `BUILDING` это центр
/// 2×2-прямоугольника, у остальных — центр клетки.
pub fn unit_anchor_world(unit: &max_core::unit::UnitInfo, visual: (f32, f32)) -> (i32, i32) {
    let mut x = (visual.0 * 64.0 + 32.0).round() as i32;
    let mut y = (visual.1 * 64.0 + 32.0).round() as i32;

    if unit.flags & unit_flags::BUILDING != 0 {
        x += BUILDING_ANCHOR_OFFSET;
        y += BUILDING_ANCHOR_OFFSET;
    }

    (x, y)
}

/// Диагностика одного кадра.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// Сколько клеток карты нарисовано.
    pub map_tiles: u32,
    /// Сколько маркеров ресурсов нарисовано.
    pub resource_icons: u32,
    /// Сколько юнитов нарисовано спрайтами.
    pub units_drawn: usize,
    /// Сколько юнитов нарисовано маркерами (нет спрайта/ассетов).
    pub units_marker: usize,
    /// Сколько маркеров стройки `BLDMRK*` нарисовано (`DrawMap_RenderBuildMarker`).
    pub build_markers: usize,
    /// Сколько юнитов пропущено (нет в списках/невидимы камере).
    pub units_skipped: usize,
}

impl RenderStats {
    /// Всего нарисованных юнитов.
    pub fn units(self) -> usize {
        self.units_drawn + self.units_marker
    }
}

/// Что нарисовано для одного юнита (диагностика и тесты покрытия типов).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnitSpriteStats {
    /// Пиксели корпуса (вместе с тенью — она только затемняет уже
    /// нарисованное, поэтому на пустом кадре это ровно корпус).
    pub unit: u32,
    /// Пиксели башни/пушки; 0 — у типа нет башни или флага.
    pub turret: u32,
    /// Пиксели коннекторов (`UnitInfo::RenderWithConnectors`): у шахты — 8
    /// кадров `D_MINING.connector_image_base = 16`, у `POWGEN` — 4 трубы
    /// `D_SMLBLD.connector_image_base = 2`. 0 — коннекторов нет/нет соседей.
    pub connectors: u32,
}

/// Рисует юнит спрайтами: тень → корпус → коннекторы → башня/пушка.
///
/// Повторяет `UnitInfo::RenderWithConnectors`/`Render` C++: корпус с
/// `zoom/64`, тень со сдвигом `shadow_offset`, коннекторы комплекса отдельными
/// кадрами `connector_image_base + n`, башня отдельным кадром
/// `turret_image_base + turret_angle` со сдвигом `angle_offsets[angle]`.
///
/// База кадров — `unit.image_base` (без кланового варианта шахты). Карта и HUD
/// вызывают [`draw_unit_sprites_with_base`] с
/// `image_base_for(unit, clan)`, чтобы шахта показывала свой клан.
///
/// Возвращает `None`, если для типа нет загруженного спрайта/кадра — тогда
/// вызывающий рисует маркер команды (единственный такой тип в данных игры —
/// `MASTER`, см. [`crate::assets::UNIT_TYPES_WITHOUT_SPRITE_DATA`]).
///
/// Это общая точка карты ([`App::draw_units`]), HUD-портрета
/// (`panels::draw_portrait`) и тестов, которые гоняют рендер по всем 93
/// типам юнитов.
pub fn draw_unit_sprites(
    assets: &crate::assets::Assets,
    unit: &max_core::unit::UnitInfo,
    frame: &mut RgbaBuffer,
    anchor: (i32, i32),
    zoom: u32,
) -> Option<UnitSpriteStats> {
    draw_unit_sprites_with_base(assets, unit, unit.image_base, frame, anchor, zoom)
}

/// [`draw_unit_sprites`] с явной базой кадров корпуса (клан/сейв/степень
/// включённости здания). Кадр выбирает [`unit_frame_index`].
pub fn draw_unit_sprites_with_base(
    assets: &crate::assets::Assets,
    unit: &max_core::unit::UnitInfo,
    image_base: i16,
    frame: &mut RgbaBuffer,
    anchor: (i32, i32),
    zoom: u32,
) -> Option<UnitSpriteStats> {
    draw_unit_sprites_with_angle(assets, unit, image_base, unit.angle, frame, anchor, zoom)
}

/// [`draw_unit_sprites_with_base`] с явным углом корпуса для кадра.
///
/// `body_angle` — направление, с которым спрайт **сейчас** движется по карте
/// (в оконном цикле это `App::visual_angle`). Корпус берёт кадр
/// `image_base + body_angle`, а башня — свой `turret_image_base +
/// turret_angle` со сдвигом `angle_offsets[body_angle]`, как в
/// `UnitInfo::RenderWithConnectors` (`unitinfo.cpp:4838`). Остальные
/// вызывающие (HUD-портрет, тесты) передают `unit.angle`.
pub fn draw_unit_sprites_with_angle(
    assets: &crate::assets::Assets,
    unit: &max_core::unit::UnitInfo,
    image_base: i16,
    body_angle: u8,
    frame: &mut RgbaBuffer,
    anchor: (i32, i32),
    zoom: u32,
) -> Option<UnitSpriteStats> {
    draw_unit_sprites_with_animation(
        assets,
        &assets.system_palette,
        unit,
        image_base,
        body_angle,
        FrameAnimation::default(),
        true,
        unit.connectors,
        frame,
        anchor,
        zoom,
    )
}

/// [`draw_unit_sprites_with_angle`] с кадровыми часами юнита.
///
/// `animation` — [`FrameAnimation`] из [`crate::App::unit_animation`]:
/// строитель (`+16 → +24 → +32`), `ANIMATED`-здание (`ProcessOrderSentry`) и
/// **промежуточный** цикл кадров работающего завода. HUD и тесты передают
/// [`FrameAnimation::default()`] (статичный кадр C++, как при `dt = 0`).
///
/// `effects` — `settings["effects"]`; `false` отключает покадровые анимации,
/// как C++.
///
/// Палитра — системная палитра игры (`Assets::system_palette`); карта
/// передаёт сюда снимок с палитровыми циклами (`App::animated_palette`),
/// чтобы маяки зданий мигали. Коннекторы рисуются по маске `connectors`
/// (см. [`connector_frame_indices`]).
#[allow(clippy::too_many_arguments)]
pub fn draw_unit_sprites_with_animation(
    assets: &crate::assets::Assets,
    palette: &max_render::palette::Palette,
    unit: &max_core::unit::UnitInfo,
    image_base: i16,
    body_angle: u8,
    animation: FrameAnimation,
    effects: bool,
    connectors: u16,
    frame: &mut RgbaBuffer,
    anchor: (i32, i32),
    zoom: u32,
) -> Option<UnitSpriteStats> {
    let sprite = assets.sprite(unit.unit_type)?;
    // `FrameInfo` (`D_*`) задаёт длину блока кадров анимации; у RADAR это 16,
    // хотя в спрайте 20 кадров (последние 4 — коннекторы).
    let sentry_frames = sprite
        .frame_info
        .map(|info| usize::from(info.image_count))
        .filter(|frames| *frames > 0)
        .unwrap_or(sprite.sheet.len());
    let image_index = unit_frame_index_with_animation(
        unit,
        image_base,
        sprite.sheet.len(),
        sentry_frames,
        body_angle,
        animation,
        effects,
    );
    let sprite_frame = sprite.frame_at(image_index)?;
    let shadow = sprite.shadow_at(image_index);
    let colors = TeamColors::ALL
        .get(unit.team.0 as usize)
        .copied()
        .unwrap_or(TeamColors::Derelict);
    let level = unit.brightness.min(FULL_BRIGHTNESS_LEVEL);
    let renderer = SpriteRenderer::new(palette, &assets.brightness, ColorLut::for_team(colors), level);

    let shadow_offset = (
        i32::from(unit.shadow_offset.x),
        i32::from(unit.shadow_offset.y),
    );
    let mut stats = UnitSpriteStats::default();

    stats.unit = renderer
        .draw_unit_scaled(frame, sprite_frame, shadow, anchor, shadow_offset, zoom)
        .drawn;

    // Коннекторы комплекса — `UnitInfo::RenderWithConnectors`
    // (`unitinfo.cpp:3157`): у каждого свой кадр `connector_image_base + n`,
    // рисуется в том же якоре, что и корпус (горячая точка кадра задаёт
    // сторону), тень — из того же `S_*`, что и корпус.
    stats.connectors = 0;

    for index in connector_frame_indices(sprite, connectors) {
        if let Some(connector) = sprite.sheet.frame(index) {
            stats.connectors += renderer
                .draw_unit_scaled(
                    frame,
                    connector,
                    sprite.shadow_at(index),
                    anchor,
                    shadow_offset,
                    zoom,
                )
                .drawn;
        }
    }

    // Башня/пушка — отдельный кадр поверх корпуса со сдвигом
    // `angle_offsets[body_angle]` (`UnitInfo::RenderWithConnectors`).
    if unit.flags & (unit_flags::TURRET_SPRITE | unit_flags::SPINNING_TURRET) != 0 {
        if let Some(turret) = sprite.turret_frame(unit.turret_angle) {
            stats.turret = renderer
                .draw_turret_scaled(
                    frame,
                    turret,
                    sprite.turret_shadow(unit.turret_angle),
                    anchor,
                    shadow_offset,
                    sprite.turret_offset(body_angle),
                    zoom,
                )
                .drawn;
        }
    }

    Some(stats)
}

/// Цвета маркеров команд (запасной вариант без спрайтов): порядок команд
/// оригинала — `RED, GREEN, BLUE, GRAY, ALIEN`.
const MARKER_COLORS: [Rgba; 5] = [
    Rgba::rgb(224, 48, 48),
    Rgba::rgb(48, 192, 72),
    Rgba::rgb(64, 112, 224),
    Rgba::rgb(192, 192, 192),
    Rgba::rgb(212, 168, 64),
];

impl App {
    /// Рисует кадр текущего состояния в буфер `screen_width × screen_height`.
    pub fn render_frame(&self) -> RgbaBuffer {
        self.render_frame_with_stats().0
    }

    /// То же, что [`App::render_frame`], плюс статистика отрисовки.
    ///
    /// Если загружен хром окна (`FRAMEPIC`) и размер кадра — 640×480, рисуется
    /// окно 1:1 как в оригинале: фон-панели из `MAX.RES`, а карта — в области
    /// [`max_render::layout::MAIN_MAP`] (7×7 клеток). Иначе — прежний
    /// полноэкранный режим.
    pub fn render_frame_with_stats(&self) -> (RgbaBuffer, RenderStats) {
        let (width, height) = self.viewport().screen();

        if width == max_render::layout::WINDOW_WIDTH as u32
            && height == max_render::layout::WINDOW_HEIGHT as u32
        {
            if let Some(chrome) = self.assets().and_then(|assets| assets.chrome.as_ref()) {
                if chrome.is_ready() {
                    return self.render_with_chrome(chrome);
                }
            }
        }

        let style = TileStyle::default();
        let mut frame = RgbaBuffer::filled(width, height, style.unknown);
        let mut stats = RenderStats::default();

        // Поверхность: если загружены данные планеты — настоящие тайлы 64×64 из
        // `.WRL` (как в оригинале), иначе процедурная заглушка `TileStyle`.
        match self.assets() {
            Some(assets) if !assets.terrain.is_empty() => {
                stats.map_tiles = assets.terrain.render_into(&mut frame, self.viewport());
                stats.resource_icons = render_resources(
                    &mut frame,
                    assets,
                    &self.state().map,
                    self.viewport(),
                    &style,
                    self.state().player_team.0,
                    self.hud().panels.survey,
                );
            }
            _ => {
                stats.map_tiles =
                    render_map_into(&mut frame, &self.state().map, self.viewport(), &style);
            }
        }

        self.draw_units(&mut frame, self.viewport(), &mut stats);
        // Призрак стройплощадки (`ORDER_STATE_SELECT_SITE`) — поверх юнитов.
        self.draw_build_site_ghost(&mut frame, self.viewport(), &mut stats);

        // `BuildMenu` в полноэкранном режиме (без `FRAMEPIC`).
        self.draw_build_menu(&mut frame);
        // Окно сохранения/загрузки (P3) — модальное, как `AbstractMenu` в C++.
        self.draw_save_load(&mut frame);
        // Экран апгрейдов (P6).
        self.draw_upgrade_menu(&mut frame);
        // Экран исследований (P7) и окно помощи поверх него.
        self.draw_research_menu(&mut frame);
        // Диалог распределения добычи шахты (`AllocMenu`, `ALLOCFRM`).
        self.draw_alloc_menu(&mut frame);
        // Окно X-FER (`TransferMenu`, `XFERPIC`) — поверх карты.
        self.draw_transfer_menu(&mut frame);
        // Меню команд выделенного юнита (`GameManager_PopupButtons`).
        self.draw_unit_menu(&mut frame);
        // Диалог `Preferences`/`Setup` (`OptionsMenu`, `PREFSPIC`).
        self.draw_preferences(&mut frame);
        self.draw_help(&mut frame);

        if self.hud().hint.is_some() {
            let whole = PixelRect::new(0, 0, frame.width() as i32, frame.height() as i32);
            self.draw_status_message(&mut frame, whole);
        }

        (frame, stats)
    }

    /// Окно сообщения `MESSAGE_BOX_INFO` над картой — `MessageManager_
    /// DrawMessage` (`message_manager.cpp:144`): подсказка выделенного юнита
    /// (`GameManager_DrawUnitStatusMessage`).
    fn draw_status_message(&self, frame: &mut RgbaBuffer, area: PixelRect) {
        let Some(hint) = self.hud().hint.as_deref() else {
            return;
        };
        let Some(assets) = self.assets() else {
            return;
        };
        let palette = crate::panels::UiPalette::from_assets(assets)
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));
        let Some(palette) = palette else {
            return;
        };

        crate::panels::draw_status_message(frame, area, &palette, hint);
    }

    /// Кадр в окне 1:1: `FRAMEPIC` + карта в области карты.
    fn render_with_chrome(&self, chrome: &crate::chrome::Chrome) -> (RgbaBuffer, RenderStats) {
        let style = TileStyle::default();
        let mut frame = RgbaBuffer::filled(
            max_render::layout::WINDOW_WIDTH as u32,
            max_render::layout::WINDOW_HEIGHT as u32,
            style.unknown,
        );
        chrome.draw(&mut frame);

        // Карта рисуется в собственный буфер размером с область карты: так она
        // сама обрезается по краям `MAIN_MAP` (как в оригинале), а камера и зум
        // остаются общими с приложением. `map_viewport` — та же камера, которой
        // пользуется ввод мыши (`App::map_viewport`), иначе клики уезжают.
        let area = max_render::layout::MAIN_MAP;
        let viewport = self.map_viewport();
        let mut map = RgbaBuffer::filled(area.width as u32, area.height as u32, style.unknown);
        let mut stats = RenderStats::default();

        match self.assets() {
            Some(assets) if !assets.terrain.is_empty() => {
                stats.map_tiles = assets.terrain.render_into(&mut map, &viewport);
                stats.resource_icons = render_resources(
                    &mut map,
                    assets,
                    &self.state().map,
                    &viewport,
                    &style,
                    self.state().player_team.0,
                    self.hud().panels.survey,
                );
            }
            _ => {
                stats.map_tiles = render_map_into(&mut map, &self.state().map, &viewport, &style);
            }
        }

        self.draw_units(&mut map, &viewport, &mut stats);
        // Маркеры стройки — вокруг выделенного строителя/здания
        // (`DrawMap_RenderBuildMarker`), поверх юнитов и под призраком площадки.
        self.draw_build_markers(&mut map, &viewport, &mut stats);
        // Призрак стройплощадки — в область карты, как `GameManager_TempTape`
        // (`ORDER_STATE_SELECT_SITE`), до подтверждения клетки.
        self.draw_build_site_ghost(&mut map, &viewport, &mut stats);
        // Предпросмотр маршрута (зелёная линия RMB) — поверх юнитов, как
        // `GameManager_DrawMapView`: в оконном режиме `render_hud` не
        // вызывается (панели рисует `FRAMEPIC`), поэтому без этой строки
        // предпросмотр был виден только в полноэкранном fallback.
        if let Some(preview) = &self.hud.preview {
            crate::hud::draw_path_preview(&mut map, &viewport, preview);
        }
        crate::chrome::blit_map(&mut frame, &map);

        // Панели поверх карты: кнопки, портрет, статистика, сетка, зум,
        // координаты — как `GameManager_Draw*` в оригинале.
        if let Some(panels) = self.assets().and_then(|assets| assets.panels.as_ref()) {
            panels.draw(&mut frame, self);
        }

        // Подсказка выделенного строителя — окно сообщения `MESSAGE_BOX_INFO`
        // в начале области карты (`MessageManager_DrawMessage`).
        self.draw_status_message(&mut frame, area);

        // `BuildMenu` — модальный экран поверх кадра (C++ `AbstractMenu`
        // закрывает главное меню: `GameManager_DisableMainMenu`).
        self.draw_build_menu(&mut frame);
        // Окно сохранения/загрузки (P3).
        self.draw_save_load(&mut frame);
        // Экран апгрейдов (P6).
        self.draw_upgrade_menu(&mut frame);
        // Экран исследований (P7) и окно помощи поверх него.
        self.draw_research_menu(&mut frame);
        // Диалог распределения добычи шахты — полноэкранный `ALLOCFRM`.
        self.draw_alloc_menu(&mut frame);
        // Окно X-FER (`XFERPIC`) — над картой и панелями.
        self.draw_transfer_menu(&mut frame);
        // Меню команд выделенного юнита — над картой и панелями.
        self.draw_unit_menu(&mut frame);
        // Диалог `Preferences`/`Setup` (`OptionsMenu`, `PREFSPIC`).
        self.draw_preferences(&mut frame);
        self.draw_help(&mut frame);

        (frame, stats)
    }

    /// Рисует меню команд юнита поверх кадра (C++ `GameManager_InitPopupButtons`).
    ///
    /// Кнопки — те же `UNTBTN_U`/`UNTBTN_D`, что в C++; фон не закрашивается:
    /// в оригинале попап рисуется прямо над картой (окно `WINDOW_POPUP_BUTTONS`
    /// только ограничивает область перерисовки).
    pub(crate) fn draw_unit_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.unit_menu(), self.assets()) else {
            return;
        };
        let palette = UiPalette::from_assets(assets)
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));

        if let Some(palette) = palette {
            menu.draw(frame, assets, &palette);
        }
    }

    /// Рисует открытый экран апгрейдов поверх кадра (P6).
    pub(crate) fn draw_upgrade_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.upgrade_menu(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets, self.state());
    }

    /// Рисует открытый экран исследований поверх кадра (P7).
    pub(crate) fn draw_research_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.research_menu(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets, self.state());
    }

    /// Рисует открытый диалог распределения добычи шахты (C++ `AllocMenu`).
    pub(crate) fn draw_alloc_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.alloc_menu(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets);
    }

    /// Рисует открытое окно X-FER (C++ `TransferMenu`, `XFERPIC`).
    pub(crate) fn draw_transfer_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.transfer_menu(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets, self.state());
    }

    /// Рисует открытый диалог `Preferences`/`Setup` (C++ `OptionsMenu`).
    pub(crate) fn draw_preferences(&self, frame: &mut RgbaBuffer) {
        let (Some(dialog), Some(assets)) = (self.preferences_dialog(), self.assets()) else {
            return;
        };

        dialog.draw(frame, assets);
    }

    /// Рисует открытое окно помощи поверх кадра (P7).
    ///
    /// Палитра — палитра окна (`FRAMEPIC`), как у остальных модальных окон;
    /// если хрома нет — палитра меню.
    pub(crate) fn draw_help(&self, frame: &mut RgbaBuffer) {
        let (Some(help), Some(assets)) = (self.help_screen(), self.assets()) else {
            return;
        };
        let fallback = UiPalette::from_assets(assets)
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));

        help.draw(frame, assets.help.as_ref(), fallback);
    }

    /// Рисует открытое окно сохранения/загрузки поверх кадра (P3).
    pub(crate) fn draw_save_load(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.save_load(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets);
    }

    /// Рисует открытый `BuildMenu` поверх кадра (P2).
    pub(crate) fn draw_build_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.build_menu(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets, self.state(), self.hud().mouse_position);
    }

    /// Юниты в буфер через камеру: только видимые и известные команде.
    fn draw_units(
        &self,
        frame: &mut RgbaBuffer,
        viewport: &max_render::viewport::Viewport,
        stats: &mut RenderStats,
    ) {
        let mut positions = Vec::new();
        let mut visible = Vec::new();
        // Спрайт юнита может свисать в кадр из-за края, даже если его клетка
        // уже за границей видимого диапазона: берём запас в 2 клетки.
        let range = viewport.visible_tiles();
        let margin = 2;

        for id in self.state().unit_ids_in_order() {
            let Some(unit) = self.state().unit(id) else {
                stats.units_skipped += 1;
                continue;
            };
            // Видимость и порядок отрисовки — по визуальной позиции: пока
            // спрайт идёт по маршруту, симуляционная клетка уже может быть
            // впереди (`update_visual_units`).
            let (visual_x, visual_y) = self
                .visual_position(id)
                .unwrap_or((f32::from(unit.grid.x), f32::from(unit.grid.y)));
            let tile_x = visual_x.round() as i16;
            let tile_y = visual_y.round() as i16;
            let in_range = range.is_some_and(|range| {
                tile_x >= range.x0 - margin
                    && tile_y >= range.y0 - margin
                    && tile_x <= range.x1 + margin
                    && tile_y <= range.y1 + margin
            });
            if !in_range {
                continue;
            }
            if !unit.is_visible_to_team(self.state().player_team) {
                continue;
            }
            // `UnitInfoGroup::IsRelevant` (`unitinfogroup.cpp:59`): idle-юниты
            // не рисуются (кроме QuickBuild-превью, которого у нас нет), а на
            // мелком масштабе (`Gfx_ZoomLevel < 8`) не рисуется и `GROUND_COVER`.
            if unit.orders == max_core::enums::UnitOrder::Idle
                || (unit.flags & unit_flags::GROUND_COVER != 0 && viewport.zoom() < 8)
            {
                stats.units_skipped += 1;
                continue;
            }
            positions.push((tile_x, tile_y));
            visible.push(id);
        }

        for index in draw_order(&positions) {
            let Some(unit) = self.state().unit(visible[index]) else {
                continue;
            };
            // Визуальная позиция — дробная клетка, идущая по маршруту
            // (см. `App::update_visual_units`). Якорь — центр клетки в мировых
            // пикселях (у зданий `BUILDING` — +31, как `UnitInfo::SetPosition`),
            // затем камера переводит его в экранные.
            let (grid_x, grid_y) = self
                .visual_position(visible[index])
                .unwrap_or((f32::from(unit.grid.x), f32::from(unit.grid.y)));
            let (world_x, world_y) = unit_anchor_world(unit, (grid_x, grid_y));
            // `UnitInfo::PositionInTape` + `OffsetDrawZones` (`unitinfo.cpp:4478`):
            // конструктор сдвигается на 8 px за тик к центру 2×2-ленты, всего
            // на 32 px (половину клетки) — в центр стройплощадки. Это кадровый
            // сдвиг спрайта, сетка строителя не меняется.
            let (tape_offset_x, tape_offset_y) =
                crate::buildsite::tape_center_offset(self.state(), unit);
            let anchor = viewport.world_to_screen(world_x + tape_offset_x, world_y + tape_offset_y);

            let image_base = image_base_for(
                unit,
                self.state()
                    .teams
                    .get(unit.team.0 as usize)
                    .map(|team| team.team_clan),
            );
            // Угол корпуса — текущего визуального отрезка (`visual_angle`):
            // симуляция поворачивает юнита на клетке маршрута, а спрайт идёт по
            // ней с реальной скоростью; иначе кадр «забегал» бы вперёд и юнит
            // смотрел бы вбок, двигаясь прямо.
            let body_angle = self.visual_angle(visible[index]).unwrap_or(unit.angle);
            // Покадровая анимация строителя, `ANIMATED`-зданий и промежуточный
            // полный цикл кадров работающих шахт/заводов — визуальные часы
            // `App` (реальный `dt`), в снапшот/CRC они не входят. `dt = 0`
            // (QA/headless) даёт статичный кадр C++ (`+16` для строителя,
            // базовый/включённый кадр здания) — поэтому golden не дрейфуют.
            let animation = self.unit_animation(visible[index]);
            // Коннекторы комплекса (`UnitInfo::RenderWithConnectors`): сохранённая
            // маска из сейва ∪ вычисленная по соседям (см. [`drawn_connector_mask`]).
            let connectors = drawn_connector_mask(self.state(), unit);
            // C++ `settings["effects"]` (`settings.cpp:128`): `effects = false`
            // отключает покадровые анимации спрайтов (`UnitInfo::Animate`,
            // `units_manager.cpp:2857/3030`) — кадр статичен.
            let effects = self.preferences.effects;
            let zoom = self.unit_render_zoom(visible[index], viewport.zoom());
            let drawn = self.assets().and_then(|assets| {
                // Палитра кадра: снимок с палитровыми циклами или исходная
                // системная палитра (`dt = 0` — без изменений).
                let palette = self.animated_palette().unwrap_or(&assets.system_palette);
                draw_unit_sprites_with_animation(
                    assets, palette, unit, image_base, body_angle, animation, effects, connectors,
                    frame, anchor, zoom,
                )
            });

            match drawn {
                Some(_) => stats.units_drawn += 1,
                None => {
                    draw_marker(frame, anchor, unit.team.0, viewport.zoom());
                    stats.units_marker += 1;
                }
            }
        }
    }

    /// Призрак стройплощадки (`GameManager_TempTape`) в буфер карты.
    ///
    /// C++ на время `ORDER_STATE_SELECT_SITE` спавнит настоящую ленту
    /// `LRGTAPE`/`SMLTAPE` и двигает её за курсором; порт рисует **тот же
    /// спрайт** без записи в состояние партии — до LMB приказ не отдан, а
    /// счётчик юнитов/CRC/сейв не меняются.
    fn draw_build_site_ghost(
        &self,
        frame: &mut RgbaBuffer,
        viewport: &max_render::viewport::Viewport,
        stats: &mut RenderStats,
    ) {
        let (Some(site), Some(assets)) = (self.build_site(), self.assets()) else {
            return;
        };

        if site.draw_ghost(frame, assets, viewport).is_some() {
            stats.units_drawn += 1;
        }
    }

    /// Мигающие маркеры стройки `BLDMRK1..5` — `DrawMap_RenderBuildMarker`
    /// (`drawmap.cpp:353`) + `DrawMap_RedrawUnit` (`drawmap.cpp:304`).
    ///
    /// Рисуются, когда выделен юнит своей команды в `ORDER_IDLE` (достроенное
    /// здание ждёт менеджера) или мобильный строитель в
    /// `ORDER_BUILD`/`ORDER_STATE_UNIT_READY` (достроил и ждёт приказа уйти с
    /// площадки): символы ставятся на клетки вокруг **родителя** выделенного
    /// юнита (`Access_IsAccessible` фильтрует занятые), и клик по такой клетке —
    /// обычный приказ движения, который в этом состоянии отдаёт
    /// `ORDER_ACTIVATE` (`game_manager.cpp:6879`). Подсказка `_(276c)` прямо
    /// отсылает к этим символам.
    fn draw_build_markers(
        &self,
        frame: &mut RgbaBuffer,
        viewport: &max_render::viewport::Viewport,
        stats: &mut RenderStats,
    ) {
        let Some(selected) = self.hud().selected else {
            return;
        };
        let Some(unit) = self.state().unit(selected) else {
            return;
        };
        let Some(assets) = self.assets() else {
            return;
        };

        let cells = build_marker_cells(self.state(), unit);

        if cells.is_empty() {
            return;
        }

        let Some(image) = assets
            .build_markers
            .get(usize::from(self.build_marker_step))
            else {
            return;
        };
        let palette = window_palette(assets);
        let zoom = viewport.zoom() as i32;

        for cell in cells {
            let rect = viewport.tile_rect(cell.x, cell.y);

            if rect.width <= 0 || rect.height <= 0 {
                continue;
            }

            let centre = (rect.ulx + rect.width / 2, rect.uly + rect.height / 2);

            // `WindowManager_RescaleSimpleImage(image, Gfx_MapScalingFactor)`:
            // 62×62 — полный кадр клетки 64 px, масштаб `zoom/64`.
            if max_render::map_render::blit_simple_image(
                frame, image, &palette, centre, zoom, 64,
            ) > 0
            {
                stats.build_markers += 1;
            }
        }
    }
}

/// Клетки, на которых `DrawMap_RedrawUnit` (`drawmap.cpp:304`) рисует маркеры
/// стройки: вокруг **родителя** выделенного юнита.
///
/// Условие C++: `unit->team == PlayerTeam` и `(order == ORDER_IDLE ||
/// (order == ORDER_BUILD && state == ORDER_STATE_UNIT_READY && !(flags &
/// STATIONARY)))`, родитель есть и это не `AIRTRANS`. Старт —
/// `(parent.x - 1, parent.y + 1)`, для здания `(+1 по y, limit = 3)`, затем
/// четыре направления (`N, E, S, W` — `direction += 2`) по `limit` шагов,
/// накопительно; в список попадают только проходимые клетки
/// (`Access_IsAccessible(... AccessModifier_SameClassBlocks)`).
pub fn build_marker_cells(state: &max_core::game::GameState, unit: &max_core::unit::UnitInfo) -> Vec<max_core::geo::Point> {
    if unit.team != state.player_team {
        return Vec::new();
    }

    let eligible = unit.orders == UnitOrder::Idle
        || (unit.orders == UnitOrder::Build
            && unit.state == UnitOrderState::UnitReady
            && unit.flags & unit_flags::STATIONARY == 0);

    if !eligible {
        return Vec::new();
    }

    let Some(parent) = unit.parent_unit.and_then(|id| state.unit(id)) else {
        return Vec::new();
    };

    if parent.unit_type == UnitType::AirTransport {
        return Vec::new();
    }

    let building = parent.flags & unit_flags::BUILDING != 0;
    let mut position = max_core::geo::Point::new(parent.grid.x - 1, parent.grid.y + 1);
    let limit = if building {
        position.y += 1;
        3
    } else {
        2
    };
    let mut cells = Vec::new();

    for direction in (0..8).step_by(2) {
        for _ in 0..limit {
            position += max_core::geo::DIRECTION_OFFSETS[direction];

            if max_core::access::is_accessible(
                state,
                unit.unit_type,
                state.player_team,
                position,
                max_core::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
            ) > 0
            {
                cells.push(position);
            }
        }
    }

    cells
}

/// Системная палитра окна для simple image (маркеры ресурсов, курсоры).
///
/// В игре это палитра `FRAMEPIC` (её C++ ставит системной при загрузке хрома);
/// без хрома — `COLOR.PAL` как разумный fallback.
pub fn window_palette(assets: &crate::assets::Assets) -> PaletteRgba {
    if let Some(chrome) = assets.chrome.as_ref() {
        return UiPalette::from_big_image(&chrome.frame).palette_rgba();
    }

    let mut palette = [[0u8, 0, 0, 255]; 256];

    for (index, entry) in palette.iter_mut().enumerate() {
        let [r, g, b] = assets.palette.rgb(index as u8);

        *entry = [r, g, b, 255];
    }

    palette
}

/// Маркеры ресурсов: настоящие `RAWMSK*`/`FUELMK*`/`GOLDMK*`, иначе точки.
#[allow(clippy::too_many_arguments)]
fn render_resources(
    frame: &mut RgbaBuffer,
    assets: &crate::assets::Assets,
    map: &max_core::map::Map,
    viewport: &max_render::viewport::Viewport,
    style: &TileStyle,
    team: u8,
    show_all: bool,
) -> u32 {
    if assets.resource_markers.is_ready() {
        max_render::map_render::render_resource_icons_into(
            frame,
            map,
            viewport,
            &assets.resource_markers,
            &window_palette(assets),
            team,
            show_all,
        )
    } else {
        max_render::map_render::render_resources_into(frame, map, viewport, style)
    }
}

/// Маркер юнита 6×6 в точке `anchor` (центр клетки), масштабируется вместе
/// с картой (`zoom`, 64 — 1:1).
fn draw_marker(frame: &mut RgbaBuffer, anchor: (i32, i32), team: u8, zoom: u32) {
    let color = MARKER_COLORS
        .get(usize::from(team))
        .copied()
        .unwrap_or(MARKER_COLORS[4]);
    let radius = ((6 * i64::from(zoom.max(1))) / 64).max(2) as i32;

    for y in -radius..=radius {
        for x in -radius..=radius {
            frame.set(anchor.0 + x, anchor.1 + y, color);
        }
    }
}

/// Пишет [`RgbaBuffer`] в бинарный PPM (`P6`) — просмотр кадра без SDL.
///
/// PPM выбран потому, что формат — три строки заголовка и сырой RGB, без
/// кодеков и зависимостей: кадр из headless-прогона можно открыть любым
/// просмотрщиком изображений.
pub fn write_ppm(path: impl AsRef<Path>, buffer: &RgbaBuffer) -> io::Result<()> {
    let path = path.as_ref();
    let file = fs::File::create(path)?;
    let mut writer = io::BufWriter::new(file);

    write!(writer, "P6\n{} {}\n255\n", buffer.width(), buffer.height())?;

    let mut rgb = Vec::with_capacity(buffer.len() * 3);
    for pixel in buffer.pixels().chunks_exact(4) {
        rgb.extend_from_slice(&pixel[..3]);
    }

    writer.write_all(&rgb)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[test]
    fn frame_index_uses_the_given_body_angle_and_power_on_offset() {
        use max_core::enums::UnitType;
        use max_core::team::TeamId;
        use max_core::unit::{UnitId, UnitInfo};

        let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        unit.angle = 7;

        for angle in 0..8u8 {
            assert_eq!(
                unit_frame_index_for_angle(&unit, 10, 100, angle),
                10 + usize::from(angle),
                "кадр корпуса = image_base + angle"
            );
        }
        assert_eq!(unit_frame_index(&unit, 10, 100), 17);

        // `ORDER_POWER_ON` — следующий кадр включённого здания.
        unit.orders = UnitOrder::PowerOn;
        assert_eq!(unit_frame_index_for_angle(&unit, 0, 100, 2), 3);
        // Последний кадр не переполняется.
        assert_eq!(unit_frame_index_for_angle(&unit, 0, 3, 2), 2);
    }

    #[test]
    fn builder_work_cycle_matches_the_cpp_plus_eight_minus_sixteen_loop() {
        use max_core::enums::UnitType;
        use max_core::team::TeamId;
        use max_core::unit::{UnitId, UnitInfo};

        let mut builder = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId(1));
        builder.orders = UnitOrder::Build;
        builder.state = UnitOrderState::BuildInProgress;

        // CONSTRCT: 40 кадров, угол 3 → image_index = 3 + 16 = 19.
        // C++ ProcessOrderBuild: +8, +8, затем 35 + 8 >= 40 → 35 - 16 = 19.
        let cycle: Vec<usize> = (0..7)
            .map(|step| unit_frame_index_for_animation(&builder, 0, 40, 3, step % 3))
            .collect();
        assert_eq!(cycle, vec![19, 27, 35, 19, 27, 35, 19]);

        // Угол 0: кадры начала стройки +16 = 16, далее 24 и 32.
        let cycle: Vec<usize> = (0..3)
            .map(|step| unit_frame_index_for_animation(&builder, 0, 40, 0, step))
            .collect();
        assert_eq!(cycle, vec![16, 24, 32]);

        // У инженера цикла нет: рабочая поза +16 постоянна (StartBuilding).
        let engineer = {
            let mut unit = UnitInfo::new(UnitType::Engineer, TeamId::RED, UnitId(2));
            unit.orders = UnitOrder::Build;
            unit.state = UnitOrderState::BuildInProgress;
            unit
        };
        for step in 0..3 {
            assert_eq!(
                unit_frame_index_for_animation(&engineer, 0, 32, 3, step),
                19,
                "ENGINEER держит DrawSpriteFrame(image_index + 16)"
            );
        }

        // После стройки кадр возвращается к базовому независимо от шага.
        builder.orders = UnitOrder::Await;
        builder.state = UnitOrderState::ExecutingOrder;
        for step in 0..3 {
            assert_eq!(unit_frame_index_for_animation(&builder, 0, 40, 3, step), 3);
        }
    }

    #[test]
    fn scaler_scale_matches_the_cpp_divisor() {
        assert_eq!(scaler_scale(0), 1.0);
        assert_eq!(scaler_scale(1), 0.5);
        assert_eq!(scaler_scale(2), 0.25);
        assert_eq!(scaler_scale(3), 0.125);
        assert_eq!(scaler_scale(4), 0.0625);
        // `scaler_adjust` в C++ — uint8_t, но реально 0..4; климпим.
        assert_eq!(scaler_scale(9), 0.0625);
    }

    #[test]
    fn frame_without_assets_has_screen_size_and_is_opaque() {
        let app = App::new(AppConfig::default().with_screen(64, 48));
        let (frame, stats) = app.render_frame_with_stats();

        assert_eq!(frame.width(), 64);
        assert_eq!(frame.height(), 48);
        assert_eq!(frame.len(), 64 * 48);
        assert!(stats.map_tiles > 0, "карта должна рисоваться без ассетов");
        assert_eq!(stats.units(), stats.units_drawn + stats.units_marker);
        assert!(frame.pixels().chunks_exact(4).all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn enemy_units_are_rendered_only_when_visible() {
        use max_core::enums::UnitType;
        use max_core::geo::Point;
        use max_core::team::TeamId;

        let mut app = App::new(AppConfig::default());
        *app.state_mut() = max_core::game::GameState::new();

        let own = app
            .state_mut()
            .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(55, 55), 0)
            .expect("свой танк");
        let enemy = app
            .state_mut()
            .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(56, 55), 0)
            .expect("чужой танк");

        assert!(app
            .state()
            .unit(own)
            .unwrap()
            .is_visible_to_team(TeamId::RED));
        assert!(
            !app.state()
                .unit(enemy)
                .unwrap()
                .is_visible_to_team(TeamId::RED),
            "чужой юнит вне FOV не виден"
        );

        // Без ассетов юниты рисуются маркерами: свой — один.
        let (frame, stats) = app.render_frame_with_stats();
        assert_eq!(stats.units_marker, 1, "виден только свой юнит");
        assert_eq!(stats.units_drawn, 0);
        assert_eq!(
            frame.get(
                55 * 64 + 32 - app.viewport().origin().0,
                55 * 64 + 32 - app.viewport().origin().1
            ),
            Some(MARKER_COLORS[0]),
            "свой маркер на карте"
        );

        // Появился в FOV (разведка/радар) — рисуется и чужой.
        app.state_mut()
            .unit_mut(enemy)
            .unwrap()
            .set_visible_to_team(TeamId::RED, true);
        let (_, stats) = app.render_frame_with_stats();
        assert_eq!(stats.units_marker, 2, "видимый чужой юнит рисуется");
    }

    #[test]
    fn ppm_header_is_p6_with_frame_dimensions() {
        let path = std::env::temp_dir().join("max_app_render_test.ppm");
        let frame = RgbaBuffer::filled(3, 2, Rgba::rgb(1, 2, 3));

        write_ppm(&path, &frame).expect("ppm written");
        let bytes = std::fs::read(&path).expect("ppm read back");
        let header = b"P6\n3 2\n255\n";

        assert_eq!(&bytes[..header.len()], header);
        assert_eq!(bytes.len(), header.len() + 3 * 2 * 3);
        assert_eq!(&bytes[header.len()..header.len() + 3], &[1, 2, 3]);

        let _ = std::fs::remove_file(&path);
    }
}
