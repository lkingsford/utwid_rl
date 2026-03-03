use std::collections::{HashMap, HashSet};

use crate::mon2y::game::{Action, Actor, State};
use crate::mon2y::Reward;

type ActorId = usize; // If I keep using this code, this might need to be u64, or something else

#[derive(Clone)]
pub struct UtwidState {
    pub current_level: usize,
    pub board: Board,
    pub actors: HashMap<ActorId, GameActor>,
    pub to_act: ActorId,
}

impl UtwidState {
    pub fn new() -> UtwidState {
        UtwidState {
            current_level: 0,
            board: Board::new(),
            actors: HashMap::from([(0, GameActor::YouActor())]),
            to_act: 0,
        }
    }

    // Urgh - I don't know if I should be using an index here...
    pub fn add_actor(&mut self, actor: GameActor) -> ActorId {
        self.actors.insert(self.actors.len(), actor);
        self.actors.len() - 1
    }
}

impl State for UtwidState {
    type ActionType = UtwidAction;

    fn permitted_actions(&self) -> Vec<Self::ActionType> {
        let permitted_actions = Vec::new();
        let next_actor = self.actors.get(&self.to_act).unwrap();
        let available_moves = self.board.permitted_moves(
            next_actor.x,
            next_actor.y,
            next_actor.traits.contains(&ActorTrait::CardinalMove),
            next_actor.traits.contains(&ActorTrait::DiagonalMove),
        );
        permitted_actions
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
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
    Wait,
}

impl Action for UtwidAction {
    type StateType = UtwidState;

    fn execute(&self, state: &Self::StateType) -> Self::StateType {
        unimplemented!()
    }
}

#[derive(Clone, PartialEq, PartialOrd, Eq, Hash)]
pub enum TileTrait {
    Walkable,
    ConsoleRepr(char),
}

#[derive(Clone)]
pub struct Tile {
    traits: HashSet<TileTrait>,
}

impl Tile {
    fn floor() -> Tile {
        Tile {
            traits: HashSet::from([TileTrait::Walkable, TileTrait::ConsoleRepr('.')]),
        }
    }

    fn wall() -> Tile {
        Tile {
            traits: HashSet::from([TileTrait::Walkable, TileTrait::ConsoleRepr('#')]),
        }
    }

    pub fn console_repr(&self) -> Option<char> {
        self.traits.iter().find_map(|trait_| match trait_ {
            TileTrait::ConsoleRepr(c) => Some(*c),
            _ => None,
        })
    }
}

#[derive(Clone)]
pub struct Board {
    pub geography: Vec<Tile>,
    pub width: usize,
    pub height: usize,
}

impl Board {
    pub fn new() -> Self {
        let width: usize = 11;
        let height: usize = 11;
        let mut geography = vec![Tile::floor(); (width * height) as usize];
        for ix in 5..11 {
            geography[width * 8 + ix] = Tile::wall()
        }
        Board {
            geography,
            width,
            height,
        }
    }

    fn get(&self, x: usize, y: usize) -> &Tile {
        &self.geography[self.width * y + x]
    }

    fn permitted_moves(
        &self,
        from_x: usize,
        from_y: usize,
        cardinal: bool,
        diagonal: bool,
    ) -> Vec<UtwidAction> {
        let mut directions = Vec::new();
        if cardinal {
            directions.extend(vec![
                (UtwidAction::N, (from_x, from_y - 1)),
                (UtwidAction::S, (from_x, from_y + 1)),
                (UtwidAction::E, (from_x - 1, from_y)),
                (UtwidAction::W, (from_x - 1, from_y)),
            ])
        };
        if diagonal {
            directions.extend(vec![
                (UtwidAction::NE, (from_x - 1, from_y - 1)),
                (UtwidAction::NW, (from_x + 1, from_y + 1)),
                (UtwidAction::SE, (from_x - 1, from_y + 1)),
                (UtwidAction::SW, (from_x + 1, from_y + 1)),
            ])
        };

        directions
            .iter()
            .filter(|(_, coords)| {
                self.get(coords.0, coords.1)
                    .traits
                    .contains(&TileTrait::Walkable)
            })
            .map(|(action, _)| action.clone())
            .collect()
    }
}

#[derive(Clone, PartialEq, PartialOrd, Eq, Hash)]
pub enum ActorTrait {
    Human,
    Mon2y { iterations: usize },
    CardinalMove,
    DiagonalMove,
    Wait,
    ConsoleRepr(char),
}

#[derive(Clone)]
pub struct GameActor {
    pub x: usize,
    pub y: usize,
    pub traits: HashSet<ActorTrait>,
}

impl GameActor {
    pub fn console_repr(&self) -> Option<char> {
        self.traits.iter().find_map(|trait_| match trait_ {
            ActorTrait::ConsoleRepr(c) => Some(*c),
            _ => None,
        })
    }
}

impl GameActor {
    // Feels logical that these should be seperate
    fn YouActor() -> GameActor {
        GameActor {
            x: 1,
            y: 3,
            traits: HashSet::from([
                ActorTrait::ConsoleRepr('@'),
                ActorTrait::Human,
                ActorTrait::CardinalMove,
                ActorTrait::DiagonalMove,
            ]),
        }
    }

    fn MonteActor() -> GameActor {
        GameActor {
            x: 7,
            y: 7,
            traits: HashSet::from([
                ActorTrait::ConsoleRepr('&'),
                ActorTrait::Mon2y { iterations: 1000 },
                ActorTrait::CardinalMove,
                ActorTrait::DiagonalMove,
                ActorTrait::Wait,
            ]),
        }
    }
}
