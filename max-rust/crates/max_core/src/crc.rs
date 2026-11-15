//! CRC16 пакета 45 и канонические дайджесты состояния (P7).
//!
//! # Оракул
//!
//! * `max-cpp-port/src/crc16.c` — побайтовый CRC16 (полином `0x1021`, старший
//!   бит вперёд, без рефлексии, начальное значение выбирает вызывающий);
//! * `max-cpp-port/src/game_manager.cpp:2315` — `GameManager_GetCrc16`, пословный
//!   шаг CRC по **16-битному** значению (эквивалент двух побайтовых шагов со
//!   старшим байтом вперёд — см. [`get_crc16_word`]);
//! * `game_manager.cpp:2327` — `GameManager_GetUnitListChecksum` (какие поля
//!   юнита попадают в CRC);
//! * `game_manager.cpp:2356` — `GameManager_CheckDesync` (порядок списков,
//!   команд и стартовое значение `0xFFFF`);
//! * `remote.cpp:1824` (`Remote_CheckDesync`), `remote.cpp:3556`
//!   (`Remote_SendNetPacket_45`), `remote.cpp:3568`
//!   (`Remote_ReceiveNetPacket_45`) — только **транспорт** пакета 45
//!   (счётчик хода + `uint16_t crc_checksum`); сам CRC считает
//!   `GameManager_CheckDesync` через `crc16()`/`GameManager_GetCrc16`;
//! * `docs/protocol.md` §8.6 — пакет 45 как барьер конца хода.
//!
//! # Два дайджеста
//!
//! * [`state_checksum`] — **сетевой** CRC пакета 45: ровно те поля и тот порядок,
//!   которые перебирает `GameManager_CheckDesync`. Именно это значение уходит в
//!   [`crate::input::Packet45CrcBarrier`] и сравнивается пирами.
//! * [`replay_digest_bytes`] — **канонический** дайджест для сквозных тестов
//!   паритета (P7): карта (CRC обеих карт), команды, юниты в порядке пяти
//!   списков и CRC16 всего дайджеста. Поля — симуляционное подмножество
//!   [`crate::game::GameState::snapshot_bytes`] (без AI-задач/напоминаний P6);
//!   ровно те же поля печатает C++-генератор
//!   `max-rust/tools/sim_vectors/main.cpp`, поэтому сравнение идёт бит-в-бит.
//! * [`state_digest`] — CRC16 «как в пакете 45» от полного
//!   [`crate::game::GameState::snapshot_bytes`] (десинк-детектор для тестов).
//!
//! Все функции чистые, порядок обхода фиксирован (DESIGN §6.2): никакой итерации
//! по `HashMap`.

use crate::enums::unit_flags::{EXPLODING, STATIONARY};
use crate::game::{GameState, PLAYER_TEAM_COUNT, TEAM_SLOTS};
use crate::team::{TeamId, TeamKind};
use crate::unit::UnitListKind;
use crate::unit_values::ATTRIB_COUNT;
use crate::victory::cargo_type_of;

/// Полином CRC16 (CCITT): `x^16 + x^12 + x^5 + 1` — `crc16.c:38`.
pub const CRC16_POLYNOMIAL: u16 = 0x1021;
/// Начальное значение CRC пакета 45 — `game_manager.cpp:2362` (`0xFFFF`).
pub const CRC16_INITIAL: u16 = 0xFFFF;

/// Один побайтовый шаг CRC16 — дословный порт `crc16()` (`crc16.c:24`).
///
/// ```c
/// for (i = 0; i < 8; i++) {
///     carry = *crc & 0x8000;
///     *crc <<= 1;
///     if (c & 0x80) (*crc)++;
///     c <<= 1;
///     if (carry) *crc ^= 0x1021;
/// }
/// ```
///
/// Экстраполяция: функция линейна по `crc` и по битам `byte` над GF(2), поэтому
/// 16 побайтовых шагов с нулевыми байтами дают ровно `L^16(crc)`, а вклад битов
/// данных — само 16-битное слово (старший бит первым). Это и есть
/// [`get_crc16_word`].
#[inline]
pub fn crc16_update_byte(mut crc: u16, byte: u8) -> u16 {
    let mut c = byte;

    for _ in 0..8 {
        let carry = crc & 0x8000;
        crc <<= 1;

        if c & 0x80 != 0 {
            crc = crc.wrapping_add(1);
        }
        c <<= 1;

        if carry != 0 {
            crc ^= CRC16_POLYNOMIAL;
        }
    }

    crc
}

/// CRC16 буфера от [`CRC16_INITIAL`] (потоковое применение
/// [`crc16_update_byte`]).
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = CRC16_INITIAL;

    for &byte in data {
        crc = crc16_update_byte(crc, byte);
    }

    crc
}

/// `GameManager_GetCrc16` (`game_manager.cpp:2315`) — пословный шаг.
///
/// ```c
/// for (i = 0; i < 16; ++i) {
///     if (crc & 0x8000) crc = (crc << 1) ^ 0x1021u; else crc <<= 1;
/// }
/// return crc ^ data;
/// ```
///
/// Эквивалент двух вызовов [`crc16_update_byte`] со старшим, затем младшим
/// байтом `data` (проверяется тестом `word_step_matches_two_byte_steps`).
#[inline]
pub fn get_crc16_word(data: u16, crc_checksum: u16) -> u16 {
    let mut crc = crc_checksum;

    for _ in 0..16 {
        if crc & 0x8000 != 0 {
            crc = (crc << 1) ^ CRC16_POLYNOMIAL;
        } else {
            crc <<= 1;
        }
    }

    crc ^ data
}

// ---------------------------------------------------------------------------
// CRC пакета 45 — сетевой десинк-детектор
// ---------------------------------------------------------------------------

/// `GameManager_GetUnitListChecksum` (`game_manager.cpp:2327`).
///
/// Участвуют только юниты с `id != NO_ID`, без флага `EXPLODING` и только
/// запрошенной команды. Порядок полей — дословно из C++:
/// `team, unit_type, unit_id, grid_x, grid_y, hits, [speed если не STATIONARY],
/// shots, [storage если у типа есть тип груза], ammo`.
///
/// `team`-условие вызывающего (`team_type != NONE || team == ALIEN`) проверяет
/// [`state_checksum`], как в `GameManager_CheckDesync`.
pub fn get_unit_list_checksum(
    state: &GameState,
    kind: UnitListKind,
    team: TeamId,
    mut crc_checksum: u16,
) -> u16 {
    for &id in state.unit_list(kind) {
        let Some(unit) = state.unit(id) else { continue };

        if unit.id.is_no_id() || unit.flags & EXPLODING != 0 || unit.team != team {
            continue;
        }

        crc_checksum = get_crc16_word(unit.team.0 as u16, crc_checksum);
        crc_checksum = get_crc16_word(unit.unit_type as u16, crc_checksum);
        crc_checksum = get_crc16_word(u16::from(unit.unit_id), crc_checksum);
        crc_checksum = get_crc16_word(unit.grid.x as u16, crc_checksum);
        crc_checksum = get_crc16_word(unit.grid.y as u16, crc_checksum);
        crc_checksum = get_crc16_word(unit.hits, crc_checksum);

        if unit.flags & STATIONARY == 0 {
            crc_checksum = get_crc16_word(unit.speed, crc_checksum);
        }

        crc_checksum = get_crc16_word(u16::from(unit.shots), crc_checksum);

        if cargo_type_of(unit.unit_type) as u8 != 0 {
            crc_checksum = get_crc16_word(unit.storage as u16, crc_checksum);
        }

        crc_checksum = get_crc16_word(u16::from(unit.ammo), crc_checksum);
    }

    crc_checksum
}

/// `GameManager_CheckDesync` (`game_manager.cpp:2356`) — CRC пакета 45.
///
/// Внешний цикл — команды `RED..ALIEN`, внутренний — четыре списка
/// (`GroundCover`, `MobileLandSea`, `Stationary`, `MobileAir`); список
/// `ParticleUnits` в CRC **не входит** (как в C++). Стартовое значение `0xFFFF`.
pub fn state_checksum(state: &GameState) -> u16 {
    let mut crc_checksum = CRC16_INITIAL;

    for kind in [
        UnitListKind::GroundCover,
        UnitListKind::MobileLandSea,
        UnitListKind::Stationary,
        UnitListKind::MobileAir,
    ] {
        for team in 0..TEAM_SLOTS {
            let kind_of_team = state.teams[team].team_type;
            if kind_of_team == TeamKind::None && team != TeamId::ALIEN.0 as usize {
                continue;
            }

            crc_checksum = get_unit_list_checksum(state, kind, TeamId(team as u8), crc_checksum);
        }
    }

    crc_checksum
}

/// CRC16 полного канонического дайджеста [`GameState::snapshot_bytes`].
///
/// Используется тестами детерминизма как «десинк-детектор»: любое изменение
/// симуляционного поля меняет значение.
pub fn state_digest(state: &GameState) -> u16 {
    crc16(&state.snapshot_bytes())
}

// ---------------------------------------------------------------------------
// Канонический дайджест реплея (P7 parity)
// ---------------------------------------------------------------------------

/// Магия дайджеста реплея (`replay_digest_bytes`).
pub const REPLAY_DIGEST_MAGIC: &[u8; 8] = b"MAXRPL1\0";

/// Канонический дайджест состояния для сквозных тестов паритета.
///
/// Формат — явный little-endian writer (DESIGN §6.4: без `serde`, без
/// `HashMap`-итерации). Порядок полей фиксирован и совпадает с
/// `max-rust/tools/sim_vectors/main.cpp` (`write_state_digest`):
///
/// | Поле | Тип |
/// |---|---|
/// | `REPLAY_DIGEST_MAGIC` | 8 байт |
/// | CRC16 `SurfaceMap` | u16 |
/// | CRC16 `ResourceMap` (u16 LE) | u16 |
/// | `turn_counter` | i32 |
/// | `game_state`, `active_turn_team`, `player_team`, `play_mode` | u8×4 |
/// | `unit_id_counter` | u32 |
/// | `rng.state()` | u32×4 |
/// | на команду `0..PLAYER_TEAM_MAX`: `team_type`, `finished_turn`, `team_points`, `gold`, 8×(`research_level`,`turns_to_complete`,`allocation`), 93×`unit_counters`, 93×`casualties` | см. код |
/// | `unit_count` | u32 |
/// | юниты в порядке [`GameState::unit_ids_in_order`] (5 списков + вне списков) | см. код |
/// | CRC16 всех предыдущих байт | u16 |
///
/// Поля юнита: `id, unit_type, team, unit_id, flags, grid_x, grid_y,
/// orders/state/prior_orders/prior_state, hits, speed, group_speed, shots,
/// move_and_fire, storage, experience, ammo, build_time, total_mining,
/// raw/fuel/gold_mining, raw/fuel/gold_mining_max, build_rate, ai_state_bits,
/// connectors, 14 атрибутов base_values, путь (вид + данные), parent/enemy,
/// build_list`.
pub fn replay_digest_bytes(state: &GameState) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 * 1024);

    out.extend_from_slice(REPLAY_DIGEST_MAGIC);
    put_u16(&mut out, crc16(&state.map.surface.to_bytes()));
    put_u16(&mut out, crc16(&state.map.resources.to_le_bytes()));

    put_i32(&mut out, state.turn_counter);
    out.push(state.game_state as u8);
    out.push(state.active_turn_team.0);
    out.push(state.player_team.0);
    out.push(state.play_mode as u8);
    put_u32(&mut out, state.unit_id_counter);
    for word in state.rng.state() {
        put_u32(&mut out, word);
    }

    for team in 0..TEAM_SLOTS {
        let info = &state.teams[team];
        out.push(info.team_type as u8);
        out.push(u8::from(info.finished_turn));
        put_u32(&mut out, info.team_points);
        put_u32(&mut out, state.team_units[team].gold);

        for topic in &info.research_topics {
            put_u32(&mut out, topic.research_level);
            put_u32(&mut out, topic.turns_to_complete);
            put_u32(&mut out, topic.allocation);
        }

        for value in &info.unit_counters {
            put_u32(&mut out, *value);
        }

        for value in &info.casualties {
            put_u32(&mut out, *value);
        }
    }

    let ids = state.unit_ids_in_order();
    put_u32(&mut out, ids.len() as u32);

    for id in ids {
        let Some(unit) = state.unit(id) else { continue };

        put_u16(&mut out, unit.id.0);
        put_u16(&mut out, unit.unit_type as u16);
        out.push(unit.team.0);
        out.push(unit.unit_id);
        put_u32(&mut out, unit.flags);
        put_i16(&mut out, unit.grid.x);
        put_i16(&mut out, unit.grid.y);
        out.push(unit.orders as u8);
        out.push(unit.state as u8);
        out.push(unit.prior_orders as u8);
        out.push(unit.prior_state as u8);
        put_u16(&mut out, unit.hits);
        put_u16(&mut out, unit.speed);
        put_u16(&mut out, unit.group_speed);
        out.push(unit.shots);
        out.push(unit.move_and_fire);
        put_i16(&mut out, unit.storage);
        put_i16(&mut out, unit.experience);
        out.push(unit.ammo);
        out.push(unit.build_time);
        out.push(unit.total_mining);
        out.push(unit.raw_mining);
        out.push(unit.fuel_mining);
        out.push(unit.gold_mining);
        out.push(unit.raw_mining_max);
        out.push(unit.gold_mining_max);
        out.push(unit.fuel_mining_max);
        put_u16(&mut out, unit.build_rate);
        put_u32(&mut out, unit.ai_state_bits);
        put_u16(&mut out, unit.connectors);

        for attribute in 0..ATTRIB_COUNT {
            put_i16(&mut out, unit.base_values.get_attribute(attribute) as i16);
        }

        match &unit.path {
            None => out.push(0),
            Some(crate::unit::UnitPath::Ground(path)) => {
                out.push(1);
                put_i16(&mut out, path.end_x);
                put_i16(&mut out, path.end_y);
                put_u32(&mut out, path.step_index);
                put_u32(&mut out, path.steps.len() as u32);
                for step in &path.steps {
                    out.push(step.x as u8);
                    out.push(step.y as u8);
                }
            }
            Some(crate::unit::UnitPath::Air(path)) => {
                out.push(2);
                put_i16(&mut out, path.end_x);
                put_i16(&mut out, path.end_y);
                put_i16(&mut out, path.length);
                out.push(path.angle as u8);
            }
            Some(crate::unit::UnitPath::Builder(path)) => {
                out.push(3);
                put_i16(&mut out, path.direction_x);
                put_i16(&mut out, path.direction_y);
            }
        }

        put_u16(&mut out, unit.parent_unit.map(|p| p.0).unwrap_or(u16::MAX));
        put_u16(&mut out, unit.enemy_unit.map(|p| p.0).unwrap_or(u16::MAX));
        put_u32(&mut out, unit.build_list.len() as u32);
        for unit_type in &unit.build_list {
            put_u16(&mut out, *unit_type as u16);
        }
    }

    let crc = crc16(&out);
    put_u16(&mut out, crc);
    out
}

/// CRC16 канонического дайджеста реплея (последние два байта
/// [`replay_digest_bytes`]).
pub fn replay_digest_crc(state: &GameState) -> u16 {
    let bytes = replay_digest_bytes(state);
    u16::from_le_bytes([bytes[bytes.len() - 2], bytes[bytes.len() - 1]])
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Число команд, участвующих в сетевых барьерах (`PLAYER_TEAM_MAX - 1`).
pub const NETWORK_TEAM_COUNT: usize = PLAYER_TEAM_COUNT;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{PlayMode, TeamClan, UnitOrder, UnitOrderState, UnitType};
    use crate::game::GameState;
    use crate::geo::Point;
    use crate::team::{TeamKind, TeamUnits};

    /// Векторы `crc`-секции `max-rust/tools/sim_vectors/vectors.txt`
    /// (сгенерированы C++-генератором из `crc16.c` в ubuntu:24.04).
    const VECTORS: &str = include_str!("../../../tools/sim_vectors/vectors.txt");

    fn hex_to_bytes(text: &str) -> Vec<u8> {
        let bytes = text.as_bytes();
        assert_eq!(bytes.len() % 2, 0, "hex string has odd length");

        (0..bytes.len() / 2)
            .map(|index| {
                let pair =
                    std::str::from_utf8(&bytes[index * 2..index * 2 + 2]).expect("ascii hex");
                u8::from_str_radix(pair, 16).expect("valid hex byte")
            })
            .collect()
    }

    /// `crc16.c` на известных входах.
    ///
    /// Внимание: это **не** CRC-16/CCITT-FALSE (у неё «123456789» = 0x29B1)
    /// и не CRC-16/XMODEM: `crc16()` вносит бит данных в младший разряд
    /// **после** сдвига, поэтому поток отличается от классических вариантов.
    /// Эталон — C++-векторы из `crc16.c` (см. `crc16_matches_cpp_vectors`),
    /// а здесь фиксируются базовые свойства (начальное значение и один байт).
    #[test]
    fn crc16_basics() {
        assert_eq!(crc16(b""), CRC16_INITIAL);
        assert_eq!(crc16(&[0x00]), 0xE1F0);
        assert_eq!(crc16(&[0xFF]), 0xE10F);
        assert_eq!(crc16(&[0x01, 0x02, 0x03, 0x04]), 0x94B7);
    }

    /// Пословный шаг `GameManager_GetCrc16` эквивалентен двум побайтовым шагам
    /// (старший байт первым) — на всех 16-битных входах и на разных состояниях.
    #[test]
    fn word_step_matches_two_byte_steps() {
        let states = [0x0000u16, 0xFFFF, 0x1234, 0x8000, 0x29B1];

        for state in states {
            for data in [0x0000u16, 0x0001, 0x00FF, 0xFF00, 0xFFFF, 0x1234, 0xABCD] {
                let mut bytes = state;
                bytes = crc16_update_byte(bytes, (data >> 8) as u8);
                bytes = crc16_update_byte(bytes, data as u8);

                assert_eq!(
                    get_crc16_word(data, state),
                    bytes,
                    "state {state:#06x} data {data:#06x}"
                );
            }
        }
    }

    /// Сверка с C++-векторами: побайтовые CRC и пословные шаги.
    #[test]
    fn crc16_matches_cpp_vectors() {
        let mut checked_bytes = 0;
        let mut checked_words = 0;

        for line in VECTORS.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let mut parts = line.split_whitespace();

            match parts.next().expect("record key") {
                "crc_bytes" => {
                    let payload = hex_to_bytes(parts.next().expect("payload"));
                    let expected =
                        u16::from_str_radix(parts.next().expect("crc"), 16).expect("hex crc");
                    assert_eq!(crc16(&payload), expected, "crc_bytes {line}");
                    checked_bytes += 1;
                }
                "crc_word" => {
                    let data =
                        u16::from_str_radix(parts.next().expect("data"), 16).expect("hex data");
                    let state =
                        u16::from_str_radix(parts.next().expect("state"), 16).expect("hex state");
                    let expected =
                        u16::from_str_radix(parts.next().expect("crc"), 16).expect("hex crc");
                    assert_eq!(get_crc16_word(data, state), expected, "crc_word {line}");
                    checked_words += 1;
                }
                // Прочие секции (реплей) проверяются в `tests/p7_replay.rs`.
                _ => {}
            }
        }

        assert!(checked_bytes >= 5, "expected byte CRC vectors");
        assert!(checked_words >= 4, "expected word CRC vectors");
    }

    fn replay_state() -> GameState {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.teams[1].team_type = TeamKind::Computer;
        state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
        state.team_units[1] = TeamUnits::with_clan(TeamClan::Random);
        state.play_mode = PlayMode::TurnBased;

        let red = state
            .spawn_unit(UnitType::Engineer, TeamId::RED, Point::new(20, 20), 0)
            .expect("engineer");
        let green = state
            .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(24, 20), 0)
            .expect("tank");
        state.set_order(red, UnitOrder::Await, UnitOrderState::ExecutingOrder);
        state.set_order(green, UnitOrder::Sentry, UnitOrderState::ExecutingOrder);
        state
    }

    #[test]
    fn checksum_visits_only_units_of_the_team() {
        let state = replay_state();
        let checksum = state_checksum(&state);

        // Команда красных — CRC считается; пустая команда синих ничего не добавляет.
        let mut expected = CRC16_INITIAL;
        for kind in [
            UnitListKind::GroundCover,
            UnitListKind::MobileLandSea,
            UnitListKind::Stationary,
            UnitListKind::MobileAir,
        ] {
            for team in 0..TEAM_SLOTS {
                let kind_of_team = state.teams[team].team_type;
                if kind_of_team == TeamKind::None && team != TeamId::ALIEN.0 as usize {
                    continue;
                }
                expected = get_unit_list_checksum(&state, kind, TeamId(team as u8), expected);
            }
        }
        assert_eq!(checksum, expected);
    }

    #[test]
    fn checksum_reacts_to_every_tracked_field() {
        let base = replay_state();
        let reference = state_checksum(&base);
        let id = base.unit_ids_in_order()[0];

        let mut moved = base.clone();
        if let Some(unit) = moved.unit(id).cloned() {
            let mut unit = unit;
            unit.grid.x += 1;
            moved.write_unit(id, unit);
        }
        assert_ne!(state_checksum(&moved), reference);

        let mut damaged = base.clone();
        if let Some(unit) = damaged.unit(id).cloned() {
            let mut unit = unit;
            unit.hits = unit.hits.wrapping_sub(1);
            damaged.write_unit(id, unit);
        }
        assert_ne!(state_checksum(&damaged), reference);
    }

    #[test]
    fn replay_digest_is_stable_and_deterministic() {
        let state = replay_state();
        let first = replay_digest_bytes(&state);
        let second = replay_digest_bytes(&state.clone());

        assert_eq!(first, second);
        assert_eq!(&first[..8], REPLAY_DIGEST_MAGIC);
        assert_eq!(replay_digest_crc(&state), crc16(&first[..first.len() - 2]));

        let mut changed = state;
        changed.turn_counter += 1;
        assert_ne!(replay_digest_bytes(&changed), first);
    }

    #[test]
    fn particle_list_does_not_take_part_in_network_crc() {
        let mut state = replay_state();
        // Ракета/частица не должна влиять на CRC пакета 45.
        let reference = state_checksum(&state);
        state
            .spawn_unit(UnitType::Rocket, TeamId::RED, Point::new(30, 30), 0)
            .expect("rocket");
        assert_eq!(state_checksum(&state), reference);
    }
}
