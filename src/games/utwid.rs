use std::collections::VecDeque;

use bitflags::bitflags;

use crate::game::Game;
use crate::mcts::Reward;
use crate::mcts::game_trait::{Action, Actor, State};

use rand::{SeedableRng, prelude::*, rngs::SmallRng};

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
    pub actors: Vec<Option<GameActor>>,
    pub to_act: ActorId,
    pub game_state: GameState,
    pub turn_order: VecDeque<ActorId>,
    pub turn_number: usize,
    pub short_circuit_at_turns: Option<usize>,
    pub short_circuit_at_turns_increment: Option<usize>,

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
            actors: vec![Some(GameActor::you_actor())],
            to_act: 0,
            game_state: GameState::Ongoing,
            turn_number: 0,
            turn_order: VecDeque::from(vec![0]),
            short_circuit_at_turns: None,
            short_circuit_at_turns_increment: None,
            ai_turn_weight: 0.0,
            spawn_rng,
            actor_id_counter: 1,
        }
    }

    // Urgh - I don't know if I should be using an index here...
    pub fn add_actor(&mut self, actor: GameActor) -> ActorId {
        let id = self.actor_id_counter;
        self.actors.push(Some(actor));
        self.actor_id_counter += 1;
        self.turn_order.push_back(id);
        id
    }

    fn actor(&self, actor_id: ActorId) -> Option<&GameActor> {
        self.actors.get(actor_id).and_then(Option::as_ref)
    }

    fn actor_mut(&mut self, actor_id: ActorId) -> Option<&mut GameActor> {
        self.actors.get_mut(actor_id).and_then(Option::as_mut)
    }

    fn has_actor(&self, actor_id: ActorId) -> bool {
        self.actor(actor_id).is_some()
    }

    fn remove_actor(&mut self, actor_id: ActorId) -> Option<GameActor> {
        self.actors.get_mut(actor_id).and_then(Option::take)
    }

    fn actors_iter(&self) -> impl Iterator<Item = (ActorId, &GameActor)> {
        self.actors
            .iter()
            .enumerate()
            .filter_map(|(id, actor)| actor.as_ref().map(|actor| (id, actor)))
    }

    fn actors_iter_mut(&mut self) -> impl Iterator<Item = (ActorId, &mut GameActor)> {
        self.actors
            .iter_mut()
            .enumerate()
            .filter_map(|(id, actor)| actor.as_mut().map(|actor| (id, actor)))
    }

    fn actor_count(&self) -> usize {
        self.actors_iter().count()
    }

    pub fn mon2y_high_actor_id(&self) -> u8 {
        self.actors
            .iter()
            .filter_map(|actor| actor.as_ref())
            .map(|actor| actor.mon2y.as_ref().map(|mon2y| mon2y.tree_id).unwrap_or(0))
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
            .iter()
            .filter_map(|actor| actor.as_ref())
            .filter(|actor| !actor.traits.contains(ActorTraits::DEAD))
            .find(|actor| actor.x == x && actor.y == y)
    }

    fn actor_debug_rows(&self) -> Vec<String> {
        let mut rows: Vec<String> = self
            .actors_iter()
            .map(|(id, actor)| {
                let label = if actor.traits.contains(ActorTraits::HUMAN) {
                    Some("Human".to_string())
                } else {
                    actor.mon2y.as_ref().map(|mon2y| {
                        format!("Mon2y(tree={},iters={})", mon2y.tree_id, mon2y.iterations)
                    })
                }
                .unwrap_or_else(|| "Other".to_string());
                format!(
                    "id={} pos=({}, {}) repr={:?} label={} dead={} health={:?}",
                    id,
                    actor.x,
                    actor.y,
                    actor.console_repr(),
                    label,
                    actor.traits.contains(ActorTraits::DEAD),
                    actor.health,
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
            self.actors_iter().map(|(id, _)| id).collect::<Vec<_>>(),
            self.actor_debug_rows().join(" | "),
        )
    }

    fn normalize_turn_state(&mut self) {
        let before = if log::log_enabled!(log::Level::Trace) {
            Some(self.debug_summary())
        } else {
            None
        };
        let live_actor_ids: Vec<_> = self.actors_iter().map(|(id, _)| id).collect();
        self.turn_order.retain(|id| live_actor_ids.contains(id));

        if self.turn_order.is_empty() {
            if self.has_actor(0) {
                self.turn_order.push_back(0);
            } else if let Some(actor_id) = self.actors_iter().map(|(id, _)| id).min() {
                self.turn_order.push_back(actor_id);
            }
        }

        if !self.has_actor(self.to_act) {
            if let Some(actor_id) = self.turn_order.front().copied() {
                self.to_act = actor_id;
            }
        }

        if log::log_enabled!(log::Level::Trace)
            && before.is_some()
            && before.clone().unwrap() != self.debug_summary()
        {
            log::trace!(
                "normalize_turn_state changed state: before=[{}] after=[{}]",
                before.unwrap(),
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
        let next_actor = self.actor(self.to_act).unwrap_or_else(|| {
            panic!(
                "Invalid to_act in permitted_actions: {}",
                self.debug_summary()
            )
        });
        // board permitted doesn't look for actor health interactions
        let board_permitted_moves = self.board.board_permitted_moves(
            next_actor.x,
            next_actor.y,
            next_actor.traits.contains(ActorTraits::CARDINAL_MOVE),
            next_actor.traits.contains(ActorTraits::DIAGONAL_MOVE),
            next_actor.traits.contains(ActorTraits::MELEE),
        );

        board_permitted_moves
            .iter()
            .filter(|action| {
                let (x, y) = apply_dir(next_actor.x, next_actor.y, **action);
                let on_point = self.actor_in_space(x, y);
                if let Some(actor) = on_point {
                    (next_actor.traits.contains(ActorTraits::MELEE)
                        && next_actor.allegiance != actor.allegiance)
                } else {
                    true
                }
            })
            .map(|action_ref| *action_ref)
            .chain(if next_actor.traits.contains(ActorTraits::BOMB) {
                vec![UtwidAction::Explode]
            } else {
                vec![]
            })
            .collect()
    }

    fn next_actor(&self) -> Actor<Self::ActionType> {
        log::trace!("next_actor state {}", self.debug_summary());
        let next_actor = self
            .actor(self.to_act)
            .unwrap_or_else(|| panic!("Invalid to_act in next_actor: {}", self.debug_summary()));
        if next_actor.traits.contains(ActorTraits::HUMAN) {
            Actor::Player(0)
        } else {
            Actor::Player(next_actor.mon2y.as_ref().unwrap().tree_id)
        }
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
                for (_, actor) in self.actors_iter() {
                    if let Some(tree_id) = actor.mon2y.as_ref().map(|mon2y| mon2y.tree_id as usize)
                    {
                        rewards[tree_id] = -0.5;
                    }
                }
                rewards[YOU_ID] =
                    (1.0 + self.current_level as f64 / 20.0) * (1.0 - self.ai_turn_weight);
            }
            GameState::Mon2yShortcircuit => {
                for (_, actor) in self.actors_iter() {
                    if let Some(tree_id) = actor.mon2y.as_ref().map(|mon2y| mon2y.tree_id as usize)
                    {
                        rewards[tree_id] = -0.5;
                    }
                }
                rewards[YOU_ID] =
                    (0.5 + self.current_level as f64 / 20.0) * (1.0 - self.ai_turn_weight);
            }
            GameState::Lost => {
                for (_, actor) in self.actors_iter() {
                    if let Some(tree_id) = actor.mon2y.as_ref().map(|mon2y| mon2y.tree_id as usize)
                    {
                        rewards[tree_id] = 1.0;
                    }
                }
                rewards[YOU_ID] = -1.0 - 1.0 * self.ai_turn_weight;
            }
            GameState::Won => {
                for (_, actor) in self.actors_iter() {
                    if let Some(tree_id) = actor.mon2y.as_ref().map(|mon2y| mon2y.tree_id as usize)
                    {
                        rewards[tree_id] = -1.0;
                    }
                }
                rewards[YOU_ID] = 3.0 * f64::max(0.5, (1.0 - self.ai_turn_weight));
            }
            _ => { /* rewards are already 0.0 */ }
        };
        log::trace!(
            "Game State: {:?}, AI Weight: {}, Reward {:?}",
            self.game_state,
            self.ai_turn_weight,
            rewards
        );
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
    Explode,
}

const AI_TURN_WEIGHT: f64 = 1.0 / 1000.0;

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
            UtwidAction::Explode => self.execute_explode(state),
            _ => unimplemented!(),
        };
        if state
            .actor(state.to_act)
            .unwrap()
            .traits
            .contains(ActorTraits::HUMAN)
        {
            new_state.turn_number += 1;

            new_state.ai_turn_weight += AI_TURN_WEIGHT;
            if let Some(short_circuit_turns_remaining) = new_state.short_circuit_at_turns {
                new_state.short_circuit_at_turns = Some(short_circuit_turns_remaining - 1);
                if short_circuit_turns_remaining == 1 {
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
            .actors_iter()
            .filter(|(_, actor)| actor.traits.contains(ActorTraits::DEAD))
        {
            dead_actor_ids.push(actor_id);
        }
        if dead_actor_ids.iter().any(|actor_id| {
            new_state
                .actor(*actor_id)
                .map(|actor| actor.traits.contains(ActorTraits::HUMAN))
                .unwrap_or(false)
        }) {
            new_state.game_state = GameState::Lost;
        }

        // Remove dead actors from the actors map
        for actor_id in &dead_actor_ids {
            new_state.remove_actor(*actor_id);
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
                if new_state.has_actor(actor_id) {
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
            let actor = new_state.actor(actor_id).unwrap();
            (
                apply_dir(actor.x, actor.y, *self),
                actor.attack_damage.unwrap_or(0) as isize * -1,
            )
        };

        for (_, actor) in new_state
            .actors_iter_mut()
            .filter(|(_, actor)| actor.x == new_coords.0 && actor.y == new_coords.1)
        {
            actor.modify_health(damage);
        }

        let tile = new_state.board.get_mut(new_coords.0, new_coords.1);
        if tile.health.is_some() {
            tile.modify_health(damage);
        }

        // --- And the rest ---

        if new_state
            .actors_iter()
            .map(|(_, actor)| actor)
            .find(|actor| actor.x == new_coords.0 && actor.y == new_coords.1)
            .is_none()
            && new_state
                .board
                .get(new_coords.0, new_coords.1)
                .traits
                .contains(TileTraits::WALKABLE)
        {
            let actor = new_state.actor_mut(actor_id).unwrap();
            (actor.x, actor.y) = new_coords;
        }

        let actor_ref = new_state.actor(actor_id).unwrap();

        if actor_ref.traits.contains(ActorTraits::HUMAN) {
            let tile = new_state.board.get(actor_ref.x, actor_ref.y);
            if tile.traits.contains(TileTraits::STAIRS) {
                self.execute_stairs(&new_state)
            } else if tile.traits.contains(TileTraits::WIN) {
                self.execute_win(&new_state)
            } else {
                new_state
            }
        } else {
            new_state
        }
    }

    fn execute_stairs(&self, state: &UtwidState) -> UtwidState {
        log::debug!("execute_stairs before {}", state.debug_summary());
        let mut new_state = state.clone();
        new_state.current_level = state.current_level + 1;
        new_state.game_state = GameState::Checkpoint;
        let mut board_rng = state.board.rng.clone();
        new_state.board = Board::new(new_state.current_level, &mut board_rng);
        let mut you = state.actor(0).cloned().unwrap_or_else(GameActor::you_actor);
        you.x = 1;
        you.y = 3;
        you.traits.remove(ActorTraits::DEAD);
        new_state.actors = vec![Some(you)];
        new_state.turn_order = VecDeque::from([0]);
        new_state.to_act = 0;
        new_state.actor_id_counter = 1;
        if let Some(current_short_circuit) = new_state.short_circuit_at_turns {
            if let Some(increment) = new_state.short_circuit_at_turns_increment {
                new_state.short_circuit_at_turns = Some(current_short_circuit + increment);
            }
        };
        log::debug!("execute_stairs after {}", new_state.debug_summary());
        new_state
    }

    fn execute_win(&self, state: &UtwidState) -> UtwidState {
        let mut new_state = state.clone();
        new_state.game_state = GameState::Won;
        new_state
    }

    fn execute_explode(&self, state: &UtwidState) -> UtwidState {
        log::debug!("execute explode");
        let mut new_state = state.clone();
        new_state
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct TileTraits: u8 {
        const WALKABLE = 1 << 0;
        const STAIRS = 1 << 1;
        const WIN = 1 << 2;
    }
}

#[derive(Clone)]
pub struct Tile {
    traits: TileTraits,
    health: Option<isize>,
    pub console_repr: Option<char>,
}

impl Tile {
    fn floor() -> Tile {
        Tile {
            traits: TileTraits::WALKABLE,
            console_repr: Some('.'),
            health: None,
        }
    }

    fn wall() -> Tile {
        Tile {
            traits: TileTraits::empty(),
            console_repr: Some('#'),
            health: Some(5),
        }
    }

    fn stair() -> Tile {
        Tile {
            traits: TileTraits::STAIRS | TileTraits::WALKABLE,
            console_repr: Some('>'),
            health: None,
        }
    }

    fn win() -> Tile {
        Tile {
            traits: TileTraits::WALKABLE | TileTraits::WIN,
            console_repr: Some('W'),
            health: None,
        }
    }

    pub fn console_repr(&self) -> Option<char> {
        self.console_repr
    }

    pub fn modify_health(&mut self, dhealth: isize) {
        if self.health.is_some() {
            self.health = Some(self.health.unwrap() + dhealth);

            if self.health.unwrap() <= 0 {
                self.traits = TileTraits::WALKABLE;
                self.console_repr = Some('.');
                self.health = None;
            }
        }
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

    // These should always be non-negative due to prior filtering by board_permitted_moves
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
        geography[stair_location.0 + width * stair_location.1] = if _level < 9 {
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

    fn get_mut(&mut self, x: usize, y: usize) -> &mut Tile {
        &mut self.geography[self.width * y + x]
    }

    fn board_permitted_moves(
        &self,
        from_x: usize,
        from_y: usize,
        cardinal: bool,
        diagonal: bool,
        melee: bool,
    ) -> Vec<UtwidAction> {
        CARDINAL_DIRS
            .iter()
            .filter(|_| cardinal)
            .chain(DIAGONAL_DIRS.iter().filter(|_| diagonal))
            .filter_map(|(action, dx, dy)| {
                let x = from_x as isize + *dx as isize;
                let y = from_y as isize + *dy as isize;

                if x >= 0 && (x as usize) < self.width && y >= 0 && (y as usize) < self.height {
                    let tile = self.get(x as usize, y as usize);
                    (tile.traits.contains(TileTraits::WALKABLE) || tile.health.is_some())
                        .then_some(*action)
                } else {
                    None
                }
            })
            .collect()
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ActorTraits: u16 {
        const HUMAN = 1 << 0;
        const MON2Y = 1 << 1;
        const CARDINAL_MOVE = 1 << 2;
        const DIAGONAL_MOVE = 1 << 3;
        const WAIT = 1 << 4;
        const DEAD = 1 << 5;
        const MELEE = 1 << 6;
        const BOMB = 1 << 7;
    }
}

#[derive(Clone, PartialEq, PartialOrd)]
pub enum Allegiance {
    You,
    Monty,
}

#[derive(Clone)]
pub struct Mon2yData {
    pub tree_id: u8,
    pub iterations: usize,
}

#[derive(Clone)]
pub struct GameActor {
    pub x: usize,
    pub y: usize,
    pub traits: ActorTraits,
    pub mon2y: Option<Mon2yData>,
    pub console_repr: Option<char>,
    pub health: Option<usize>,
    pub attack_damage: Option<usize>,
    pub allegiance: Allegiance,
}

impl GameActor {
    pub fn console_repr(&self) -> Option<char> {
        self.console_repr
    }

    pub fn modify_health(&mut self, d_health: isize) -> () {
        let current_health = self.health.unwrap_or(0);
        let new_health = (current_health as isize + d_health).max(0) as usize;
        self.health = Some(new_health);

        if new_health <= 0 {
            self.traits.insert(ActorTraits::DEAD);
        }
    }
}

impl GameActor {
    // Feels logical that these should be seperate
    fn you_actor() -> GameActor {
        GameActor {
            x: 1,
            y: 3,
            traits: ActorTraits::HUMAN
                | ActorTraits::CARDINAL_MOVE
                | ActorTraits::DIAGONAL_MOVE
                | ActorTraits::MELEE,
            mon2y: None,
            console_repr: Some('@'),
            health: Some(7),
            attack_damage: Some(1),
            allegiance: Allegiance::You,
        }
    }

    fn monte_actor() -> GameActor {
        GameActor {
            x: 7,
            y: 7,
            traits: ActorTraits::MON2Y
                | ActorTraits::CARDINAL_MOVE
                | ActorTraits::DIAGONAL_MOVE
                | ActorTraits::WAIT
                | ActorTraits::MELEE,
            mon2y: Some(Mon2yData {
                tree_id: 1,
                iterations: 1000,
            }),
            console_repr: Some('&'),
            health: Some(7),
            attack_damage: Some(1),
            allegiance: Allegiance::Monty,
        }
    }

    fn them_actor(x: usize, y: usize) -> GameActor {
        GameActor {
            x,
            y,
            traits: ActorTraits::MON2Y | ActorTraits::DIAGONAL_MOVE | ActorTraits::MELEE,
            mon2y: Some(Mon2yData {
                tree_id: 1,
                iterations: 1000,
            }),
            console_repr: Some('t'),
            health: Some(2),
            attack_damage: Some(1),
            allegiance: Allegiance::Monty,
        }
    }

    fn are_actor(x: usize, y: usize) -> GameActor {
        GameActor {
            x,
            y,
            traits: ActorTraits::MON2Y | ActorTraits::CARDINAL_MOVE | ActorTraits::MELEE,
            mon2y: Some(Mon2yData {
                tree_id: 1,
                iterations: 1000,
            }),
            console_repr: Some('r'),
            health: Some(2),
            attack_damage: Some(1),
            allegiance: Allegiance::Monty,
        }
    }

    fn one_actor(x: usize, y: usize) -> GameActor {
        GameActor {
            x,
            y,
            traits: ActorTraits::MON2Y | ActorTraits::CARDINAL_MOVE | ActorTraits::BOMB,
            mon2y: Some(Mon2yData {
                tree_id: 1,
                iterations: 1000,
            }),
            console_repr: Some('1'),
            health: Some(4),
            attack_damage: Some(5),
            allegiance: Allegiance::Monty,
        }
    }
}

pub struct Utwid;

impl Game for Utwid {
    type StateType = UtwidState;
    type ActionType = UtwidAction;
    type HyperrewardsType = ();

    fn visualise_state(&self, state: &Self::StateType) {
        for iy in 0..state.board.height {
            for ix in 0..state.board.width {
                let actor_repr = state
                    .actors_iter()
                    .map(|(_, actor)| actor)
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

    fn init_game(&self) -> Self::StateType {
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
                if state.board.get(x, y).traits.contains(TileTraits::STAIRS) {
                    return (x, y);
                }
            }
        }
        panic!("Expected stairs on board");
    }

    fn win_location(state: &UtwidState) -> (usize, usize) {
        for y in 0..state.board.height {
            for x in 0..state.board.width {
                if state.board.get(x, y).traits.contains(TileTraits::WIN) {
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
                .contains(TileTraits::WALKABLE)
            {
                let you = state.actor_mut(0).unwrap();
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
            state = UtwidAction::N.execute_stairs(&state);

            assert_eq!(state.to_act, 0);
            assert_eq!(state.turn_order, VecDeque::from([0]));
            assert_eq!(state.actor_id_counter, 1);
            assert_eq!(state.actor_count(), 1);
            assert!(state.has_actor(0));
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
            if let Some(actor) = state.actor_mut(are_id) {
                actor.traits.insert(ActorTraits::DEAD);
            }
            state.turn_order.push_back(9999);
            state.turn_order.push_back(are_id);
            state.turn_order.push_back(them_id);
            state.to_act = them_id;

            let (stairs_x, stairs_y) = stair_location(&state);
            state = UtwidAction::N.execute_stairs(&state);

            assert_eq!(state.to_act, 0);
            assert_eq!(state.turn_order, VecDeque::from([0]));
            assert_eq!(state.actor_id_counter, 1);
            assert_eq!(state.actor_count(), 1);
            assert!(state.has_actor(0));
            assert!(!state.has_actor(are_id));
            assert!(!state.has_actor(them_id));
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

            assert!(state.has_actor(state.to_act));
            assert!(matches!(state.game_state, GameState::Checkpoint));

            state.game_state = GameState::Ongoing;
            assert!(state.has_actor(state.to_act));
            assert!(matches!(state.next_actor(), Actor::Player(0)));
        }

        let (win_x, win_y) = win_location(&state);
        let action = adjacent_move_to(&mut state, win_x, win_y);
        state = action.execute(&state);

        assert!(matches!(state.game_state, GameState::Won));
        assert!(state.has_actor(state.to_act));
        assert!(matches!(state.next_actor(), Actor::Player(0)));
    }

    #[test]
    fn entering_stairs_turn_keeps_to_act_valid_and_terminal_node_safe() {
        let mut state = UtwidState::new();
        let are_id = state.add_actor(GameActor::are_actor(2, 2));
        let them_id = state.add_actor(GameActor::them_actor(3, 3));
        if let Some(actor) = state.actor_mut(are_id) {
            actor.traits.insert(ActorTraits::DEAD);
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
        assert!(post_stairs.has_actor(post_stairs.to_act));
        assert!(matches!(post_stairs.next_actor(), Actor::Player(0)));

        let node = create_expanded_node(post_stairs.clone(), None);
        match node {
            crate::mcts::node::Node::Expanded { children, .. } => assert!(children.is_empty()),
            crate::mcts::node::Node::Placeholder { .. } => panic!("Expected expanded node"),
        }
    }
}
