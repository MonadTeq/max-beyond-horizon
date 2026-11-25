//! Desync audit 23/24/49 (`remote.cpp:880-1428`, `PLAN-host-mode.md` §5 H5).
//!
//! The host detects a CRC divergence at the packet-45 barrier and runs the audit
//! the Klei oracle runs in `Remote_AnalyzeDesyncHost`:
//!
//! 1. **23 (host → peers, one unit at a time).** The authority sends the canonical
//!    `UnitState` (`Packet23Data`) of a unit that is not a mining station; mining
//!    stations are skipped exactly like `if ((*it).GetUnitType() != MININGST)`.
//! 2. **24 (peer → host).** A peer compares the snapshot with its own copy and
//!    answers with its team. The C++ client only raises `Remote_P24_Signals`
//!    (`status` is always `1`) and prints the field-level difference locally; the
//!    Rust client reports the comparison result in the same `status` bit, so the
//!    host can aggregate the mismatches into the report file.
//! 3. **49 (host → peers).** `Remote_SendNetPacket_Signal(REMOTE_PACKET_49, …)`
//!    closes the audit; both sides then continue the lockstep turn.
//!
//! The host writes `desync-turn<turn>.txt` (the fork's `desync-turn*.txt` report)
//! with the turn, the packet-45 CRCs and the per-unit mismatches, and prints one
//! audit line from `--host`.
//!
//! The audit is diagnostic only: it never changes `snapshot_bytes`/CRC, the
//! barrier FSM or the packet order of the match.

#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use max_core::team::TeamId;
use max_core::unit::UnitId;
use max_core::universe::StateRef;
use max_net::{Packet, UnitState};

/// How long the authority waits for one unit's packet-24 acknowledgement.
///
/// The oracle waits `REMOTE_RESPONSE_TIMEOUT` (30 s) for the whole audit; the
/// per-unit budget is smaller so a lost peer cannot stall the report forever.
pub(crate) const DESYNC_AUDIT_UNIT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a peer waits for the authority's closing packet 49.
pub(crate) const DESYNC_AUDIT_TIMEOUT: Duration = Duration::from_secs(30);
/// Event poll interval inside the audit loops.
pub(crate) const DESYNC_AUDIT_EVENT_WAIT: Duration = Duration::from_millis(5);

/// One unit whose state did not match on a peer (`24 status=false`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditMismatch {
    /// Audited unit.
    pub unit: UnitId,
    /// Team that reported the difference.
    pub team: TeamId,
}

/// Result of one desync audit round (`PLAN-host-mode.md` §5 H5).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DesyncAudit {
    /// 1-based `GameManager_TurnCounter` of the audited turn.
    pub turn: i32,
    /// Units the authority sent in packet 23.
    pub units: usize,
    /// Remote teams the authority expected acknowledgements from.
    pub peers: Vec<TeamId>,
    /// Units some peer disagreed with.
    pub mismatches: Vec<AuditMismatch>,
    /// Acknowledgements that did not arrive before the per-unit timeout.
    pub missing_acks: usize,
    /// The audit stopped early (the link or the authority went away).
    pub aborted: bool,
    /// Report file written by the host (`None` — a guest, or no audit directory).
    pub report: Option<PathBuf>,
}

impl DesyncAudit {
    /// A fresh audit round of `turn` for `peers`.
    pub fn new(turn: i32, peers: Vec<TeamId>) -> Self {
        Self {
            turn,
            peers,
            ..Self::default()
        }
    }

    /// One line for the CLI: the `host-audit:`/`audit:` report of `--host`.
    pub fn to_line(&self) -> String {
        let report = self
            .report
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "-".to_string());
        format!(
            "turn={} units={} peers={} mismatches={} missing_acks={} aborted={} report={report}",
            self.turn,
            self.units,
            self.peers.len(),
            self.mismatches.len(),
            self.missing_acks,
            self.aborted
        )
    }

    /// Writes `desync-turn<turn>.txt` into `dir` and stores its path in [`Self::report`].
    ///
    /// The file is a diagnostic artifact, not part of the simulation state: it
    /// carries the turn, the packet-45 CRCs and the mismatches the peers reported.
    pub(crate) fn write_report(
        &mut self,
        dir: &Path,
        own_crc: u16,
        peer_crc: &[(TeamId, u16)],
    ) -> std::io::Result<PathBuf> {
        let path = dir.join(format!("desync-turn{}.txt", self.turn));
        let mut text = String::new();
        let _ = writeln!(
            text,
            "M.A.X. : Beyond Rust Horizon — desync audit (host, packets 23/24/49)"
        );
        let _ = writeln!(text, "turn: {}", self.turn);
        let _ = writeln!(text, "units audited: {}", self.units);
        let _ = writeln!(text, "own crc45: 0x{own_crc:04X}");
        let peers: Vec<String> = peer_crc
            .iter()
            .map(|(team, crc)| format!("{}:0x{crc:04X}", team.0))
            .collect();
        let _ = writeln!(text, "peer crc45: [{}]", peers.join(", "));
        let _ = writeln!(text, "mismatches: {}", self.mismatches.len());
        for mismatch in &self.mismatches {
            let _ = writeln!(
                text,
                "mismatch: unit {} team {}",
                mismatch.unit.0, mismatch.team.0
            );
        }
        let _ = writeln!(text, "missing acks: {}", self.missing_acks);
        let _ = writeln!(text, "aborted: {}", self.aborted);
        std::fs::write(&path, text)?;
        self.report = Some(path.clone());
        Ok(path)
    }
}

/// Field-level difference of two canonical unit states (diagnostic log).
pub(crate) fn diff_unit_state(
    host: &UnitState,
    local: &UnitState,
) -> Vec<(&'static str, String, String)> {
    let mut diff = Vec::new();
    macro_rules! diff_fields {
        ($($field:ident),+ $(,)?) => {
            $(
                if host.$field != local.$field {
                    diff.push((
                        stringify!($field),
                        format!("{:?}", host.$field),
                        format!("{:?}", local.$field),
                    ));
                }
            )+
        };
    }
    diff_fields!(
        orders,
        state,
        prior_orders,
        prior_state,
        disabled_reaction_fire,
        parent_unit_id,
        experience,
        move_to_grid_x,
        move_to_grid_y,
        fire_on_grid_x,
        fire_on_grid_y,
        enemy_unit_id,
        transfer_cargo,
        stealth_dice_roll,
        total_mining,
        raw_mining,
        fuel_mining,
        gold_mining,
        build_time,
        build_rate,
        unit_type,
        unit_id,
        grid_x,
        grid_y,
        team,
        hits,
        speed,
        shots,
        storage,
        ammo,
    );
    diff
}

/// Peer side of the audit: compares packet 23 with the local unit and builds packet 24.
///
/// `None` for every other packet; `AuditDone` (49) only closes the wait and is
/// logged. A missing local unit and a mismatching state both answer `status =
/// false` and log the field-level difference (`Remote_NetErrorUnitInfoOutOfSync`
/// in the oracle).
pub(crate) fn audit_reply(
    snapshot: StateRef<'_>,
    local: TeamId,
    packet: &Packet,
) -> Option<Packet> {
    match packet {
        Packet::Audit { unit, state } => {
            let actual = snapshot
                .unit(*unit)
                .map(|unit| UnitState::from_unit(snapshot, unit));
            let matched = actual.as_ref() == Some(state);
            if !matched {
                match actual {
                    Some(actual) => {
                        let diff = diff_unit_state(state, &actual);
                        let fields: Vec<String> = diff
                            .iter()
                            .map(|(field, host, local)| {
                                format!("{field}: host={host}, local={local}")
                            })
                            .collect();
                        tracing::error!(
                            "десинк-аудит: юнит {} (тип {}) не совпал: {}",
                            unit.0,
                            actual.unit_type as u16,
                            fields.join("; ")
                        );
                    }
                    None => {
                        tracing::error!(
                            "десинк-аудит: юнит {} отсутствует в локальном мире",
                            unit.0
                        );
                    }
                }
            }
            Some(Packet::AuditAck {
                team: local,
                status: matched,
            })
        }
        Packet::AuditDone { team, param } => {
            tracing::info!("десинк-аудит завершён: 49 team={} param={param}", team.0);
            None
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod tests;
