use std::collections::HashMap;

use crate::mon2y::game::{Action, Actor, State};
use crate::mon2y::Reward;

type ActorId = u64;

#[derive(Clone)]
pub struct UtwidState {
    pub current_level: u8,
    pub board: Board,
    pub actors: HashMap<ActorId, GameActor>,
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
pub struct GameActor {
    pub console_repr: Option<Tile>,
}

#[derive(Clone)]
pub struct Tile {
    pub walkable: bool,
    pub console_repr: char,
}

#[derive(Clone)]
pub struct Board {
    pub geography: Vec<Tile>,
    pub width: u8,
    pub height: u8,
}

impl Board {
    pub fn new() -> Self {
        let width: u8 = 11;
        let height: u8 = 11;
        let mut geography = vec![
            Tile {
                walkable: true,
                console_repr: '.',
            };
            (width * height) as usize
        ];
        for ix in 5..11 {
            geography[(width * 7 + ix) as usize] = Tile {
                walkable: false,
                console_repr: '#',
            }
        }
        Board {
            geography,
            width,
            height,
        }
    }
}
