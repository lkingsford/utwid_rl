use std::collections::HashMap;
use std::io;

use crate::game::Game;
use crate::hyperparam::{Hyperparams, ParamMeta, ParamRange, ParamValue};
use crate::mcts::game_trait::{Action, Actor, State};

pub const BOARD_WIDTH: usize = 7;
pub const BOARD_HEIGHT: usize = 6;

#[derive(Clone, Debug)]
pub struct C4Hyperparams {
    pub board_width: i64,
    pub board_height: i64,
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
    fn metadata() -> std::collections::HashMap<String, crate::hyperparam::ParamMeta> {
        HashMap::from([
            (
                String::from("board_width"),
                ParamMeta {
                    default: ParamValue::Int(7),
                    range: Option::Some(ParamRange::IntRange(0, i64::MAX)),
                },
            ),
            (
                String::from("board_height"),
                ParamMeta {
                    default: ParamValue::Int(6),
                    range: Option::Some(ParamRange::IntRange(0, i64::MAX)),
                },
            ),
        ])
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
                for y in (0..BOARD_HEIGHT).rev() {
                    if new_board[y * BOARD_WIDTH + column] == C4Cell::Empty {
                        new_board[y * BOARD_WIDTH + column] = C4Cell::Filled(state.next_player);
                        break;
                    }
                }
                let winner = check_for_win(&new_board);
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

fn check_for_win(board: &Vec<C4Cell>) -> CheckForWinResult {
    // Check stalemate
    if board.iter().all(|&cell| cell != C4Cell::Empty) {
        return CheckForWinResult::Stalemate;
    }

    // Check Horizontal win
    for row in 0..BOARD_HEIGHT {
        for column in 0..BOARD_WIDTH - 3 {
            if board[row * BOARD_WIDTH + column] == board[row * BOARD_WIDTH + column + 1]
                && board[row * BOARD_WIDTH + column] == board[row * BOARD_WIDTH + column + 2]
                && board[row * BOARD_WIDTH + column] == board[row * BOARD_WIDTH + column + 3]
                && board[row * BOARD_WIDTH + column] != C4Cell::Empty
            {
                return CheckForWinResult::Winner(match board[row * BOARD_WIDTH + column] {
                    C4Cell::Filled(player) => player,
                    _ => unreachable!(),
                });
            }
        }
    }

    // Check Vertical win
    for column in 0..BOARD_WIDTH {
        for row in 0..BOARD_HEIGHT - 3 {
            if board[row * BOARD_WIDTH + column] == board[(row + 1) * BOARD_WIDTH + column]
                && board[row * BOARD_WIDTH + column] == board[(row + 2) * BOARD_WIDTH + column]
                && board[row * BOARD_WIDTH + column] == board[(row + 3) * BOARD_WIDTH + column]
                && board[row * BOARD_WIDTH + column] != C4Cell::Empty
            {
                return CheckForWinResult::Winner(match board[row * BOARD_WIDTH + column] {
                    C4Cell::Filled(player) => player,
                    _ => unreachable!(),
                });
            }
        }
    }

    // Check \ win
    for column in 0..BOARD_WIDTH - 3 {
        for row in 0..BOARD_HEIGHT - 3 {
            if board[row * BOARD_WIDTH + column] == board[(row + 1) * BOARD_WIDTH + column + 1]
                && board[row * BOARD_WIDTH + column] == board[(row + 2) * BOARD_WIDTH + column + 2]
                && board[row * BOARD_WIDTH + column] == board[(row + 3) * BOARD_WIDTH + column + 3]
                && board[row * BOARD_WIDTH + column] != C4Cell::Empty
            {
                return CheckForWinResult::Winner(match board[row * BOARD_WIDTH + column] {
                    C4Cell::Filled(player) => player,
                    _ => unreachable!(),
                });
            }
        }
    }

    // Check / win
    for column in 0..BOARD_WIDTH - 3 {
        for row in 3..BOARD_HEIGHT {
            if board[row * BOARD_WIDTH + column] == board[(row - 1) * BOARD_WIDTH + column + 1]
                && board[row * BOARD_WIDTH + column] == board[(row - 2) * BOARD_WIDTH + column + 2]
                && board[row * BOARD_WIDTH + column] == board[(row - 3) * BOARD_WIDTH + column + 3]
                && board[row * BOARD_WIDTH + column] != C4Cell::Empty
            {
                return CheckForWinResult::Winner(match board[row * BOARD_WIDTH + column] {
                    C4Cell::Filled(player) => player,
                    _ => unreachable!(),
                });
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
}

impl State for C4State {
    type ActionType = C4Action;
    fn permitted_actions(&self) -> Vec<Self::ActionType> {
        (0..BOARD_WIDTH)
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
}

pub struct C4;

impl Game for C4 {
    type StateType = C4State;
    type ActionType = C4Action;
    type HyperparamsType = ();

    fn visualise_state(&self, state: &Self::StateType) {
        for x in 0..BOARD_WIDTH {
            print!("{}", x);
        }
        print!("\n");
        for y in 0..BOARD_HEIGHT {
            for x in 0..BOARD_WIDTH {
                print!(
                    "{}",
                    match state.board[y * BOARD_WIDTH + x] {
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

    fn init_game(&self, _hyperparams: &Self::HyperparamsType) -> Self::StateType {
        C4State {
            board: vec![C4Cell::Empty; BOARD_HEIGHT * BOARD_WIDTH],
            next_player: 0,
            terminal: false,
            reward: [0.0 as f64, 0.0 as f64].to_vec(),
        }
    }
}
