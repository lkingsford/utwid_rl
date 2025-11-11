mod game;
mod games;
mod mon2y;
mod test;

//use crate::mon2y::action_log::{Action, ActionLogEntry};
use clap::{Parser, ValueEnum};
use env_logger::fmt::Formatter;
use game::Game;
use games::Games;
use games::{C4, CS, EBR, NT};
use log::Record;
use mon2y::game::{Action, Actor, State};
use mon2y::{calculate_best_turn, BestTurnPolicy};
use std::io;
use std::io::Write;
use std::thread;

use rand::Rng;

#[derive(Debug, Clone, ValueEnum)]
enum PlayerType {
    H,
    R,
    M,
}

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg()]
    game: Games,

    /// Players participating in the game
    #[arg(short, long, value_delimiter = ',', value_enum)]
    players: Vec<PlayerType>,
    #[command(flatten)]
    verbose: clap_verbosity_flag::Verbosity,
    #[arg(short, long, default_value_t = 10000)]
    iterations: usize,
    #[arg(short, long, default_value_t = 1)]
    episodes: usize,
    #[arg(short, long, default_value_t = 4)]
    threads: usize,
    #[arg(short('I'), long, default_value_t = false)]
    inject_game_turns: bool,
    #[arg(short('T'), long)]
    limit_time: Option<f32>,
    #[arg(short('P'), long, default_value_t=BestTurnPolicy::MostVisits)]
    policy: BestTurnPolicy,
    #[arg(short('c'), long, default_value_t = 1.4142135623730951)]
    exploration_constant: f64,
    #[arg(long, default_value_t = false)]
    log_children: bool,
}

/// Play a game of the given type with the given players.
///
/// Each player is specified by their type, which can be:
/// - `H` for a human player
/// - `R` for a random player
/// - `M` for a player that uses the MCTS algorithm to play
///
/// The game is played until it is terminal.
///
/// If `inject_game_turns` is true, the game will pause after each game action
/// and ask the user to enter the index of the action to take.
fn run_game<G: Game>(
    game: G,
    players: Vec<PlayerType>,
    iterations: usize,
    time_limit: Option<f32>,
    threads: usize,
    inject_game_turns: bool,
    policy: BestTurnPolicy,
    constant: f64,
    log_children: bool,
) {
    let mut state = game.init_game(&G::HyperparamsType::default());
    while !state.terminal() {
        let actor = state.next_actor();
        game.visualise_state(&state);
        match actor {
            Actor::Player(player) => {
                let action: G::ActionType = match players.get(player as usize) {
                    Some(PlayerType::H) => game.get_human_turn(&state),
                    Some(PlayerType::R) => {
                        let permitted_actions = state.permitted_actions();
                        permitted_actions[rand::thread_rng().gen_range(0..permitted_actions.len())]
                            .clone()
                    }
                    Some(PlayerType::M) => calculate_best_turn(
                        iterations,
                        match time_limit {
                            None => None,
                            Some(time_limit) => {
                                Some(std::time::Duration::from_secs_f32(time_limit))
                            }
                        },
                        threads,
                        state.clone(),
                        policy,
                        constant,
                        log_children,
                    ),
                    _ => todo!(),
                };
                log::info!("Player {} plays {:?}", player, action);
                state = action.execute(&state);
            }
            Actor::GameAction(actions) => {
                if inject_game_turns {
                    println!("GAME ACTION");
                    let mut sorted_actions = actions.clone();
                    sorted_actions.sort_by(|a, b| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)));

                    for (i, action) in sorted_actions.iter().enumerate() {
                        println!("{} {:?} {}", i, action.0, action.1);
                    }
                    loop {
                        let mut input = String::new();
                        if io::stdin().read_line(&mut input).is_err() {
                            println!("Failed to read line. Please try again.");
                            continue;
                        }
                        let action = match input.trim().parse::<usize>() {
                            Ok(action) => sorted_actions[action].clone(),
                            Err(_) => {
                                println!("Failed to parse action. Please enter a valid number.");
                                continue;
                            }
                        };
                        state = action.0.execute(&state);
                        break;
                    }
                } else {
                    //TODO: Use a weighted random (because the second variable is supposed to be the weight)
                    let action = actions[rand::thread_rng().gen_range(0..actions.len())]
                        .0
                        .clone();
                    log::info!("Game Action {:?}", action);
                    state = action.execute(&state);
                }
            }
        }
    }
    game.visualise_state(&state);
}

fn main() {
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

    let players = args.players;

    for _ in 0..args.episodes {
        match args.game {
            Games::C4 => {
                run_game(
                    C4,
                    players.clone(),
                    args.iterations,
                    args.limit_time,
                    args.threads,
                    args.inject_game_turns,
                    args.policy,
                    args.exploration_constant,
                    args.log_children,
                );
            }
            Games::NT => {
                run_game(
                    NT {
                        player_count: players.len() as u8,
                    },
                    players.clone(),
                    args.iterations,
                    args.limit_time,
                    args.threads,
                    args.inject_game_turns,
                    args.policy,
                    args.exploration_constant,
                    args.log_children,
                );
            }
            Games::CS => {
                run_game(
                    CS {
                        player_count: players.len() as u8,
                    },
                    players.clone(),
                    args.iterations,
                    args.limit_time,
                    args.threads,
                    args.inject_game_turns,
                    args.policy,
                    args.exploration_constant,
                    args.log_children,
                );
            }
            Games::EBR => {
                run_game(
                    EBR {
                        player_count: players.len() as u8,
                    },
                    players.clone(),
                    args.iterations,
                    args.limit_time,
                    args.threads,
                    args.inject_game_turns,
                    args.policy,
                    args.exploration_constant,
                    args.log_children,
                );
            }
        }
    }
}
