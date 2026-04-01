use clap::Parser;
use crossterm::{
    cursor::MoveTo,
    event::{self, KeyCode, KeyModifiers},
    queue,
    style::Print,
    terminal::{self, Clear, ClearType},
};
use env_logger::fmt::Formatter;
use log::Record;
use std::io::{stdout, Stdout, Write};
use std::thread;

use mon2y::games::utwid::{ActorTrait, GameState, UtwidAction, UtwidState};
use mon2y::mcts::game_trait::Action;
use mon2y::mcts::{calculate_best_turn, BestTurnPolicy};

const DRAW_BOARD_X: u16 = 3;
const DRAW_BOARD_Y: u16 = 3;

fn draw_board(stdout: &mut Stdout, state: UtwidState) -> std::io::Result<()> {
    for iy in 0..state.board.height {
        queue!(stdout, MoveTo(DRAW_BOARD_X, DRAW_BOARD_Y + iy as u16))?;
        for ix in 0..state.board.width {
            let actor_repr = state
                .actors
                .values()
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
        if let Some(actor) = state.actors.get(actor_id) {
            queue!(
                stdout,
                MoveTo(DRAW_MONSTER_X, DRAW_MONSTER_Y + i as u16),
                Print(format!(
                    "{} ({}, {}) - {}",
                    actor.console_repr().unwrap_or(' '),
                    actor.x,
                    actor.y,
                    actor
                        .traits
                        .iter()
                        .find_map(|t| {
                            if let ActorTrait::Health(h) = t {
                                Some(*h)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0)
                ))
            )?;
        }
    }
    Ok(())
}

const DRAW_MONSTER_X: u16 = 20;
const DRAW_MONSTER_Y: u16 = 2;

const HUMAN_ITERATIONS: usize = 3000;
const THREADS: usize = 6;
const EXPLORATION_CONSTANT: f64 = 1.4142135623730951; // sqrt(2.0)
const SHORT_CIRCUIT_AT_TURNS: usize = 20000;

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
    let mut stdout = stdout();

    while matches!(state.game_state, GameState::Ongoing | GameState::Checkpoint) {
        if !args.plain_mode {
            queue!(stdout, Clear(ClearType::All))?;
            draw_board(&mut stdout, state.clone())?;
            draw_monsters(&mut stdout, &state)?;
            stdout.flush()?;
        }
        let to_act = state.actors.get(&state.to_act).unwrap();
        let next_act = if args.human && to_act.traits.contains(&ActorTrait::Human) {
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
            calculate_best_turn(
                {
                    to_act.traits.iter().find_map(|trait_| match trait_ {
                        ActorTrait::Mon2y {
                            tree_id,
                            iterations,
                        } => Some(*iterations),
                        ActorTrait::Human => Some(HUMAN_ITERATIONS),
                        _ => None,
                    })
                }
                .unwrap(), // This would fail if we'd stopped on the wrong player
                None,
                THREADS,
                state.clone(),
                BestTurnPolicy::Ucb0,
                EXPLORATION_CONSTANT,
                false,
            )
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
