//! P6: сквозные тесты ИИ (DESIGN.md §7, критерии P6).
//!
//! Проверяются:
//!
//! 1. ИИ-ход на маленькой карте: один конструктор и руда рядом → ИИ создаёт
//!    `TaskManageBuildings` → `TaskCreateBuilding(MININGST)` →
//!    `TaskObtainUnits(CONSTRCT)` и отдаёт конструктору `ORDER_BUILD` с
//!    `build_list = [MININGST]`.
//! 2. Армия атакует врага: танк ИИ замечает врага, `AiPlayer::DetermineAttack`
//!    создаёт `TaskAttack`, и враг получает урон.
//! 3. Детерминизм: два одинаковых прогона дают одинаковый дайджест состояния
//!    (включая состояние ИИ — задачи, напоминания, карты).
//! 4. Переходы задачи: `CreateBuilding` проходит состояния
//!    `INITIALIZING → GETTING_BUILDER → … → BUILDING`, напоминания
//!    обрабатываются полностью.
//!
//! # Модель хода в тесте
//!
//! Сетевой слой (тики, барьеры, `FinishedTurn`) — не часть max_core, поэтому
//! тест сам проигрывает ход так, как это делает `game.rs`:
//! `manage_economy` (сброс скорости/выстрелов) → `ai_begin_turn` → N тиков
//! приказов → `EndTurn` + `finish_turn`.

use max_core::ai::{TaskKind, TASK_TYPE_CREATE_BUILDING, TASK_TYPE_MOVE};
use max_core::enums::{
    unit_flags, GameStateType, Opponent, Surface, TeamClan, UnitOrder, UnitOrderState, UnitType,
};
use max_core::game::PLAYER_TEAM_COUNT;
use max_core::geo::Point;
use max_core::map::{Map, ResourceKind, ResourceMapEntry};
use max_core::team::{TeamId, TeamKind};
use max_core::victory;
use max_core::GameState;

/// Маленькая карта: суша 24×24 в левом верхнем углу, остальное — вода.
fn small_land_map() -> Map {
    let mut map = Map::new();

    for y in 0..max_core::geo::MAP_HEIGHT {
        for x in 0..max_core::geo::MAP_WIDTH {
            let surface = if x < 24 && y < 24 {
                Surface::LAND
            } else {
                Surface::WATER
            };

            map.surface.set(x, y, surface);
        }
    }

    map
}

/// ИИ-компьютер за красных; остальные команды — `None`.
fn computer_state() -> GameState {
    let map = small_land_map();
    let mut state = GameState::with_setup(
        map,
        [
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
            TeamKind::None,
        ],
    );

    state.teams[0].team_type = TeamKind::Computer;
    state.teams[0].team_clan = TeamClan::Musashi;
    state.ai.settings.opponent = Opponent::Average as u8;
    state
}

/// Кладёт залежь материалов в клетку.
fn put_ore(state: &mut GameState, site: Point, amount: u8) {
    state.map.resources.set(
        site.x,
        site.y,
        ResourceMapEntry::new(amount, Some(ResourceKind::Raw), 0x0F),
    );
}

/// Задачи команды по типу.
fn tasks_of_type(state: &GameState, team: TeamId, task_type: u8) -> Vec<u32> {
    state
        .ai
        .task_manager
        .tasks()
        .iter()
        .filter(|task| task.team == team && task.get_type() == task_type)
        .map(|task| task.id)
        .collect()
}

/// Один ход команды: экономика → `Ai_BeginTurn` → `ticks` тиков → конец хода.
fn play_turn(state: &mut GameState, ticks: usize) {
    let team = TeamId::RED;

    state.manage_economy(team);
    state.ai_begin_turn(team);

    for _ in 0..ticks {
        state.tick();
    }

    state.game_state = GameStateType::EndTurn;
    let _ = state.finish_turn();
}

#[test]
fn ai_constructor_builds_mining_station() {
    let mut state = computer_state();
    put_ore(&mut state, Point::new(12, 11), 20);
    put_ore(&mut state, Point::new(13, 11), 20);

    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor spawned");

    // `Ai_SetupStrategy` — только стратегия (без перегенерации карты).
    assert!(
        max_core::ai_setup_strategy(&mut state, TeamId::RED),
        "strategy must be selected for a viable map"
    );

    let mut saw_create_building = false;
    let mut saw_move = false;
    let mut max_op_state = 0u8;

    // Наблюдение — покадрово: `TaskMove` живёт всего несколько тиков (ИИ
    // заканчивает ход, как только юниты перестали работать), поэтому проверка
    // на границе хода его пропускала.
    for _ in 0..12 {
        state.manage_economy(TeamId::RED);
        state.ai_begin_turn(TeamId::RED);

        for _ in 0..24 {
            state.tick();

            if !tasks_of_type(&state, TeamId::RED, TASK_TYPE_CREATE_BUILDING).is_empty() {
                saw_create_building = true;
            }

            if !tasks_of_type(&state, TeamId::RED, TASK_TYPE_MOVE).is_empty() {
                saw_move = true;
            }

            for task in state.ai.task_manager.tasks() {
                if let TaskKind::CreateBuilding { op_state, .. } = &task.kind {
                    max_op_state = max_op_state.max(*op_state);
                }
            }
        }

        state.game_state = GameStateType::EndTurn;
        let _ = state.finish_turn();

        let building = state
            .unit(constructor)
            .map(|unit| {
                unit.orders == UnitOrder::Build
                    && unit.build_list.first().copied() == Some(UnitType::MiningStation)
            })
            .unwrap_or(false);

        if building {
            break;
        }
    }

    assert!(
        saw_create_building,
        "AI must create TaskCreateBuilding for the mining station"
    );
    assert!(saw_move, "TaskMove must bring the builder to the site");
    assert!(
        max_op_state >= max_core::ai::tasks::create_building_state::MOVING_TO_SITE,
        "TaskCreateBuilding must progress past INITIALIZING, got {max_op_state}"
    );

    let constructor_unit = state.unit(constructor).expect("constructor alive");

    assert_eq!(
        constructor_unit.build_list.first().copied(),
        Some(UnitType::MiningStation),
        "constructor must be ordered to build a mining station"
    );
    assert_eq!(
        constructor_unit.orders,
        UnitOrder::Build,
        "constructor must hold ORDER_BUILD"
    );
    assert!(
        max_op_state >= max_core::ai::tasks::create_building_state::BUILDING,
        "TaskCreateBuilding must reach BUILDING, got {max_op_state}"
    );
}

#[test]
fn ai_army_attacks_enemy() {
    let mut state = computer_state();

    let attacker = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
        .expect("red tank");
    let defender = state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(13, 10), 0)
        .expect("green tank");

    // Врага видно красным: помечаем видимость и сообщаем ИИ.
    let mut enemy = state.unit(defender).expect("green tank exists").clone();
    enemy.set_visible_to_team(TeamId::RED, true);
    state.write_unit(defender, enemy);
    max_core::ai_unit_spotted(&mut state, defender, TeamId::RED);

    let hits_before = state.unit(defender).expect("green tank").hits;

    let mut attacked = false;

    for _ in 0..12 {
        play_turn(&mut state, 24);

        let hits_now = state.unit(defender).map(|unit| unit.hits).unwrap_or(0);
        if hits_now < hits_before {
            attacked = true;
            break;
        }
    }

    assert!(
        attacked,
        "AI army must damage the spotted enemy (attacker {}, hits before {})",
        attacker.0, hits_before
    );

    // `TaskAttack`/`TaskKillUnit` — для «задачных» целей (ключевые объекты,
    // конструктор); одиночный танк обрабатывается реакцией `AiAttack_EvaluateAttack`
    // (напоминание `RemindAttack`), поэтому критерий теста — нанесённый урон.
    assert!(
        state
            .unit(defender)
            .map(|unit| unit.hits < hits_before)
            .unwrap_or(true),
        "enemy must stay damaged or be destroyed"
    );
}

#[test]
fn ai_two_runs_have_identical_digest() {
    fn run() -> Vec<u8> {
        let mut state = computer_state();
        put_ore(&mut state, Point::new(12, 11), 20);

        let _constructor = state
            .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
            .expect("constructor");
        let enemy = state
            .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(20, 20), 0)
            .expect("green tank");

        let mut enemy_unit = state.unit(enemy).expect("enemy").clone();
        enemy_unit.set_visible_to_team(TeamId::RED, true);
        state.write_unit(enemy, enemy_unit);
        max_core::ai_unit_spotted(&mut state, enemy, TeamId::RED);

        max_core::ai_setup_strategy(&mut state, TeamId::RED);

        for _ in 0..8 {
            play_turn(&mut state, 16);
        }

        state.snapshot_bytes()
    }

    let first = run();
    let second = run();

    assert_eq!(
        first, second,
        "two identical AI runs must produce the same state digest"
    );
}

#[test]
fn ai_turns_advance_and_reminders_are_processed() {
    let mut state = computer_state();

    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");

    play_turn(&mut state, 8);

    // Очередь напоминаний детерминированно разбирается: `deferred`-хвост
    // (перенос на следующий вызов `ExecuteReminders`) ограничен числом задач.
    assert!(
        state.ai.task_manager.reminders_count() <= state.ai.task_manager.task_count() * 2 + 1,
        "reminder queues must stay bounded ({} reminders, {} tasks)",
        state.ai.task_manager.reminders_count(),
        state.ai.task_manager.task_count()
    );

    // Менеджер застройки и стартовые задачи живы.
    assert!(
        state
            .ai
            .task_manager
            .tasks()
            .iter()
            .any(|task| matches!(task.kind, TaskKind::ManageBuildings { .. })),
        "TaskManageBuildings must stay active"
    );

    let unit = state.unit(constructor).expect("constructor alive");
    assert!(
        matches!(
            unit.orders,
            UnitOrder::Build | UnitOrder::Await | UnitOrder::Move
        ),
        "constructor must be building or moving, got {:?}",
        unit.orders
    );
}

// ---------------------------------------------------------------------------
// Alien-юниты вне командных структур (C++ `PLAYER_TEAM_ALIEN`)
// ---------------------------------------------------------------------------

/// Спавн от имени `ALIEN` не трогает heat-map/игроков 0..4 и не паникует.
///
/// C++: `ResourceManager_InitHeatMaps` не создаёт карту для
/// `PLAYER_TEAM_ALIEN` (`team_type == TEAM_TYPE_NONE`), а `DeployUnit` для
/// обломков вызывается с `skip_map_status_update = true`.
#[test]
fn alien_units_stay_outside_team_heat_maps_and_victory() {
    let mut state = computer_state();

    // Ровно тот вызов, что делает `App::new_game_on_surface`: `flags = 0`,
    // флаги типа (включая `REGENERATING_UNIT`) добавляет `spawn_unit`.
    let alien_tank = state
        .spawn_unit(UnitType::AlienTank, TeamId::ALIEN, Point::new(12, 12), 0)
        .expect("alien tank deploys");
    let alien_gun = state
        .spawn_unit(
            UnitType::AlienAssaultGun,
            TeamId::ALIEN,
            Point::new(13, 12),
            0,
        )
        .expect("alien assault gun deploys");
    let alien_building = state
        .spawn_unit(UnitType::LargeSlab, TeamId::ALIEN, Point::new(15, 15), 0)
        .expect("alien building deploys");

    // `DeployUnit`: `team == PLAYER_TEAM_ALIEN && flags & REGENERATING_UNIT`
    // → `ORDER_DISABLE` (флаги берутся из типа, а не только из аргумента).
    assert_eq!(
        state.unit(alien_tank).map(|unit| unit.orders),
        Some(UnitOrder::Disable)
    );
    assert_eq!(
        state.unit(alien_gun).map(|unit| unit.orders),
        Some(UnitOrder::Disable)
    );

    // Юниты в списках и в хэше карты; здание занимает 2×2, как
    // `Hash_MapHash.Add(unit, flags & GROUND_COVER)` в C++.
    for id in [alien_tank, alien_gun, alien_building] {
        let unit = state.unit(id).expect("alien unit alive");
        assert_eq!(unit.team, TeamId::ALIEN);
        assert!(state.unit_list(unit.unit_list).contains(&id));
        assert!(state.units_at(unit.grid).contains(&id));
    }

    let building = state.unit(alien_building).expect("building alive");
    assert_ne!(building.flags & unit_flags::BUILDING, 0);
    for cell in [
        Point::new(building.grid.x + 1, building.grid.y),
        Point::new(building.grid.x, building.grid.y + 1),
        Point::new(building.grid.x + 1, building.grid.y + 1),
    ] {
        assert!(
            state.units_at(cell).contains(&alien_building),
            "building must occupy {cell:?}"
        );
    }

    // Слои реальных команд не получили вклада от alien-юнитов.
    for team in 0..PLAYER_TEAM_COUNT {
        for cell in [Point::new(12, 12), Point::new(13, 12), Point::new(15, 15)] {
            let team = TeamId(team as u8);
            assert_eq!(
                state.ai_heat_map_complete(team, i32::from(cell.x), i32::from(cell.y)),
                0
            );
            assert_eq!(
                state.ai_heat_map_stealth_sea(team, i32::from(cell.x), i32::from(cell.y)),
                0
            );
            assert_eq!(
                state.ai_heat_map_stealth_land(team, i32::from(cell.x), i32::from(cell.y)),
                0
            );
        }
    }

    // Для `ALIEN` карты нет: C++-проверки `heat_map && …` дают 0/false.
    assert_eq!(state.ai_heat_map_complete(TeamId::ALIEN, 12, 12), 0);
    assert_eq!(state.ai_heat_map_stealth_sea(TeamId::ALIEN, 12, 12), 0);
    assert_eq!(state.ai_heat_map_stealth_land(TeamId::ALIEN, 12, 12), 0);
    let unit = state.unit(alien_tank).expect("alien tank alive");
    assert!(!state.is_visible_on_heat_map(unit, TeamId::ALIEN));

    // Служебная команда не участвует в условиях победы: раньше `ALIEN` с
    // `team_type = Computer` попадал в `teams_total`.
    let status = victory::evaluate_status(&state, state.turn_counter);
    assert_eq!(status.teams_total, 1, "в партии только красные");
    assert_eq!(
        status.teams_in_play, 0,
        "у красных нет юнитов, а ALIEN не считается играющей командой"
    );
    assert_eq!(status.team_type[TeamId::ALIEN.0 as usize], 0);

    // Тик, смена приказа и уничтожение alien-юнитов не паникуют.
    state.tick();
    state.set_order(
        alien_building,
        UnitOrder::Sentry,
        UnitOrderState::ExecutingOrder,
    );
    state.tick();

    // Атака alien-юнита: C++ `UnitInfo::AttackUnit` не заводит трекер
    // реакции для команды с `team_type == NONE`, а `Ai_UnitSpotted` с
    // `team = ALIEN` не обращается к несуществующему `AiPlayer`.
    let commando = state
        .spawn_unit(UnitType::Commando, TeamId::RED, Point::new(11, 12), 0)
        .expect("commando deploys");
    max_core::combat::attack_unit(&mut state, alien_tank, commando, 0, 0);
    state.tick();

    state.destroy_unit(alien_building);
    state.destroy_unit(alien_tank);
    state.destroy_unit(alien_gun);
    state.destroy_unit(commando);
    state.tick();

    for id in [alien_building, alien_tank, alien_gun, commando] {
        assert!(
            state.unit(id).is_none(),
            "юнит {id:?} должен быть уничтожен"
        );
    }
    // Взрыв уничтоженного alien-танка — законный юнит-эффект его команды
    // (`StartExplosion` спавнит `HITEXPLD` с `team` погибшего), поэтому
    // проверяем только отсутствие самих обломков.
}

/// P2: ИИ доводит заказ до юнита через `ProductionManager`, а не мгновенным
/// спавном — очередь строителя тикает в конце хода
/// (`GameManager_ProgressBuildState`).
///
/// Ход проигрывается явно (`progress_build_state` после `Ai_BeginTurn`), потому
/// что «голая» база без армии закрывает партию generic-условиями
/// (`SC_L0002`) и `finish_turn` до производства не доходит.
#[test]
fn ai_completes_a_build_queue_through_production() {
    let mut state = computer_state();
    // Золото команды: производство списывает ставку C++ за ход.
    state.team_units[TeamId::RED.0 as usize].set_gold(500);
    put_ore(&mut state, Point::new(12, 11), 20);
    put_ore(&mut state, Point::new(13, 11), 20);

    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    assert!(max_core::ai_setup_strategy(&mut state, TeamId::RED));

    let mut built = None;
    let mut saw_queue = false;

    for _ in 0..26 {
        state.manage_economy(TeamId::RED);
        state.ai_begin_turn(TeamId::RED);

        for _ in 0..24 {
            state.tick();
        }

        // Ход производства команды (C++ `GameManager_ProgressBuildState`).
        max_core::production::progress_build_state(&mut state, TeamId::RED);

        if let Some(unit) = state.unit(constructor) {
            if !unit.build_list.is_empty() {
                saw_queue = true;
            }
        }

        built = state.unit_ids_in_order().into_iter().find(|id| {
            state.unit(*id).is_some_and(|unit| {
                unit.team == TeamId::RED && unit.unit_type == UnitType::MiningStation
            })
        });

        if built.is_some() {
            break;
        }
    }

    assert!(
        saw_queue || built.is_some(),
        "ИИ обязан держать очередь постройки (build_list) у строителя"
    );
    assert!(
        built.is_some(),
        "ИИ должен достроить шахту через очередь производства (12 ходов)"
    );
}

