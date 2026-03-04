mod game;
mod games;
mod mon2y;
mod test;

use utwid_rl::utwid_game;

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Print},
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

fn main() -> std::io::Result<()> {
    let state = utwid_game::UtwidState::new();

    let mut stdout = stdout();
    queue!(stdout, Clear(ClearType::All))?;
    draw_board(&mut stdout, state)?;

    Ok(())
}
