use rand::seq::SliceRandom;
use rand::thread_rng;
use chrono::Local;
use clap::Parser;
use crossterm::{
    ExecutableCommand,
    cursor::MoveTo,
    event::{self, KeyCode, KeyModifiers},
    queue,
    style::{Color, Print, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, size},
};
use env_logger::fmt::Formatter;
use log::Record;
use serde::{Deserialize, Serialize};
use std::thread;
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{Stdout, Write, stdout},
    sync::Arc,
    time::Duration,
};

use mon2y::mcts::tree::Tree;
use mon2y::mcts::{BestTurnPolicy, calculate_best_turn};
use mon2y::{games::utwid::ReprSet, mcts::game_trait::Action};
use mon2y::{
    games::utwid::{
        ActorTraits, Allegiance, Dir, GameActor, GameState, Repr, RewardConfig, UtwidAction,
        UtwidEvent, UtwidState,
    },
    mcts::mcts::run_mcts_iterations,
};

fn repr_to_char(repr: Repr) -> char {
    match repr {
        Repr::Floor => '.',
        Repr::Wall => '#',
        Repr::Stairs => '>',
        Repr::Win => 'W',
        Repr::You => '@',
        Repr::Monte => '&',
        Repr::Them => 't',
        Repr::Are => 'r',
        Repr::One => '1',
    }
}

fn repr_set_to_color(repr_set: ReprSet) -> Color {
    match repr_set {
        ReprSet::Room1 => Color::Black,
        ReprSet::Room2 => Color::DarkBlue,
        ReprSet::Room3 => Color::DarkGrey,
        ReprSet::Room4 => Color::DarkRed,
        ReprSet::Room5 => Color::DarkGreen,
        ReprSet::Room6 => Color::DarkYellow,
        ReprSet::Room7 => Color::DarkMagenta,
        _ => Color::Black,
    }
}

fn draw_board(stdout: &mut Stdout, state: UtwidState) -> std::io::Result<()> {
    for iy in 0..state.board.height {
        queue!(
            stdout,
            MoveTo(DRAW_BOARD_X, DRAW_BOARD_Y + iy as u16),
            SetForegroundColor(Color::White)
        )?;
        for ix in 0..state.board.width {
            let actor_with_id = state
                .actors
                .iter()
                .enumerate()
                .filter_map(|(id, actor_opt)| actor_opt.as_ref().map(|actor| (id, actor)))
                .find(|(_, actor)| actor.x == ix && actor.y == iy);

            let actor_repr = actor_with_id
                .and_then(|(_, actor)| actor.repr())
                .map(repr_to_char);

            queue!(
                stdout,
                SetBackgroundColor(if let Some((actor_id, actor_ref)) = actor_with_id {
                    if state.to_act == actor_id && actor_ref.traits.contains(ActorTraits::HUMAN) {
                        Color::Grey
                    } else {
                        match actor_ref.effective_allegiance() {
                            Allegiance::You => Color::DarkCyan,
                            Allegiance::Monty => Color::DarkRed,
                        }
                    }
                } else {
                    repr_set_to_color(
                        state.board.geography[(ix + iy * state.board.width) as usize].repr_set,
                    )
                }),
                Print(if let Some(actor_repr) = actor_repr {
                    actor_repr
                } else if let Some(tile_repr) = state.board.geography
                    [(ix + iy * state.board.width) as usize]
                    .repr()
                    .map(repr_to_char)
                {
                    tile_repr
                } else {
                    ' '
                })
            )?;
        }
    }
    queue!(stdout, SetBackgroundColor(Color::Black));

    Ok(())
}

fn draw_monsters(stdout: &mut Stdout, state: &UtwidState) -> std::io::Result<()> {
    // Clear the monster listing
    let width = size().unwrap_or((80, 0)).0;
    let empty_row_segment = " ".repeat((width - DRAW_MONSTER_X) as usize);
    for row in DRAW_MONSTER_X..ACTOR_HP_ROWS + DRAW_MONSTER_Y {
        queue!(
            stdout,
            MoveTo(DRAW_MONSTER_X, row),
            Print(empty_row_segment.clone()),
        )?;
    }

    for (i, actor_id) in state.turn_order.iter().enumerate() {
        if let Some(actor) = state.actors.get(*actor_id) {
            let actor = match actor.as_ref() {
                Some(actor) => actor,
                None => continue,
            };
            queue!(
                stdout,
                MoveTo(DRAW_MONSTER_X, DRAW_MONSTER_Y + i as u16),
                Print(format!(
                    "{} ({}, {}) - {}  ",
                    actor.repr().map(repr_to_char).unwrap_or(' '),
                    actor.x,
                    actor.y,
                    actor.health.unwrap_or(0)
                ))
            )?;
        }
    }
    Ok(())
}

fn draw_status(stdout: &mut Stdout, _state: &UtwidState) -> std::io::Result<()> {
    queue!(stdout, MoveTo(0, STATUS_LINE_Y),)?;
    Ok(())
}

fn draw_status_mcts(
    stdout: &mut Stdout,
    completed_iterations: usize,
    mcts_iterations: usize,
) -> std::io::Result<()> {
    queue!(
        stdout,
        MoveTo(MCTS_STATUS_LINE_X1, MCTS_STATUS_LINE_Y),
        Print("|"),
        SetForegroundColor(Color::Blue),
    )?;
    let width = (MCTS_STATUS_LINE_X2 - MCTS_STATUS_LINE_X1);
    let completed_length =
        (width as f32 * (completed_iterations as f32) / (mcts_iterations as f32)) as u16;
    for _ in 0..completed_length {
        queue!(stdout, Print("-"))?;
    }

    let to_go_length = if width > completed_length {
        (width - completed_length)
    } else {
        0
    };
    for _ in 0..to_go_length {
        queue!(stdout, Print(" "))?;
    }

    queue!(
        stdout,
        MoveTo(MCTS_STATUS_LINE_X2, MCTS_STATUS_LINE_Y),
        SetForegroundColor(Color::Grey),
        Print("|")
    )?;
    Ok(())
}

fn draw_log(stdout: &mut Stdout, action_log: &[LogEntry]) -> std::io::Result<()> {
    let width = size().unwrap_or((80, 0)).0 as usize;
    let empty_row_segment = " ".repeat(width);
    for row in DRAW_LOG_Y..DRAW_LOG_Y + LOG_ROWS {
        queue!(stdout, MoveTo(0, row), Print(empty_row_segment.clone()),)?;
    }
    let start = action_log.len().saturating_sub(LOG_ROWS as usize);
    for (i, entry) in action_log[start..].iter().enumerate() {
        queue!(
            stdout,
            MoveTo(0, DRAW_LOG_Y + i as u16),
            Print(match entry {
                LogEntry::Action(entry) => format!(
                    "{} ({},{}) {:?}",
                    entry.actor.repr().map(repr_to_char).unwrap_or('?'),
                    entry.actor.x,
                    entry.actor.y,
                    entry.action,
                ),
                LogEntry::Event(event) => format!("{:?}", event),
            }),
        )?;
    }
    Ok(())
}

fn poll_for_exit() -> std::io::Result<bool> {
    while event::poll(Duration::from_millis(0))? {
        if let event::Event::Key(key_event) = event::read()?
            && key_event.modifiers.intersects(KeyModifiers::CONTROL)
            && matches!(key_event.code, KeyCode::Char('c'))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn direction_from_key(key_code: KeyCode, diagonal: bool) -> Option<Dir> {
    match key_code {
        KeyCode::Char('h') | KeyCode::Left => Some(Dir::W),
        KeyCode::Char('l') | KeyCode::Right => Some(Dir::E),
        KeyCode::Char('j') | KeyCode::Down => Some(Dir::S),
        KeyCode::Char('k') | KeyCode::Up => Some(Dir::N),
        KeyCode::Char('y') if diagonal => Some(Dir::NW),
        KeyCode::Char('u') if diagonal => Some(Dir::NE),
        KeyCode::Char('b') if diagonal => Some(Dir::SW),
        KeyCode::Char('n') if diagonal => Some(Dir::SE),
        _ => None,
    }
}

fn prompt_direction(
    stdout: &mut Stdout,
    prompt: &str,
    diagonal: bool,
) -> std::io::Result<Option<Dir>> {
    let _ = queue!(
        stdout,
        MoveTo(0, STATUS_LINE_Y),
        Clear(ClearType::CurrentLine),
        Print(prompt)
    );
    let _ = stdout.flush();

    let result = match event::read() {
        Ok(event::Event::Key(dir_key_event)) => {
            if dir_key_event.modifiers.intersects(KeyModifiers::CONTROL)
                && matches!(dir_key_event.code, KeyCode::Char('c'))
            {
                let _ = queue!(
                    stdout,
                    MoveTo(0, STATUS_LINE_Y),
                    Clear(ClearType::CurrentLine)
                );
                let _ = stdout.flush();
                return Ok(None);
            }

            direction_from_key(dir_key_event.code, diagonal)
        }
        _ => None,
    };

    let _ = queue!(
        stdout,
        MoveTo(0, STATUS_LINE_Y),
        Clear(ClearType::CurrentLine)
    );
    let _ = stdout.flush();
    Ok(result)
}

const DRAW_MONSTER_X: u16 = 20;
const DRAW_MONSTER_Y: u16 = 2;

const STATUS_LINE_Y: u16 = 13;
const MCTS_STATUS_LINE_X1: u16 = 2;
const MCTS_STATUS_LINE_X2: u16 = 12;
const MCTS_STATUS_LINE_Y: u16 = 17;
const EXPLORATION_STATUS_LINE_Y: u16 = 18;

const HUMAN_ITERATIONS: usize = 3000;
const ITERATIONS_STEPS: usize = 10;
const THREADS: usize = 8;
const EXPLORATION_CONSTANT: f64 = 0.3;
const SHORT_CIRCUIT_AT_TURNS: usize = 100;
const SHORT_CIRCUIT_INCREMENT: usize = 100;

const DRAW_BOARD_X: u16 = 3;
const DRAW_BOARD_Y: u16 = 3;
const ACTOR_HP_ROWS: u16 = 10;

const DRAW_LOG_Y: u16 = 15;
const LOG_ROWS: u16 = 10;

struct RawModeGuard;

impl RawModeGuard {
    fn new() -> std::io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg()]
    config_file: Vec<String>,
    #[command(flatten)]
    verbose: clap_verbosity_flag::Verbosity,
    #[arg(short = 'u', long, default_value_t = false)]
    human: bool,
    #[arg(short = 'U', long, default_value_t = false)]
    very_human: bool,
    #[arg(long, default_value_t = false)]
    plain_mode: bool,
    #[arg(long, default_value_t = false)]
    exploration: bool,
    #[arg(long, default_value_t = String::from("explore.json"))]
    explore_out: String,
    #[arg(short, long, default_value_t = 1.0)]
    difficulty_mod: f32,
    #[arg(short, long, default_value_t = THREADS)]
    threads: usize,
    #[arg(short, long, default_value_t = HUMAN_ITERATIONS)]
    iterations: usize,
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = [250usize, 500, 1000, 2000, 10000, 20000, 50000]
    )]
    iteration_set: Vec<usize>,
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = [0.1f32, 0.2, 0.5, 1.0, 2.0]
    )]
    difficulty_mod_set: Vec<f32>,
    #[arg(long, default_value_t = false)]
    random: bool,
    #[arg(short = 'c', long, default_value_t = 0.3)]
    exploration_constant: f64,
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = [0.05, 0.1, 0.3, 0.6, 1.0, 1.4142135623730951, 2.0]
    )]
    exploration_constant_set: Vec<f64>,
    #[arg(long)]
    deep_copy_depth: Option<usize>,
    #[arg(long, default_value_t = 0.2)]
    reward_turn_weight: f64,
    #[arg(long, default_value_t = 0.75)]
    reward_level_base: f64,
    #[arg(long, default_value_t = 2.5)]
    reward_health_weight: f64,
    #[arg(long, default_value_t = -0.3)]
    reward_health_bias: f64,
    #[arg(long, default_value_t = 20.0)]
    reward_win: f64,
    #[arg(long, default_value_t = -20.0)]
    reward_lose: f64,
    #[arg(long, default_value_t = -5.0)]
    reward_stalemate: f64,
    #[arg(long, default_value_t = 20.0)]
    reward_level: f64,
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = [0.2f64]
    )]
    reward_turn_weight_set: Vec<f64>,
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = [0.75f64]
    )]
    reward_level_base_set: Vec<f64>,
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = [2.5f64]
    )]
    reward_health_weight_set: Vec<f64>,
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = [-0.3f64]
    )]
    reward_health_bias_set: Vec<f64>,
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = [20.0f64]
    )]
    reward_win_set: Vec<f64>,
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = [-20.0f64]
    )]
    reward_lose_set: Vec<f64>,
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = [-5.0f64]
    )]
    reward_stalemate_set: Vec<f64>,
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = [20.0f64]
    )]
    reward_level_set: Vec<f64>,
}

#[derive(Clone, Copy, Debug)]
struct GameRunConfig {
    plain_mode: bool,
    human: bool,
    very_human: bool,
    difficulty_mod: f32,
    iterations: usize,
    threads: usize,
    exploration_constant: f64,
    deep_copy_depth: Option<usize>,
    reward_config: RewardConfig,
}

#[derive(Default, Clone, Copy, Debug)]
struct ExplorationResult {
    wins: usize,
    total_games: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExploreEntry {
    difficulty_mod: f32,
    iterations: usize,
    exploration_constant: f64,
    turn_weight: f64,
    level_base: f64,
    health_weight: f64,
    health_bias: f64,
    win_reward: f64,
    lose_reward: f64,
    stalemate_reward: f64,
    level_reward: f64,
    wins: usize,
    losses: usize,
    percentage: f64,
    last_run_time: String,
}

fn write_explore_json(
    path: &str,
    configs: &[GameRunConfig],
    stats: &BTreeMap<String, ExplorationResult>,
) -> std::io::Result<()> {
    let mut existing: Vec<ExploreEntry> = std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .ok()
        .and_then(|f| serde_json::from_reader(f).ok())
        .unwrap_or_default();

    let existing_map: BTreeMap<String, usize> = existing
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let key = format_exploration_key(
                e.difficulty_mod,
                e.iterations,
                e.exploration_constant,
                &RewardConfig {
                    turn_weight: e.turn_weight,
                    level_base: e.level_base,
                    health_weight: e.health_weight,
                    health_bias: e.health_bias,
                    win_reward: e.win_reward,
                    lose_reward: e.lose_reward,
                    stalemate_reward: e.stalemate_reward,
                    level_reward: e.level_reward,
                },
            );
            (key, i)
        })
        .collect();

    let now = Local::now().to_rfc3339();
    for config in configs {
        let key = format_exploration_key(
            config.difficulty_mod,
            config.iterations,
            config.exploration_constant,
            &config.reward_config,
        );
        let stat = stats.get(&key).copied().unwrap_or_default();
        let wins = stat.wins;
        let total = stat.total_games;
        let losses = total.saturating_sub(wins);
        let percentage = if total == 0 {
            0.0
        } else {
            wins as f64 * 100.0 / total as f64
        };

        if let Some(&idx) = existing_map.get(&key) {
            let entry = &mut existing[idx];
            entry.wins = wins;
            entry.losses = losses;
            entry.percentage = percentage;
            entry.last_run_time = now.clone();
        } else {
            existing.push(ExploreEntry {
                difficulty_mod: config.difficulty_mod,
                iterations: config.iterations,
                exploration_constant: config.exploration_constant,
                turn_weight: config.reward_config.turn_weight,
                level_base: config.reward_config.level_base,
                health_weight: config.reward_config.health_weight,
                health_bias: config.reward_config.health_bias,
                win_reward: config.reward_config.win_reward,
                lose_reward: config.reward_config.lose_reward,
                stalemate_reward: config.reward_config.stalemate_reward,
                level_reward: config.reward_config.level_reward,
                wins,
                losses,
                percentage,
                last_run_time: now.clone(),
            });
        }
    }

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    serde_json::to_writer_pretty(file, &existing)?;
    Ok(())
}

fn format_reward_config_key(config: &RewardConfig) -> String {
    format!(
        "tw{}_lb{}_hw{}_hb{}_wr{}_lr{}_sr{}_lvr{}",
        config.turn_weight,
        config.level_base,
        config.health_weight,
        config.health_bias,
        config.win_reward,
        config.lose_reward,
        config.stalemate_reward,
        config.level_reward,
    )
}

fn format_exploration_key(difficulty_mod: f32, iterations: usize, exploration_constant: f64, reward_config: &RewardConfig) -> String {
    format!("{difficulty_mod}x{iterations}i_c{exploration_constant}_{}", format_reward_config_key(reward_config))
}

fn draw_exploration_status(
    stdout: &mut Stdout,
    stats: &BTreeMap<String, ExplorationResult>,
) -> std::io::Result<()> {
    for (i, (label, result)) in stats.iter().enumerate() {
        let win_percentage = if result.total_games == 0 {
            0.0
        } else {
            (result.wins as f32 * 100.0) / (result.total_games as f32)
        };
        queue!(
            stdout,
            MoveTo(0, EXPLORATION_STATUS_LINE_Y + i as u16),
            Clear(ClearType::CurrentLine),
            Print(format!(
                "{label} - {}/{} ({win_percentage:.2} %)",
                result.wins, result.total_games
            ))
        )?;
    }
    Ok(())
}

fn append_exploration_log(
    iterations: usize,
    difficulty_mod: f32,
    exploration_constant: f64,
    reward_config: &RewardConfig,
    win: bool,
) -> std::io::Result<()> {
    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("exploration_log.log")?;
    writeln!(
        log_file,
        "{}, iterations={}, difficulty_mod={}, exploration_constant={}, reward_config={}, win={}",
        Local::now().to_rfc3339(),
        iterations,
        difficulty_mod,
        exploration_constant,
        format_reward_config_key(reward_config),
        win
    )?;
    Ok(())
}

fn sample_actions(stdout: &mut Stdout, state: &UtwidState, iterations: usize) {
    let tree = std::sync::Arc::new(mon2y::mcts::tree::Tree::new(
        mon2y::mcts::node::create_expanded_node(state.clone(), None, None),
    ));
    run_mcts_iterations(tree.clone(), iterations, None, 8, None);
    let root_ref = tree.root.clone();
    let root = root_ref.read().unwrap();
    if let mon2y::mcts::node::Node::Expanded { children, .. } = &*root {
        queue!(stdout, MoveTo(0, MCTS_STATUS_LINE_Y));
        for action_value in {
            (children.iter().map(|(action, node)| {
                let node = node.read().unwrap();
                (format!(
                    "{:?} - V:{} E:{:?}\n",
                    action.clone(),
                    node.visit_count(),
                    node.value_sums_ref().to_vec().iter().map(|value_sum| {
                        if value_sum.visit_count == 0 {
                            0.0
                        } else {
                            value_sum.value_sum / (value_sum.visit_count as f64)
                        }
                    }),
                ))
            }))
        }
        .enumerate()
        {
            queue!(
                stdout,
                MoveTo(0, MCTS_STATUS_LINE_Y + action_value.0 as u16),
                Print(action_value.1)
            );
        }
    }
    stdout.flush();
}

struct ActionEntry {
    action: UtwidAction,
    actor: GameActor,
}

enum LogEntry {
    Action(ActionEntry),
    Event(UtwidEvent),
}

fn run_game(
    stdout: &mut Stdout,
    config: GameRunConfig,
    exploration_stats: Option<&BTreeMap<String, ExplorationResult>>,
) -> std::io::Result<Option<GameState>> {
    let mut state = UtwidState::new();
    state.reward_config = config.reward_config;
    state.short_circuit_at_turns = Some(SHORT_CIRCUIT_AT_TURNS);
    state.short_circuit_at_turns_increment = Some(SHORT_CIRCUIT_AT_TURNS);

    let mut action_log: Vec<LogEntry> = vec![];

    queue!(stdout, Clear(ClearType::All))?;
    if !config.plain_mode {
        if let Some(stats) = exploration_stats {
            draw_exploration_status(stdout, stats)?;
        }
        stdout.flush()?;
    }
    while matches!(state.game_state, GameState::Ongoing | GameState::Checkpoint) {
        if !config.plain_mode {
            draw_board(stdout, state.clone())?;
            draw_monsters(stdout, &state)?;
            draw_status(stdout, &state)?;
            draw_log(stdout, &action_log)?;
            if let Some(stats) = exploration_stats {
                draw_exploration_status(stdout, stats)?;
            }
            stdout.flush()?;
            if poll_for_exit()? {
                return Ok(None);
            }
        }
        let to_act = state
            .actors
            .get(state.to_act)
            .and_then(|actor| actor.as_ref())
            .unwrap();
        let next_act = if config.human && to_act.traits.contains(ActorTraits::HUMAN)
            || config.very_human
        {
            stdout.execute(MoveTo(
                (to_act.x) as u16 + DRAW_BOARD_X,
                (to_act.y) as u16 + DRAW_BOARD_Y,
            ));
            let mut this_attempt: Option<UtwidAction> = None;
            while this_attempt.is_none() {
                let read_event_result = event::read();
                //log::info!("Result {:?}", read_event_result);
                this_attempt = match read_event_result {
                    Ok(read_event) => match read_event {
                        event::Event::Key(key_event) => match key_event.code {
                            KeyCode::Char('h') | KeyCode::Left => Some(UtwidAction::Move(Dir::W)),
                            KeyCode::Char('l') | KeyCode::Right => Some(UtwidAction::Move(Dir::E)),
                            KeyCode::Char('j') | KeyCode::Down => Some(UtwidAction::Move(Dir::S)),
                            KeyCode::Char('k') | KeyCode::Up => Some(UtwidAction::Move(Dir::N)),
                            KeyCode::Char('y') => Some(UtwidAction::Move(Dir::NW)),
                            KeyCode::Char('u') => Some(UtwidAction::Move(Dir::NE)),
                            KeyCode::Char('b') => Some(UtwidAction::Move(Dir::SW)),
                            KeyCode::Char('n') => Some(UtwidAction::Move(Dir::SE)),
                            KeyCode::Char('x') => Some(UtwidAction::Explode),
                            KeyCode::Char('?') => {
                                // This... is an abuse of side effects. I don't think I like this :s
                                sample_actions(stdout, &state, 10000);
                                None
                            }
                            KeyCode::Char('1') => {
                                prompt_direction(stdout, "Conclusion: choose a direction", true)
                                    .map(|direction| direction.map(UtwidAction::Conclusion))?
                            } // Jump to a position
                            KeyCode::Char('2') => Some(UtwidAction::Redemption), // Jump through a line of actors, injuring all
                            KeyCode::Char('3') => Some(UtwidAction::Attention), // Pull a whole direction closer
                            KeyCode::Char('4') => {
                                prompt_direction(stdout, "Stagnation: choose a direction", false)
                                    .map(|direction| direction.map(UtwidAction::Stagnation))?
                            } // Create a wall
                            KeyCode::Char('5') => Some(UtwidAction::Prescription), // Take multiple moves in a row
                            KeyCode::Char('6') => {
                                prompt_direction(stdout, "Contemplation: choose a direction", true)
                                    .map(|direction| direction.map(UtwidAction::Contemplation))?
                            }
                            KeyCode::Char('7') => {
                                prompt_direction(stdout, "Multiplication: choose a direction", true)
                                    .map(|direction| direction.map(UtwidAction::Multiplication))?
                            }
                            KeyCode::Char('8') => {
                                prompt_direction(stdout, "Contention: choose a direction", true)
                                    .map(|direction| direction.map(UtwidAction::Contention))?
                            } // The glitch one
                            KeyCode::Char('9') => {
                                prompt_direction(stdout, "Assumption: choose a direction", true)
                                    .map(|direction| direction.map(UtwidAction::Assumption))?
                            } // Take over a perso
                            KeyCode::Char('c') => {
                                if key_event.modifiers.intersects(KeyModifiers::CONTROL) {
                                    return Ok(None);
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        },
                        _ => None,
                    },
                    _ => None,
                };
            }
            this_attempt.unwrap()
        } else {
            let mut completed_iterations = 0;
            let iterations_step = std::cmp::max(1, config.iterations / ITERATIONS_STEPS);
            let mut tree: Option<Arc<Tree<UtwidState, UtwidAction>>> = None;
            let mut best_turn: Option<UtwidAction> = None;
            let (mcts_iterations, short_circuit_increment) = {
                if let Some(mon2y) = to_act.mon2y.as_ref() {
                    Some((
                        usize::max(
                            1,
                            ((mon2y.iterations as f32) * config.difficulty_mod) as usize,
                        ),
                        0,
                    ))
                } else if to_act.traits.contains(ActorTraits::HUMAN) {
                    Some((config.iterations, SHORT_CIRCUIT_INCREMENT))
                } else {
                    None
                }
            }
            .unwrap();
            while completed_iterations < mcts_iterations {
                log::debug!("Completed {} iterations", completed_iterations);
                let mut ai_marked_state = state.clone();
                ai_marked_state.short_circuit_at_turns = Some(SHORT_CIRCUIT_AT_TURNS);
                ai_marked_state.short_circuit_at_turns_increment = Some(short_circuit_increment);
                ai_marked_state.ai_turns = 0;
                let (best_turn_from_calculate, tree_from_calculate) = calculate_best_turn(
                    mcts_iterations,
                    None,
                    config.threads,
                    ai_marked_state,
                    BestTurnPolicy::Ucb0,
                    config.exploration_constant,
                    false,
                    tree,
                    config.deep_copy_depth,
                );
                tree = tree_from_calculate;
                best_turn = Some(best_turn_from_calculate);
                log::debug!(
                    "calculate_best_turn iteration {} result: {:?}",
                    completed_iterations,
                    best_turn_from_calculate,
                );
                completed_iterations += iterations_step;
                if !config.plain_mode {
                    draw_status_mcts(stdout, completed_iterations, mcts_iterations)?;
                    if let Some(stats) = exploration_stats {
                        draw_exploration_status(stdout, stats)?;
                    }
                    stdout.flush()?;
                    if poll_for_exit()? {
                        return Ok(None);
                    }
                }
            }
            best_turn.unwrap()
        };
        log::debug!("Running action: {:?}", next_act);
        let acting_actor = state
            .actors
            .get(state.to_act)
            .and_then(|actor| actor.as_ref())
            .unwrap()
            .clone();
        let (new_state, events) = next_act.execute(&state);
        state = new_state;
        action_log.push(LogEntry::Action(ActionEntry {
            action: next_act.clone(),
            actor: acting_actor,
        }));
        for event in events {
            action_log.push(LogEntry::Event(event));
        }
        if matches!(state.game_state, GameState::Checkpoint) {
            state.game_state = GameState::Ongoing;
        }
        if matches!(state.game_state, GameState::Mon2yShortcircuit) {
            state.game_state = GameState::Ongoing;
        };
        state.ai_turns = 0;
    }

    // Last time game end redraw
    if !config.plain_mode {
        draw_board(stdout, state.clone())?;
        draw_monsters(stdout, &state)?;
        draw_status(stdout, &state)?;
        draw_log(stdout, &action_log)?;
        stdout.flush()?;
    }

    Ok(Some(state.game_state))
}

fn build_reward_configs(args: &Args) -> Vec<RewardConfig> {
    let mut configs = Vec::new();
    for turn_weight in &args.reward_turn_weight_set {
        for level_base in &args.reward_level_base_set {
            for health_weight in &args.reward_health_weight_set {
                for health_bias in &args.reward_health_bias_set {
                    for win_reward in &args.reward_win_set {
                        for lose_reward in &args.reward_lose_set {
                            for stalemate_reward in &args.reward_stalemate_set {
                                for level_reward in &args.reward_level_set {
                                    configs.push(RewardConfig {
                                        turn_weight: *turn_weight,
                                        level_base: *level_base,
                                        health_weight: *health_weight,
                                        health_bias: *health_bias,
                                        win_reward: *win_reward,
                                        lose_reward: *lose_reward,
                                        stalemate_reward: *stalemate_reward,
                                        level_reward: *level_reward,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    configs
}

fn run_exploration(stdout: &mut Stdout, args: &Args) -> std::io::Result<()> {
    let mut stats: BTreeMap<String, ExplorationResult> = BTreeMap::new();
    let mut configs: Vec<GameRunConfig> = Vec::new();
    let reward_configs = build_reward_configs(args);
    for difficulty_mod in &args.difficulty_mod_set {
        for iterations in &args.iteration_set {
            for exploration_constant in &args.exploration_constant_set {
                for reward_config in &reward_configs {
                    let config = GameRunConfig {
                        plain_mode: args.plain_mode,
                        human: false,
                        very_human: false,
                        difficulty_mod: *difficulty_mod,
                        iterations: *iterations,
                        threads: args.threads,
                        exploration_constant: *exploration_constant,
                        deep_copy_depth: args.deep_copy_depth,
                        reward_config: *reward_config,
                    };
                    stats.insert(
                        format_exploration_key(*difficulty_mod, *iterations, *exploration_constant, reward_config),
                        ExplorationResult::default(),
                    );
                    configs.push(config);
                }
            }
        }
    }

    if args.random {
        configs.shuffle(&mut thread_rng());
    }

    if !args.plain_mode {
        draw_exploration_status(stdout, &stats)?;
        stdout.flush()?;
    }

    loop {
        for config in &configs {
            let label = format_exploration_key(
                config.difficulty_mod,
                config.iterations,
                config.exploration_constant,
                &config.reward_config,
            );
            let game_result = run_game(stdout, *config, Some(&stats))?;

            let Some(game_state) = game_result else {
                return Ok(());
            };
            let win = matches!(game_state, GameState::Won);
            let entry = stats
                .get_mut(&label)
                .expect("exploration entry should exist");
            if win {
                entry.wins += 1;
            }
            entry.total_games += 1;
            append_exploration_log(config.iterations, config.difficulty_mod, config.exploration_constant, &config.reward_config, win)?;
            write_explore_json(&args.explore_out, &configs, &stats)?;

            if !args.plain_mode {
                draw_exploration_status(stdout, &stats)?;
                stdout.flush()?;
            }
        }

        if args.random {
            configs.shuffle(&mut thread_rng());
        }

        if !args.plain_mode && poll_for_exit()? {
            return Ok(());
        }
    }
}

fn validate_args(args: &Args) {
    for iterations in &args.iteration_set {
        assert!(
            *iterations > 0,
            "--iteration-set values must be positive integers"
        );
    }
    for difficulty_mod in &args.difficulty_mod_set {
        assert!(
            *difficulty_mod > 0.0 && difficulty_mod.is_finite(),
            "--difficulty-mod-set values must be positive finite floats"
        );
    }
    for exploration_constant in &args.exploration_constant_set {
        assert!(
            *exploration_constant > 0.0 && exploration_constant.is_finite(),
            "--exploration-constant-set values must be positive finite floats"
        );
    }
    for val in &args.reward_turn_weight_set {
        assert!(val.is_finite(), "--reward-turn-weight-set values must be finite");
    }
    for val in &args.reward_level_base_set {
        assert!(val.is_finite(), "--reward-level-base-set values must be finite");
    }
    for val in &args.reward_health_weight_set {
        assert!(val.is_finite(), "--reward-health-weight-set values must be finite");
    }
    for val in &args.reward_health_bias_set {
        assert!(val.is_finite(), "--reward-health-bias-set values must be finite");
    }
    for val in &args.reward_win_set {
        assert!(val.is_finite(), "--reward-win-set values must be finite");
    }
    for val in &args.reward_lose_set {
        assert!(val.is_finite(), "--reward-lose-set values must be finite");
    }
    for val in &args.reward_stalemate_set {
        assert!(val.is_finite(), "--reward-stalemate-set values must be finite");
    }
    for val in &args.reward_level_set {
        assert!(val.is_finite(), "--reward-level-set values must be finite");
    }
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    validate_args(&args);
    let _raw_mode = if args.plain_mode {
        None
    } else {
        Some(RawModeGuard::new()?)
    };
    env_logger::Builder::new()
        .format(|buf: &mut Formatter, record: &Record| {
            let thread_id = thread::current().id();
            let timestamp = buf.timestamp_millis();
            writeln!(
                buf,
                "[{}] [Thread: {:?}] [{}] - {}",
                timestamp,
                thread_id,
                record.level(),
                record.args()
            )
        })
        .filter_level(args.verbose.log_level_filter())
        .init();

    let mut stdout = stdout();

    if args.exploration {
        return run_exploration(&mut stdout, &args);
    }

    match run_game(
        &mut stdout,
        GameRunConfig {
            plain_mode: args.plain_mode,
            human: args.human,
            very_human: args.very_human,
            difficulty_mod: args.difficulty_mod,
            iterations: args.iterations,
            threads: args.threads,
            exploration_constant: args.exploration_constant,
            deep_copy_depth: args.deep_copy_depth,
            reward_config: RewardConfig {
                turn_weight: args.reward_turn_weight,
                level_base: args.reward_level_base,
                health_weight: args.reward_health_weight,
                health_bias: args.reward_health_bias,
                win_reward: args.reward_win,
                lose_reward: args.reward_lose,
                stalemate_reward: args.reward_stalemate,
                level_reward: args.reward_level,
            },
        },
        None,
    )? {
        Some(GameState::Won) => print!("Won!"),
        Some(GameState::Lost) => print!("Lost!"),
        Some(GameState::Stalemate) => print!("Stalemate!"),
        Some(_) => panic!("Invalid end game state"),
        None => {}
    }

    Ok(())
}
