use std::collections::{HashMap, HashSet};

use crate::mon2y::game::{Action, Actor, State};
use crate::mon2y::Reward;

use rand::rngs::ThreadRng;
use rand::{Rng, RngExt};

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
        let mut rng = rand::rng();
        UtwidState {
            current_level: 0,
            board: Board::new(0, &mut rng),
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
        let next_actor = self.actors.get(&self.to_act).unwrap();
        self.board.permitted_moves(
            next_actor.x,
            next_actor.y,
            next_actor.traits.contains(&ActorTrait::CardinalMove),
            next_actor.traits.contains(&ActorTrait::DiagonalMove),
        )
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
        let to_act = state.actors.get(&state.to_act).unwrap();

        match self {
            UtwidAction::N
            | UtwidAction::S
            | UtwidAction::E
            | UtwidAction::E
            | UtwidAction::NE
            | UtwidAction::NW
            | UtwidAction::SE
            | UtwidAction::SW => self.execute_move(state),
            _ => unimplemented!(),
        }
    }
}

impl UtwidAction {
    fn execute_move(&self, state: &UtwidState) -> UtwidState {
        let mut state = state.clone();
        let mut to_act = state.actors.get_mut(&state.to_act).unwrap();
        (to_act.x, to_act.y) = apply_dir(to_act.x, to_act.y, self.clone());
        if to_act.traits.contains(&ActorTrait::Human) {
            let tile = state.board.get(to_act.x, to_act.y);

            tile.traits
                .iter()
                .find_map(|trait_| match trait_ {
                    TileTrait::Stairs => self.execute_stairs(&state, &tile, &to_act),
                    _ => state,
                })
                .or_else({ state })
        } else {
            state
        }
    }

    fn execute_stairs(&self, state: &UtwidState, tile: &Tile, to_act: &GameActor) -> UtwidState {
        state.clone()
    }
}

#[derive(Clone, PartialEq, PartialOrd, Eq, Hash)]
pub enum TileTrait {
    Walkable,
    ConsoleRepr(char),
    Stairs,
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

    fn stair() -> Tile {
        Tile {
            traits: HashSet::from([
                TileTrait::Stairs,
                TileTrait::Walkable,
                TileTrait::ConsoleRepr('>'),
            ]),
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

fn cardinal_dirs() -> Vec<(UtwidAction, isize, isize)> {
    vec![
        (UtwidAction::N, 0, -1),
        (UtwidAction::S, 0, 1),
        (UtwidAction::E, 1, 0),
        (UtwidAction::W, -1, 0),
    ]
}

fn diagonal_dirs() -> Vec<(UtwidAction, isize, isize)> {
    vec![
        (UtwidAction::NE, 1, -1),
        (UtwidAction::NW, -1, -1),
        (UtwidAction::SE, 1, 1),
        (UtwidAction::SW, -1, 1),
    ]
}

fn apply_dir(x: usize, y: usize, direction: UtwidAction) -> (usize, usize) {
    let (_, rx, ry) = cardinal_dirs()
        .iter()
        .chain(diagonal_dirs().iter())
        .find(|(action, _, _)| action == &direction)
        .unwrap()
        .clone();
    (x - rx as usize, y - ry as usize)
}

impl Board {
    pub fn new(level: usize, rng: &mut ThreadRng) -> Self {
        let width: usize = 11;
        let height: usize = 11;
        let mut geography = vec![Tile::floor(); (width * height) as usize];
        for ix in 5..11 {
            geography[width * 8 + ix] = Tile::wall()
        }
        let stair_location = (rng.random_range(0..width), rng.random_range(0..height));
        geography[stair_location.0 + width * stair_location.1] = Tile::stair({ rng });

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
        cardinal_dirs()
            .iter()
            .filter(|_| cardinal)
            .chain(diagonal_dirs().iter().filter(|_| diagonal))
            .filter_map(|(action, dx, dy)| {
                let x = from_x.checked_add_signed(*dx)?;
                let y = from_y.checked_add_signed(*dy)?;

                self.get(x, y)
                    .traits
                    .contains(&TileTrait::Walkable)
                    .then_some(*action)
            })
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
