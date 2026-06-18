mod action;
mod action_player;
mod actions_common;
mod actions_mon2y;
mod actor;
mod board;
mod state;
mod types;

#[cfg(test)]
mod tests;

pub use action::{Dir, UtwidAction, UtwidEvent, action_cost};
pub use actor::{ActorTraits, Allegiance, GameActor, Mon2yData};
pub use board::{Board, Tile, TileTraits};
pub use state::UtwidState;
pub use types::{GameState, Repr, ReprSet};

use crate::game::Game;

impl Game for Utwid {
    type StateType = UtwidState;
    type ActionType = UtwidAction;
    type HyperrewardsType = ();

    fn visualise_state(&self, state: &Self::StateType) {
        println!("{}", state.debug_summary());
    }

    fn init_game(&self) -> Self::StateType {
        UtwidState::new()
    }
}

pub struct Utwid;
