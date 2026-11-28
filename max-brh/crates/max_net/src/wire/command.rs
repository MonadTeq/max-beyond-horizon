//! Codecs for the in-game APPL commands (`max-brh-docs/protocol.md` §8.4).
//!
//! The DTOs are [`max_types::input`] types: the protocol model already exists
//! there and is what [`max_core::events::apply_command`] consumes, so the wire
//! layer is a pure byte ⇄ `Command` mapping (no duplicate packet structs).
//!
//! Layouts were cross-checked against the headless server (`src/remote.cpp`,
//! `Remote_SendNetPacket_*` / `Remote_ReceiveNetPacket_*`). The fork's `src/`
//! carries the same richer layouts as `max-brh-docs/protocol.md` (grid in 38, round in
//! 12, full form of 41), while the upstream `max-cpp-port/` snapshot still has
//! shorter forms; both forms of 41 are accepted on decode.

use max_types::enums::{UnitOrder, UnitOrderState, UnitType};
use max_types::geo::{PathStep, Point};
use max_types::input::{
    Command, ComplexSummaryEntry, GroupSelectMode, MissionSupplyEntry, MovePathData, OrderPayload,
    Packet00TurnBarrier, Packet01FrameBarrier, Packet06FinishedTurn, Packet08Order,
    Packet09TeamState, Packet10UpgradeUnitType, Packet11ComplexOrder, Packet12MissionSupply,
    Packet13Seed, Packet14DeployUnit, Packet18Chat, Packet20UpgradeUnit, Packet38Move,
    Packet41MoveFinished, Packet43Rename, Packet45CrcBarrier, Packet50StopUnit,
    Packet52EndTurnBarrier, Packet53ComplexSummary, Packet57GroupSelect, PayloadLevel, PayloadP1,
    PayloadP2, PayloadP3, PayloadP4, PayloadP5, UPGRADE_VALUE_COUNT,
};
use max_types::team::{ResearchTopicInfo, TeamId};
use max_types::unit::UnitId;

use super::reader::{Reader, WireError, WireResult, Writer, MAX_VECTOR_LEN};

/// Number of research topics in packet 09 (C++ `RESEARCH_TOPIC_COUNT`).
const RESEARCH_TOPIC_COUNT: usize = max_types::enums::RESEARCH_TOPIC_COUNT;

/// Reads a `u16` team index into [`TeamId`].
fn read_team(r: &mut Reader<'_>) -> WireResult<TeamId> {
    let raw = r.read_u16()?;
    Ok(TeamId(raw as u8))
}

fn write_team(w: &mut Writer, team: TeamId) {
    w.write_u16(u16::from(team.0));
}

fn read_unit(r: &mut Reader<'_>) -> WireResult<UnitId> {
    Ok(UnitId(r.read_u16()?))
}

fn read_point(r: &mut Reader<'_>) -> WireResult<Point> {
    Ok(Point::new(r.read_i16()?, r.read_i16()?))
}

fn write_point(w: &mut Writer, point: Point) {
    w.write_i16(point.x);
    w.write_i16(point.y);
}

fn read_unit_type(r: &mut Reader<'_>) -> WireResult<UnitType> {
    let raw = r.read_u16()?;
    UnitType::try_from(raw).map_err(|_| WireError::InvalidValue {
        kind: "UnitType",
        value: u32::from(raw),
    })
}

fn read_order(r: &mut Reader<'_>, kind: &'static str) -> WireResult<UnitOrder> {
    let raw = r.read_u8()?;
    UnitOrder::try_from(raw).map_err(|_| WireError::InvalidValue {
        kind,
        value: u32::from(raw),
    })
}

fn read_order_state(r: &mut Reader<'_>) -> WireResult<UnitOrderState> {
    let raw = r.read_u8()?;
    UnitOrderState::try_from(raw).map_err(|_| WireError::InvalidValue {
        kind: "UnitOrderState",
        value: u32::from(raw),
    })
}

fn read_parent(r: &mut Reader<'_>) -> WireResult<Option<UnitId>> {
    let raw = r.read_u16()?;
    Ok((raw != 0).then_some(UnitId(raw)))
}

fn write_parent(w: &mut Writer, parent: Option<UnitId>) {
    w.write_u16(parent.map_or(0, |unit| unit.0));
}

fn read_values13(r: &mut Reader<'_>) -> WireResult<[u16; UPGRADE_VALUE_COUNT]> {
    let mut values = [0u16; UPGRADE_VALUE_COUNT];
    for value in &mut values {
        *value = r.read_u16()?;
    }
    Ok(values)
}

fn write_values13(w: &mut Writer, values: &[u16; UPGRADE_VALUE_COUNT]) {
    for value in values {
        w.write_u16(*value);
    }
}

// ---------------------------------------------------------------------------
// Packet 08 — unit order
// ---------------------------------------------------------------------------

fn write_p1(w: &mut Writer, p1: &PayloadP1) {
    w.write_u8(p1.order as u8);
    w.write_u8(p1.state as u8);
    w.write_u8(p1.prior_order as u8);
    w.write_u8(p1.prior_state as u8);
    w.write_bool(p1.disabled_reaction_fire);
}

fn write_p2(w: &mut Writer, p2: &PayloadP2) {
    write_p1(w, &p2.p1);
    write_parent(w, p2.parent_unit);
    w.write_i16(p2.experience);
}

fn write_p3(w: &mut Writer, p3: &PayloadP3) {
    write_p2(w, &p3.p2);
    write_point(w, p3.move_to);
    write_point(w, p3.fire_on);
    write_parent(w, p3.enemy_unit);
    w.write_i16(p3.transfer_cargo);
    w.write_u8(p3.stealth_dice_roll);
}

fn write_p4(w: &mut Writer, p4: &PayloadP4) -> WireResult<()> {
    if p4.build_list.len() > MAX_VECTOR_LEN {
        return Err(WireError::VectorTooLong {
            len: p4.build_list.len(),
            max: MAX_VECTOR_LEN,
        });
    }
    write_p3(w, &p4.p3);
    w.write_bool(p4.repeat_build);
    w.write_u8(p4.build_time);
    w.write_u16(p4.build_rate);
    w.write_u32(p4.build_list.len() as u32);
    for unit_type in &p4.build_list {
        w.write_u16(*unit_type as u16);
    }
    Ok(())
}

fn write_p5(w: &mut Writer, p5: &PayloadP5) {
    write_p3(w, &p5.p3);
    w.write_u8(p5.total_mining);
    w.write_u8(p5.raw_mining);
    w.write_u8(p5.fuel_mining);
    w.write_u8(p5.gold_mining);
}

/// Reads the processor payload whose shape is selected by the order byte.
///
/// Mirrors `Remote_OrderProcessors[unit->GetOrder()]` selection: the first
/// payload field is the new order, and the processor level decides how many
/// further fields follow (`PayloadLevel::for_order`).
fn read_order_payload(r: &mut Reader<'_>) -> WireResult<OrderPayload> {
    let order = read_order(r, "order")?;
    let level = PayloadLevel::for_order(order);

    // P0 (`ORDER_17`) has an empty payload: no state/prior/disabled bytes.
    if level == PayloadLevel::Empty {
        return Ok(OrderPayload::Empty);
    }

    let state = read_order_state(r)?;
    let prior_order = read_order(r, "prior_order")?;
    let prior_state = read_order_state(r)?;
    let disabled_reaction_fire = r.read_bool()?;
    let p1 = PayloadP1 {
        order,
        state,
        prior_order,
        prior_state,
        disabled_reaction_fire,
    };

    match level {
        // Handled above; included for exhaustiveness.
        PayloadLevel::Empty => Ok(OrderPayload::Empty),
        PayloadLevel::P1 => Ok(OrderPayload::P1(p1)),
        PayloadLevel::P2 => Ok(OrderPayload::P2(PayloadP2 {
            p1,
            parent_unit: read_parent(r)?,
            experience: r.read_i16()?,
        })),
        PayloadLevel::P3 => {
            let p2 = PayloadP2 {
                p1,
                parent_unit: read_parent(r)?,
                experience: r.read_i16()?,
            };
            Ok(OrderPayload::P3(read_p3_tail(r, p2)?))
        }
        PayloadLevel::P4 => {
            let p2 = PayloadP2 {
                p1,
                parent_unit: read_parent(r)?,
                experience: r.read_i16()?,
            };
            let p3 = read_p3_tail(r, p2)?;
            let repeat_build = r.read_bool()?;
            let build_time = r.read_u8()?;
            let build_rate = r.read_u16()?;
            let count = r.read_vec_len()?;
            let mut build_list = Vec::with_capacity(count);
            for _ in 0..count {
                build_list.push(read_unit_type(r)?);
            }
            Ok(OrderPayload::P4(PayloadP4 {
                p3,
                repeat_build,
                build_time,
                build_rate,
                build_list,
            }))
        }
        PayloadLevel::P5 => {
            let p2 = PayloadP2 {
                p1,
                parent_unit: read_parent(r)?,
                experience: r.read_i16()?,
            };
            let p3 = read_p3_tail(r, p2)?;
            Ok(OrderPayload::P5(PayloadP5 {
                p3,
                total_mining: r.read_u8()?,
                raw_mining: r.read_u8()?,
                fuel_mining: r.read_u8()?,
                gold_mining: r.read_u8()?,
            }))
        }
    }
}

/// Reads the P3-only tail (`move_to`, `fire_on`, `enemy`, `transfer`,
/// `stealth`) around an already-decoded `P2`.
fn read_p3_tail(r: &mut Reader<'_>, p2: PayloadP2) -> WireResult<PayloadP3> {
    Ok(PayloadP3 {
        p2,
        move_to: read_point(r)?,
        fire_on: read_point(r)?,
        enemy_unit: read_parent(r)?,
        transfer_cargo: r.read_i16()?,
        stealth_dice_roll: r.read_u8()?,
    })
}

fn write_order_payload(w: &mut Writer, payload: &OrderPayload) -> WireResult<()> {
    match payload {
        OrderPayload::Empty => w.write_u8(UnitOrder::Order17 as u8),
        OrderPayload::P1(p1) => write_p1(w, p1),
        OrderPayload::P2(p2) => write_p2(w, p2),
        OrderPayload::P3(p3) => write_p3(w, p3),
        OrderPayload::P4(p4) => write_p4(w, p4)?,
        OrderPayload::P5(p5) => write_p5(w, p5),
    }
    Ok(())
}

fn read_packet_08(r: &mut Reader<'_>) -> WireResult<Command> {
    let unit = read_unit(r)?;
    let payload = read_order_payload(r)?;
    Ok(Command::Order(Packet08Order { unit, payload }))
}

fn write_packet_08(w: &mut Writer, packet: &Packet08Order) -> WireResult<()> {
    w.write_u16(packet.unit.0);
    write_order_payload(w, &packet.payload)
}

// ---------------------------------------------------------------------------
// Packet 09 — team state (gold / upgrade spending / research allocation)
// ---------------------------------------------------------------------------
//
// Oracle: `Remote_SendNetPacket_09` / `Remote_ReceiveNetPacket_09`
// (`remote.cpp:2067/2088`). The research array is written as its raw in-memory
// struct bytes: 8 × { i32 level, i32 turns, i32 allocation } = 96 bytes.

fn read_packet_09(r: &mut Reader<'_>) -> WireResult<Command> {
    let team = read_team(r)?;
    let gold = r.read_u32()?;
    let gold_spent_on_upgrades = r.read_u32()?;

    let mut research = [ResearchTopicInfo::default(); RESEARCH_TOPIC_COUNT];
    for topic in &mut research {
        *topic = ResearchTopicInfo {
            research_level: r.read_i32()? as u32,
            turns_to_complete: r.read_i32()? as u32,
            allocation: r.read_i32()? as u32,
        };
    }

    let name = r.read_smart_string()?;

    Ok(Command::TeamState(Packet09TeamState {
        team,
        gold,
        gold_spent_on_upgrades,
        research,
        name,
    }))
}

fn write_packet_09(w: &mut Writer, packet: &Packet09TeamState) -> WireResult<()> {
    write_team(w, packet.team);
    w.write_u32(packet.gold);
    w.write_u32(packet.gold_spent_on_upgrades);
    for topic in &packet.research {
        w.write_i32(topic.research_level as i32);
        w.write_i32(topic.turns_to_complete as i32);
        w.write_i32(topic.allocation as i32);
    }
    w.write_smart_string(&packet.name)
}

// ---------------------------------------------------------------------------
// Packet 38 — movement with a path
// ---------------------------------------------------------------------------

fn read_packet_38(r: &mut Reader<'_>) -> WireResult<Command> {
    let unit = read_unit(r)?;
    let order = read_order(r, "order")?;
    let state = read_order_state(r)?;
    let grid = read_point(r)?;
    let move_to = read_point(r)?;
    let group_speed = r.read_u16()?;
    let steps_count = r.read_u16()? as usize;

    let path = if steps_count == 0 {
        None
    } else if steps_count == 1 && r.remaining() == 18 {
        // Air path: the receiver disambiguates by unit flag, the wire has no
        // explicit discriminator. Eighteen bytes of tail is unambiguous.
        Some(MovePathData::Air {
            end_x: r.read_i16()?,
            end_y: r.read_i16()?,
            distance_x: r.read_i32()?,
            distance_y: r.read_i32()?,
            euclidean_distance: r.read_i16()?,
            speed: r.read_u16()?,
            move_fraction: r.read_u8()?,
            max_velocity: r.read_u8()?,
        })
    } else {
        if steps_count > MAX_VECTOR_LEN {
            return Err(WireError::VectorTooLong {
                len: steps_count,
                max: MAX_VECTOR_LEN,
            });
        }
        let mut steps = Vec::with_capacity(steps_count);
        for _ in 0..steps_count {
            steps.push(PathStep::new(r.read_i8()?, r.read_i8()?));
        }
        Some(MovePathData::Ground(steps))
    };

    Ok(Command::Move(Packet38Move {
        unit,
        order,
        state,
        grid,
        move_to,
        group_speed,
        path,
    }))
}

fn write_packet_38(w: &mut Writer, packet: &Packet38Move) -> WireResult<()> {
    w.write_u16(packet.unit.0);
    w.write_u8(packet.order as u8);
    w.write_u8(packet.state as u8);
    write_point(w, packet.grid);
    write_point(w, packet.move_to);
    w.write_u16(packet.group_speed);

    match &packet.path {
        None => w.write_u16(0),
        Some(MovePathData::Ground(steps)) => {
            if steps.len() > MAX_VECTOR_LEN || steps.len() > u16::MAX as usize {
                return Err(WireError::VectorTooLong {
                    len: steps.len(),
                    max: MAX_VECTOR_LEN,
                });
            }
            w.write_u16(steps.len() as u16);
            for step in steps {
                w.write_i8(step.x);
                w.write_i8(step.y);
            }
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
            w.write_u16(1);
            w.write_i16(*end_x);
            w.write_i16(*end_y);
            w.write_i32(*distance_x);
            w.write_i32(*distance_y);
            w.write_i16(*euclidean_distance);
            w.write_u16(*speed);
            w.write_u8(*move_fraction);
            w.write_u8(*max_velocity);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Packet 41 — movement finished
// ---------------------------------------------------------------------------

/// Bytes after `type, unit_id, mode` in the full protocol form.
const PACKET_41_TAIL: usize = 10;

fn read_packet_41(r: &mut Reader<'_>) -> WireResult<Command> {
    let unit = read_unit(r)?;
    let mode = r.read_bool()?;

    // Upstream `max-cpp-port` sends only `unit_id, mode` (short form); the
    // fork (`src/remote.cpp`) and max-brh-docs/protocol.md send the full form.
    if r.remaining() == 0 {
        return Ok(Command::MoveFinished(Packet41MoveFinished::port_form(
            unit, mode,
        )));
    }
    if r.remaining() != PACKET_41_TAIL {
        return Err(WireError::TrailingBytes(r.remaining()));
    }

    Ok(Command::MoveFinished(Packet41MoveFinished {
        unit,
        mode,
        grid: Some(read_point(r)?),
        speed: Some(r.read_u16()?),
        group_speed: Some(r.read_u16()?),
        shots: Some(r.read_u8()?),
        move_fraction: Some(r.read_u8()?),
    }))
}

fn write_packet_41(w: &mut Writer, packet: &Packet41MoveFinished) -> WireResult<()> {
    w.write_u16(packet.unit.0);
    w.write_bool(packet.mode);

    let fields = (
        packet.grid,
        packet.speed,
        packet.group_speed,
        packet.shots,
        packet.move_fraction,
    );
    let set = [
        fields.0.is_some(),
        fields.1.is_some(),
        fields.2.is_some(),
        fields.3.is_some(),
        fields.4.is_some(),
    ];
    if set.iter().all(|value| !value) {
        return Ok(());
    }
    if !set.iter().all(|value| *value) {
        return Err(WireError::PartialFields {
            packet: "41 (move finished)",
        });
    }

    let (Some(grid), Some(speed), Some(group_speed), Some(shots), Some(move_fraction)) =
        (fields.0, fields.1, fields.2, fields.3, fields.4)
    else {
        return Err(WireError::PartialFields {
            packet: "41 (move finished)",
        });
    };

    write_point(w, grid);
    w.write_u16(speed);
    w.write_u16(group_speed);
    w.write_u8(shots);
    w.write_u8(move_fraction);
    Ok(())
}

// ---------------------------------------------------------------------------
// 00 / 01 / 06 / 45 / 50 / 52 — barriers and stops
// ---------------------------------------------------------------------------

fn read_packet_00(r: &mut Reader<'_>) -> WireResult<Command> {
    Ok(Command::TurnBarrier(Packet00TurnBarrier {
        team: read_team(r)?,
        counter: r.read_u8()?,
    }))
}

fn read_packet_01(r: &mut Reader<'_>) -> WireResult<Command> {
    Ok(Command::FrameBarrier(Packet01FrameBarrier {
        team: read_team(r)?,
        counter: r.read_u8()?,
    }))
}

fn read_packet_06(r: &mut Reader<'_>) -> WireResult<Command> {
    Ok(Command::FinishedTurn(Packet06FinishedTurn {
        team: read_team(r)?,
    }))
}

fn read_packet_45(r: &mut Reader<'_>) -> WireResult<Command> {
    Ok(Command::CrcBarrier(Packet45CrcBarrier {
        team: read_team(r)?,
        next_turn_index: r.read_u8()?,
        crc16: r.read_u16()?,
    }))
}

fn read_packet_50(r: &mut Reader<'_>) -> WireResult<Command> {
    Ok(Command::Stop(Packet50StopUnit {
        unit: read_unit(r)?,
    }))
}

fn read_packet_52(r: &mut Reader<'_>) -> WireResult<Command> {
    Ok(Command::EndTurnBarrier(Packet52EndTurnBarrier {
        team: read_team(r)?,
        counter: r.read_u8()?,
    }))
}

// ---------------------------------------------------------------------------
// 10 / 20 / 11 / 53 — upgrades and complexes
// ---------------------------------------------------------------------------

fn read_packet_10(r: &mut Reader<'_>) -> WireResult<Command> {
    Ok(Command::UpgradeUnitType(Packet10UpgradeUnitType {
        team: read_team(r)?,
        unit_type: read_unit_type(r)?,
        values: read_values13(r)?,
    }))
}

fn read_packet_20(r: &mut Reader<'_>) -> WireResult<Command> {
    Ok(Command::UpgradeUnit(Packet20UpgradeUnit {
        unit: read_unit(r)?,
        values: read_values13(r)?,
    }))
}

fn read_packet_11(r: &mut Reader<'_>) -> WireResult<Command> {
    Ok(Command::ComplexOrder(Packet11ComplexOrder {
        team: read_team(r)?,
        complex_id: r.read_u16()?,
        raw: r.read_i16()?,
        fuel: r.read_i16()?,
        gold: r.read_i16()?,
        power: r.read_i16()?,
        workers: r.read_i16()?,
        buildings: r.read_i16()?,
    }))
}

fn read_packet_53(r: &mut Reader<'_>) -> WireResult<Command> {
    let team = read_team(r)?;
    let turn = r.read_i32()?;
    let count = r.read_u16()? as usize;
    if count > MAX_VECTOR_LEN {
        return Err(WireError::VectorTooLong {
            len: count,
            max: MAX_VECTOR_LEN,
        });
    }
    let mut complexes = Vec::with_capacity(count);
    for _ in 0..count {
        complexes.push(ComplexSummaryEntry {
            complex_id: r.read_u16()?,
            values: [
                r.read_i16()?,
                r.read_i16()?,
                r.read_i16()?,
                r.read_i16()?,
                r.read_i16()?,
                r.read_i16()?,
            ],
        });
    }
    Ok(Command::ComplexSummary(Packet53ComplexSummary {
        team,
        turn,
        complexes,
    }))
}

// ---------------------------------------------------------------------------
// 12 / 13 / 14 / 18 / 43 — mission supply, seed, deploy, chat, rename
// ---------------------------------------------------------------------------

fn read_packet_12(r: &mut Reader<'_>) -> WireResult<Command> {
    let team = read_team(r)?;
    let round = r.read_u8()?;
    let start_gold = r.read_u16()?;
    let team_gold = r.read_u16()?;
    let count = r.read_vec_len()?;
    let gold_spent_on_upgrades = r.read_u32()?;
    let starting_position = read_point(r)?;
    let proximity_alert_ack = r.read_u8()?;
    let mut units = Vec::with_capacity(count);
    for _ in 0..count {
        units.push(MissionSupplyEntry {
            unit_type: read_unit_type(r)?,
            cargo: r.read_u16()?,
        });
    }
    Ok(Command::MissionSupply(Packet12MissionSupply {
        team,
        round,
        start_gold,
        team_gold,
        units,
        gold_spent_on_upgrades,
        starting_position,
        proximity_alert_ack,
    }))
}

fn write_packet_12(w: &mut Writer, packet: &Packet12MissionSupply) -> WireResult<()> {
    if packet.units.len() > MAX_VECTOR_LEN {
        return Err(WireError::VectorTooLong {
            len: packet.units.len(),
            max: MAX_VECTOR_LEN,
        });
    }
    write_team(w, packet.team);
    w.write_u8(packet.round);
    w.write_u16(packet.start_gold);
    w.write_u16(packet.team_gold);
    w.write_u32(packet.units.len() as u32);
    w.write_u32(packet.gold_spent_on_upgrades);
    write_point(w, packet.starting_position);
    w.write_u8(packet.proximity_alert_ack);
    for entry in &packet.units {
        w.write_u16(entry.unit_type as u16);
        w.write_u16(entry.cargo);
    }
    Ok(())
}

fn read_packet_13(r: &mut Reader<'_>) -> WireResult<Command> {
    let _team = read_team(r)?;
    Ok(Command::Seed(Packet13Seed {
        seed: r.read_u32()?,
    }))
}

fn read_packet_14(r: &mut Reader<'_>) -> WireResult<Command> {
    let team = read_team(r)?;
    let unit_type = read_unit_type(r)?;
    let x = r.read_i32()?;
    let y = r.read_i32()?;
    let grid = Point::new(
        i16::try_from(x).map_err(|_| WireError::OutOfRange {
            value: i64::from(x),
            target: "Point.x",
        })?,
        i16::try_from(y).map_err(|_| WireError::OutOfRange {
            value: i64::from(y),
            target: "Point.y",
        })?,
    );
    Ok(Command::Deploy(Packet14DeployUnit {
        team,
        unit_type,
        grid,
    }))
}

fn write_packet_14(w: &mut Writer, packet: &Packet14DeployUnit) -> WireResult<()> {
    write_team(w, packet.team);
    w.write_u16(packet.unit_type as u16);
    w.write_i32(i32::from(packet.grid.x));
    w.write_i32(i32::from(packet.grid.y));
    Ok(())
}

fn read_packet_18(r: &mut Reader<'_>) -> WireResult<Command> {
    Ok(Command::Chat(Packet18Chat {
        sender: read_team(r)?,
        message: r.read_smart_string()?,
    }))
}

fn write_packet_18(w: &mut Writer, packet: &Packet18Chat) -> WireResult<()> {
    write_team(w, packet.sender);
    w.write_smart_string(&packet.message)
}

fn read_packet_43(r: &mut Reader<'_>) -> WireResult<Command> {
    Ok(Command::Rename(Packet43Rename {
        unit: read_unit(r)?,
        name: r.read_smart_string()?,
    }))
}

fn write_packet_43(w: &mut Writer, packet: &Packet43Rename) -> WireResult<()> {
    w.write_u16(packet.unit.0);
    w.write_smart_string(&packet.name)
}

// ---------------------------------------------------------------------------
// Packet 57 — group selection (BRH extension, no oracle layout)
// ---------------------------------------------------------------------------

fn read_packet_57(r: &mut Reader<'_>) -> WireResult<Command> {
    let team = read_team(r)?;
    let mode = r.read_u8()?;
    let mode = GroupSelectMode::from_u8(mode).ok_or(WireError::InvalidValue {
        kind: "GroupSelectMode",
        value: u32::from(mode),
    })?;
    let unit = read_parent(r)?;
    let member = read_parent(r)?;
    let upper = read_point(r)?;
    let lower = read_point(r)?;

    Ok(Command::GroupSelect(Packet57GroupSelect {
        team,
        mode,
        unit,
        member,
        upper,
        lower,
    }))
}

fn write_packet_57(w: &mut Writer, packet: &Packet57GroupSelect) -> WireResult<()> {
    write_team(w, packet.team);
    w.write_u8(packet.mode as u8);
    write_parent(w, packet.unit);
    write_parent(w, packet.member);
    write_point(w, packet.upper);
    write_point(w, packet.lower);
    Ok(())
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Decodes a command body (the APPL type byte is already consumed).
pub fn decode_command(kind: u8, r: &mut Reader<'_>) -> WireResult<Command> {
    match kind {
        0x00 => read_packet_00(r),
        0x01 => read_packet_01(r),
        0x06 => read_packet_06(r),
        0x08 => read_packet_08(r),
        0x09 => read_packet_09(r),
        0x0A => read_packet_10(r),
        0x0B => read_packet_11(r),
        0x0C => read_packet_12(r),
        0x0D => read_packet_13(r),
        0x0E => read_packet_14(r),
        0x12 => read_packet_18(r),
        0x14 => read_packet_20(r),
        0x26 => read_packet_38(r),
        0x29 => read_packet_41(r),
        0x2B => read_packet_43(r),
        0x2D => read_packet_45(r),
        0x32 => read_packet_50(r),
        0x34 => read_packet_52(r),
        0x35 => read_packet_53(r),
        0x39 => read_packet_57(r),
        other => Err(WireError::Unencodable { kind: other }),
    }
}

/// Encodes a command body (the APPL type byte is written by the envelope).
pub fn encode_command(command: &Command, w: &mut Writer) -> WireResult<()> {
    match command {
        Command::TurnBarrier(packet) => {
            write_team(w, packet.team);
            w.write_u8(packet.counter);
            Ok(())
        }
        Command::FrameBarrier(packet) => {
            write_team(w, packet.team);
            w.write_u8(packet.counter);
            Ok(())
        }
        Command::FinishedTurn(packet) => {
            write_team(w, packet.team);
            Ok(())
        }
        Command::Order(packet) => write_packet_08(w, packet),
        Command::TeamState(packet) => write_packet_09(w, packet),
        Command::UpgradeUnitType(packet) => {
            write_team(w, packet.team);
            w.write_u16(packet.unit_type as u16);
            write_values13(w, &packet.values);
            Ok(())
        }
        Command::ComplexOrder(packet) => {
            write_team(w, packet.team);
            w.write_u16(packet.complex_id);
            w.write_i16(packet.raw);
            w.write_i16(packet.fuel);
            w.write_i16(packet.gold);
            w.write_i16(packet.power);
            w.write_i16(packet.workers);
            w.write_i16(packet.buildings);
            Ok(())
        }
        Command::MissionSupply(packet) => write_packet_12(w, packet),
        Command::Seed(packet) => {
            write_team(w, TeamId(0));
            w.write_u32(packet.seed);
            Ok(())
        }
        Command::Deploy(packet) => write_packet_14(w, packet),
        Command::Chat(packet) => write_packet_18(w, packet),
        Command::UpgradeUnit(packet) => {
            w.write_u16(packet.unit.0);
            write_values13(w, &packet.values);
            Ok(())
        }
        Command::Move(packet) => write_packet_38(w, packet),
        Command::MoveFinished(packet) => write_packet_41(w, packet),
        Command::Rename(packet) => write_packet_43(w, packet),
        Command::GroupSelect(packet) => write_packet_57(w, packet),
        Command::CrcBarrier(packet) => {
            write_team(w, packet.team);
            w.write_u8(packet.next_turn_index);
            w.write_u16(packet.crc16);
            Ok(())
        }
        Command::Stop(packet) => {
            w.write_u16(packet.unit.0);
            Ok(())
        }
        Command::EndTurnBarrier(packet) => {
            write_team(w, packet.team);
            w.write_u8(packet.counter);
            Ok(())
        }
        Command::ComplexSummary(packet) => {
            if packet.complexes.len() > u16::MAX as usize {
                return Err(WireError::VectorTooLong {
                    len: packet.complexes.len(),
                    max: u16::MAX as usize,
                });
            }
            write_team(w, packet.team);
            w.write_i32(packet.turn);
            w.write_u16(packet.complexes.len() as u16);
            for entry in &packet.complexes {
                w.write_u16(entry.complex_id);
                for value in &entry.values {
                    w.write_i16(*value);
                }
            }
            Ok(())
        }
    }
}
