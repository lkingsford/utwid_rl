use crate::mon2y::game::{Action, Actor, State};
use crate::mon2y::Reward;

#[derive(Clone)]
pub struct UtwidState {
    current_level: u8,
    geography: Vec<Tile>,
}

impl UtwidState {}

impl State for UtwidState {
    type ActionType = UtwidAction;

    fn permitted_actions(&self) -> Vec<Self::ActionType> {
        unimplemented!()
    }

    fn next_actor(&self) -> Actor<Self::ActionType> {
        unimplemented!()
    }

    fn terminal(&self) -> bool {
        unimplemented!()
    }

    fn reward(&self) -> Vec<Reward> {
        unimplemented!()
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum UtwidAction {
    NoAction,
}

impl Action for UtwidAction {
    type StateType = UtwidState;

    fn execute(&self, _state: &Self::StateType) -> Self::StateType {
        unimplemented!()
    }
}

#[derive(Clone)]
pub struct Tile {
    walkable: bool,
    actors: Vec<Actor<UtwidAction>>,
    console_repr: char,
}

pub struct Board {
    geography: Vec<Tile>,
    width: u8,
    height: u8,
}

impl Board {
    fn new() -> Self {
        let width: u8 = 11;
        let height: u8 = 11;
        let geography = vec![
            Tile {
                walkable: true,
                actors: vec![],
                console_repr: '.'
            };
            (width * height) as usize
        ];
        Board {
            geography,
            width: width,
            height: height,
        }
    }
}
