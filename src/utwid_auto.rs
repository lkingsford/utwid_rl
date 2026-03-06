use clap::Parser;
use env_logger::fmt::Formatter;
use log::Record;
mod game;
mod games;
mod mon2y;
mod test;
use std::{fs, thread};

use utwid_rl::{
    mon2y::{calculate_best_turn, game::Action},
    utwid_game::{self, ActorTrait, GameState},
};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::Print,
    terminal::{Clear, ClearType},
};

use std::io::{stdout, Stdout, Write};

const DRAW_BOARD_X: u16 = 3;
const DRAW_BOARD_Y: u16 = 3;

fn draw_board(stdout: &mut Stdout, state: utwid_game::UtwidState) -> std::io::Result<()> {
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

fn draw_monsters(stdout: &mut Stdout, state: &utwid_game::UtwidState) -> std::io::Result<()> {
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
                    actor.traits.iter().find_map(|t| {
                        if let utwid_game::ActorTrait::Health(h) = t {
                            Some(*h)
                        } else {
                            None
                        }
                    }).unwrap_or(0)
                ))
            )?;
        }
    }
    Ok(())
}

const DRAW_MONSTER_X: u16 = 20;
const DRAW_MONSTER_Y: u16 = 2;

const HUMAN_ITERATIONS: usize = 10000;
const THREADS: usize = 6;
const EXPLORATION_CONSTANT: f64 = 1.4142135623730951; // sqrt(2.0)
const SHORT_CIRCUIT_AT_TURNS: usize = 20000;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg()]
    config_file: Vec<String>,
    #[command(flatten)]
    verbose: clap_verbosity_flag::Verbosity,
}
fn main() -> std::io::Result<()> {
    let args = Args::parse();
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

    let mut state = utwid_game::UtwidState::new();
    state.short_circuit_at_turns = Some(SHORT_CIRCUIT_AT_TURNS);
    let mut stdout = stdout();

    while matches!(state.game_state, GameState::Ongoing | GameState::Checkpoint) {
        queue!(stdout, Clear(ClearType::All))?;
        draw_board(&mut stdout, state.clone())?;
        draw_monsters(&mut stdout, &state)?;
        stdout.flush();
        let next_act = calculate_best_turn(
            {
                let to_act = state.actors.get(&state.to_act).unwrap();
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
            utwid_rl::mon2y::BestTurnPolicy::Ucb0,
            EXPLORATION_CONSTANT,
            false,
        );
        state = next_act.execute(&state);
        if matches!(state.game_state, GameState::Mon2yShortcircuit) {
            state.game_state = GameState::Ongoing;
        };
        state.ai_turn_weight = 0.0;
        log::debug!("GameStateType {:?}", state.clone().game_state);
    }

    Ok(())
}
