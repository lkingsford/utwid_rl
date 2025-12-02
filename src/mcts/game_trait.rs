use super::Reward;
use std::fmt::Debug;

pub trait Action: Debug + Clone + Eq + std::hash::Hash {
    type StateType: State<ActionType = Self>;
    fn execute(&self, state: &Self::StateType) -> Self::StateType;
}

///
/// An actor is either a player or a game action.
///
/// A player is just an identifier, typically a number between 0 and n-1.
///
/// A game action is a action that the game takes, rather than a player.
#[derive(Debug, Clone, PartialEq)]
pub enum Actor<ActionType> {
    /// A player is just an identifier, typically a number between 0 and n-1.
    Player(u8),
    /// A game action is a action that the game takes (such as rolling a dice, or drawing a card), rather than a player.
    /// The value is a list of possible actions and their probabilities.
    GameAction(Vec<(ActionType, u32)>),
}

pub trait State: Clone {
    type ActionType: Action<StateType = Self>;
    type GameHyperrewardType: std::fmt::Debug + Clone + Send + Sync;

    fn permitted_actions(&self) -> Vec<Self::ActionType>;
    fn possible_non_player_actions(&self) -> Vec<(Self::ActionType, u32)> {
        vec![]
    }
    fn next_actor(&self) -> Actor<Self::ActionType>;
    fn terminal(&self) -> bool;
    fn reward(&self) -> Vec<Reward>;
    fn round_hyperreward(&self) -> Self::GameHyperrewardType;
}
