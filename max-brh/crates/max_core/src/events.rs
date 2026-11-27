//! Command queue, unit events and application of packets to [`GameState`].
//!
//! Oracle: `max-cpp-port/src/remote.cpp` (`Remote_ProcessNetPackets` and
//! the `Remote_ReceiveNetPacket_*` handlers, order processors
//! `Remote_OrderProcessor{0..5}_Read`), `unitevents.cpp`
//! (`UnitEventEmergencyStop`), DESIGN.md §5.1 (step 1) and §6.2.
//!
//! # Application model
//!
//! There is no network in `max_core`: commands arrive already parsed
//! ([`crate::input::Command`]) and are put into [`CommandQueue`]. They are applied
//! **strictly in receive order** at a deterministic point of the loop — at the start
//! of the tick ([`crate::game::GameState::tick`], step 2), the analog of
//! `Remote_Synchronize()`/`Remote_ProcessNetPackets()`.
//!
//! # Correspondence with the C++ port
//!
//! * Packet **08** repeats the processor nesting: first
//!   `Remote_OrderProcessor1_Read` as a preliminary check
//!   (`Remote_ReceiveNetPacket_08` parses a copy of the packet), then the
//!   order-level processor (which applies the `P1` fields again). Both passes
//!   are performed, as in C++.
//! * Packet **50** does not change the unit immediately: C++ creates
//!   `UnitEventEmergencyStop` and puts it into `UnitEvent_UnitEvents`, and the event
//!   is processed in `UnitsManager_ProcessOrders` — likewise here
//!   ([`UnitEvent::EmergencyStop`]).
//! * Packets **00/01/45/52** are barrier facts: they are only recorded in
//!   state fields and do not affect units (DESIGN §8.4/§8.6).
//! * Packets **10/20** upgrade attributes via a copy of `UnitValues`
//!   (`UpdateVersion`/`MarkAsInUse`), like `Remote_ReceiveNetPacket_10/20`.
//! * Packet **14** (deployment) returns [`ApplyOutcome::Unsupported`]:
//!   `UnitsManager_DeployUnit` requires mission/landing-point data that
//!   comes from the setup layer (P3); the type flags are already in
//!   [`crate::access::unit_type_flags`], and the caller layer deploys the unit via
//!   [`crate::game::GameState::spawn_unit`].
//! * Packet **12** (mission supplies) is stored as data
//!   ([`crate::game::GameState::mission_supplies`]) and updates
//!   `stats_gold_spent_on_upgrades` — like the C++ receiver.
//! * Packet **54** is not a [`Command`] but an authoritative team state:
//!   [`apply_state_sync`] applies each unit's `Packet23Data`, deletes
//!   the extra ones (`Remote_RemoveExtraUnits`) and aligns the economy tail
//!   (`Remote_ReceiveNetPacket_54`).

use crate::enums::{ResearchTopic, UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::geo::Point;
use crate::input::{
    is_valid_order, is_valid_team_index, Command, MovePathData, OrderPayload, Packet08Order,
    Packet09TeamState, PayloadLevel, PayloadP1, PayloadP2, PayloadP3, PayloadP4, PayloadP5,
    UPGRADE_VALUE_COUNT,
};
use crate::team::{TeamId, TeamKind};
use crate::unit::{AirPath, GroundPath, UnitId, UnitInfo, UnitListKind, UnitPath};
use crate::unit_order;
use crate::unit_values::{
    ATTRIB_AGENT_ADJUST, ATTRIB_AMMO, ATTRIB_ARMOR, ATTRIB_ATTACK, ATTRIB_ATTACK_RADIUS,
    ATTRIB_HITS, ATTRIB_MOVE_AND_FIRE, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SCAN, ATTRIB_SPEED,
    ATTRIB_STORAGE, ATTRIB_TURNS,
};

// ---------------------------------------------------------------------------
// Unit events (C++ `UnitEvent_UnitEvents`)
// ---------------------------------------------------------------------------

/// A unit event — C++ `class UnitEvent` (`unitevents.hpp`).
///
/// The only variant in the original is an emergency stop (`UnitEventEmergencyStop`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitEvent {
    /// `UnitEventEmergencyStop::Process` (`unitevents.cpp:38`).
    EmergencyStop {
        /// The unit that received packet 50.
        unit: UnitId,
    },
}

impl UnitEvent {
    /// The unit the event refers to.
    pub const fn unit(&self) -> UnitId {
        match self {
            Self::EmergencyStop { unit } => *unit,
        }
    }
}

/// `UnitEvent::Process` — applies the event to the game.
///
/// `UnitEventEmergencyStop::Process` (`unitevents.cpp:38`) only calls
/// `UnitInfo::StopMovement` (`unitinfo.cpp:3222`). On the turn state that reaches
/// `GroundPath::CancelMovement` (`groundpath.cpp:161`) → `BlockedOnPathRequest()`
/// with the default `mode = true`, and `UnitInfo::BlockedOnPathRequest`
/// (`unitinfo.cpp:3628`) → `MoveFinished(true)` → `UnitsManager_AddToDelayedReactionList`
/// (`unitinfo.cpp:3692-3694`). The stopped unit becomes a reaction target at the
/// cell where it halted — that is the intercept: enemy units open fire there
/// instead of at the destination of the interrupted route.
pub(crate) fn process_unit_event(state: &mut GameState, event: &UnitEvent) {
    match event {
        UnitEvent::EmergencyStop { unit } => {
            let Some(target) = state.unit(*unit).cloned() else {
                return;
            };
            if target.hits == 0 {
                return;
            }

            // `UnitInfo::StopMovement` reacts only to a move order whose path is
            // still being walked; everything else is a no-op.
            if target.orders != UnitOrder::Move
                || target.path.is_none()
                || !matches!(
                    target.state,
                    UnitOrderState::InProgress | UnitOrderState::InTransition
                )
            {
                return;
            }

            // `GroundPath::CancelMovement`: the turn state calls
            // `BlockedOnPathRequest()` (`mode = true`), the transition state only
            // empties the remaining steps and the reaction is queued when the
            // current cell completes.
            let blocked = target.state == UnitOrderState::InProgress;
            let mut target = target;
            unit_order::stop_movement(&mut target, state.disable_fire);
            state.write_unit(*unit, target.clone());

            if blocked && !state.disable_fire {
                state.add_to_delayed_reaction_list(*unit);

                // `MoveFinished(true)` also reports a minelayer and evaluates the
                // unit's own attack targets (`Ai_CheckMines`/`Ai_EvaluateAttackTargets`).
                if matches!(
                    target.unit_type,
                    UnitType::MineLayer | UnitType::SeaMineLayer
                ) {
                    crate::ai::ai_check_mines(state, *unit);
                }
                if target.ammo > 0 {
                    state.ai_evaluate_attack_targets(*unit);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Command queue
// ---------------------------------------------------------------------------

/// Queue of input commands in receive order (DESIGN §5.1 step 1).
///
/// `VecDeque` is a FIFO without hash-table iteration; the order is fully
/// determined by packet arrival order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandQueue {
    pending: std::collections::VecDeque<Command>,
    applied: u64,
}

impl CommandQueue {
    /// Empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Pushes a command to the back (receive order).
    pub fn push(&mut self, command: Command) {
        self.pending.push_back(command);
    }

    /// Pops the oldest command.
    pub fn pop_front(&mut self) -> Option<Command> {
        let command = self.pending.pop_front()?;
        self.applied = self.applied.wrapping_add(1);
        Some(command)
    }

    /// How many commands are not applied yet.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// How many commands have been applied since creation (diagnostics).
    pub const fn applied(&self) -> u64 {
        self.applied
    }

    /// Clears the queue (reconnect/new run).
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

// ---------------------------------------------------------------------------
// Application result
// ---------------------------------------------------------------------------

/// Command application result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// The command was applied.
    Applied,
    /// The packet refers to an unknown unit (`Remote_NetErrorUnknownUnit`).
    UnknownUnit,
    /// Malformed packet (`NetPacket::SetMalformed` in C++).
    Malformed,
    /// The packet is valid but not applicable at this phase (see the module header).
    Unsupported,
    /// The packet was ignored (invalid team index, unknown complex).
    Ignored,
}

// ---------------------------------------------------------------------------
// Applying the queue
// ---------------------------------------------------------------------------

/// Applies all accumulated commands in receive order.
///
/// Returns the number of applied commands (as `Remote_ProcessNetPackets`
/// drains the queue to `REMOTE_RECEIVE_EMPTY`).
pub(crate) fn apply_pending(state: &mut GameState) -> usize {
    let mut applied = 0;

    while let Some(command) = state.commands.pop_front() {
        apply_command(state, &command);
        applied += 1;
    }

    applied
}

/// Applies one command to the state — the analog of `Remote_ReceiveNetPacket_*`.
pub(crate) fn apply_command(state: &mut GameState, command: &Command) -> ApplyOutcome {
    match command {
        Command::Order(order) => apply_packet_08(state, order),
        Command::Move(movement) => apply_packet_38(state, movement),
        Command::MoveFinished(finished) => apply_packet_41(state, finished),
        Command::Stop(stop) => apply_packet_50(state, stop),
        Command::FinishedTurn(finished) => apply_packet_06(state, finished),
        Command::EndTurnBarrier(barrier) => apply_packet_52(state, barrier),
        Command::CrcBarrier(barrier) => apply_packet_45(state, barrier),
        Command::TurnBarrier(barrier) => apply_packet_00(state, barrier),
        Command::FrameBarrier(barrier) => apply_packet_01(state, barrier),
        Command::Chat(chat) => apply_packet_18(state, chat),
        Command::ComplexOrder(order) => apply_packet_11(state, order),
        Command::ComplexSummary(summary) => apply_packet_53(state, summary),
        Command::TeamState(team_state) => apply_packet_09(state, team_state),
        Command::UpgradeUnitType(upgrade) => apply_packet_10(state, upgrade),
        Command::UpgradeUnit(upgrade) => apply_packet_20(state, upgrade),
        Command::Rename(rename) => apply_packet_43(state, rename),
        Command::Seed(seed) => {
            // Packet 13 is the only simulation seeding point
            // (DESIGN §6.1). C++ stores the seed in `Remote_RngSeed` and applies it
            // at game start; `max_core` applies it immediately — there are no other
            // seed consumers in the simulation.
            state.rng.set_seed(seed.seed);
            ApplyOutcome::Applied
        }
        Command::Deploy(_deploy) => ApplyOutcome::Unsupported,
        Command::MissionSupply(supply) => apply_packet_12(state, supply),
    }
}

// ---------------------------------------------------------------------------
// 08 — order to a unit
// ---------------------------------------------------------------------------

fn apply_packet_08(state: &mut GameState, packet: &Packet08Order) -> ApplyOutcome {
    let Some(mut unit) = state.unit(packet.unit).cloned() else {
        return ApplyOutcome::UnknownUnit;
    };

    // `UnitsManager_NewOrderWhileScaling` (`units_manager.cpp:2529`): an order
    // arriving during `EXPAND` immediately calls
    // `UnitsManager_UpdateConnectors` in C++ — otherwise a building moved by an order
    // would never join the complex (the animation will not complete).
    let connectors_due =
        unit.orders == UnitOrder::AwaitScaling && unit.state == UnitOrderState::Expand;

    // `Remote_ReceiveNetPacket_08`: the preliminary `P1` pass over a copy of
    // the packet — validating orders before processor selection.
    if let Some(p1) = packet.payload.p1() {
        if !is_valid_order(p1.order) || !is_valid_order(p1.prior_order) {
            return ApplyOutcome::Malformed;
        }
        // Construction cannot be cancelled by a new order: in C++ moving the builder away from
        // an unfinished site via the map is impossible (`game_manager.cpp:5927-5931`
        // — `CURSOR_UNIT_NO_GO`), and cancellation goes only through an explicit `stop`
        // (`UnitsManager_Popup_OnClick_BuildStop` → `ORDER_BUILD`/
        // `ORDER_STATE_BUILD_CANCEL` → `UnitInfo::CancelBuilding`).
        apply_payload_p1(&mut unit, p1);
    }

    // Processor selection by the unit's *new* order (`Remote_OrderProcessors[...]`).
    let Some(order) = packet.payload.order() else {
        // `ORDER_17` (P0): empty payload, nothing to change.
        state.write_unit(packet.unit, unit);
        return ApplyOutcome::Applied;
    };

    // The processor must match the order (otherwise malformed). In C++
    // the processor is chosen by the unit's already applied order, so
    // the preliminary `P1` pass has already changed the unit by this point — as
    // here (first we write the copy, then report malformed).
    if packet.payload.level() != PayloadLevel::for_order(order) {
        state.write_unit(packet.unit, unit);
        return ApplyOutcome::Malformed;
    }

    let result = match &packet.payload {
        OrderPayload::Empty => ApplyOutcome::Applied,
        OrderPayload::P1(p1) => {
            apply_payload_p1(&mut unit, p1);
            ApplyOutcome::Applied
        }
        OrderPayload::P2(p2) => {
            apply_payload_p2(state, &mut unit, p2);
            ApplyOutcome::Applied
        }
        OrderPayload::P3(p3) => {
            apply_payload_p3(state, &mut unit, p3);
            ApplyOutcome::Applied
        }
        OrderPayload::P4(p4) => {
            apply_payload_p4(state, &mut unit, p4);
            ApplyOutcome::Applied
        }
        OrderPayload::P5(p5) => {
            apply_payload_p5(state, &mut unit, p5);
            ApplyOutcome::Applied
        }
    };

    state.write_unit(packet.unit, unit);

    if connectors_due {
        crate::complex::update_connectors(state, packet.unit);
    }

    // The port places the site at order time (`DeployConstructionSiteUtilities` +
    // `PrepareConstructionSite`): C++ `StartBuilding` happens on the next tick
    // (`UnitsManager_ProcessOrderBuild`/`INIT`), but the port applies P4 immediately and
    // the slab (`LRGSLAB`/`SMLSLAB`) must appear on the confirmation frame
    // of the site — without a tick delay. `deploy_construction_site` is idempotent,
    // so `start_building` on the next tick does not place a second site.
    if let Some(unit) = state.unit(packet.unit) {
        if unit.orders == UnitOrder::Build
            && unit.state == UnitOrderState::Init
            && unit.build_time > 0
            && crate::production::is_mobile_builder(unit.unit_type)
        {
            if let Some(head) = unit.build_list.first().copied() {
                let site = unit.target_grid;
                let _ = crate::production::deploy_construction_site(
                    state,
                    packet.unit,
                    head,
                    site,
                );
            }
        }
    }

    result
}

/// `Remote_OrderProcessor1_Read` (`remote.cpp:379`).
fn apply_payload_p1(unit: &mut UnitInfo, p1: &PayloadP1) {
    if !is_valid_order(p1.order) || !is_valid_order(p1.prior_order) {
        return;
    }

    // Reset of the unfinished scaling animation before the order change.
    unit_order::new_order_while_scaling(unit);

    unit.disabled_reaction_fire = p1.disabled_reaction_fire;
    unit.orders = p1.order;
    unit.state = p1.state;
    unit.prior_orders = p1.prior_order;
    unit.prior_state = p1.prior_state;
}

/// `Remote_OrderProcessor2_Read` (`remote.cpp:426`).
fn apply_payload_p2(state: &mut GameState, unit: &mut UnitInfo, p2: &PayloadP2) {
    apply_payload_p1(unit, &p2.p1);

    unit.parent_unit = p2.parent_unit;
    unit.experience = p2.experience;
    let _ = state;
}

/// `Remote_OrderProcessor3_Read` (`remote.cpp:462`).
fn apply_payload_p3(state: &mut GameState, unit: &mut UnitInfo, p3: &PayloadP3) {
    apply_payload_p2(state, unit, &p3.p2);

    let mut data = state.order_data(unit.id);
    data.move_to = p3.move_to;
    data.fire_on = p3.fire_on;
    // The V70 field `target_grid` mirrors `move_to` (see `OrderData`).
    unit.target_grid = p3.move_to;
    state.set_order_data(unit.id, data);

    unit.enemy_unit = p3.enemy_unit;
    unit.transfer_cargo = p3.transfer_cargo;
    unit.stealth_dice_roll = p3.stealth_dice_roll;
}

/// `Remote_OrderProcessor4_Read` (`remote.cpp:502`).
fn apply_payload_p4(state: &mut GameState, unit: &mut UnitInfo, p4: &PayloadP4) {
    apply_payload_p3(state, unit, &p4.p3);

    unit.repeat_build = p4.repeat_build;
    unit.build_time = p4.build_time;
    unit.build_rate = p4.build_rate;

    unit.build_list.clear();
    unit.build_list.extend_from_slice(&p4.build_list);
}

/// `Remote_OrderProcessor5_Read` (`remote.cpp:535`).
fn apply_payload_p5(state: &mut GameState, unit: &mut UnitInfo, p5: &PayloadP5) {
    apply_payload_p3(state, unit, &p5.p3);

    unit.total_mining = p5.total_mining;
    unit.raw_mining = p5.raw_mining;
    unit.fuel_mining = p5.fuel_mining;
    unit.gold_mining = p5.gold_mining;
}

// ---------------------------------------------------------------------------
// 38 — movement with a path
// ---------------------------------------------------------------------------

fn apply_packet_38(state: &mut GameState, packet: &crate::input::Packet38Move) -> ApplyOutcome {
    let Some(mut unit) = state.unit(packet.unit).cloned() else {
        return ApplyOutcome::UnknownUnit;
    };

    // `Remote_ReceiveNetPacket_38`: clear the old path, finish scaling,
    // then set the order/state.
    if unit.path.is_some() {
        unit.path = None;
    }

    unit_order::new_order_while_scaling(&mut unit);

    // A movement order does not cancel construction: cancellation is only an explicit `stop`
    // (`UnitInfo::CancelBuilding`), see `apply_packet_08`.

    unit.orders = packet.order;
    unit.state = packet.state;

    let mut data = state.order_data(packet.unit);
    data.move_to = packet.move_to;
    unit.target_grid = packet.move_to;
    state.set_order_data(packet.unit, data);

    unit.group_speed = packet.group_speed;

    // `grid_x/grid_y` (protocol §8.4, `Remote_RelocateUnit`): at the receiver
    // the unit stands on the owner's cell. The C++ port sends 38 without these fields,
    // so only an actual discrepancy is applied.
    if unit.grid != packet.grid {
        unit.grid = packet.grid;
    }

    match &packet.path {
        Some(MovePathData::Ground(steps)) if !steps.is_empty() => {
            unit.path = Some(UnitPath::Ground(GroundPath {
                end_x: packet.move_to.x,
                end_y: packet.move_to.y,
                step_index: 0,
                steps: steps.clone(),
            }));
        }
        Some(MovePathData::Air {
            end_x,
            end_y,
            distance_x,
            distance_y,
            euclidean_distance,
            speed,
            move_fraction,
            max_velocity,
        }) => {
            let angle = unit_order::paths_get_angle(
                i32::from(*end_x) - i32::from(unit.grid.x),
                i32::from(*end_y) - i32::from(unit.grid.y),
            );
            unit.path = Some(UnitPath::Air(AirPath {
                length: *euclidean_distance,
                angle: (angle & 7) as i8,
                start_x: unit.grid.x,
                start_y: unit.grid.y,
                end_x: *end_x,
                end_y: *end_y,
                step_x: *distance_x,
                step_y: *distance_y,
                delta_x: 0,
                delta_y: 0,
            }));
            unit.speed = *speed;
            unit.move_fraction = *move_fraction;
            unit.max_velocity = *max_velocity;
        }
        _ => {}
    }

    let path_is_missing = unit.path.is_none();
    let needs_path = matches!(
        unit.orders,
        UnitOrder::Move | UnitOrder::MoveToUnit | UnitOrder::MoveToAttack
    ) && matches!(
        unit.state,
        UnitOrderState::Init
            | UnitOrderState::MoveInit
            | UnitOrderState::NewOrder
            | UnitOrderState::MoveGettingPath
    );

    state.write_unit(packet.unit, unit);
    state.set_order_data(packet.unit, data);

    // Packet 38 without steps (the C++ port short form): the path is requested from
    // the synchronous pathfinding instead of being built linearly (DESIGN §5.1, risk 2).
    if path_is_missing && needs_path {
        crate::pathfinding::request_path(
            state,
            packet.unit,
            crate::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
        );
    }

    ApplyOutcome::Applied
}

/// Helper check "path without steps" (see the 38 branch).
#[allow(dead_code)]
fn steps_is_empty(packet: &crate::input::Packet38Move) -> bool {
    match &packet.path {
        None => true,
        Some(MovePathData::Ground(steps)) => steps.is_empty(),
        Some(MovePathData::Air { .. }) => false,
    }
}

// ---------------------------------------------------------------------------
// 41 / 50 — completion and emergency stop
// ---------------------------------------------------------------------------

fn apply_packet_41(
    state: &mut GameState,
    packet: &crate::input::Packet41MoveFinished,
) -> ApplyOutcome {
    let Some(mut unit) = state.unit(packet.unit).cloned() else {
        return ApplyOutcome::UnknownUnit;
    };

    // Protocol §8.4 fields (absent in the C++ port short form).
    if let Some(grid) = packet.grid {
        unit.grid = grid;
    }
    if let Some(speed) = packet.speed {
        unit.speed = speed;
    }
    if let Some(group_speed) = packet.group_speed {
        unit.group_speed = group_speed;
    }
    if let Some(shots) = packet.shots {
        unit.shots = shots;
    }
    if let Some(move_fraction) = packet.move_fraction {
        unit.move_fraction = move_fraction;
    }

    // `Remote_ReceiveNetPacket_41`: `unit->BlockedOnPathRequest(mode, true)`.
    // `MoveFinished(mode)` adds the unit to the delayed reaction list only
    // with `mode` and return fire enabled.
    unit_order::blocked_on_path_request(&mut unit, packet.mode, state.disable_fire);
    state.write_unit(packet.unit, unit);
    if packet.mode && !state.disable_fire {
        state.add_to_delayed_reaction_list(packet.unit);
    }
    ApplyOutcome::Applied
}

fn apply_packet_50(state: &mut GameState, packet: &crate::input::Packet50StopUnit) -> ApplyOutcome {
    if !state.has_unit(packet.unit) {
        return ApplyOutcome::UnknownUnit;
    }

    // `Remote_ReceiveNetPacket_50`: the event is enqueued and executed
    // in `UnitsManager_ProcessOrders`.
    state
        .unit_events
        .push(UnitEvent::EmergencyStop { unit: packet.unit });
    ApplyOutcome::Applied
}

// ---------------------------------------------------------------------------
// 06 / 52 / 45 / 00 / 01 — end of turn and barriers
// ---------------------------------------------------------------------------

fn apply_packet_06(
    state: &mut GameState,
    packet: &crate::input::Packet06FinishedTurn,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }

    // `GameManager_RefreshOrders` (`game_manager.cpp:838-891`) runs right before
    // `Remote_SendNetPacket_06`: it credits the turn's mining into the complex
    // stocks (`Access_UpdateResourcesTotal`) and only then determines the shortage
    // (`GameManager_OptimizeProduction`). Doing it in the `06` application keeps
    // every peer at the same deterministic point — the end-of-turn resource
    // processing of the oracle (manual defect "resources are not spent/credited
    // at the end of the turn").
    state.refresh_orders(packet.team);

    state.teams[packet.team.0 as usize].finished_turn = true;

    if state.play_mode == crate::enums::PlayMode::TurnBased {
        state.game_state = crate::enums::GameStateType::EndTurn;
    } else {
        // `GameManager_HandleTurnTimer()` — UI; the check "are there still
        // unfinished teams" is for logs/messages.
        let _pending = !state.are_teams_finished_turn();
    }

    ApplyOutcome::Applied
}

fn apply_packet_52(
    state: &mut GameState,
    packet: &crate::input::Packet52EndTurnBarrier,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }
    state.turn_indices[packet.team.0 as usize] = packet.counter;
    ApplyOutcome::Applied
}

fn apply_packet_45(
    state: &mut GameState,
    packet: &crate::input::Packet45CrcBarrier,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }
    state.next_turn_indices[packet.team.0 as usize] = packet.next_turn_index;
    state.team_data_crc16[packet.team.0 as usize] = packet.crc16;
    ApplyOutcome::Applied
}

fn apply_packet_00(
    state: &mut GameState,
    packet: &crate::input::Packet00TurnBarrier,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }
    state.frame_sync_counter[packet.team.0 as usize] = packet.counter;
    ApplyOutcome::Applied
}

fn apply_packet_01(
    state: &mut GameState,
    packet: &crate::input::Packet01FrameBarrier,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }
    state.frame_sync_counter2[packet.team.0 as usize] = packet.counter;
    ApplyOutcome::Applied
}

// ---------------------------------------------------------------------------
// 18 / 11 / 53 — chat, complexes
// ---------------------------------------------------------------------------

fn apply_packet_18(state: &mut GameState, packet: &crate::input::Packet18Chat) -> ApplyOutcome {
    if !is_valid_team_index(packet.sender) {
        return ApplyOutcome::Ignored;
    }
    state.chat_log.push((packet.sender, packet.message.clone()));
    ApplyOutcome::Applied
}

fn apply_packet_11(
    state: &mut GameState,
    packet: &crate::input::Packet11ComplexOrder,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }

    let Some(complex) = state.complex_mut(packet.team, packet.complex_id) else {
        return ApplyOutcome::Ignored;
    };

    complex.raw = packet.raw;
    complex.fuel = packet.fuel;
    complex.gold = packet.gold;
    complex.power = packet.power;
    complex.workers = packet.workers;
    complex.buildings = packet.buildings;
    ApplyOutcome::Applied
}

fn apply_packet_53(
    state: &mut GameState,
    packet: &crate::input::Packet53ComplexSummary,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }

    // Apply all entries; missing complexes are skipped (like
    // `TeamUnits::ReadComplexPacket` with `GetComplex(id)` == nullptr in the port).
    for entry in &packet.complexes {
        if let Some(complex) = state.complex_mut(packet.team, entry.complex_id) {
            complex.raw = entry.values[0];
            complex.fuel = entry.values[1];
            complex.gold = entry.values[2];
            complex.power = entry.values[3];
            complex.workers = entry.values[4];
            complex.buildings = entry.values[5];
        }
    }

    ApplyOutcome::Applied
}

// ---------------------------------------------------------------------------
// 54 — authoritative team state (P23 record + economy tail)
// ---------------------------------------------------------------------------

// Packet 54 DTOs are pure lower-layer data (`max_types`, F5.2); here
// re-exported so that the path `max_core::events::…` does not change, while `apply_unit_state`
// and `apply_state_sync` (which work with `GameState`) stay in this module.
pub use max_types::events::{StateSyncPacket, TeamStateSyncTail, UnitStatePacket};

/// Port of `Remote_ApplyUnitState` (`remote.cpp:4383-4512`) for one unit.
///
/// Accepts only a unit of its own team and type; nonexistent/exploded
/// units and orders outside `ORDER_COUNT_MAX` are ignored — as in C++.
pub(crate) fn apply_unit_state(
    state: &mut GameState,
    team: TeamId,
    unit_id: UnitId,
    data: &UnitStatePacket,
) -> ApplyOutcome {
    let Some(mut unit) = state.unit(unit_id).cloned() else {
        // `Remote_ApplyUnitState`: «unit ... does not exist locally».
        return ApplyOutcome::UnknownUnit;
    };

    if unit.team != team || unit.unit_type as u16 != data.unit_type {
        return ApplyOutcome::Ignored;
    }
    if unit.flags & crate::enums::unit_flags::EXPLODING != 0 {
        return ApplyOutcome::Ignored;
    }

    let (Ok(orders), Ok(order_state), Ok(prior_orders), Ok(prior_state)) = (
        UnitOrder::try_from(data.orders),
        UnitOrderState::try_from(data.state),
        UnitOrder::try_from(data.prior_orders),
        UnitOrderState::try_from(data.prior_state),
    ) else {
        return ApplyOutcome::Malformed;
    };

    unit.orders = orders;
    unit.state = order_state;
    unit.prior_orders = prior_orders;
    unit.prior_state = prior_state;
    unit.disabled_reaction_fire = data.disabled_reaction_fire;

    // `SetParent`/`SetEnemy` in C++ take the unit from `Hash_UnitHash`; a missing
    // id gives `nullptr`, so here — `None`.
    unit.parent_unit = if data.parent_unit_id == 0 {
        None
    } else {
        state
            .unit(UnitId(data.parent_unit_id))
            .map(|parent| parent.id)
    };
    unit.enemy_unit = if data.enemy_unit_id == 0 {
        None
    } else {
        state.unit(UnitId(data.enemy_unit_id)).map(|enemy| enemy.id)
    };

    unit.experience = data.experience;
    unit.transfer_cargo = data.transfer_cargo;
    unit.stealth_dice_roll = data.stealth_dice_roll;
    unit.total_mining = data.total_mining;
    unit.raw_mining = data.raw_mining;
    unit.fuel_mining = data.fuel_mining;
    unit.gold_mining = data.gold_mining;
    unit.build_time = data.build_time;
    unit.build_rate = data.build_rate;
    unit.unit_id = data.unit_id;
    unit.hits = data.hits;
    unit.speed = data.speed;
    unit.shots = data.shots;
    unit.storage = data.storage;
    unit.ammo = data.ammo;

    // Movement/fire points in Rust live in `OrderData` (`unit.target_grid`
    // mirrors `move_to`, as in packet 08 P3).
    let mut order_data = state.order_data(unit_id);
    order_data.move_to = Point::new(data.move_to_grid_x, data.move_to_grid_y);
    order_data.fire_on = Point::new(data.fire_on_grid_x, data.fire_on_grid_y);
    unit.target_grid = order_data.move_to;
    state.set_order_data(unit_id, order_data);

    // `Remote_RelocateUnit`: at the receiver the unit stands on the owner's cell;
    // `write_unit` moves it into `Hash_MapHash`.
    unit.grid = Point::new(data.grid_x, data.grid_y);

    state.write_unit(unit_id, unit);
    ApplyOutcome::Applied
}

/// Port of `Remote_ReceiveNetPacket_54` (`remote.cpp:4547-4652`).
///
/// Applies the authoritative state of a remote team, deletes units
/// absent on the owner (except buildings — as `Remote_RemoveExtraUnits`), and
/// aligns id counters/gold/research on the last chunk. The
/// round index is written to [`GameState::state_sync_indices`], which closes
/// the 54 barrier.
pub(crate) fn apply_state_sync(state: &mut GameState, packet: &StateSyncPacket) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }

    let slot = packet.team.0 as usize;
    // "only the owner may dictate the state of its units" — its own state and
    // teams without the remote type are not applied.
    if state.teams[slot].team_type != TeamKind::Remote {
        return ApplyOutcome::Ignored;
    }

    if state.state_sync_receiving_indices[slot] != packet.sync_index {
        state.state_sync_receiving_indices[slot] = packet.sync_index;
        state.state_sync_unit_ids[slot].clear();
    }

    for (unit_id, data) in &packet.units {
        state.state_sync_unit_ids[slot].push(*unit_id);
        apply_unit_state(state, packet.team, *unit_id, data);
    }

    if packet.is_last {
        let Some(tail) = &packet.tail else {
            return ApplyOutcome::Malformed;
        };

        // Id counters — before deleting extra units, as in C++.
        state.teams[slot].number_of_objects_created = tail.number_of_objects_created;
        state.teams[slot].unit_counters = tail.unit_counters;

        remove_extra_units(state, packet.team);
        state.state_sync_unit_ids[slot].clear();

        state.team_units[slot].gold = tail.gold;
        state.teams[slot].stats_gold_spent_on_upgrades = tail.gold_spent_on_upgrades;
        state.teams[slot].research_topics = tail.research;
        state.state_sync_indices[slot] = packet.sync_index;
    }

    ApplyOutcome::Applied
}

/// Port of `Remote_RemoveExtraUnits` (`remote.cpp:4517-4545`).
///
/// Team units absent on the owner are deleted; buildings/ground
/// cover (`STATIONARY | GROUND_COVER`) are not deleted — as in C++.
fn remove_extra_units(state: &mut GameState, team: TeamId) {
    use crate::enums::unit_flags::{EXPLODING, GROUND_COVER};

    let mut owner_unit_ids = state.state_sync_unit_ids[team.0 as usize].clone();
    owner_unit_ids.sort_unstable();

    let mut extra = Vec::new();

    for kind in [
        UnitListKind::GroundCover,
        UnitListKind::MobileLandSea,
        UnitListKind::Stationary,
        UnitListKind::MobileAir,
    ] {
        for &id in state.unit_list(kind) {
            let Some(unit) = state.unit(id) else {
                continue;
            };
            if unit.team != team
                || unit.id.is_no_id()
                || unit.flags & EXPLODING != 0
                || owner_unit_ids.binary_search(&unit.id).is_ok()
            {
                continue;
            }
            if !unit.is_mobile() || unit.flags & GROUND_COVER != 0 {
                continue;
            }
            extra.push(unit.id);
        }
    }

    for id in extra {
        state.destroy_unit(id);
    }
}

// ---------------------------------------------------------------------------
// 10 / 20 / 43 / 12 — upgrades, name, supplies
// ---------------------------------------------------------------------------

/// Order of the 13 attributes of packets 10/20 (`max-brh-docs/protocol.md` §8.4).
///
/// Matches the assignment order in `Remote_ReceiveNetPacket_10/20`.
pub const UPGRADE_ATTRIBUTE_ORDER: [u8; UPGRADE_VALUE_COUNT] = [
    ATTRIB_TURNS,
    ATTRIB_HITS,
    ATTRIB_ARMOR,
    ATTRIB_ATTACK,
    ATTRIB_SPEED,
    ATTRIB_RANGE,
    ATTRIB_ROUNDS,
    ATTRIB_MOVE_AND_FIRE,
    ATTRIB_SCAN,
    ATTRIB_STORAGE,
    ATTRIB_AMMO,
    ATTRIB_ATTACK_RADIUS,
    ATTRIB_AGENT_ADJUST,
];

fn apply_packet_10(
    state: &mut GameState,
    packet: &crate::input::Packet10UpgradeUnitType,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }

    let index = packet.unit_type as usize;
    if index >= crate::unit_values::UNIT_COUNT {
        return ApplyOutcome::Malformed;
    }

    let Some(team_units) = state.team_units.get_mut(packet.team.0 as usize) else {
        return ApplyOutcome::Ignored;
    };

    let mut values = team_units.current_values[index];
    values.update_version();
    values.mark_as_not_in_use();

    for (attribute, value) in UPGRADE_ATTRIBUTE_ORDER.iter().zip(packet.values.iter()) {
        values.set_attribute(*attribute, i32::from(*value));
    }

    team_units.current_values[index] = values;
    ApplyOutcome::Applied
}

fn apply_packet_20(
    state: &mut GameState,
    packet: &crate::input::Packet20UpgradeUnit,
) -> ApplyOutcome {
    let Some(mut unit) = state.unit(packet.unit).cloned() else {
        return ApplyOutcome::UnknownUnit;
    };

    // The upgrade lands on a copy: base values are shared per type until the first
    // copy (`Remote_ReceiveNetPacket_20`).
    let mut values = unit.base_values;
    values.update_version();
    values.mark_as_in_use();

    for (attribute, value) in UPGRADE_ATTRIBUTE_ORDER.iter().zip(packet.values.iter()) {
        values.set_attribute(*attribute, i32::from(*value));
    }

    unit.base_values = values;
    state.write_unit(packet.unit, unit);
    ApplyOutcome::Applied
}

fn apply_packet_43(state: &mut GameState, packet: &crate::input::Packet43Rename) -> ApplyOutcome {
    let Some(mut unit) = state.unit(packet.unit).cloned() else {
        return ApplyOutcome::UnknownUnit;
    };
    unit.name = packet.name.clone();
    state.write_unit(packet.unit, unit);
    ApplyOutcome::Applied
}

fn apply_packet_12(
    state: &mut GameState,
    packet: &crate::input::Packet12MissionSupply,
) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }

    state.teams[packet.team.0 as usize].stats_gold_spent_on_upgrades =
        packet.gold_spent_on_upgrades;
    state.mission_supplies[packet.team.0 as usize] = Some(packet.clone());
    ApplyOutcome::Applied
}

// ---------------------------------------------------------------------------
// 09 — team state (gold / upgrade spending / research allocation)
// ---------------------------------------------------------------------------

/// `Remote_ReceiveNetPacket_09` (`remote.cpp:2088-2107`).
///
/// Oracle order: validate the team index (`Remote_IsValidTeamIndex`), then assign
/// the absolute gold (`SetGold`), the absolute cumulative
/// `stats_gold_spent_on_upgrades` and the research topics.
///
/// **Research assignment.** Packet 09 is what carries `ResearchMenu_OnClick_Done`
/// over the command channel (the port's UI may not write the simulation): the
/// `allocation` entry of every topic is the **menu target** the player committed,
/// and applying it is exactly the C++ loop
/// ([`GameState::set_research_allocation`]): each powered-on `RESEARCH` center keeps
/// its topic while the count lasts, moves to the first topic with room, or is
/// powered off. `research_level`/`turns_to_complete` are informational here — the
/// same loop recomputes them from the target, so a sender that puts the *resulting*
/// values in those fields (the oracle does) is applied identically.
///
/// The `team_units.gold` / `stats_gold_spent_on_upgrades` / array writes are the
/// oracle's field-by-field assignment; the loop only runs when the packet's array
/// differs from the current one, so a gold-only commit (the upgrade menu copies the
/// team state verbatim) never touches the centers.
///
/// **Team name.** C++ writes the `SmartString` into
/// `settings[menu_team_name_setting[team]]`; the team name is a UI preference, not
/// simulation state, and `max_core` does not own preferences. The field stays in the
/// packet for wire compatibility and is applied by the application layer.
fn apply_packet_09(state: &mut GameState, packet: &Packet09TeamState) -> ApplyOutcome {
    if !is_valid_team_index(packet.team) {
        return ApplyOutcome::Ignored;
    }

    let index = packet.team.0 as usize;

    let Some(team_units) = state.team_units.get_mut(index) else {
        return ApplyOutcome::Ignored;
    };

    team_units.gold = packet.gold;

    let (targets, research_changed) = {
        let Some(team) = state.teams.get_mut(index) else {
            return ApplyOutcome::Ignored;
        };

        team.stats_gold_spent_on_upgrades = packet.gold_spent_on_upgrades;

        let mut targets = [0u32; crate::enums::RESEARCH_TOPIC_COUNT];
        for (slot, incoming) in targets.iter_mut().zip(packet.research.iter()) {
            *slot = incoming.allocation;
        }

        // A packet that repeats the current array is a gold-only commit (the
        // upgrade screen copies the team state verbatim): the loop is skipped, so
        // buying an upgrade never touches the research centers. Any difference
        // (`allocation`, and the level/turns the loop recomputes) is a research
        // commit and takes the `Done` path.
        (targets, team.research_topics != packet.research)
    };

    if research_changed {
        state.set_research_allocation(packet.team, targets);
    }

    ApplyOutcome::Applied
}

// ---------------------------------------------------------------------------
// Helpers for tests and the network layer
// ---------------------------------------------------------------------------

/// Builds a `P1` payload with an order, keeping the other fields as on `unit`.
pub fn payload_p1_for(
    order: UnitOrder,
    state: UnitOrderState,
    prior_order: UnitOrder,
    prior_state: UnitOrderState,
) -> PayloadP1 {
    PayloadP1 {
        order,
        state,
        prior_order,
        prior_state,
        disabled_reaction_fire: false,
    }
}

/// Builds command 08 with a `P1`-level payload.
pub fn command_order_p1(unit: UnitId, p1: PayloadP1) -> Command {
    Command::Order(Packet08Order {
        unit,
        payload: OrderPayload::P1(p1),
    })
}

/// Builds command 38 "movement along a linear route" (as
/// `GroundPath::AppendLinearSteps`).
pub fn command_move_linear(
    unit: UnitId,
    from: Point,
    to: Point,
    order: UnitOrder,
    state: UnitOrderState,
) -> Command {
    let mut steps = Vec::new();
    let mut x = i32::from(to.x) - i32::from(from.x);
    let mut y = i32::from(to.y) - i32::from(from.y);

    while x != 0 || y != 0 {
        let step_x = x.signum() as i8;
        let step_y = y.signum() as i8;
        steps.push(crate::geo::PathStep::new(step_x, step_y));
        x -= i32::from(step_x);
        y -= i32::from(step_y);
    }

    Command::Move(crate::input::Packet38Move {
        unit,
        order,
        state,
        grid: from,
        move_to: to,
        group_speed: 0,
        path: Some(MovePathData::Ground(steps)),
    })
}

/// Builds command 06 (end of the team's turn).
pub fn command_finished_turn(team: TeamId) -> Command {
    Command::FinishedTurn(crate::input::Packet06FinishedTurn { team })
}

/// Builds command 52 (end-of-turn barrier).
pub fn command_end_turn_barrier(team: TeamId, counter: u8) -> Command {
    Command::EndTurnBarrier(crate::input::Packet52EndTurnBarrier { team, counter })
}

/// Research topic by index — checking the shape of packets 10/20 in tests.
pub fn research_topic_at(index: u8) -> Option<ResearchTopic> {
    RESEARCH_TOPICS.get(index as usize).copied()
}

const RESEARCH_TOPICS: [ResearchTopic; 8] = [
    ResearchTopic::Attack,
    ResearchTopic::Shots,
    ResearchTopic::Range,
    ResearchTopic::Armor,
    ResearchTopic::Hits,
    ResearchTopic::Speed,
    ResearchTopic::Scan,
    ResearchTopic::Cost,
];

/// Unit type from a `u16` (for building packets) — `None` outside `UNIT_END`.
pub fn unit_type_from_raw(raw: u16) -> Option<UnitType> {
    UnitType::try_from(raw).ok()
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
