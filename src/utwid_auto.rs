mod game;
mod games;
mod mon2y;
mod test;

use utwid_rl::{
    mon2y::{calculate_best_turn, game::Action},
    utwid_game::{self, GameState},
};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::Print,
    terminal::{Clear, ClearType},
};

use std::io::{stdout, Stdout};

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

const HUMAN_ITERATIONS: usize = 10000;
const THREADS: usize = 2;
const EXPLORATION_CONSTANT: f64 = 1.4142135623730951; // sqrt(2.0)

fn main() -> std::io::Result<()> {
    let mut state = utwid_game::UtwidState::new();

    while matches!(state.game_state, GameState::Ongoing) {
        let next_act = calculate_best_turn(
            HUMAN_ITERATIONS,
            None,
            THREADS,
            &state,
            utwid_rl::mon2y::BestTurnPolicy::MostVisits,
            EXPLORATION_CONSTANT,
            false,
        );
        state = next_act.execute(state);
    }

    let mut stdout = stdout();
    queue!(stdout, Clear(ClearType::All))?;
    draw_board(&mut stdout, state)?;

    Ok(())
}
