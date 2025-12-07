use crate::game::Game;
use crate::hyper::{
    GameHyperrewardTrait, Hyperparams, Hyperrewards, ParamMeta, ParamRange, ParamValue,
};
use crate::mcts::game_trait::{Action, Actor, State};
use pyo3::prelude::*;
use pyo3::types::PyAny;
use serde::Serialize;
use std::collections::HashMap;
use std::io;

#[derive(Clone, Debug)]
pub struct C4Hyperparams {
    pub board_width: usize,
    pub board_height: usize,
}

impl Default for C4Hyperparams {
    fn default() -> Self {
        C4Hyperparams {
            board_width: 7,
            board_height: 6,
        }
    }
}

impl Hyperparams for C4Hyperparams {
    fn metadata() -> std::collections::HashMap<String, crate::hyper::ParamMeta> {
        HashMap::from([
            (
                String::from("board_width"),
                ParamMeta {
                    default: ParamValue::Uint(7),
                    range: Option::None,
                },
            ),
            (
                String::from("board_height"),
                ParamMeta {
                    default: ParamValue::Uint(6),
                    range: Option::None,
                },
            ),
        ])
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct C4Hyperrewards {
    pub first_player_won: bool,
}

impl GameHyperrewardTrait for C4Hyperrewards {
    fn meta() -> HashMap<String, String> {
        HashMap::from([(
            String::from("first_player_won"),
            String::from("bool"),
        )])
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum C4Action {
    Drop(u8),
}

impl Action for C4Action {
    type StateType = C4State;
    fn execute(&self, state: &C4State) -> C4State {
        let mut new_board = state.board.clone();
        match self {
            C4Action::Drop(x) => {
                let column = *x as usize;
                for y in (0..state.hyperparams.board_height).rev() {
                    if new_board[y * state.hyperparams.board_width + column] == C4Cell::Empty {
                        new_board[y * state.hyperparams.board_width + column] =
                            C4Cell::Filled(state.next_player);
                        break;
                    }
                }
                let winner = check_for_win(&new_board, &state);
                let (terminal, reward) = match winner {
                    CheckForWinResult::Winner(0) => (true, [1.0 as f64, -1.0 as f64].to_vec()),
                    CheckForWinResult::Winner(1) => (true, [-1.0 as f64, 1.0 as f64].to_vec()),
                    CheckForWinResult::Stalemate => (true, [-0.5 as f64, -0.5 as f64].to_vec()),
                    CheckForWinResult::Ongoing => (false, [0.0 as f64, 0.0 as f64].to_vec()),
                    _ => panic!("Unexpected check_for_win result"),
                };
                C4State {
                    board: new_board,
                    next_player: (state.next_player + 1) % 2,
                    terminal,
                    hyperparams: C4Hyperparams::default(),
                    reward,
                }
            }
        }
    }
}

#[derive(PartialEq)]
enum CheckForWinResult {
    Winner(u8),
    Stalemate,
    Ongoing,
}

fn check_for_win(board: &Vec<C4Cell>, state: &C4State) -> CheckForWinResult {
    // Check stalemate
    if board.iter().all(|&cell| cell != C4Cell::Empty) {
        return CheckForWinResult::Stalemate;
    }

    // Check Horizontal win
    for row in 0..state.hyperparams.board_height {
        for column in 0..state.hyperparams.board_width - 3 {
            if board[row * state.hyperparams.board_width + column]
                == board[row * state.hyperparams.board_width + column + 1]
                && board[row * state.hyperparams.board_width + column]
                    == board[row * state.hyperparams.board_width + column + 2]
                && board[row * state.hyperparams.board_width + column]
                    == board[row * state.hyperparams.board_width + column + 3]
                && board[row * state.hyperparams.board_width + column] != C4Cell::Empty
            {
                return CheckForWinResult::Winner(
                    match board[row * state.hyperparams.board_width + column] {
                        C4Cell::Filled(player) => player,
                        _ => unreachable!(),
                    },
                );
            }
        }
    }

    // Check Vertical win
    for column in 0..state.hyperparams.board_width {
        for row in 0..state.hyperparams.board_height - 3 {
            if board[row * state.hyperparams.board_width + column]
                == board[(row + 1) * state.hyperparams.board_width + column]
                && board[row * state.hyperparams.board_width + column]
                    == board[(row + 2) * state.hyperparams.board_width + column]
                && board[row * state.hyperparams.board_width + column]
                    == board[(row + 3) * state.hyperparams.board_width + column]
                && board[row * state.hyperparams.board_width + column] != C4Cell::Empty
            {
                return CheckForWinResult::Winner(
                    match board[row * state.hyperparams.board_width + column] {
                        C4Cell::Filled(player) => player,
                        _ => unreachable!(),
                    },
                );
            }
        }
    }

    // Check \ win
    for column in 0..state.hyperparams.board_width - 3 {
        for row in 0..state.hyperparams.board_height - 3 {
            if board[row * state.hyperparams.board_width + column]
                == board[(row + 1) * state.hyperparams.board_width + column + 1]
                && board[row * state.hyperparams.board_width + column]
                    == board[(row + 2) * state.hyperparams.board_width + column + 2]
                && board[row * state.hyperparams.board_width + column]
                    == board[(row + 3) * state.hyperparams.board_width + column + 3]
                && board[row * state.hyperparams.board_width + column] != C4Cell::Empty
            {
                return CheckForWinResult::Winner(
                    match board[row * state.hyperparams.board_width + column] {
                        C4Cell::Filled(player) => player,
                        _ => unreachable!(),
                    },
                );
            }
        }
    }

    // Check / win
    for column in 0..state.hyperparams.board_width - 3 {
        for row in 3..state.hyperparams.board_height {
            if board[row * state.hyperparams.board_width + column]
                == board[(row - 1) * state.hyperparams.board_width + column + 1]
                && board[row * state.hyperparams.board_width + column]
                    == board[(row - 2) * state.hyperparams.board_width + column + 2]
                && board[row * state.hyperparams.board_width + column]
                    == board[(row - 3) * state.hyperparams.board_width + column + 3]
                && board[row * state.hyperparams.board_width + column] != C4Cell::Empty
            {
                return CheckForWinResult::Winner(
                    match board[row * state.hyperparams.board_width + column] {
                        C4Cell::Filled(player) => player,
                        _ => unreachable!(),
                    },
                );
            }
        }
    }

    CheckForWinResult::Ongoing
}

#[derive(Copy, Clone, PartialEq)]
enum C4Cell {
    Empty,
    Filled(u8),
}

#[derive(Clone)]
pub struct C4State {
    board: Vec<C4Cell>,
    next_player: u8,
    terminal: bool,
    reward: Vec<f64>,
    hyperparams: C4Hyperparams,
}

impl State for C4State {
    type ActionType = C4Action;
    type GameHyperrewardType = C4Hyperrewards;
    fn permitted_actions(&self) -> Vec<Self::ActionType> {
        (0..self.hyperparams.board_width)
            .filter(|&i| self.board[i] == C4Cell::Empty)
            .map(|i| C4Action::Drop(i as u8))
            .collect::<Vec<C4Action>>()
    }
    fn next_actor(&self) -> Actor<C4Action> {
        Actor::Player(self.next_player)
    }
    fn terminal(&self) -> bool {
        self.terminal
    }

    fn reward(&self) -> Vec<f64> {
        self.reward.clone()
    }
    fn round_hyperreward(&self) -> Self::GameHyperrewardType {
        if !self.terminal {
            return C4Hyperrewards::default();
        }
        match check_for_win(&self.board, self) {
            CheckForWinResult::Winner(0) => C4Hyperrewards {
                first_player_won: true,
            },
            CheckForWinResult::Winner(1) => C4Hyperrewards {
                first_player_won: false,
            },
            _ => C4Hyperrewards::default(),
        }
    }
}

pub struct C4;

impl Game for C4 {
    type StateType = C4State;
    type ActionType = C4Action;
    type HyperparamsType = C4Hyperparams;
    type HyperrewardsType = C4Hyperrewards;

    fn visualise_state(&self, state: &Self::StateType) {
        for x in 0..state.hyperparams.board_width {
            print!("{}", x);
        }
        print!("\n");
        for y in 0..state.hyperparams.board_height {
            for x in 0..state.hyperparams.board_width {
                print!(
                    "{}",
                    match state.board[y * state.hyperparams.board_width + x] {
                        C4Cell::Empty => "◦",
                        C4Cell::Filled(1) => "◯",
                        C4Cell::Filled(0) => "●",
                        _ => " ",
                    }
                )
            }
            print!("\n");
        }
    }

    fn get_human_turn(&self, _state: &Self::StateType) -> Self::ActionType {
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to read line");
        let action = input.trim().parse().expect("Failed to parse action");
        C4Action::Drop(action)
    }

    fn init_game(&self, hyperparams: &Self::HyperparamsType) -> Self::StateType {
        C4State {
            board: vec![
                C4Cell::Empty;
                usize::from(hyperparams.board_width * hyperparams.board_height)
            ],
            next_player: 0,
            terminal: false,
            reward: [0.0 as f64, 0.0 as f64].to_vec(),
            hyperparams: hyperparams.clone(),
        }
    }
}
