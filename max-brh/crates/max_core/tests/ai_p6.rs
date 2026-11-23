//! P6: end-to-end AI tests (DESIGN.md §7, P6 criteria).
//!
//! Checked:
//!
//! 1. AI turn on a small map: one constructor and ore nearby → the AI creates
//!    `TaskManageBuildings` → `TaskCreateBuilding(MININGST)` →
//!    `TaskObtainUnits(CONSTRCT)` and issues `ORDER_BUILD` to the constructor with
//!    `build_list = [MININGST]`.
//! 2. The army attacks the enemy: an AI tank spots the enemy, `AiPlayer::DetermineAttack`
//!    creates `TaskAttack`, and the enemy takes damage.
//! 3. Determinism: two identical runs give the same state digest
//!    (including AI state — tasks, reminders, maps).
//! 4. Task transitions: `CreateBuilding` passes through states
//!    `INITIALIZING → GETTING_BUILDER → … → BUILDING`, reminders
//!    are processed fully.
//!
//! # Turn model in the test
//!
//! The network layer (ticks, barriers, `FinishedTurn`) is not part of max_core, so
//! the test plays a turn itself the way `game.rs` does:
//! `manage_economy` (speed/shots reset) → `ai_begin_turn` → N order
//! ticks → `EndTurn` + `finish_turn`.

use max_core::ai::{TaskKind, TASK_TYPE_CREATE_BUILDING, TASK_TYPE_MOVE};
use max_core::enums::{
    unit_flags, GameStateType, Opponent, Surface, TeamClan, UnitOrder, UnitOrderState, UnitType,
};
use max_core::game::PLAYER_TEAM_COUNT;
use max_core::geo::Point;
use max_core::map::{Map, ResourceKind, ResourceMapEntry};
use max_core::team::{TeamId, TeamKind};
use max_core::universe::GameUniverse;

/// A small map: land 24×24 in the top-left corner, the rest — water.
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

/// An AI computer for red; the other teams are `None`.
fn computer_state() -> GameUniverse {
    let map = small_land_map();
    let mut state = GameUniverse::with_setup(
        map,
        [
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
            TeamKind::None,
        ],
    );

    state.teams_mut()[0].team_type = TeamKind::Computer;
    state.teams_mut()[0].team_clan = TeamClan::Musashi;
    state.ai_mut().settings.opponent = Opponent::Average as u8;
    state
}

/// Places a materials deposit on a cell.
fn put_ore(state: &mut GameUniverse, site: Point, amount: u8) {
    state.map_mut().resources.set(
        site.x,
        site.y,
        ResourceMapEntry::new(amount, Some(ResourceKind::Raw), 0x0F),
    );
}

/// Team tasks by type.
fn tasks_of_type(state: &GameUniverse, team: TeamId, task_type: u8) -> Vec<u32> {
    state
        .snapshot()
        .ai()
        .task_manager
        .tasks()
        .iter()
        .filter(|task| task.team == team && task.get_type() == task_type)
        .map(|task| task.id)
        .collect()
}

/// One team turn: economy → `Ai_BeginTurn` → `ticks` ticks → end of turn.
fn play_turn(state: &mut GameUniverse, ticks: usize) {
    let team = TeamId::RED;

    state.manage_economy(team);
    state.ai_begin_turn(team);

    for _ in 0..ticks {
        state.tick();
    }

    state.set_game_state(GameStateType::EndTurn);
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

    // `Ai_SetupStrategy` — strategy only (without map regeneration).
    assert!(
        state.ai_setup_strategy(TeamId::RED),
        "strategy must be selected for a viable map"
    );

    let mut saw_create_building = false;
    let mut saw_move = false;
    let mut max_op_state = 0u8;

    // Observation — frame by frame: `TaskMove` lives only a few ticks (the AI
    // ends the turn as soon as the units stop working), so a check
    // at the turn boundary missed it.
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

            for task in state.snapshot().ai().task_manager.tasks() {
                if let TaskKind::CreateBuilding { op_state, .. } = &task.kind {
                    max_op_state = max_op_state.max(*op_state);
                }
            }
        }

        state.set_game_state(GameStateType::EndTurn);
        let _ = state.finish_turn();

        let building = state
            .snapshot()
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
    assert!(
        saw_move || max_op_state >= max_core::ai::tasks::create_building_state::BUILDING,
        "TaskMove must bring the builder to the site (or the builder already stands on it)"
    );
    assert!(
        max_op_state >= max_core::ai::tasks::create_building_state::MOVING_TO_SITE,
        "TaskCreateBuilding must progress past INITIALIZING, got {max_op_state}"
    );

    let constructor_unit = state
        .snapshot()
        .unit(constructor)
        .expect("constructor alive");

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

    // The enemy is visible to red: mark visibility and notify the AI.
    let mut enemy = state
        .snapshot()
        .unit(defender)
        .expect("green tank exists")
        .clone();
    enemy.set_visible_to_team(TeamId::RED, true);
    state.write_unit(defender, enemy);
    state.ai_unit_spotted(defender, TeamId::RED);

    let hits_before = state.snapshot().unit(defender).expect("green tank").hits;

    let mut attacked = false;

    for _ in 0..12 {
        play_turn(&mut state, 24);

        let hits_now = state
            .snapshot()
            .unit(defender)
            .map(|unit| unit.hits)
            .unwrap_or(0);
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

    // `TaskAttack`/`TaskKillUnit` — for "task" targets (key objects,
    // constructor); a single tank is handled by the `AiAttack_EvaluateAttack`
    // reaction (the `RemindAttack` reminder), so the test criterion is the damage dealt.
    assert!(
        state
            .snapshot()
            .unit(defender)
            .map(|unit| unit.hits < hits_before)
            .unwrap_or(true),
        "enemy must stay damaged or be destroyed"
    );
}

/// The AI moves the builder away from a completed site itself — by the
/// `TaskCreateBuilding::Activate`/`TaskActivate` task, not by an "auto-exit" in
/// `UnitsManager_ProcessOrderBuild`.
///
/// C++ (`taskcreatebuilding.cpp:670`, `taskactivate.cpp:104`): the task's `EndTurn` in
/// `CREATE_BUILDING_STATE_BUILDING` with `builder->GetOrderState() ==
/// ORDER_STATE_UNIT_READY` calls `Activate()` → `TaskActivate`, which searches
/// a passable cell around the building and issues `ORDER_ACTIVATE`/
/// `ORDER_STATE_IN_TRANSITION`. Then `PositionInTape` DEINIT →
/// `ActivateEngineer` → `ORDER_MOVE`, and `UnitsManager_BuildingReady` unfolds
/// the building when the builder leaves the 2×2 footprint. If the AI waited for the "player",
/// its construction would hang forever (the completed building is not drawn).
#[test]
fn ai_builder_leaves_the_finished_site_and_the_building_appears() {
    let mut state = computer_state();
    let site = Point::new(10, 10);

    let builder = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, site, 0)
        .expect("constructor");
    let building = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, site, 0)
        .expect("training hall");

    // The site: tape/cones/slab — as after `StartBuilding`.
    state.deploy_construction_site(builder, UnitType::TrainingHall, site)
    .expect("construction site");

    // `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3520-3592`): mutual parents,
    // the building `ORDER_IDLE`/`BUILDING_READY`, the builder `ORDER_BUILD`/`UNIT_READY`.
    let mut builder_unit = state.snapshot().unit(builder).expect("builder").clone();
    builder_unit.grid = site;
    builder_unit.parent_unit = Some(building);
    builder_unit.orders = UnitOrder::Build;
    builder_unit.state = UnitOrderState::UnitReady;
    builder_unit.build_list.clear();
    state.write_unit(builder, builder_unit);

    let mut building_unit = state.snapshot().unit(building).expect("building").clone();
    building_unit.parent_unit = Some(builder);
    building_unit.orders = UnitOrder::Idle;
    building_unit.state = UnitOrderState::BuildingReady;
    state.write_unit(building, building_unit);

    assert_eq!(
        state.snapshot().builder_awaiting_exit(builder),
        Some(building)
    );

    // The AI task in state `BUILDING` (as after `BeginBuilding`).
    let task = state.ai_append_task(
        TeamId::RED,
        None,
        0,
        TaskKind::CreateBuilding {
            unit_type: UnitType::TrainingHall,
            site,
            op_state: max_core::ai::tasks::create_building_state::BUILDING,
            builder: Some(builder),
            building: None,
            manager: None,
            needs_water_platforms: false,
            tasks: Vec::new(),
        },
    );
    state
        .ai_attach_task_to_unit(builder, task);

    // `TaskCreateBuilding::EndTurn` on the team turn: `Activate()` → `TaskActivate`.
    state.manage_economy(TeamId::RED);
    state.ai_begin_turn(TeamId::RED);

    let activated = state
        .snapshot()
        .unit(builder)
        .map(|unit| (unit.orders, unit.state))
        .expect("builder alive");
    assert!(
        matches!(
            activated,
            (
                UnitOrder::Activate,
                UnitOrderState::InTransition
                    | UnitOrderState::BuildInProgress
                    | UnitOrderState::TapePositioningDeinit
            ) | (UnitOrder::AwaitTapePositioning, _)
        ),
        "ИИ обязан отдать ORDER_ACTIVATE строителю, получено {activated:?}"
    );

    // The builder leaves, the building unfolds (no auto-exit — the exit goes
    // by the task's order, as for the player by a click).
    let mut expanded = false;

    for _ in 0..200 {
        state.tick();

        if state
            .snapshot()
            .unit(building)
            .is_some_and(|unit| unit.orders != UnitOrder::Idle)
        {
            expanded = true;

            break;
        }
    }

    assert!(
        expanded,
        "здание обязано раскрыться после ухода строителя (соло-ИИ не зависает)"
    );
    assert!(
        state.snapshot().construction_utility(TeamId::RED, site).is_none(),
        "лента и конусы снимаются `UnitsManager_BuildingReady`"
    );
    assert!(
        state.snapshot().unit(builder).is_some_and(|unit| {
            !max_core::production::unit_bounds_contains(
                state.snapshot().unit(building).expect("building"),
                unit.grid,
            )
        }),
        "строитель вне габаритов 2×2"
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

        let mut enemy_unit = state.snapshot().unit(enemy).expect("enemy").clone();
        enemy_unit.set_visible_to_team(TeamId::RED, true);
        state.write_unit(enemy, enemy_unit);
        state.ai_unit_spotted(enemy, TeamId::RED);

        state.ai_setup_strategy(TeamId::RED);

        for _ in 0..8 {
            play_turn(&mut state, 16);
        }

        state.snapshot().snapshot_bytes()
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

    // The reminder queue is processed deterministically: the `deferred` tail
    // (carried to the next `ExecuteReminders` call) is limited by the number of tasks.
    assert!(
        state.snapshot().ai().task_manager.reminders_count()
            <= state.snapshot().ai().task_manager.task_count() * 2 + 1,
        "reminder queues must stay bounded ({} reminders, {} tasks)",
        state.snapshot().ai().task_manager.reminders_count(),
        state.snapshot().ai().task_manager.task_count()
    );

    // The construction manager and starting tasks are alive.
    assert!(
        state
            .snapshot()
            .ai()
            .task_manager
            .tasks()
            .iter()
            .any(|task| matches!(task.kind, TaskKind::ManageBuildings { .. })),
        "TaskManageBuildings must stay active"
    );

    let unit = state
        .snapshot()
        .unit(constructor)
        .expect("constructor alive");
    assert!(
        matches!(
            unit.orders,
            // A construction site is prepared with `AwaitTapePositioning`, so it
            // counts as "building" as well.
            UnitOrder::Build | UnitOrder::Await | UnitOrder::Move | UnitOrder::AwaitTapePositioning
        ),
        "constructor must be building or moving, got {:?}",
        unit.orders
    );
}

// ---------------------------------------------------------------------------
// Alien units outside team structures (C++ `PLAYER_TEAM_ALIEN`)
// ---------------------------------------------------------------------------

/// Spawning as `ALIEN` does not touch heat maps/players 0..4 and does not panic.
///
/// C++: `ResourceManager_InitHeatMaps` does not create a map for
/// `PLAYER_TEAM_ALIEN` (`team_type == TEAM_TYPE_NONE`), and `DeployUnit` for
/// derelicts is called with `skip_map_status_update = true`.
#[test]
fn alien_units_stay_outside_team_heat_maps_and_victory() {
    let mut state = computer_state();

    // Exactly the call `App::new_game_on_surface` makes: `flags = 0`;
    // type flags (including `REGENERATING_UNIT`) are added by `spawn_unit`.
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
    // → `ORDER_DISABLE` (flags are taken from the type, not only from the argument).
    assert_eq!(
        state.snapshot().unit(alien_tank).map(|unit| unit.orders),
        Some(UnitOrder::Disable)
    );
    assert_eq!(
        state.snapshot().unit(alien_gun).map(|unit| unit.orders),
        Some(UnitOrder::Disable)
    );

    // Units in the lists and in the map hash; the building occupies 2×2, as
    // `Hash_MapHash.Add(unit, flags & GROUND_COVER)` in C++.
    for id in [alien_tank, alien_gun, alien_building] {
        let unit = state.snapshot().unit(id).expect("alien unit alive");
        assert_eq!(unit.team, TeamId::ALIEN);
        assert!(state.snapshot().unit_list(unit.unit_list).contains(&id));
        assert!(state.snapshot().units_at(unit.grid).contains(&id));
    }

    let building = state
        .snapshot()
        .unit(alien_building)
        .expect("building alive");
    assert_ne!(building.flags & unit_flags::BUILDING, 0);
    for cell in [
        Point::new(building.grid.x + 1, building.grid.y),
        Point::new(building.grid.x, building.grid.y + 1),
        Point::new(building.grid.x + 1, building.grid.y + 1),
    ] {
        assert!(
            state.snapshot().units_at(cell).contains(&alien_building),
            "building must occupy {cell:?}"
        );
    }

    // The layers of real teams received no contribution from alien units.
    for team in 0..PLAYER_TEAM_COUNT {
        for cell in [Point::new(12, 12), Point::new(13, 12), Point::new(15, 15)] {
            let team = TeamId(team as u8);
            assert_eq!(
                state
                    .snapshot()
                    .ai_heat_map_complete(team, i32::from(cell.x), i32::from(cell.y)),
                0
            );
            assert_eq!(
                state
                    .snapshot()
                    .ai_heat_map_stealth_sea(team, i32::from(cell.x), i32::from(cell.y)),
                0
            );
            assert_eq!(
                state
                    .snapshot()
                    .ai_heat_map_stealth_land(team, i32::from(cell.x), i32::from(cell.y)),
                0
            );
        }
    }

    // There is no map for `ALIEN`: the C++ checks `heat_map && …` give 0/false.
    assert_eq!(state.snapshot().ai_heat_map_complete(TeamId::ALIEN, 12, 12), 0);
    assert_eq!(
        state.snapshot().ai_heat_map_stealth_sea(TeamId::ALIEN, 12, 12),
        0
    );
    assert_eq!(
        state
            .snapshot()
            .ai_heat_map_stealth_land(TeamId::ALIEN, 12, 12),
        0
    );
    let unit = state.snapshot().unit(alien_tank).expect("alien tank alive");
    assert!(!state.snapshot().is_visible_on_heat_map(unit, TeamId::ALIEN));

    // The service team does not participate in victory conditions: previously `ALIEN` with
    // `team_type = Computer` got into `teams_total`.
    let status = state.snapshot().evaluate_status();
    assert_eq!(status.teams_total, 1, "в партии только красные");
    assert_eq!(
        status.teams_in_play, 0,
        "у красных нет юнитов, а ALIEN не считается играющей командой"
    );
    assert_eq!(status.team_type[TeamId::ALIEN.0 as usize], 0);

    // Tick, order change and destruction of alien units do not panic.
    state.tick();
    state.set_order(
        alien_building,
        UnitOrder::Sentry,
        UnitOrderState::ExecutingOrder,
    );
    state.tick();

    // Attack by an alien unit: C++ `UnitInfo::AttackUnit` does not create a reaction
    // tracker for a team with `team_type == NONE`, and `Ai_UnitSpotted` with
    // `team = ALIEN` does not access a nonexistent `AiPlayer`.
    let commando = state
        .spawn_unit(UnitType::Infiltrator, TeamId::RED, Point::new(11, 12), 0)
        .expect("commando deploys");
    state.attack_unit(alien_tank, commando, 0, 0);
    state.tick();

    state.destroy_unit(alien_building);
    state.destroy_unit(alien_tank);
    state.destroy_unit(alien_gun);
    state.destroy_unit(commando);
    state.tick();

    for id in [alien_building, alien_tank, alien_gun, commando] {
        assert!(
            state.snapshot().unit(id).is_none(),
            "юнит {id:?} должен быть уничтожен"
        );
    }
    // The explosion of a destroyed alien tank is a legitimate unit effect of its team
    // (`StartExplosion` spawns `HITEXPLD` with the dead unit's `team`), so
    // we only check the absence of the rubble itself.
}

/// P2: the AI brings the order to a unit through `ProductionManager`, not by instant
/// spawning — the builder's queue ticks at the end of the turn
/// (`GameManager_ProgressBuildState`).
///
/// The turn is played explicitly (`progress_build_state` after `Ai_BeginTurn`), because
/// a "bare" base without an army closes the game by generic conditions
/// (`SC_L0002`) and `finish_turn` does not reach production.
#[test]
fn ai_completes_a_build_queue_through_production() {
    let mut state = computer_state();
    // Team gold: production charges the C++ rate per turn.
    state.team_units_table_mut()[TeamId::RED.0 as usize].set_gold(500);
    put_ore(&mut state, Point::new(12, 11), 20);
    put_ore(&mut state, Point::new(13, 11), 20);

    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    // Real landing gives the constructor raw material (`DEFAULT_MISSION_LOADOUT`);
    // `UnitsManager_DeployUnit` zeroes the new unit's storage, so the test
    // sets the cargo explicitly — otherwise `BuildMenu::Build` would refuse (no 24 units).
    state.unit_mut(constructor).expect("constructor").storage = 60;
    assert!(state.ai_setup_strategy(TeamId::RED));

    let mut built = None;
    let mut saw_queue = false;

    for _ in 0..26 {
        state.manage_economy(TeamId::RED);
        state.ai_begin_turn(TeamId::RED);

        for _ in 0..24 {
            state.tick();
        }

        // Team production turn (C++ `GameManager_ProgressBuildState`).
        state.progress_build_state(TeamId::RED);

        if let Some(unit) = state.snapshot().unit(constructor) {
            if !unit.build_list.is_empty() {
                saw_queue = true;
            }
        }

        built = state.snapshot().unit_ids_in_order().into_iter().find(|id| {
            state.snapshot().unit(*id).is_some_and(|unit| {
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
