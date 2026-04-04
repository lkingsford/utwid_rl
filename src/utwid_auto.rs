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
    io::{stdout, Stdout, Write},
    sync::Arc,
    time::Duration,
};

use mon2y::games::utwid::{ActorTraits, GameState, UtwidAction, UtwidState};
use mon2y::mcts::game_trait::Action;
use mon2y::mcts::tree::Tree;
use mon2y::mcts::{calculate_best_turn, BestTurnPolicy};

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
                    "{} ({}, {}) - {}",
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

fn draw_status(stdout: &mut Stdout, state: &UtwidState) -> std::io::Result<()> {
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
    for _ in (0..completed_length) {
        queue!(stdout, Print("-"))?;
    }
    let to_go_length = (MCTS_STATUS_LINE_X2 - MCTS_STATUS_LINE_X1) - completed_length;
    for _ in (0..to_go_length) {
        queue!(stdout, Print(" "))?;
    }
    queue!(stdout, SetForegroundColor(Color::Grey), Print("|"));
    Ok(())
}

fn check_for_killing_process() {
    while event::poll(Duration::from_millis(0)).unwrap_or(false) {
        let incoming_event = event::read();
        if let Ok(event::Event::Key(key_event)) = incoming_event {
            if key_event.modifiers.intersects(KeyModifiers::CONTROL) {
                unimplemented!("Lazy quit");
            }
        }
    }
}

const DRAW_MONSTER_X: u16 = 20;
const DRAW_MONSTER_Y: u16 = 2;

const STATUS_LINE_Y: u16 = 13;
const MCTS_STATUS_LINE_X1: u16 = 2;
const MCTS_STATUS_LINE_X2: u16 = 12;
const MCTS_STATUS_LINE_Y: u16 = 17;

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
    #[arg(short, long, default_value_t = 1.0)]
    difficulty_mod: f32,
    #[arg(short, long, default_value_t = HUMAN_ITERATIONS)]
    iterations: usize,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
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

    let mut state = UtwidState::new();
    state.short_circuit_at_turns = Some(SHORT_CIRCUIT_AT_TURNS);
    state.short_circuit_at_turns_increment = Some(SHORT_CIRCUIT_AT_TURNS);
    let mut stdout = stdout();

    queue!(stdout, Clear(ClearType::All))?;
    while matches!(state.game_state, GameState::Ongoing | GameState::Checkpoint) {
        if !args.plain_mode {
            draw_board(&mut stdout, state.clone())?;
            draw_monsters(&mut stdout, &state)?;
            draw_status(&mut stdout, &state)?;
            stdout.flush()?;
        }
        let to_act = state
            .actors
            .get(state.to_act)
            .and_then(|actor| actor.as_ref())
            .unwrap();
        let next_act = if args.human && to_act.traits.contains(ActorTraits::HUMAN) {
            let mut this_attempt: Option<UtwidAction> = None;
            while this_attempt.is_none() {
                let read_event_result = event::read();
                log::info!("Result {:?}", read_event_result);
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
                                    unimplemented!("Lazy Quit")
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
            let iterations_step = args.iterations / ITERATIONS_STEPS;
            let mut tree: Option<Arc<Tree<UtwidState, UtwidAction>>> = None;
            let mut best_turn: Option<UtwidAction> = None;
            let (mcts_iterations, short_circuit_increment) = {
                if let Some(mon2y) = to_act.mon2y.as_ref() {
                    Some((
                        (((mon2y.iterations) as f32) * args.difficulty_mod) as usize,
                        0,
                    ))
                } else if to_act.traits.contains(ActorTraits::HUMAN) {
                    Some((args.iterations, SHORT_CIRCUIT_INCREMENT))
                } else {
                    None
                }
            }
            .unwrap(); // This would fail if we'd stopped on the wrong player
            while completed_iterations < args.iterations {
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
                if !args.plain_mode {
                    draw_status_mcts(&mut stdout, completed_iterations, mcts_iterations);
                    stdout.flush()?;

                    check_for_killing_process();
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
        log::debug!("GameStateType {:?}", state.clone().game_state);
    }

    match state.game_state {
        GameState::Won => print!("Won!"),
        GameState::Lost => print!("Lost!"),
        _ => panic!("Invalid end game state"),
    }

    Ok(())
}
