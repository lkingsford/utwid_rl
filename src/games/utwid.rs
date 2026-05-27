use std::cmp::{max, min};
use std::collections::{HashMap, HashSet, VecDeque};

use bitflags::bitflags;
use lazy_static::lazy_static;

use crate::game::Game;
use crate::mcts::Reward;
use crate::mcts::game_trait::{Action, Actor, State};

use rand::{SeedableRng, prelude::*, rngs::SmallRng};

type ActorId = usize; // If I keep using this code, this might need to be u64, or something else
const CARDINAL_DIRS: [(Dir, isize, isize); 4] = [
    (Dir::N, 0, -1),
    (Dir::S, 0, 1),
    (Dir::E, 1, 0),
    (Dir::W, -1, 0),
];
const DIAGONAL_DIRS: [(Dir, isize, isize); 4] = [
    (Dir::NE, 1, -1),
    (Dir::NW, -1, -1),
    (Dir::SE, 1, 1),
    (Dir::SW, -1, 1),
];
const ACTOR_TYPE_NAMES: [&str; 5] = ["you", "monte", "them", "are", "one"];
const ACTOR_TYPE_YOU: usize = 0;
const ACTOR_TYPE_THEM: usize = 2;
const ACTOR_TYPE_ARE: usize = 3;
const ACTOR_TYPE_ONE: usize = 4;
const MON2Y_ID: usize = 1;
const PLAYER_MAX_HEALTH: usize = 7;
const ROOM_SPLITS_MIN: usize = 2;
const ROOM_SPLITS_MAX: usize = 8;
const PRESCRIPTION_TURNS: usize = 5;

#[derive(Clone, std::fmt::Debug, PartialEq)]
pub enum GameState {
    Ongoing,
    Won,
    Lost,
    Checkpoint,
    Mon2yShortcircuit,
}

const YOU_ID: usize = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Repr {
    Floor,
    Wall,
    Stairs,
    Win,
    You,
    Monte,
    Them,
    Are,
    One,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReprSet {
    Room1,
    Room2,
    Room3,
    Room4,
    Room5,
    Room6,
    Room7,
}

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
    pub witnessed_you_actions: WitnessedYouActions,
    pub prescription_turns: Option<usize>,
    pub temporary_damage_bonus: Option<usize>,

    pub ai_turn_weight: f64,
    pub spawn_rng: SmallRng,
    pub actor_id_counter: ActorId,
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct WitnessedYouActions: u8 {
    }
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
            witnessed_you_actions: WitnessedYouActions::empty(),
            prescription_turns: None,
            temporary_damage_bonus: None,
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

    fn reward_actor_count(&self) -> usize {
        usize::max(self.mon2y_high_actor_id() as usize, MON2Y_ID) + 1
    }

    fn player_health_ratio(&self) -> f64 {
        let health = self
            .actor(YOU_ID)
            .and_then(|actor| actor.health)
            .unwrap_or(0) as f64;
        health / PLAYER_MAX_HEALTH as f64
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

    fn actor_id_in_space(&self, x: usize, y: usize) -> Option<ActorId> {
        self.actors_iter()
            .find(|(_, actor)| {
                !actor.traits.contains(ActorTraits::DEAD) && actor.x == x && actor.y == y
            })
            .map(|(id, _)| id)
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
                    "id={} type={}({}) pos=({}, {}) repr={:?} label={} dead={} health={:?}",
                    id,
                    actor.actor_type,
                    actor.actor_type_name(),
                    actor.x,
                    actor.y,
                    actor.repr(),
                    label,
                    actor.traits.contains(ActorTraits::DEAD),
                    actor.health,
                )
            })
            .collect();
        rows.sort();
        rows
    }

    fn neighboring_tile_rows(&self, x: usize, y: usize) -> Vec<String> {
        CARDINAL_DIRS
            .iter()
            .chain(DIAGONAL_DIRS.iter())
            .map(|(action, dx, dy)| {
                let target_x = x as isize + dx;
                let target_y = y as isize + dy;

                if target_x < 0
                    || target_y < 0
                    || target_x as usize >= self.board.width
                    || target_y as usize >= self.board.height
                {
                    return format!("{action:?}->out_of_bounds");
                }

                let target_x = target_x as usize;
                let target_y = target_y as usize;
                let tile = self.board.get(target_x, target_y);
                let occupant = self
                    .actor_id_in_space(target_x, target_y)
                    .and_then(|actor_id| {
                        self.actor(actor_id).map(|actor| {
                            format!(
                                "id={} type={}({}) repr={:?} allegiance={:?} dead={}",
                                actor_id,
                                actor.actor_type,
                                actor.actor_type_name(),
                                actor.repr(),
                                actor.allegiance,
                                actor.traits.contains(ActorTraits::DEAD),
                            )
                        })
                    });

                format!(
                    "{action:?}->({}, {}) walkable={} tile_health={:?} tile_repr={:?} occupant={}",
                    target_x,
                    target_y,
                    tile.traits.contains(TileTraits::WALKABLE),
                    tile.health,
                    tile.repr(),
                    occupant.unwrap_or_else(|| "none".to_string()),
                )
            })
            .collect()
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

    fn permitted_actions(&self, _per: Option<u8>) -> Vec<Self::ActionType> {
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

        let is_you = next_actor.actor_type == ACTOR_TYPE_YOU;

        let mut permitted_actions: Vec<_> = board_permitted_moves
            .iter()
            .copied()
            .filter(|direction| {
                let (x, y) = apply_dir(next_actor.x, next_actor.y, *direction);
                let on_point = self.actor_in_space(x, y);
                if let Some(actor) = on_point {
                    next_actor.traits.contains(ActorTraits::MELEE)
                        && next_actor.allegiance != actor.allegiance
                } else {
                    true
                }
            })
            .map(UtwidAction::Move)
            .chain(
                next_actor
                    .traits
                    .contains(ActorTraits::BOMB)
                    .then_some(UtwidAction::Explode),
            )
            .collect();

        if is_you {
            permitted_actions.extend(
                board_permitted_moves
                    .iter()
                    .copied()
                    .map(UtwidAction::Conclusion),
            );
            let stagnation_moves = CARDINAL_DIRS
                .iter()
                .filter_map(|(direction, _, _)| match direction {
                    Dir::N if next_actor.y > 0 => Some(*direction),
                    Dir::S if next_actor.y + 1 < self.board.height => Some(*direction),
                    Dir::E if next_actor.x + 1 < self.board.width => Some(*direction),
                    Dir::W if next_actor.x > 0 => Some(*direction),
                    _ => None,
                })
                .map(UtwidAction::Stagnation);
            permitted_actions.extend(stagnation_moves);
            permitted_actions.push(UtwidAction::Prescription);
        }

        if permitted_actions.is_empty() {
            let permitted_actions = vec![UtwidAction::Wait];
            log::trace!(
                "permitted_actions output actor_id={} actor_type={} actions={:?}",
                self.to_act,
                next_actor.actor_type_name(),
                permitted_actions
            );
            return permitted_actions;
        }

        log::trace!(
            "permitted_actions output actor_id={} actor_type={} actions={:?}",
            self.to_act,
            next_actor.actor_type_name(),
            permitted_actions
        );
        permitted_actions
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
        let mut rewards = vec![0.0; self.reward_actor_count()];

        match self.game_state {
            GameState::Checkpoint => {
                let player_health_ratio = self.player_health_ratio();
                rewards[YOU_ID] = player_health_ratio;
                rewards[MON2Y_ID] = -player_health_ratio;
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
                rewards[YOU_ID] = -1.0;
                rewards[MON2Y_ID] = 1.0;
            }
            GameState::Won => {
                rewards[YOU_ID] = 1.0;
                rewards[MON2Y_ID] = -1.0;
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
pub enum Dir {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum UtwidAction {
    NoAction,
    Move(Dir),
    Wait,
    Explode,

    Conclusion(Dir),    // Jump to a position
    Redemption,         // Jump through a line of actors, injuring all
    Contemplation(Dir), // Hit adjacent into wall
    Stagnation(Dir),    // Create a wall
    Prescription,       // Take multiple moves in a row
    Attention,          // Pull a whole direction closer
    Demonstration,      // Play two timelines at once
    Contention(Dir),    // Glitch the map
    Assumption,         // Take over a person
}

pub fn action_cost(action: UtwidAction) -> usize {
    match action {
        UtwidAction::Conclusion(_) => 1,
        UtwidAction::Redemption => 1,
        UtwidAction::Attention => 1,
        UtwidAction::Stagnation(_) => 2,
        UtwidAction::Prescription => 2,
        UtwidAction::Contemplation(_) => 2,
        UtwidAction::Demonstration => 3,
        UtwidAction::Contention(_) => 0,
        UtwidAction::Assumption => 3,
        _ => 0,
    }
}

const AI_TURN_WEIGHT: f64 = 1.0 / 1000.0;

impl Action for UtwidAction {
    type StateType = UtwidState;

    fn execute(&self, state: &Self::StateType) -> Self::StateType {
        let mut new_state = match self {
            UtwidAction::Move(_) => self.execute_move(state),
            UtwidAction::Wait => state.clone(),
            UtwidAction::Explode => self.execute_explode(state),
            UtwidAction::Prescription => {
                let mut new_state = state.clone();
                new_state.prescription_turns = Some(PRESCRIPTION_TURNS);
                new_state
            }
            UtwidAction::Conclusion(_) => self.execute_conclusion(state),
            UtwidAction::Stagnation(_) => self.execute_stagnation(state),
            UtwidAction::Contention(_) => self.execute_contention(state),
            _ => unimplemented!(),
        };

        new_state
            .actor_mut(state.to_act)
            .unwrap()
            .modify_health(-1 * action_cost(*self) as isize);

        if state
            .actor(state.to_act)
            .unwrap()
            .traits
            .contains(ActorTraits::HUMAN)
        {
            if let Some(_prescription_turns) = new_state.prescription_turns {
                new_state.prescription_turns = if _prescription_turns > 0 {
                    Some(_prescription_turns - 1)
                } else {
                    None
                }
            } else {
                new_state.turn_number += 1;

                if (new_state.turn_number % 9) == 0 {
                    let spawn = new_state.suggest_spawn();
                    new_state.add_actor(GameActor::are_actor(spawn.0, spawn.1));
                }
                if (new_state.turn_number % 13) == 0 {
                    let spawn = new_state.suggest_spawn();
                    new_state.add_actor(GameActor::them_actor(spawn.0, spawn.1));
                }
                if (new_state.turn_number % 5) == 0 {
                    let spawn = new_state.suggest_spawn();
                    new_state.add_actor(GameActor::one_actor(spawn.0, spawn.1));
                }
            }

            new_state.ai_turn_weight += AI_TURN_WEIGHT;
            if let Some(short_circuit_turns_remaining) = new_state.short_circuit_at_turns {
                new_state.short_circuit_at_turns = Some(short_circuit_turns_remaining - 1);
                if short_circuit_turns_remaining == 1 {
                    new_state.game_state = GameState::Mon2yShortcircuit;
                }
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

        if new_state.prescription_turns.is_some() {
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

fn neighborhood_range(center: usize, max: usize) -> std::ops::Range<usize> {
    center.saturating_sub(1)..center.saturating_add(2).min(max)
}

impl UtwidAction {
    fn execute_move(&self, state: &UtwidState) -> UtwidState {
        let direction = match self {
            UtwidAction::Move(direction) => *direction,
            _ => unreachable!("execute_move only handles Move actions"),
        };

        let actor = state.actor(state.to_act).unwrap();
        let (new_x, new_y) = apply_dir(actor.x, actor.y, direction);
        self.move_to(state, new_x, new_y)
    }

    fn move_to(&self, state: &UtwidState, new_x: usize, new_y: usize) -> UtwidState {
        let mut new_state = state.clone();
        let actor_id = new_state.to_act;

        // --- Attack ---
        let damage = {
            let actor = new_state.actor(actor_id).unwrap();
            actor.attack_damage.unwrap_or(0) as isize * -1
        } - new_state.temporary_damage_bonus.unwrap_or_default() as isize;

        for (_, actor) in new_state
            .actors_iter_mut()
            .filter(|(_, actor)| actor.x == new_x && actor.y == new_y)
        {
            actor.modify_health(damage);
        }

        let tile = new_state.board.get_mut(new_x, new_y);
        if tile.health.is_some() {
            tile.modify_health(damage);
        }

        // --- And the rest ---

        if new_state
            .actors_iter()
            .map(|(_, actor)| actor)
            .find(|actor| actor.x == new_x && actor.y == new_y)
            .is_none()
            && new_state
                .board
                .get(new_x, new_y)
                .traits
                .contains(TileTraits::WALKABLE)
        {
            let actor = new_state.actor_mut(actor_id).unwrap();
            (actor.x, actor.y) = (new_x, new_y);
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

    fn execute_conclusion(&self, state: &UtwidState) -> UtwidState {
        let direction = match self {
            UtwidAction::Conclusion(direction) => *direction,
            _ => unreachable!("execute_conclusion only handles Conclusion actions"),
        };

        let actor = state.actor(state.to_act).unwrap();

        let (mut new_x, mut new_y) = apply_dir(actor.x, actor.y, direction);
        let (mut last_x, mut last_y) = (new_x, new_y);
        let mut damage_bonus: usize = 0;

        while state
            .board
            .board_permitted_moves(new_x, new_y, true, true, false)
            .contains(&direction)
            && !state
                .actors_iter()
                .any(|actor| actor.1.x == new_x && actor.1.y == new_y)
        {
            (last_x, last_y) = (new_x, new_y);
            (new_x, new_y) = apply_dir(new_x, new_y, direction);
            damage_bonus += 1;
        }
        // This is ugly
        (new_x, new_y) = (last_x, last_y);

        let mut new_state = self.move_to(state, new_x, new_y);
        new_state.temporary_damage_bonus = Some(damage_bonus);
        //
        // Attack at end too
        if new_state
            .board
            .board_permitted_moves(new_x, new_y, true, true, true)
            .contains(&direction)
        {
            (new_x, new_y) = apply_dir(new_x, new_y, direction);
            self.move_to(&new_state, new_x, new_y)
        } else {
            new_state
        }
    }

    fn execute_stagnation(&self, state: &UtwidState) -> UtwidState {
        let direction = match self {
            UtwidAction::Stagnation(direction) => *direction,
            _ => unreachable!("execute_stagnation only handles Stagnation actions"),
        };

        let actor = state.actor(state.to_act).unwrap();
        let (split_x, split_y) = apply_dir(actor.x, actor.y, direction);
        let vertical = match direction {
            Dir::N | Dir::S => false,
            Dir::E | Dir::W => true,
            _ => unreachable!("Cardinal directions only."),
        };

        let mut new_state = state.clone();
        // I've basically copy/pasted this from rooms_builder... I probably should refactor it
        {
            let idx = split_x + split_y * new_state.board.width;
            if new_state.board.geography[idx]
                .traits
                .contains(TileTraits::WALKABLE)
                && !new_state.board.geography[idx]
                    .traits
                    .contains(TileTraits::STAIRS)
                && new_state.actor_in_space(split_x, split_y).is_none()
            {
                new_state.board.geography[idx] = Tile::wall();
            }
        }
        if vertical {
            for y in (0..=split_y as isize - 1).rev() {
                if y < 0 {
                    continue;
                };
                let y = y as usize;
                let idx = split_x + y * new_state.board.width;
                if idx > new_state.board.geography.len() {
                    continue;
                }
                if new_state.board.geography[idx]
                    .traits
                    .contains(TileTraits::WALKABLE)
                    && !new_state.board.geography[idx]
                        .traits
                        .contains(TileTraits::STAIRS)
                    && new_state.actor_in_space(split_x, y).is_none()
                {
                    new_state.board.geography[idx] = Tile::wall();
                } else {
                    break;
                }
            }
            for y in (split_y + 1)..new_state.board.height {
                let idx = split_x + y * new_state.board.width;
                if new_state.board.geography[idx]
                    .traits
                    .contains(TileTraits::WALKABLE)
                    && !new_state.board.geography[idx]
                        .traits
                        .contains(TileTraits::STAIRS)
                    && new_state.actor_in_space(split_x, y).is_none()
                {
                    new_state.board.geography[idx] = Tile::wall();
                } else {
                    break;
                }
            }
        } else {
            for x in (0..=split_x as isize - 1).rev() {
                if x < 0 {
                    continue;
                };
                let x = x as usize;
                let idx = x + split_y * new_state.board.width;
                if new_state.board.geography[idx]
                    .traits
                    .contains(TileTraits::WALKABLE)
                    && !new_state.board.geography[idx]
                        .traits
                        .contains(TileTraits::STAIRS)
                    && new_state.actor_in_space(x, split_y).is_none()
                {
                    new_state.board.geography[idx] = Tile::wall();
                } else {
                    break;
                }
            }
            for x in (split_x + 1)..new_state.board.width {
                let idx = x + split_y * new_state.board.width;
                if new_state.board.geography[idx]
                    .traits
                    .contains(TileTraits::WALKABLE)
                    && !new_state.board.geography[idx]
                        .traits
                        .contains(TileTraits::STAIRS)
                    && new_state.actor_in_space(x, split_y).is_none()
                {
                    new_state.board.geography[idx] = Tile::wall();
                } else {
                    break;
                }
            }
        }
        new_state
    }

    fn execute_contention(&self, state: &UtwidState) -> UtwidState {
        let direction = match self {
            UtwidAction::Contention(direction) => *direction,
            _ => unreachable!("execute_contention only handles Contention actions"),
        };
        let actor = state.actor(state.to_act).unwrap();

        let (mut action, dx, dy) = CARDINAL_DIRS
            .iter()
            .chain(DIAGONAL_DIRS.iter())
            .find(|(action, _, _)| action == &direction)
            .unwrap()
            .clone();
        let (mut target_x, mut target_y) = (actor.x as isize + dx, actor.y as isize + dy);
        while target_x >= 0
            && target_x < state.board.width as isize
            && target_y >= 0
            && target_y < state.board.height as isize
            && state.board.geography[target_x as usize + target_y as usize * state.board.width]
                .traits
                .contains(TileTraits::WALKABLE)
        {
            target_x += dx;
            target_y += dy;
        }

        let idx_rotation = -1 * (actor.x as isize - target_x)
            + (actor.y as isize - target_y) * state.board.width as isize;
        let idx_rotation = if idx_rotation >= 0 {
            idx_rotation as usize
        } else {
            (idx_rotation + state.board.width as isize * state.board.height as isize) as usize
        };

        let mut new_state = state.clone();

        new_state.board.geography = (0..(new_state.board.width * new_state.board.height))
            .map(|idx| {
                state.board.geography
                    [(idx + idx_rotation) % (state.board.width * state.board.height)]
                    .clone()
            })
            .collect();

        for mut actor_to_move in new_state
            .actors_iter_mut()
            .filter(|actor_to_move| actor_to_move.0 != state.to_act)
        {
            let old_idx = actor.x + actor.y * state.board.width;
            let new_idx = (old_idx + idx_rotation) % (state.board.width * state.board.height);
            let actor = actor_to_move.1;
            actor.x = new_idx % state.board.width;
            actor.y = new_idx / state.board.width;
        }

        new_state
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
        let actor_id = new_state.to_act;
        let (x0, y0, damage) = {
            let actor = new_state.actor(actor_id).unwrap();
            (
                actor.x,
                actor.y,
                actor.attack_damage.unwrap_or_default() as isize * -1,
            )
        };
        for ix in neighborhood_range(x0, new_state.board.width) {
            for iy in neighborhood_range(y0, new_state.board.height) {
                let tile = new_state.board.get_mut(ix, iy);
                tile.modify_health(damage);
                for (_, actor) in new_state
                    .actors_iter_mut()
                    .filter(|(_, actor)| actor.x == ix && actor.y == iy)
                {
                    actor.modify_health(damage);
                }
            }
        }
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
    pub repr: Option<Repr>,
    pub repr_set: ReprSet,
}

impl Tile {
    fn floor() -> Tile {
        Tile {
            traits: TileTraits::WALKABLE,
            repr: Some(Repr::Floor),
            repr_set: ReprSet::Room1,
            health: None,
        }
    }

    fn wall() -> Tile {
        Tile {
            traits: TileTraits::empty(),
            repr: Some(Repr::Wall),
            repr_set: ReprSet::Room1,
            health: Some(5),
        }
    }

    fn stair() -> Tile {
        Tile {
            traits: TileTraits::STAIRS | TileTraits::WALKABLE,
            repr: Some(Repr::Stairs),
            repr_set: ReprSet::Room1,
            health: None,
        }
    }

    fn win() -> Tile {
        Tile {
            traits: TileTraits::WALKABLE | TileTraits::WIN,
            repr: Some(Repr::Win),
            repr_set: ReprSet::Room1,
            health: None,
        }
    }

    pub fn repr(&self) -> Option<Repr> {
        self.repr
    }

    pub fn modify_health(&mut self, dhealth: isize) {
        if self.health.is_some() {
            self.health = Some(self.health.unwrap() + dhealth);

            if self.health.unwrap() <= 0 {
                self.traits = TileTraits::WALKABLE;
                self.repr = Some(Repr::Floor);
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

fn apply_dir(x: usize, y: usize, direction: Dir) -> (usize, usize) {
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
        let (geography, width, height, rng) = Board::rooms_builder(_level, rng);
        Board {
            geography,
            width,
            height,
            rng,
        }
    }
    fn rooms_builder(_level: usize, rng: &mut SmallRng) -> (Vec<Tile>, usize, usize, SmallRng) {
        let mut rng = rng.clone();
        let width: usize = 11;
        let height: usize = 11;
        let mut geography = vec![Tile::floor(); width * height];

        // Track vertical and horizontal positions separately to prevent parallel crowding
        let mut used_x: Vec<usize> = Vec::new();
        let mut used_y: Vec<usize> = Vec::new();
        let mut splits: Vec<(usize, usize)> = Vec::new();

        'split_attempt: for _ in
            0..rng.random_range(ROOM_SPLITS_MIN + _level..ROOM_SPLITS_MAX + _level)
        {
            let vertical = rng.random_bool(0.5);
            let split_x = rng.random_range(1..width - 1);
            let split_y = rng.random_range(1..height - 1);

            if vertical {
                if used_x.iter().any(|&x| split_x.abs_diff(x) <= 3) {
                    continue 'split_attempt;
                }

                if !geography[split_x + split_y * width]
                    .traits
                    .contains(TileTraits::WALKABLE)
                {
                    continue 'split_attempt;
                }

                for y in (0..=split_y).rev() {
                    let idx = split_x + y * width;
                    if geography[idx].traits.contains(TileTraits::WALKABLE) {
                        geography[idx] = Tile::wall();
                    } else {
                        break;
                    }
                }
                for y in (split_y + 1)..height {
                    let idx = split_x + y * width;
                    if geography[idx].traits.contains(TileTraits::WALKABLE) {
                        geography[idx] = Tile::wall();
                    } else {
                        break;
                    }
                }
                used_x.push(split_x);
            } else {
                if used_y.iter().any(|&y| split_y.abs_diff(y) <= 3) {
                    continue 'split_attempt;
                }

                if !geography[split_x + split_y * width]
                    .traits
                    .contains(TileTraits::WALKABLE)
                {
                    continue 'split_attempt;
                }

                for x in (0..=split_x).rev() {
                    let idx = x + split_y * width;
                    if geography[idx].traits.contains(TileTraits::WALKABLE) {
                        geography[idx] = Tile::wall();
                    } else {
                        break;
                    }
                }
                for x in (split_x + 1)..width {
                    let idx = x + split_y * width;
                    if geography[idx].traits.contains(TileTraits::WALKABLE) {
                        geography[idx] = Tile::wall();
                    } else {
                        break;
                    }
                }
                used_y.push(split_y);
            }
            geography[split_x + split_y * width] = Tile::floor();
            splits.push((split_x, split_y));
        }

        // Ensure stairs don't spawn in a wall
        let mut stair_idx;
        loop {
            let x = rng.random_range(0..width);
            let y = rng.random_range(0..height);
            stair_idx = x + y * width;
            if geography[stair_idx].traits.contains(TileTraits::WALKABLE) {
                break;
            }
        }

        geography[stair_idx] = if _level < 9 {
            Tile::stair()
        } else {
            Tile::win()
        };

        for coords in splits {
            geography[coords.0 + coords.1 * width] = Tile::floor();
        }

        (geography, width, height, rng)
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
    ) -> Vec<Dir> {
        CARDINAL_DIRS
            .iter()
            .filter(|_| cardinal)
            .chain(DIAGONAL_DIRS.iter().filter(|_| diagonal))
            .filter_map(|(action, dx, dy)| {
                let x = from_x as isize + *dx as isize;
                let y = from_y as isize + *dy as isize;

                if x >= 0 && (x as usize) < self.width && y >= 0 && (y as usize) < self.height {
                    let tile = self.get(x as usize, y as usize);
                    (tile.traits.contains(TileTraits::WALKABLE) || (melee && tile.health.is_some()))
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

#[derive(Clone, Debug, PartialEq, PartialOrd)]
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
    pub actor_type: usize,
    pub traits: ActorTraits,
    pub mon2y: Option<Mon2yData>,
    pub repr: Option<Repr>,
    pub repr_set: ReprSet,
    pub health: Option<usize>,
    pub attack_damage: Option<usize>,
    pub allegiance: Allegiance,
}

impl GameActor {
    pub fn repr(&self) -> Option<Repr> {
        self.repr
    }

    pub fn actor_type_name(&self) -> &'static str {
        ACTOR_TYPE_NAMES
            .get(self.actor_type)
            .copied()
            .unwrap_or("unknown")
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
            actor_type: ACTOR_TYPE_YOU,
            traits: ActorTraits::HUMAN
                | ActorTraits::CARDINAL_MOVE
                | ActorTraits::DIAGONAL_MOVE
                | ActorTraits::MELEE,
            mon2y: None,
            repr: Some(Repr::You),
            repr_set: ReprSet::Room1,
            health: Some(7),
            attack_damage: Some(1),
            allegiance: Allegiance::You,
        }
    }

    fn monte_actor() -> GameActor {
        GameActor {
            x: 7,
            y: 7,
            actor_type: 1,
            traits: ActorTraits::MON2Y
                | ActorTraits::CARDINAL_MOVE
                | ActorTraits::DIAGONAL_MOVE
                | ActorTraits::WAIT
                | ActorTraits::MELEE,
            mon2y: Some(Mon2yData {
                tree_id: 1,
                iterations: 1000,
            }),
            repr: Some(Repr::Monte),
            repr_set: ReprSet::Room1,
            health: Some(7),
            attack_damage: Some(1),
            allegiance: Allegiance::Monty,
        }
    }

    fn them_actor(x: usize, y: usize) -> GameActor {
        GameActor {
            x,
            y,
            actor_type: ACTOR_TYPE_THEM,
            traits: ActorTraits::MON2Y | ActorTraits::DIAGONAL_MOVE | ActorTraits::MELEE,
            mon2y: Some(Mon2yData {
                tree_id: 1,
                iterations: 1000,
            }),
            repr: Some(Repr::Them),
            repr_set: ReprSet::Room1,
            health: Some(2),
            attack_damage: Some(1),
            allegiance: Allegiance::Monty,
        }
    }

    fn are_actor(x: usize, y: usize) -> GameActor {
        GameActor {
            x,
            y,
            actor_type: ACTOR_TYPE_ARE,
            traits: ActorTraits::MON2Y | ActorTraits::CARDINAL_MOVE | ActorTraits::MELEE,
            mon2y: Some(Mon2yData {
                tree_id: 1,
                iterations: 1000,
            }),
            repr: Some(Repr::Are),
            repr_set: ReprSet::Room1,
            health: Some(3),
            attack_damage: Some(2),
            allegiance: Allegiance::Monty,
        }
    }

    fn one_actor(x: usize, y: usize) -> GameActor {
        GameActor {
            x,
            y,
            actor_type: ACTOR_TYPE_ONE,
            traits: ActorTraits::MON2Y
                | ActorTraits::DIAGONAL_MOVE
                | ActorTraits::CARDINAL_MOVE
                | ActorTraits::BOMB,
            mon2y: Some(Mon2yData {
                tree_id: 1,
                iterations: 500,
            }),
            repr: Some(Repr::One),
            repr_set: ReprSet::Room1,
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
        println!("{}", state.debug_summary());
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
                return UtwidAction::Move(*action);
            }
        }
        panic!("Expected a walkable tile adjacent to target");
    }

    #[test]
    fn stairs_transition_resets_turn_state_across_multiple_floors() {
        let mut state = UtwidState::new();
        let mut transitioned = false;

        for _ in 0..5 {
            state.add_actor(GameActor::are_actor(2, 2));
            state.add_actor(GameActor::them_actor(3, 3));
            state.to_act = 2;

            let Some((stairs_x, stairs_y)) = (0..state.board.height)
                .flat_map(|y| (0..state.board.width).map(move |x| (x, y)))
                .find(|(x, y)| state.board.get(*x, *y).traits.contains(TileTraits::STAIRS))
            else {
                break;
            };
            state = UtwidAction::Move(Dir::N).execute_stairs(&state);
            transitioned = true;

            assert_eq!(state.to_act, 0);
            assert_eq!(state.turn_order, VecDeque::from([0]));
            assert_eq!(state.actor_id_counter, 1);
            assert_eq!(state.actor_count(), 1);
            assert!(state.has_actor(0));
            assert!(matches!(state.next_actor(), Actor::Player(0)));

            state.game_state = GameState::Ongoing;
        }

        assert!(transitioned);
    }

    #[test]
    fn stairs_transition_clears_monsters_dead_entries_and_stale_turn_ids() {
        let mut state = UtwidState::new();
        let mut transitioned = false;

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

            let Some((stairs_x, stairs_y)) = (0..state.board.height)
                .flat_map(|y| (0..state.board.width).map(move |x| (x, y)))
                .find(|(x, y)| state.board.get(*x, *y).traits.contains(TileTraits::STAIRS))
            else {
                break;
            };
            state = UtwidAction::Move(Dir::N).execute_stairs(&state);
            transitioned = true;

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

        assert!(transitioned);
    }

    #[test]
    fn execute_path_keeps_to_act_valid_between_floors_and_after_win() {
        let mut state = UtwidState::new();
        state.add_actor(GameActor::are_actor(2, 2));
        state.add_actor(GameActor::them_actor(3, 3));

        let (stairs_x, stairs_y) = stair_location(&state);
        let action = adjacent_move_to(&mut state, stairs_x, stairs_y);
        state = action.execute(&state);

        assert!(matches!(state.game_state, GameState::Checkpoint));
        assert!(state.has_actor(state.to_act));
        assert!(matches!(state.next_actor(), Actor::Player(_)));

        state.game_state = GameState::Won;
        assert!(state.has_actor(state.to_act));
        assert!(matches!(state.next_actor(), Actor::Player(_)));
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

        let node = create_expanded_node(post_stairs.clone(), None, None);
        match node {
            crate::mcts::node::Node::Expanded { children, .. } => assert!(children.is_empty()),
            crate::mcts::node::Node::Placeholder { .. } => panic!("Expected expanded node"),
        }
    }

    #[test]
    fn checkpoint_reward_uses_player_health_ratio_and_stays_terminal() {
        let mut state = UtwidState::new();
        state.add_actor(GameActor::are_actor(2, 2));

        let (stairs_x, stairs_y) = stair_location(&state);
        let action = adjacent_move_to(&mut state, stairs_x, stairs_y);
        state.actor_mut(0).unwrap().health = Some(4);

        let post_stairs = action.execute(&state);
        let rewards = post_stairs.reward();

        assert!(post_stairs.terminal());
        assert_eq!(rewards.len(), 2);
        assert!((rewards[YOU_ID] - 4.0 / 7.0).abs() < f64::EPSILON);
        assert!((rewards[MON2Y_ID] + 4.0 / 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn lost_and_won_rewards_match_actor_outcomes() {
        let mut lost_state = UtwidState::new();
        lost_state.game_state = GameState::Lost;
        assert_eq!(lost_state.reward(), vec![-1.0, 1.0]);

        let mut won_state = UtwidState::new();
        won_state.game_state = GameState::Won;
        assert_eq!(won_state.reward(), vec![1.0, -1.0]);
    }
}
