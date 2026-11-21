//! Headless soak-прогон (P4, `max-brh-docs/development/PLAN-production.md`).
//!
//! Идея: одна локальная партия «человек (RED, ничего не делает) + N
//! компьютеров», где каждый раунд закрывается `local_end_turn` человека, а ходы
//! ИИ прокручиваются [`App::local_update`] с настоящим `dt = 1/60` — ровно как
//! кадры десктопного цикла (`main.rs`). Задача — прогнать 100+ ходов и снять
//! метрики:
//!
//! * время каждого раунда и каждого кадра (`profile`);
//! * RSS процесса на старте/в конце и каждые `sample_every` ходов (`mem`);
//! * факт, что партия дошла до запрошенного хода (без паник и `Game over`).
//!
//! Модуль — библиотечная часть, поэтому его же использует пример
//! `examples/soak_headless.rs` (CLI в CI) и интеграционный тест (короткий
//! прогон). Детерминизм: `dt` используется только для таймера/интерполяции
//! кадра, симуляция по-прежнему делает один тик за кадр.

use std::time::{Duration, Instant};

use max_core::enums::VictoryType;
use max_core::team::TeamKind;

use crate::mem::{process_memory, ProcessMemory};
use crate::profile::{self, ProfileSnapshot};
use crate::{App, LocalPhase, LocalSetup};

/// Параметры soak-прогона.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoakOptions {
    /// Сколько раундов (ходов) сыграть.
    pub turns: u32,
    /// `dt` кадра: `1/60` — десктопный интерактив; `0.0` — headless как QA.
    pub dt: f32,
    /// Сколько компьютеров играет (1..=3).
    pub ai_teams: u8,
    /// Планета (`settings["world"]`).
    pub world: i64,
    /// Seed генерации карты.
    pub seed: u32,
    /// Сложность ИИ (`settings["opponent"]`, 0 — самый простой).
    pub opponent: u8,
    /// Лимит ходов условия победы (должен быть больше `turns`, иначе Duration
    /// закроет партию раньше).
    pub victory_limit: i32,
    /// Стартовые кредиты.
    pub start_gold: u32,
    /// Сколько кадров «ходит» человек перед End Turn (активность в его ход).
    pub human_ticks: u32,
    /// Режим игры (`settings["play_mode"]`): `TurnBased` — как пункт Options
    /// «Turn Based», `SimultaneousMoves` — прежний режим soak.
    pub play_mode: max_core::enums::PlayMode,
    /// Период снятия RSS в ходах (`0` — только старт/конец).
    pub sample_every: u32,
    /// Предохранитель: максимум кадров на один раунд.
    pub frame_limit_per_turn: u32,
}

impl Default for SoakOptions {
    fn default() -> Self {
        Self {
            turns: 120,
            dt: 1.0 / 60.0,
            ai_teams: 2,
            world: 0,
            seed: 0,
            opponent: 0,
            // Duration 200: 120 ходов soak не должны закрывать партию лимитом.
            victory_limit: 200,
            start_gold: 150,
            human_ticks: 5,
            // Пользовательское указание: приоритет — Turn Based.
            play_mode: max_core::enums::PlayMode::TurnBased,
            sample_every: 10,
            frame_limit_per_turn: 50_000,
        }
    }
}

/// Один завершённый раунд.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoakTurn {
    /// Номер закрытого хода (`GameState::turn_counter` на момент закрытия).
    pub turn: i32,
    /// Время раунда (ход человека + ходы ИИ + `finish_turn`).
    pub wall: Duration,
    /// Кадров `local_update` за раунд.
    pub frames: u64,
    /// Юнитов в состоянии на конец раунда.
    pub units: usize,
    /// RSS после раунда (если снимали).
    pub rss_kb: Option<u64>,
}

/// Итог soak-прогона.
#[derive(Debug, Clone, PartialEq)]
pub struct SoakReport {
    /// С какими параметрами шли.
    pub options: SoakOptions,
    /// Сколько раундов реально закрыто.
    pub turns_played: u32,
    /// Всего кадров `local_update`.
    pub frames: u64,
    /// Общее время прогона.
    pub total: Duration,
    /// Раунды (по одному на закрытый ход).
    pub turns: Vec<SoakTurn>,
    /// Память на старте.
    pub mem_start: Option<ProcessMemory>,
    /// Память в конце.
    pub mem_end: Option<ProcessMemory>,
    /// Снимок профиля тика.
    pub profile: ProfileSnapshot,
    /// Партия закончилась раньше запрошенного хода (победа/поражение).
    pub finished_early: bool,
    /// Итоговый `turn_counter`.
    pub final_turn: i32,
    /// Итоговое число юнитов.
    pub final_units: usize,
}

impl SoakReport {
    /// Среднее время раунда.
    pub fn avg_turn(&self) -> Duration {
        if self.turns_played == 0 {
            return Duration::ZERO;
        }

        self.total / self.turns_played
    }

    /// Партия дошла до запрошенного хода (главный критерий soak).
    pub fn passed(&self) -> bool {
        !self.finished_early && self.turns_played >= self.options.turns
    }

    /// Рост RSS «конец − старт», КиБ (0, если замеры недоступны).
    pub fn memory_growth_kb(&self) -> i64 {
        match (self.mem_start, self.mem_end) {
            (Some(start), Some(end)) => end.growth_kb(start),
            _ => 0,
        }
    }

    /// Память не разошлась: рост не больше 64 МиБ (грубая защита от утечки).
    ///
    /// Порог намеренно щедрый: 120 ходов живой партии — это десятки тысяч
    /// тиков, и аллокатор вправе держать свободные страницы. Линейная утечка
    /// даже в 100 КиБ/ход дала бы 12 МиБ, реальный рост — единицы МиБ.
    pub fn memory_stable(&self) -> bool {
        self.memory_growth_kb() <= 64 * 1024
    }

    /// Текст лога (`soak.txt`): по строке на раунд, память и итог.
    pub fn log(&self) -> String {
        let options = self.options;
        let mut text = String::new();

        text.push_str(&format!(
            "# soak_headless: RED (человек, {} кадров/ход) + {} компьютер(ов)\n",
            options.human_ticks, options.ai_teams
        ));
        text.push_str(&format!(
            "# options: turns={} dt={:.2}ms ai={} world={} seed={} opponent={} mode={:?} victory=Duration/{} gold={} sample_every={} frame_limit={}\n",
            options.turns,
            options.dt * 1000.0,
            options.ai_teams,
            options.world,
            options.seed,
            options.opponent,
            options.play_mode,
            options.victory_limit,
            options.start_gold,
            options.sample_every,
            options.frame_limit_per_turn
        ));
        text.push_str(&format!(
            "# mem start: {}\n",
            memory_text(self.mem_start)
        ));

        for turn in &self.turns {
            let memory = match turn.rss_kb {
                Some(rss) => format!(" rss={rss}KiB"),
                None => String::new(),
            };

            text.push_str(&format!(
                "turn={} wall={:.2}ms frames={} units={}{memory}\n",
                turn.turn,
                turn.wall.as_secs_f64() * 1000.0,
                turn.frames,
                turn.units
            ));
        }

        text.push_str(&format!(
            "# mem end: {}\n",
            memory_text(self.mem_end)
        ));
        text.push_str(&format!(
            "# MEMORY: growth={:+} KiB stable={} (threshold 65536 KiB)\n",
            self.memory_growth_kb(),
            if self.memory_stable() { "yes" } else { "NO" }
        ));
        text.push_str(&format!(
            "SOAK RESULT: {} turns={}/{} frames={} wall={:.2}s avg_turn={:.2}ms final_turn={} units={} early_end={}\n",
            if self.passed() { "PASS" } else { "FAIL" },
            self.turns_played,
            options.turns,
            self.frames,
            self.total.as_secs_f64(),
            self.avg_turn().as_secs_f64() * 1000.0,
            self.final_turn,
            self.final_units,
            self.finished_early
        ));
        text.push_str(&profile::summary());
        text.push('\n');

        text
    }
}

fn memory_text(memory: Option<ProcessMemory>) -> String {
    memory
        .map(|memory| memory.line())
        .unwrap_or_else(|| "rss=unavailable".to_string())
}

/// Играет soak-партию на уже подготовленном [`App`].
///
/// Ассеты должны грузиться (как у `qa_run`/`main.rs`): `start_local_game`
/// генерирует карту планеты. Профиль включается принудительно и обнуляется —
/// метрики этого прогона не смешиваются с предыдущими.
pub fn run_soak(app: &mut App, options: &SoakOptions) -> Result<SoakReport, String> {
    if options.turns == 0 {
        return Err("soak: turns должен быть > 0".to_string());
    }

    let ai_teams = options.ai_teams.clamp(1, 3);
    let mut setup = LocalSetup::default();
    setup.world = options.world;
    setup.seed = options.seed;
    setup.opponent = options.opponent;
    setup.timer = 0;
    setup.play_mode = options.play_mode;
    setup.victory_type = VictoryType::Duration;
    setup.victory_limit = options.victory_limit.max(options.turns as i32 + 1);
    setup.start_gold = options.start_gold;
    setup.team_types = [TeamKind::None; crate::LOCAL_TEAM_COUNT];
    setup.team_types[0] = TeamKind::Player;

    for slot in 1..=usize::from(ai_teams) {
        setup.team_types[slot] = TeamKind::Computer;
    }

    profile::enable();
    profile::reset();

    app.start_local_game(&setup)
        .map_err(|error| format!("soak: старт партии: {error}"))?;

    let mem_start = process_memory();
    let started = Instant::now();
    let mut turns = Vec::new();
    let mut frames = 0u64;
    let mut round_started = Instant::now();
    let mut round_frames = 0u64;
    let mut finished_early = false;
    let mut turns_played = 0u32;

    loop {
        if turns_played >= options.turns {
            break;
        }

        if app.local().is_some_and(|local| local.finished()) {
            finished_early = true;
            break;
        }

        let phase = app.local().map(|local| local.phase);

        match phase {
            Some(LocalPhase::PlayerTurn) => {
                // Окно перехода хода в одиночной игре не открывается, но
                // закрываем его на всякий случай — иначе цикл встанет.
                if app
                    .local()
                    .is_some_and(|local| local.notice.is_some())
                {
                    app.local_acknowledge_turn();
                }

                let before = app.state().turn_counter;
                round_started = Instant::now();
                round_frames = 0;

                for _ in 0..options.human_ticks {
                    app.local_update(options.dt);
                    frames += 1;
                    round_frames += 1;
                }

                app.local_end_turn();

                if app.local().is_some_and(|local| local.finished())
                    || app.state().turn_counter != before
                {
                    // End Turn мгновенно закрыл раунд (одна команда в раунде).
                    turns_played += 1;
                    push_turn(
                        &mut turns,
                        app,
                        before,
                        round_started.elapsed(),
                        round_frames,
                        options,
                        turns_played,
                    );
                }
            }
            Some(_) => {
                app.local_update(options.dt);
                frames += 1;
                round_frames += 1;

                let finished = app.local().is_some_and(|local| local.finished());
                let ready = !finished
                    && matches!(
                        app.local().map(|local| local.phase),
                        Some(LocalPhase::PlayerTurn)
                    );

                if finished || ready {
                    let closed = if finished {
                        app.state().turn_counter
                    } else {
                        app.state().turn_counter.saturating_sub(1)
                    };
                    turns_played += 1;
                    push_turn(
                        &mut turns,
                        app,
                        closed,
                        round_started.elapsed(),
                        round_frames,
                        options,
                        turns_played,
                    );
                    finished_early = finished && turns_played < options.turns;

                    if finished {
                        break;
                    }
                }
            }
            None => {
                break;
            }
        }

        if frames > u64::from(options.frame_limit_per_turn) * u64::from(options.turns) {
            return Err(format!(
                "soak: предохранитель кадров сработал (frames={frames}, limit={} на раунд)",
                options.frame_limit_per_turn
            ));
        }
    }

    let total = started.elapsed();
    let mem_end = process_memory();
    let profile = profile::snapshot();
    let final_turn = app.state().turn_counter;
    let final_units = app.state().unit_count();

    Ok(SoakReport {
        options: *options,
        turns_played,
        frames,
        total,
        turns,
        mem_start,
        mem_end,
        profile,
        finished_early,
        final_turn,
        final_units,
    })
}

fn push_turn(
    turns: &mut Vec<SoakTurn>,
    app: &App,
    turn: i32,
    wall: Duration,
    frames: u64,
    options: &SoakOptions,
    played: u32,
) {
    let rss_kb = if options.sample_every > 0 && played % options.sample_every == 0 {
        process_memory().map(|memory| memory.rss_kb)
    } else {
        None
    };

    turns.push(SoakTurn {
        turn,
        wall,
        frames,
        units: app.state().unit_count(),
        rss_kb,
    });
}

/// Доля времени кадра от бюджета 60 Гц (`avg/local_update`), проценты.
pub fn budget_used_percent(report: &SoakReport) -> f64 {
    let avg = report.profile.local_update.avg_ns();

    avg as f64 / profile::FRAME_BUDGET_NS as f64 * 100.0
}
