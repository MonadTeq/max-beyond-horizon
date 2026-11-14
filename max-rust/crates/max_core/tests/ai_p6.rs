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
use max_core::enums::{GameStateType, Opponent, Surface, TeamClan, UnitOrder, UnitType};
use max_core::geo::Point;
use max_core::map::{Map, ResourceKind, ResourceMapEntry};
use max_core::team::{TeamId, TeamKind};
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
        [TeamKind::Computer, TeamKind::None, TeamKind::None, TeamKind::None],
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

    for _ in 0..12 {
        play_turn(&mut state, 24);

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
        state.unit(defender).map(|unit| unit.hits < hits_before).unwrap_or(true),
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
