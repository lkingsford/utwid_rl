mod game;
mod games;
mod mon2y;
mod test;

use clap::Parser;
use game::Game;
use mon2y::{calculate_best_turn, BestTurnPolicy};
use std::{collections::HashMap, time::Instant};
use utwid_rl::utwid_game;

use crossterm::{
    cursor::MoveTo,
    event, execute, queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType},
    ExecutableCommand,
};

use std::io::{stdout, Stdout, Write};

const DRAW_BOARD_X: u16 = 0;
const DRAW_BOARD_Y: u16 = 0;

fn draw_board(stdout: &mut Stdout, state: utwid_game::UtwidState) {
    for iy in 0..state.board.height {
        queue!(stdout, MoveTo(DRAW_BOARD_X, (DRAW_BOARD_Y + iy as u16)));
        for ix in 0..state.board.width {
            queue!(
                stdout,
                Print(
                    state.board.geography[(ix + iy * state.board.width) as usize]
                        .console_repr
                        .clone()
                )
            )
            .unwrap()
        }
    }
}

fn main() -> std::io::Result<()> {
    let state = utwid_game::UtwidState {
        current_level: 0,
        board: utwid_game::Board::new(),
        actors: HashMap::from([]),
    };

    let board = utwid_game::Board::new();
    let mut stdout = stdout();
    queue!(stdout, Clear(ClearType::All));
    draw_board(&mut stdout, state);

    Ok(())
}
