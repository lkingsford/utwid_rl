use std::collections::{HashMap, HashSet, VecDeque};

use crate::game::Game;
use crate::mcts::game_trait::{Action, Actor, State};
use crate::mcts::Reward;

use rand::{prelude::*, rngs::SmallRng, SeedableRng};

type ActorId = usize; // If I keep using this code, this might need to be u64, or something else
const CARDINAL_DIRS: [(UtwidAction, isize, isize); 4] = [
    (UtwidAction::N, 0, -1),
    (UtwidAction::S, 0, 1),
    (UtwidAction::E, 1, 0),
    (UtwidAction::W, -1, 0),
];
const DIAGONAL_DIRS: [(UtwidAction, isize, isize); 4] = [
    (UtwidAction::NE, 1, -1),
    (UtwidAction::NW, -1, -1),
    (UtwidAction::SE, 1, 1),
    (UtwidAction::SW, -1, 1),
];

#[derive(Clone, std::fmt::Debug, PartialEq)]
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
    pub turn_order: VecDeque<ActorId>,
    pub turn_number: usize,
    pub short_circuit_at_turns: Option<usize>,
    pub ai_turn_weight: f64,
    pub spawn_rng: SmallRng,
    pub actor_id_counter: ActorId,
}

impl UtwidState {
    pub fn new() -> UtwidState {
        let mut board_rng = SmallRng::from_os_rng();
        let spawn_rng = SmallRng::from_os_rng();
        let board = { Board::new(0, &mut board_rng) };

        UtwidState {
            current_level: 0,
            board: board, // Use the pre-created board
            actors: HashMap::from([(0, GameActor::you_actor())]),
            to_act: 0,
            game_state: GameState::Ongoing,
            turn_number: 0,
            turn_order: VecDeque::from(vec![0]),
            short_circuit_at_turns: None,
            ai_turn_weight: 0.0,
            spawn_rng,
            actor_id_counter: 1,
        }
    }

    // Urgh - I don't know if I should be using an index here...
    pub fn add_actor(&mut self, actor: GameActor) -> ActorId {
        let id = self.actor_id_counter;
        self.actors.insert(id, actor);
        self.actor_id_counter += 1;
        self.turn_order.push_back(id);
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

    fn suggest_spawn(&mut self) -> (usize, usize) {
        let mut result: Option<(usize, usize)> = None;
        while result.is_none() {
            let (x, y) = (
                self.spawn_rng.random_range(0..self.board.width),
                self.spawn_rng.random_range(0..self.board.height),
            );
            result = if self.actor_in_space(x, y).is_some() {
                None
            } else {
                Some((x, y))
            }
        }
        result.unwrap()
    }

    fn actor_in_space(&self, x: usize, y: usize) -> Option<&GameActor> {
        self.actors
            .values()
            .find(|actor| actor.x == x && actor.y == y)
    }

    fn actor_debug_rows(&self) -> Vec<String> {
        let mut rows: Vec<String> = self
            .actors
            .iter()
            .map(|(id, actor)| {
                let label = actor
                    .traits
                    .iter()
                    .find_map(|trait_| match trait_ {
                        ActorTrait::Human => Some("Human".to_string()),
                        ActorTrait::Mon2y {
                            tree_id,
                            iterations,
                        } => Some(format!("Mon2y(tree={},iters={})", tree_id, iterations)),
                        _ => None,
                    })
                    .unwrap_or_else(|| "Other".to_string());
                format!(
                    "id={} pos=({}, {}) repr={:?} label={} dead={} health={:?}",
                    id,
                    actor.x,
                    actor.y,
                    actor.console_repr(),
                    label,
                    actor.traits.contains(&ActorTrait::Dead),
                    actor.traits.iter().find_map(|trait_| match trait_ {
                        ActorTrait::Health(h) => Some(*h),
                        _ => None,
                    }),
                )
            })
            .collect();
        rows.sort();
        rows
    }

    fn debug_summary(&self) -> String {
        format!(
            "level={} state={:?} to_act={} turn_number={} turn_order={:?} actor_ids={:?} actors=[{}]",
            self.current_level,
            self.game_state,
            self.to_act,
            self.turn_number,
            self.turn_order,
            self.actors.keys().copied().collect::<Vec<_>>(),
            self.actor_debug_rows().join(" | "),
        )
    }

    fn normalize_turn_state(&mut self) {
        let before = self.debug_summary();
        self.turn_order.retain(|id| self.actors.contains_key(id));

        if self.turn_order.is_empty() {
            if self.actors.contains_key(&0) {
                self.turn_order.push_back(0);
            } else if let Some(actor_id) = self.actors.keys().min().copied() {
                self.turn_order.push_back(actor_id);
            }
        }

        if !self.actors.contains_key(&self.to_act) {
            if let Some(actor_id) = self.turn_order.front().copied() {
                self.to_act = actor_id;
            }
        }

        if log::log_enabled!(log::Level::Trace) && before != self.debug_summary() {
            log::trace!(
                "normalize_turn_state changed state: before=[{}] after=[{}]",
                before,
                self.debug_summary()
            );
        }
    }
}

impl State for UtwidState {
    type ActionType = UtwidAction;
    type GameHyperrewardType = ();

    fn permitted_actions(&self) -> Vec<Self::ActionType> {
        log::trace!("permitted_actions state {}", self.debug_summary());
        let next_actor = self.actors.get(&self.to_act).unwrap_or_else(|| {
            panic!(
                "Invalid to_act in permitted_actions: {}",
                self.debug_summary()
            )
        });
        self.board.permitted_moves(
            next_actor.x,
            next_actor.y,
            next_actor.traits.contains(&ActorTrait::CardinalMove),
            next_actor.traits.contains(&ActorTrait::DiagonalMove),
        )
    }

    fn next_actor(&self) -> Actor<Self::ActionType> {
        log::trace!("next_actor state {}", self.debug_summary());
        let next_actor = self
            .actors
            .get(&self.to_act)
            .unwrap_or_else(|| panic!("Invalid to_act in next_actor: {}", self.debug_summary()));
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
        let max_actor_id_val = self.mon2y_high_actor_id() as usize;
        let mut rewards = vec![0.0; max_actor_id_val + 1]; // Initialize with zeros

        match self.game_state {
            GameState::Checkpoint => {
                rewards[YOU_ID] =
                    (1.0 + self.current_level as f64 / 20.0) * (1.0 - self.ai_turn_weight);
                for actor in self.actors.values() {
                    if let Some(tree_id) = actor.traits.iter().find_map(|_trait| {
                        if let ActorTrait::Mon2y { tree_id, .. } = _trait {
                            Some(*tree_id as usize)
                        } else {
                            None
                        }
                    }) {
                        rewards[tree_id] = -0.5;
                    }
                }
            }
            GameState::Mon2yShortcircuit => {
                rewards[YOU_ID] =
                    (0.5 + self.current_level as f64 / 20.0) * (1.0 - self.ai_turn_weight);
                for actor in self.actors.values() {
                    if let Some(tree_id) = actor.traits.iter().find_map(|_trait| {
                        if let ActorTrait::Mon2y { tree_id, .. } = _trait {
                            Some(*tree_id as usize)
                        } else {
                            None
                        }
                    }) {
                        rewards[tree_id] = -0.5;
                    }
                }
            }
            GameState::Lost => {
                rewards[YOU_ID] = -1.0;
                for actor in self.actors.values() {
                    if let Some(tree_id) = actor.traits.iter().find_map(|_trait| {
                        if let ActorTrait::Mon2y { tree_id, .. } = _trait {
                            Some(*tree_id as usize)
                        } else {
                            None
                        }
                    }) {
                        rewards[tree_id] = 1.0;
                    }
                }
            }
            GameState::Won => {
                rewards[YOU_ID] = 1.0 - self.ai_turn_weight;
                for actor in self.actors.values() {
                    if let Some(tree_id) = actor.traits.iter().find_map(|_trait| {
                        if let ActorTrait::Mon2y { tree_id, .. } = _trait {
                            Some(*tree_id as usize)
                        } else {
                            None
                        }
                    }) {
                        rewards[tree_id] = -1.0;
                    }
                }
            }
            _ => { /* rewards are already 0.0 */ }
        };
        log::trace!("AI Weight: {}, Reward {:?}", self.ai_turn_weight, rewards);
        rewards
    }

    fn round_hyperreward(&self) -> Self::GameHyperrewardType {
        ()
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

const AI_TURN_WEIGHT: f64 = 1.0 / 200.0;

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
                if new_state.turn_number > i {
                    new_state.game_state = GameState::Mon2yShortcircuit;
                }
            }

            if (new_state.turn_number % 11) == 0 {
                let spawn = new_state.suggest_spawn();
                new_state.add_actor(GameActor::are_actor(spawn.0, spawn.1));
            }
            if (new_state.turn_number % 13) == 0 {
                let spawn = new_state.suggest_spawn();
                new_state.add_actor(GameActor::them_actor(spawn.0, spawn.1));
            }
        }
        if matches!(state.game_state, GameState::Checkpoint)
            && matches!(new_state.game_state, GameState::Checkpoint)
        {
            new_state.game_state = GameState::Ongoing;
        }

        // Bring out yer dead!
        let mut dead_actor_ids: Vec<ActorId> = Vec::new();
        for (actor_id, actor) in new_state
            .actors
            .iter()
            .filter(|actor| actor.1.traits.contains(&ActorTrait::Dead))
        {
            dead_actor_ids.push(*actor_id);
            if actor.traits.contains(&ActorTrait::Human) {
                new_state.game_state = GameState::Lost;
            }
        }

        // Remove dead actors from the actors map
        for actor_id in &dead_actor_ids {
            new_state.actors.remove(actor_id);
        }

        // Filter dead actors from the turn order
        new_state
            .turn_order
            .retain(|id| !dead_actor_ids.contains(id));
        new_state.normalize_turn_state();

        // If the game is already in a terminal state (e.g., player died),
        // we don't need to determine the next actor or update turn order further.
        if new_state.game_state == GameState::Won || new_state.game_state == GameState::Lost {
            return new_state;
        }

        // If the turn order is empty after removing dead actors, game is over.
        if new_state.turn_order.is_empty() {
            new_state.game_state = GameState::Lost; // Or Won, depending on game rules

            return new_state;
        }

        // --- Correct Turn Rotation ---
        let actor_who_acted = state.to_act;

        // Find the position of the actor who acted. They might not be at the front if
        // other actors were removed during this turn.
        if let Some(index) = new_state
            .turn_order
            .iter()
            .position(|&id| id == actor_who_acted)
        {
            // Remove the actor from their current position.
            if let Some(actor_id) = new_state.turn_order.remove(index) {
                // If they are still alive, add them to the back of the queue.
                if new_state.actors.contains_key(&actor_id) {
                    new_state.turn_order.push_back(actor_id);
                }
            }
        }

        // If the turn order is now empty, the game is over.
        if new_state.turn_order.is_empty() {
            new_state.game_state = GameState::Lost;
            return new_state;
        }

        // The new actor to act is at the front of the updated queue.
        new_state.to_act = *new_state.turn_order.front().unwrap();

        new_state
    }
}

impl UtwidAction {
    fn execute_move(&self, state: &UtwidState) -> UtwidState {
        let mut new_state = state.clone();
        let actor_id = new_state.to_act;

        // --- Attack ---
        let (new_coords, damage) = {
            let actor = new_state.actors.get_mut(&actor_id).unwrap();
            (
                apply_dir(actor.x, actor.y, *self),
                actor
                    .traits
                    .iter()
                    .map(|trait_| match trait_ {
                        ActorTrait::Attack { damage } => (*damage).clone() as isize,
                        _ => 0,
                    })
                    .sum::<isize>()
                    * -1,
            )
        };

        for actor in new_state
            .actors
            .iter_mut()
            .map(|actor| actor.1)
            .filter(|actor| actor.x == new_coords.0 && actor.y == new_coords.1)
        {
            actor.modify_health(damage);
        }

        // --- And the rest ---

        if new_state
            .actors
            .iter()
            .map(|actor| actor.1)
            .find(|actor| actor.x == new_coords.0 && actor.y == new_coords.1)
            .is_none()
        {
            let actor = new_state.actors.get_mut(&actor_id).unwrap();
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
        log::debug!("execute_stairs before {}", state.debug_summary());
        let mut new_state = state.clone();
        new_state.current_level = state.current_level + 1;
        new_state.game_state = GameState::Checkpoint;
        let mut board_rng = state.board.rng.clone();
        new_state.board = Board::new(new_state.current_level, &mut board_rng);
        let mut you = state
            .actors
            .get(&0)
            .cloned()
            .unwrap_or_else(GameActor::you_actor);
        you.x = 1;
        you.y = 3;
        you.traits.remove(&ActorTrait::Dead);
        new_state.actors = HashMap::from([(0, you)]);
        new_state.turn_order = VecDeque::from([0]);
        new_state.to_act = 0;
        new_state.actor_id_counter = 1;
        log::debug!("execute_stairs after {}", new_state.debug_summary());
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

fn apply_dir(x: usize, y: usize, direction: UtwidAction) -> (usize, usize) {
    let (_, dx, dy) = CARDINAL_DIRS
        .iter()
        .chain(DIAGONAL_DIRS.iter())
        .find(|(action, _, _)| action == &direction)
        .unwrap()
        .clone();

    // Perform arithmetic with isize to handle negative deltas correctly
    let new_x = x as isize + dx;
    let new_y = y as isize + dy;

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
        geography[stair_location.0 + width * stair_location.1] = if _level < 10 {
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
        CARDINAL_DIRS
            .iter()
            .filter(|_| cardinal)
            .chain(DIAGONAL_DIRS.iter().filter(|_| diagonal))
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
                ActorTrait::Attack { damage: 1 },
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
                ActorTrait::Attack { damage: 1 },
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
                    iterations: 5000,
                },
                ActorTrait::DiagonalMove,
                ActorTrait::Health(2),
                ActorTrait::ConsoleRepr('t'),
                ActorTrait::Attack { damage: 1 },
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
                    iterations: 5000,
                },
                ActorTrait::CardinalMove,
                ActorTrait::Health(2),
                ActorTrait::ConsoleRepr('r'),
                ActorTrait::Attack { damage: 1 },
            ]),
        }
    }

    fn one_actor(x: usize, y: usize) -> GameActor {
        GameActor {
            x,
            y,
            traits: HashSet::from([
                ActorTrait::Mon2y {
                    tree_id: 1,
                    iterations: 5000,
                },
                ActorTrait::CardinalMove,
                ActorTrait::Health(2),
                ActorTrait::ConsoleRepr('r'),
                ActorTrait::Attack { damage: 1 },
            ]),
        }
    }
}

pub struct Utwid;

impl Game for Utwid {
    type StateType = UtwidState;
    type ActionType = UtwidAction;
    type HyperparamsType = ();
    type HyperrewardsType = ();

    fn visualise_state(&self, state: &Self::StateType) {
        for iy in 0..state.board.height {
            for ix in 0..state.board.width {
                let actor_repr = state
                    .actors
                    .values()
                    .find(|actor| actor.x == ix && actor.y == iy)
                    .and_then(|actor| actor.console_repr());
                print!(
                    "{}",
                    actor_repr.unwrap_or_else(|| {
                        state.board.geography[ix + iy * state.board.width]
                            .console_repr()
                            .unwrap_or(' ')
                    })
                );
            }
            println!();
        }
        println!("Turn: {}", state.turn_number);
        println!("State: {:?}", state.game_state);
    }

    fn init_game(&self, _hyperparams: &Self::HyperparamsType) -> Self::StateType {
        UtwidState::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcts::node::create_expanded_node;

    fn stair_location(state: &UtwidState) -> (usize, usize) {
        for y in 0..state.board.height {
            for x in 0..state.board.width {
                if state.board.get(x, y).traits.contains(&TileTrait::Stairs) {
                    return (x, y);
                }
            }
        }
        panic!("Expected stairs on board");
    }

    fn win_location(state: &UtwidState) -> (usize, usize) {
        for y in 0..state.board.height {
            for x in 0..state.board.width {
                if state.board.get(x, y).traits.contains(&TileTrait::Win) {
                    return (x, y);
                }
            }
        }
        panic!("Expected win tile on board");
    }

    fn adjacent_move_to(state: &mut UtwidState, target_x: usize, target_y: usize) -> UtwidAction {
        for (action, dx, dy) in CARDINAL_DIRS.iter().chain(DIAGONAL_DIRS.iter()) {
            let from_x = target_x as isize - dx;
            let from_y = target_y as isize - dy;
            if from_x < 0
                || from_y < 0
                || from_x as usize >= state.board.width
                || from_y as usize >= state.board.height
            {
                continue;
            }
            if state
                .board
                .get(from_x as usize, from_y as usize)
                .traits
                .contains(&TileTrait::Walkable)
            {
                let you = state.actors.get_mut(&0).unwrap();
                you.x = from_x as usize;
                you.y = from_y as usize;
                state.to_act = 0;
                state.turn_order = VecDeque::from([0]);
                return *action;
            }
        }
        panic!("Expected a walkable tile adjacent to target");
    }

    #[test]
    fn stairs_transition_resets_turn_state_across_multiple_floors() {
        let mut state = UtwidState::new();

        for _ in 0..5 {
            state.add_actor(GameActor::are_actor(2, 2));
            state.add_actor(GameActor::them_actor(3, 3));
            state.to_act = 2;

            let (stairs_x, stairs_y) = stair_location(&state);
            let tile = state.board.get(stairs_x, stairs_y).clone();
            let actor = state.actors.get(&0).unwrap().clone();

            state = UtwidAction::N.execute_stairs(&state, &tile, &actor);

            assert_eq!(state.to_act, 0);
            assert_eq!(state.turn_order, VecDeque::from([0]));
            assert_eq!(state.actor_id_counter, 1);
            assert_eq!(state.actors.len(), 1);
            assert!(state.actors.contains_key(&0));
            assert!(matches!(state.next_actor(), Actor::Player(0)));

            state.game_state = GameState::Ongoing;
        }
    }

    #[test]
    fn stairs_transition_clears_monsters_dead_entries_and_stale_turn_ids() {
        let mut state = UtwidState::new();

        for _ in 0..5 {
            let are_id = state.add_actor(GameActor::are_actor(2, 2));
            let them_id = state.add_actor(GameActor::them_actor(3, 3));
            if let Some(actor) = state.actors.get_mut(&are_id) {
                actor.traits.insert(ActorTrait::Dead);
            }
            state.turn_order.push_back(9999);
            state.turn_order.push_back(are_id);
            state.turn_order.push_back(them_id);
            state.to_act = them_id;

            let (stairs_x, stairs_y) = stair_location(&state);
            let tile = state.board.get(stairs_x, stairs_y).clone();
            let actor = state.actors.get(&0).unwrap().clone();

            state = UtwidAction::N.execute_stairs(&state, &tile, &actor);

            assert_eq!(state.to_act, 0);
            assert_eq!(state.turn_order, VecDeque::from([0]));
            assert_eq!(state.actor_id_counter, 1);
            assert_eq!(state.actors.len(), 1);
            assert!(state.actors.contains_key(&0));
            assert!(!state.actors.contains_key(&are_id));
            assert!(!state.actors.contains_key(&them_id));
            assert!(!state.turn_order.contains(&9999));
            assert!(matches!(state.next_actor(), Actor::Player(0)));

            state.game_state = GameState::Ongoing;
        }
    }

    #[test]
    fn execute_path_keeps_to_act_valid_between_floors_and_after_win() {
        let mut state = UtwidState::new();

        for _ in 0..10 {
            state.add_actor(GameActor::are_actor(2, 2));
            state.add_actor(GameActor::them_actor(3, 3));

            let (stairs_x, stairs_y) = stair_location(&state);
            let action = adjacent_move_to(&mut state, stairs_x, stairs_y);
            state = action.execute(&state);

            assert!(state.actors.contains_key(&state.to_act));
            assert!(matches!(state.game_state, GameState::Checkpoint));

            state.game_state = GameState::Ongoing;
            assert!(state.actors.contains_key(&state.to_act));
            assert!(matches!(state.next_actor(), Actor::Player(0)));
        }

        let (win_x, win_y) = win_location(&state);
        let action = adjacent_move_to(&mut state, win_x, win_y);
        state = action.execute(&state);

        assert!(matches!(state.game_state, GameState::Won));
        assert!(state.actors.contains_key(&state.to_act));
        assert!(matches!(state.next_actor(), Actor::Player(0)));
    }

    #[test]
    fn entering_stairs_turn_keeps_to_act_valid_and_terminal_node_safe() {
        let mut state = UtwidState::new();
        let are_id = state.add_actor(GameActor::are_actor(2, 2));
        let them_id = state.add_actor(GameActor::them_actor(3, 3));
        if let Some(actor) = state.actors.get_mut(&are_id) {
            actor.traits.insert(ActorTrait::Dead);
        }
        state.turn_order.push_back(9999);
        state.turn_order.push_back(are_id);
        state.turn_order.push_back(them_id);

        let (stairs_x, stairs_y) = stair_location(&state);
        let action = adjacent_move_to(&mut state, stairs_x, stairs_y);
        let post_stairs = action.execute(&state);

        assert!(matches!(post_stairs.game_state, GameState::Checkpoint));
        assert_eq!(post_stairs.to_act, 0);
        assert_eq!(post_stairs.turn_order, VecDeque::from([0]));
        assert!(post_stairs.actors.contains_key(&post_stairs.to_act));
        assert!(matches!(post_stairs.next_actor(), Actor::Player(0)));

        let node = create_expanded_node(post_stairs.clone(), None);
        match node {
            crate::mcts::node::Node::Expanded { children, .. } => assert!(children.is_empty()),
            crate::mcts::node::Node::Placeholder { .. } => panic!("Expected expanded node"),
        }
    }
}
