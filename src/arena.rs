use clap::Parser;
use env_logger::fmt::Formatter;
use log::Record;
use mon2y::game::Game;
use mon2y::games::Games;
use mon2y::games::{C4, CS, EBR, NT, Utwid};
use mon2y::mcts::game_trait::{Action, Actor, State};
use mon2y::mcts::{calculate_best_turn, BestTurnPolicy};
use rand::Rng;
use serde::Deserialize;
use std::io::Write;
use std::{fs, thread};

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg()]
    config_file: Vec<String>,
    #[command(flatten)]
    verbose: clap_verbosity_flag::Verbosity,
}

#[derive(Debug, Deserialize)]
struct ArenaSettings {
    game: Games,
    episodes: usize,
    players: Vec<PlayerSettings>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
enum PlayerSettings {
    Random,
    Mcts(MctsSettings),
}

#[derive(Debug, Deserialize, Clone)]
struct MctsSettings {
    policy: BestTurnPolicy,
    exploration_constant: Option<f64>,
    iterations: usize,
    time_limit: Option<f32>,
    threads: Option<usize>,
}

fn run_episode<G: Game>(game: G, players: Vec<PlayerSettings>) -> Vec<f64> {
    let mut state = game.init_game(&G::HyperparamsType::default());
    while !state.terminal() {
        let actor = state.next_actor();
        match actor {
            Actor::Player(player) => {
                let action: G::ActionType = match players.get(player as usize) {
                    Some(PlayerSettings::Random) => {
                        let permitted_actions = state.permitted_actions();
                        permitted_actions[rand::thread_rng().gen_range(0..permitted_actions.len())]
                            .clone()
                    }
                    Some(PlayerSettings::Mcts(mcts_settings)) => calculate_best_turn(
                        mcts_settings.iterations,
                        match mcts_settings.time_limit {
                            None => None,
                            Some(time_limit) => {
                                Some(std::time::Duration::from_secs_f32(time_limit))
                            }
                        },
                        match mcts_settings.threads {
                            None => 4,
                            Some(thread) => thread,
                        },
                        state.clone(),
                        mcts_settings.policy,
                        match mcts_settings.exploration_constant {
                            None => 2.0_f64.sqrt(),
                            Some(constant) => constant,
                        },
                        false,
                    ),
                    _ => todo!(),
                };
                log::debug!("Player {} plays {:?}", player, action);
                state = action.execute(&state);
            }
            Actor::GameAction(actions) => {
                //TODO: Use a weighted random (because the second variable is supposed to be the weight)
                let action = actions[rand::thread_rng().gen_range(0..actions.len())]
                    .0
                    .clone();
                state = action.execute(&state);
            }
        }
    }
    state.reward()
}

fn run_config(config_file: String) {
    let config_file = fs::read_to_string(&config_file).expect("Failed to read config file");
    let arena_settings: ArenaSettings =
        serde_json::from_str(&config_file).expect("Failed to parse config file");

    let mut results = vec![(0.0, 0); arena_settings.players.len()];
    for episode in 0..arena_settings.episodes {
        log::info!("Starting episode {}", episode);
        let result = match arena_settings.game {
            Games::C4 => run_episode(C4, arena_settings.players.clone()),
            Games::NT => run_episode(
                NT {
                    player_count: arena_settings.players.len() as u8,
                },
                arena_settings.players.clone(),
            ),
            Games::CS => run_episode(
                CS {
                    player_count: arena_settings.players.len() as u8,
                },
                arena_settings.players.clone(),
            ),
            Games::EBR => run_episode(
                EBR {
                    player_count: arena_settings.players.len() as u8,
                },
                arena_settings.players.clone(),
            ),
            Games::Utwid => run_episode(Utwid, arena_settings.players.clone()),
        };
        let max_result = result
            .iter()
            .map(|r| r)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Less));
        for (i, r) in result.iter().enumerate() {
            results[i].0 += *r;
            if Some(r) == max_result {
                results[i].1 += 1;
            }
        }
    }
    println!();
    println!("{:?}", arena_settings);
    println!("Player\tReward\t%\tWins\t%");
    let total: f64 = results.iter().map(|r| r.0 as f64).sum();
    for (i, r) in results.iter().enumerate() {
        println!(
            "{}\t{:?}\t{:>5.2}%\t{:?}\t{:>5.2}%",
            i + 1,
            r.0,
            (100.0 * r.0) / total,
            r.1,
            (100.0 * r.1 as f64) / arena_settings.episodes as f64
        );
    }
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

    for config_file in args.config_file {
        run_config(config_file);
    }
}
