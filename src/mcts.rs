pub mod game_trait;
mod mcts;
pub use mcts::calculate_best_turn;
pub mod node;
pub mod tree;
pub mod weighted_random;
mod sender; // Changed from noop_sender
use clap::ValueEnum;
use serde::Deserialize;

pub type Reward = f64;

#[derive(Debug, Clone, Copy, ValueEnum, Deserialize)]
pub enum BestTurnPolicy {
    MostVisits,
    Ucb0,
}

impl std::fmt::Display for BestTurnPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            BestTurnPolicy::MostVisits => write!(f, "most-visits"),
            BestTurnPolicy::Ucb0 => write!(f, "ucb0"),
        }
    }
}
