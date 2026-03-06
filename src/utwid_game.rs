use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::mon2y::game::{Action, Actor, State};
use crate::mon2y::Reward;

use rand::prelude::*;
use rand::rngs::Xoshiro256PlusPlus;

type ActorId = usize; // If I keep using this code, this might need to be u64, or something else

#[derive(Clone, std::fmt::Debug)]
pub enum GameState {
    Ongoing,
    Won,
    Lost,
    Checkpoint,
    Mon2yShortcircuit,
}

const YOU_ID: usize = 0;

#[derive(Clone)]
pub struct UtwidState {
    pub current_level: usize,
    pub board: Board,
    pub actors: HashMap<ActorId, GameActor>,
    pub to_act: ActorId,
    pub game_state: GameState,
    pub turn_order: Vec<ActorId>,
    pub turn_number: usize,
    pub short_circuit_at_turns: Option<usize>,
    pub ai_turn_weight: f64,
}

impl UtwidState {
    pub fn new() -> UtwidState {
        let mut rng = rand::make_rng();
        let board = { Board::new(0, &mut rng) };

        UtwidState {
            current_level: 0,
            board: board, // Use the pre-created board
            actors: HashMap::from([(0, GameActor::you_actor())]),
            to_act: 0,
            game_state: GameState::Ongoing,
            turn_number: 0,
            turn_order: vec![0],
            short_circuit_at_turns: None,
            ai_turn_weight: 0.0,
        }
    }

    // Urgh - I don't know if I should be using an index here...
    pub fn add_actor(&mut self, actor: GameActor) -> ActorId {
        self.actors.insert(self.actors.len(), actor);
        let id = self.actors.len() - 1;
        self.turn_order.push(id);
        id
    }

    pub fn mon2y_high_actor_id(&self) -> u8 {
        self.actors
            .iter()
            .map(|actor| {
                actor
                    .1
                    .traits
                    .iter()
                    .map(|_trait| match _trait {
                        ActorTrait::Mon2y {
                            tree_id,
                            iterations,
                        } => tree_id.clone(),
                        _ => 0,
                    })
                    .max()
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0)
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
        let next_actor = self.actors.get(&self.to_act).unwrap();
        next_actor
            .traits
            .iter()
            .find_map(|_trait| match _trait {
                ActorTrait::Human => Some(Actor::Player(0)),
                ActorTrait::Mon2y {
                    tree_id,
                    iterations,
                } => Some(Actor::Player(*tree_id)),
                _ => None,
            })
            .unwrap()
    }

    fn terminal(&self) -> bool {
        match self.game_state {
            GameState::Ongoing => false,
            GameState::Checkpoint => true,
            _ => true,
        }
    }

    fn reward(&self) -> Vec<Reward> {
        let max_actor_id = self.mon2y_high_actor_id() as usize;

        let reward = match self.game_state {
            GameState::Checkpoint => [
                vec![(1.0 + self.current_level as f64 / 20.0) * (1.0 - self.ai_turn_weight)],
                vec![-0.5; max_actor_id],
            ]
            .concat(),
            GameState::Mon2yShortcircuit => [
                vec![(0.5 + self.current_level as f64 / 20.0) * (1.0 - self.ai_turn_weight)],
                vec![-0.5; max_actor_id],
            ]
            .concat(),
            GameState::Lost => [vec![-1.0], vec![1.0; max_actor_id]].concat(),
            GameState::Won => [vec![1.0 - self.ai_turn_weight], vec![-1.0; max_actor_id]].concat(),
            _ => vec![0.0; max_actor_id + 1],
        };
        log::trace!("AI Weight: {}, Reward {:?}", self.ai_turn_weight, reward);
        reward
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

const AI_TURN_WEIGHT: f64 = 1.0 / 100.0;

impl Action for UtwidAction {
    type StateType = UtwidState;

    fn execute(&self, state: &Self::StateType) -> Self::StateType {
        let mut new_state = match self {
            UtwidAction::N
            | UtwidAction::S
            | UtwidAction::E
            | UtwidAction::W
            | UtwidAction::NE
            | UtwidAction::NW
            | UtwidAction::SE
            | UtwidAction::SW => self.execute_move(state),
            UtwidAction::Wait => state.clone(),
            _ => unimplemented!(),
        };
        if state
            .actors
            .get(&state.to_act)
            .unwrap()
            .traits
            .contains(&ActorTrait::Human)
        {
            new_state.turn_number += 1;
            new_state.ai_turn_weight += AI_TURN_WEIGHT;
            if let Some(i) = new_state.short_circuit_at_turns {
                if i > new_state.turn_number {
                    new_state.game_state = GameState::Mon2yShortcircuit;
                }
            }
        }
        if matches!(state.game_state, GameState::Checkpoint)
            && matches!(new_state.game_state, GameState::Checkpoint)
        {
            new_state.game_state = GameState::Ongoing;
        }
        new_state.to_act = new_state.turn_order.pop().unwrap();
        new_state.turn_order.push(state.to_act);
        new_state
    }
}

impl UtwidAction {
    fn execute_move(&self, state: &UtwidState) -> UtwidState {
        let mut new_state = state.clone();
        let actor_id = new_state.to_act;

        let actor = new_state.actors.get_mut(&actor_id).unwrap();
        let new_coords = apply_dir(actor.x, actor.y, *self);

        if state
            .actors
            .iter()
            .map(|actor| actor.1)
            .find(|actor| actor.x == new_coords.0 && actor.y == new_coords.1)
            .is_some()
        {
            // TODO: Attack, if possible
        } else {
            (actor.x, actor.y) = new_coords;
        }

        let actor_ref = new_state.actors.get(&actor_id).unwrap();

        if actor_ref.traits.contains(&ActorTrait::Human) {
            let tile = new_state.board.get(actor_ref.x, actor_ref.y);

            tile.traits
                .iter()
                .find_map(|trait_| match trait_ {
                    TileTrait::Stairs => Some(self.execute_stairs(&new_state, &tile, actor_ref)),
                    TileTrait::Win => Some(self.execute_win(&new_state)),
                    _ => None,
                })
                .unwrap_or(new_state)
        } else {
            new_state
        }
    }

    fn execute_stairs(&self, state: &UtwidState, _tile: &Tile, _to_act: &GameActor) -> UtwidState {
        let mut new_state = state.clone();
        new_state.game_state = GameState::Checkpoint;
        new_state.board = Board::new(state.current_level + 1, &mut state.board.rng.clone());
        let actor = new_state.actors.get(&0).unwrap();
        new_state
    }

    fn execute_win(&self, state: &UtwidState) -> UtwidState {
        let mut new_state = state.clone();
        new_state.game_state = GameState::Won;
        new_state
    }
}

#[derive(Clone, PartialEq, PartialOrd, Eq, Hash)]
pub enum TileTrait {
    Walkable,
    ConsoleRepr(char),
    Stairs,
    Win,
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
            traits: HashSet::from([TileTrait::ConsoleRepr('#')]),
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

    fn win() -> Tile {
        Tile {
            traits: HashSet::from([TileTrait::ConsoleRepr('W'), TileTrait::Win]),
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
    pub rng: SmallRng,
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
    let (_, dx, dy) = cardinal_dirs()
        .iter()
        .chain(diagonal_dirs().iter())
        .find(|(action, _, _)| action == &direction)
        .unwrap()
        .clone();

    // Perform arithmetic with isize to handle negative deltas correctly
    let new_x = (x as isize + dx);
    let new_y = (y as isize + dy);

    // These should always be non-negative due to prior filtering by permitted_moves
    (new_x as usize, new_y as usize)
}

impl Board {
    pub fn new(_level: usize, rng: &mut SmallRng) -> Self {
        let width: usize = 11;
        let height: usize = 11;
        let mut geography = vec![Tile::floor(); (width * height) as usize];
        for ix in 5..11 {
            geography[width * 8 + ix] = Tile::wall()
        }
        let stair_location = (rng.random_range(0..width), rng.random_range(0..height));
        geography[stair_location.0 + width * stair_location.1] = if (_level < 10) {
            Tile::stair()
        } else {
            Tile::win()
        };

        let rng = rng.clone();
        Board {
            geography,
            width,
            height,
            rng,
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
                let x = from_x as isize + *dx as isize;
                let y = from_y as isize + *dy as isize;

                if x >= 0 && (x as usize) < self.width && y >= 0 && (y as usize) < self.height {
                    self.get(x as usize, y as usize)
                        .traits
                        .contains(&TileTrait::Walkable)
                        .then_some(*action)
                } else {
                    None
                }
            })
            .collect()
    }
}

#[derive(Clone, PartialEq, PartialOrd, Eq, Hash)]
pub enum ActorTrait {
    Human,
    Mon2y { tree_id: u8, iterations: usize },
    CardinalMove,
    DiagonalMove,
    Wait,
    ConsoleRepr(char),
    Health(usize),
    Dead,
    Attack { damage: usize },
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

    pub fn modify_health(&mut self, d_health: isize) -> () {
        let current_health = self
            .traits
            .iter()
            .find_map(|t| match t {
                ActorTrait::Health(h) => Some(*h),
                _ => None,
            })
            .unwrap_or(0); // Default to 0 if no health trait is found

        // Remove the old health trait
        self.traits.retain(|t| !matches!(t, ActorTrait::Health(_)));

        // Add the new health trait
        let new_health = (current_health as isize + d_health).max(0) as usize;
        self.traits.insert(ActorTrait::Health(new_health));

        if new_health <= 0 {
            self.traits.insert(ActorTrait::Dead);
        }
    }
}

impl GameActor {
    // Feels logical that these should be seperate
    fn you_actor() -> GameActor {
        GameActor {
            x: 1,
            y: 3,
            traits: HashSet::from([
                ActorTrait::ConsoleRepr('@'),
                ActorTrait::Human,
                ActorTrait::CardinalMove,
                ActorTrait::DiagonalMove,
                ActorTrait::Health(7),
            ]),
        }
    }

    fn monte_actor() -> GameActor {
        GameActor {
            x: 7,
            y: 7,
            traits: HashSet::from([
                ActorTrait::ConsoleRepr('&'),
                ActorTrait::Mon2y {
                    tree_id: 1,
                    iterations: 1000,
                },
                ActorTrait::CardinalMove,
                ActorTrait::DiagonalMove,
                ActorTrait::Wait,
                ActorTrait::Health(7),
            ]),
        }
    }

    fn them_actor(x: usize, y: usize) -> GameActor {
        GameActor {
            x,
            y,
            traits: HashSet::from([
                ActorTrait::Mon2y {
                    tree_id: 1,
                    iterations: 100,
                },
                ActorTrait::DiagonalMove,
                ActorTrait::Health(2),
                ActorTrait::ConsoleRepr('t'),
            ]),
        }
    }

    fn are_actor(x: usize, y: usize) -> GameActor {
        GameActor {
            x,
            y,
            traits: HashSet::from([
                ActorTrait::Mon2y {
                    tree_id: 1,
                    iterations: 100,
                },
                ActorTrait::CardinalMove,
                ActorTrait::Health(2),
                ActorTrait::ConsoleRepr('t'),
            ]),
        }
    }
}
