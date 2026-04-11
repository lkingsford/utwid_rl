use chrono::Local;
use clap::Parser;
use crossterm::{
    cursor::MoveTo,
    event::{self, KeyCode, KeyModifiers},
    queue,
    style::Color,
    style::Print,
    style::SetForegroundColor,
    terminal::{self, Clear, ClearType},
};
use env_logger::fmt::Formatter;
use log::Record;
use std::thread;
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{BufWriter, Stdout, Write, stdout},
    sync::Arc,
    time::Duration,
};

use mon2y::games::utwid::{ActorTraits, GameState, UtwidAction, UtwidState};
use mon2y::mcts::game_trait::Action;
use mon2y::mcts::tree::Tree;
use mon2y::mcts::{BestTurnPolicy, calculate_best_turn};

const DRAW_BOARD_X: u16 = 3;
const DRAW_BOARD_Y: u16 = 3;

fn draw_board(stdout: &mut Stdout, state: UtwidState) -> std::io::Result<()> {
    for iy in 0..state.board.height {
        queue!(stdout, MoveTo(DRAW_BOARD_X, DRAW_BOARD_Y + iy as u16))?;
        for ix in 0..state.board.width {
            let actor_repr = state
                .actors
                .iter()
                .filter_map(|actor| actor.as_ref())
                .find(|actor| actor.x == ix && actor.y == iy)
                .and_then(|actor| actor.console_repr());

            queue!(
                stdout,
                Print(if let Some(actor_repr) = actor_repr {
                    actor_repr
                } else if let Some(tile_repr) =
                    state.board.geography[(ix + iy * state.board.width) as usize].console_repr()
                {
                    tile_repr
                } else {
                    ' '
                })
            )?;
        }
    }
    Ok(())
}

fn draw_monsters(stdout: &mut Stdout, state: &UtwidState) -> std::io::Result<()> {
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
                    actor.console_repr().unwrap_or(' '),
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
    let completed_length = ((MCTS_STATUS_LINE_X2 - MCTS_STATUS_LINE_X1) as f32
        * (completed_iterations as f32)
        / (mcts_iterations as f32)) as u16;
    for _ in 0..completed_length {
        queue!(stdout, Print("-"))?;
    }

    let to_go_length = (MCTS_STATUS_LINE_X2 - MCTS_STATUS_LINE_X1) - completed_length;
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

const DRAW_MONSTER_X: u16 = 20;
const DRAW_MONSTER_Y: u16 = 2;

const STATUS_LINE_Y: u16 = 13;
const MCTS_STATUS_LINE_X1: u16 = 2;
const MCTS_STATUS_LINE_X2: u16 = 12;
const MCTS_STATUS_LINE_Y: u16 = 17;
const EXPLORATION_STATUS_LINE_Y: u16 = 18;

const HUMAN_ITERATIONS: usize = 3000;
const ITERATIONS_STEPS: usize = 10;
const THREADS: usize = 6;
const EXPLORATION_CONSTANT: f64 = 1.4142135623730951; // sqrt(2.0)
const SHORT_CIRCUIT_AT_TURNS: usize = 200;
const SHORT_CIRCUIT_INCREMENT: usize = 100;

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
    #[arg(long, default_value_t = false)]
    plain_mode: bool,
    #[arg(short = 'x', long, default_value_t = false)]
    exploration: bool,
    #[arg(short, long, default_value_t = 1.0)]
    difficulty_mod: f32,
    #[arg(short, long, default_value_t = HUMAN_ITERATIONS)]
    iterations: usize,
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = [250usize, 500, 1000, 2000]
    )]
    iteration_set: Vec<usize>,
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = [0.1f32, 0.2, 0.5, 1.0, 2.0]
    )]
    difficulty_mod_set: Vec<f32>,
}

#[derive(Clone, Copy, Debug)]
struct GameRunConfig {
    plain_mode: bool,
    human: bool,
    difficulty_mod: f32,
    iterations: usize,
}

#[derive(Default, Clone, Copy, Debug)]
struct ExplorationResult {
    wins: usize,
    total_games: usize,
}

fn format_exploration_key(difficulty_mod: f32, iterations: usize) -> String {
    format!("{difficulty_mod}x{iterations}i")
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
    win: bool,
) -> std::io::Result<()> {
    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("exploration_log.log")?;
    writeln!(
        log_file,
        "{}, iterations={}, difficulty_mod={}, win={}",
        Local::now().to_rfc3339(),
        iterations,
        difficulty_mod,
        win
    )?;
    Ok(())
}

fn run_game(
    stdout: &mut Stdout,
    config: GameRunConfig,
    exploration_stats: Option<&BTreeMap<String, ExplorationResult>>,
) -> std::io::Result<Option<GameState>> {
    let mut state = UtwidState::new();
    state.short_circuit_at_turns = Some(SHORT_CIRCUIT_AT_TURNS);
    state.short_circuit_at_turns_increment = Some(SHORT_CIRCUIT_AT_TURNS);

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
        let next_act = if config.human && to_act.traits.contains(ActorTraits::HUMAN) {
            let mut this_attempt: Option<UtwidAction> = None;
            while this_attempt.is_none() {
                let read_event_result = event::read();
                //log::info!("Result {:?}", read_event_result);
                this_attempt = match read_event_result {
                    Ok(read_event) => match read_event {
                        event::Event::Key(key_event) => match key_event.code {
                            KeyCode::Char('h') | KeyCode::Left => Some(UtwidAction::W),
                            KeyCode::Char('l') | KeyCode::Right => Some(UtwidAction::E),
                            KeyCode::Char('j') | KeyCode::Down => Some(UtwidAction::S),
                            KeyCode::Char('k') | KeyCode::Up => Some(UtwidAction::N),
                            KeyCode::Char('y') => Some(UtwidAction::NW),
                            KeyCode::Char('u') => Some(UtwidAction::NE),
                            KeyCode::Char('b') => Some(UtwidAction::SW),
                            KeyCode::Char('n') => Some(UtwidAction::SE),
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
                let mut ai_marked_state = state.clone();
                ai_marked_state.short_circuit_at_turns = Some(SHORT_CIRCUIT_AT_TURNS);
                ai_marked_state.short_circuit_at_turns_increment = Some(short_circuit_increment);
                ai_marked_state.ai_turn_weight = 0.0;
                let (best_turn_from_calculate, tree_from_calculate) = calculate_best_turn(
                    mcts_iterations,
                    None,
                    THREADS,
                    ai_marked_state,
                    BestTurnPolicy::Ucb0,
                    EXPLORATION_CONSTANT,
                    false,
                    tree,
                );
                tree = tree_from_calculate;
                best_turn = Some(best_turn_from_calculate);
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
        state = next_act.execute(&state);
        if matches!(state.game_state, GameState::Checkpoint) {
            state.game_state = GameState::Ongoing;
        }
        if matches!(state.game_state, GameState::Mon2yShortcircuit) {
            state.game_state = GameState::Ongoing;
        };
        state.ai_turn_weight = 0.0;
    }

    Ok(Some(state.game_state))
}

fn run_exploration(stdout: &mut Stdout, args: &Args) -> std::io::Result<()> {
    let mut stats: BTreeMap<String, ExplorationResult> = BTreeMap::new();
    for difficulty_mod in &args.difficulty_mod_set {
        for iterations in &args.iteration_set {
            stats.insert(
                format_exploration_key(*difficulty_mod, *iterations),
                ExplorationResult::default(),
            );
        }
    }

    if !args.plain_mode {
        draw_exploration_status(stdout, &stats)?;
        stdout.flush()?;
    }

    loop {
        for difficulty_mod in &args.difficulty_mod_set {
            for iterations in &args.iteration_set {
                let label = format_exploration_key(*difficulty_mod, *iterations);
                let game_result = run_game(
                    stdout,
                    GameRunConfig {
                        plain_mode: args.plain_mode,
                        human: false,
                        difficulty_mod: *difficulty_mod,
                        iterations: *iterations,
                    },
                    Some(&stats),
                )?;

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
                append_exploration_log(*iterations, *difficulty_mod, win)?;

                if !args.plain_mode {
                    draw_exploration_status(stdout, &stats)?;
                    stdout.flush()?;
                }
            }
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
            difficulty_mod: args.difficulty_mod,
            iterations: args.iterations,
        },
        None,
    )? {
        Some(GameState::Won) => print!("Won!"),
        Some(GameState::Lost) => print!("Lost!"),
        Some(_) => panic!("Invalid end game state"),
        None => {}
    }

    Ok(())
}
