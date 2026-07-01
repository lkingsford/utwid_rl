use std::collections::{HashMap, VecDeque};

use bitflags::bitflags;
use rand::{SeedableRng, prelude::*, rngs::SmallRng};

use crate::mcts::Reward;
use crate::mcts::game_trait::{Actor, State};

use super::board::apply_dir;
use super::types::*;
use super::*;

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

    pub ai_turns: usize,
    pub spawn_rng: SmallRng,
    pub actor_id_counter: ActorId,
    pub reward_progress: Option<Vec<f64>>,
    pub reward_config: RewardConfig,
    pub(crate) spatial_hashmap: HashMap<(usize, usize), ActorId>,
    pub(crate) turns_since_aggressive_action: usize,
    pub player_kills: usize,
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct WitnessedYouActions: u8 {
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RewardConfig {
    pub turn_weight: f64,
    pub level_base: f64,
    pub health_weight: f64,
    pub health_bias: f64,
    pub win_reward: f64,
    pub lose_reward: f64,
    pub stalemate_reward: f64,
    pub level_reward: f64,
    pub passivity_penalty: f64,
}

impl Default for RewardConfig {
    fn default() -> Self {
        RewardConfig {
            turn_weight: 0.2,
            level_base: 0.75,
            health_weight: 2.5,
            health_bias: -0.3,
            win_reward: 20.0,
            lose_reward: -20.0,
            stalemate_reward: -5.0,
            level_reward: 20.0,
            passivity_penalty: 1.0,
        }
    }
}

impl UtwidState {
    pub fn new() -> UtwidState {
        let mut board_rng = SmallRng::from_os_rng();
        let spawn_rng = SmallRng::from_os_rng();
        let board = { Board::new(0, &mut board_rng) };

        let mut state = UtwidState {
            current_level: 0,
            board: board, // Use the pre-created board
            actors: vec![Some(GameActor::you_actor())],
            to_act: 0,
            game_state: GameState::Ongoing,
            turn_number: 0,
            turn_order: VecDeque::from(vec![0]),
            short_circuit_at_turns: None,
            short_circuit_at_turns_increment: None,
            ai_turns: 0,
            spawn_rng,
            actor_id_counter: 1,
            witnessed_you_actions: WitnessedYouActions::empty(),
            prescription_turns: None,
            temporary_damage_bonus: None,
            reward_progress: None,
            reward_config: RewardConfig::default(),
            spatial_hashmap: HashMap::new(),
            turns_since_aggressive_action: 0,
            player_kills: 0,
        };
        state.update_spatial_hashmap();
        state
    }

    // Urgh - I don't know if I should be using an index here...
    pub fn add_actor(&mut self, actor: GameActor) -> ActorId {
        let id = self.actor_id_counter;
        let pos = (actor.x, actor.y);
        let is_dead = actor.traits.contains(ActorTraits::DEAD);
        self.actors.push(Some(actor));
        self.actor_id_counter += 1;
        self.turn_order.push_back(id);
        if !is_dead {
            self.spatial_hashmap.insert(pos, id);
        }
        id
    }

    pub(crate) fn actor(&self, actor_id: ActorId) -> Option<&GameActor> {
        self.actors.get(actor_id).and_then(Option::as_ref)
    }

    pub(crate) fn actor_mut(&mut self, actor_id: ActorId) -> Option<&mut GameActor> {
        self.actors.get_mut(actor_id).and_then(Option::as_mut)
    }

    pub(crate) fn has_actor(&self, actor_id: ActorId) -> bool {
        self.actor(actor_id).is_some()
    }

    pub(crate) fn remove_actor(&mut self, actor_id: ActorId) -> Option<GameActor> {
        let actor = self.actors.get_mut(actor_id).and_then(Option::take);
        if let Some(actor) = &actor {
            self.spatial_hashmap.remove(&(actor.x, actor.y));
        }
        actor
    }

    pub(crate) fn actors_iter(&self) -> impl Iterator<Item = (ActorId, &GameActor)> {
        self.actors
            .iter()
            .enumerate()
            .filter_map(|(id, actor)| actor.as_ref().map(|actor| (id, actor)))
    }

    pub(crate) fn actors_iter_mut(&mut self) -> impl Iterator<Item = (ActorId, &mut GameActor)> {
        self.actors
            .iter_mut()
            .enumerate()
            .filter_map(|(id, actor)| actor.as_mut().map(|actor| (id, actor)))
    }

    pub(crate) fn actor_count(&self) -> usize {
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

    pub(crate) fn reward_actor_count(&self) -> usize {
        usize::max(self.mon2y_high_actor_id() as usize, MON2Y_ID) + 1
    }

    pub(crate) fn player_health_ratio(&self) -> f64 {
        let health = self
            .actor(YOU_ID)
            .and_then(|actor| actor.health)
            .unwrap_or(0) as f64;
        health / PLAYER_MAX_HEALTH as f64
    }

    pub(crate) fn suggest_spawn(&mut self) -> (usize, usize) {
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

    pub(crate) fn actor_in_space(&self, x: usize, y: usize) -> Option<&GameActor> {
        self.spatial_hashmap
            .get(&(x, y))
            .and_then(|&actor_id| self.actor(actor_id))
            .filter(|actor| !actor.traits.contains(ActorTraits::DEAD))
    }

    pub(crate) fn actor_id_in_space(&self, x: usize, y: usize) -> Option<ActorId> {
        self.spatial_hashmap
            .get(&(x, y))
            .copied()
            .filter(|&actor_id| {
                self.actor(actor_id)
                    .map_or(false, |actor| !actor.traits.contains(ActorTraits::DEAD))
            })
    }

    pub(crate) fn first_actor_in_direction(
        &self,
        actor: &GameActor,
        direction: Dir,
    ) -> Option<ActorId> {
        let (_action, dx, dy) = CARDINAL_DIRS
            .iter()
            .chain(DIAGONAL_DIRS.iter())
            .find(|(action, _, _)| action == &direction)
            .unwrap()
            .clone();
        let (mut target_x, mut target_y) = (actor.x as isize + dx, actor.y as isize + dy);

        loop {
            if target_x < 0
                || target_x >= self.board.width as isize
                || target_y < 0
                || target_y >= self.board.height as isize
            {
                break None;
            }

            let target_x_usize = target_x as usize;
            let target_y_usize = target_y as usize;
            if !self
                .board
                .get(target_x_usize, target_y_usize)
                .traits
                .contains(TileTraits::WALKABLE)
            {
                break None;
            }

            if let Some(&actor_id) = self.spatial_hashmap.get(&(target_x_usize, target_y_usize)) {
                if let Some(actor) = self.actor(actor_id) {
                    if !actor.traits.contains(ActorTraits::DEAD) {
                        return Some(actor_id);
                    }
                }
            }

            target_x += dx;
            target_y += dy;
        }
    }

    pub(crate) fn update_spatial_hashmap(&mut self) {
        self.spatial_hashmap.clear();
        for (actor_id, actor_opt) in self.actors.iter().enumerate() {
            if let Some(actor) = actor_opt {
                if !actor.traits.contains(ActorTraits::DEAD) {
                    self.spatial_hashmap.insert((actor.x, actor.y), actor_id);
                }
            }
        }
    }

    pub(crate) fn update_actor_position(&mut self, actor_id: ActorId, old_x: usize, old_y: usize, new_x: usize, new_y: usize) {
        if old_x != new_x || old_y != new_y {
            self.spatial_hashmap.remove(&(old_x, old_y));
        }
        if !self.actor(actor_id).map_or(true, |a| a.traits.contains(ActorTraits::DEAD)) {
            self.spatial_hashmap.insert((new_x, new_y), actor_id);
        }
    }

    pub(crate) fn remove_actor_from_spatial_hashmap(&mut self, actor_id: ActorId) {
        if let Some(actor) = self.actor(actor_id) {
            self.spatial_hashmap.remove(&(actor.x, actor.y));
        }
    }

    pub(crate) fn record_aggressive_action(&mut self) {
        if let Some(actor) = self.actor(self.to_act) {
            if actor.actor_type == ACTOR_TYPE_YOU {
                self.turns_since_aggressive_action = 0;
            }
        }
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

    pub(crate) fn debug_summary(&self) -> String {
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

    pub(crate) fn normalize_turn_state(&mut self) {
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

    /// Convenience method for that particulary filter that's used in a few places in permitted
    /// action
    fn filter_directions_by_board_space<I>(&self, directions: I, actor: &GameActor) -> Vec<Dir>
    where
        I: IntoIterator<Item = (Dir, isize, isize)>,
    {
        directions
            .into_iter()
            .filter_map(|(direction, _, _)| match direction {
                Dir::N if actor.y > 0 => Some(direction),
                Dir::S if actor.y + 1 < self.board.height => Some(direction),
                Dir::E if actor.x + 1 < self.board.width => Some(direction),
                Dir::W if actor.x > 0 => Some(direction),
                Dir::NE if actor.y > 0 && actor.x + 1 < self.board.width => Some(direction),
                Dir::SE if actor.y + 1 < self.board.height && actor.x + 1 < self.board.width => {
                    Some(direction)
                }
                Dir::NW if actor.y > 0 && actor.x > 0 => Some(direction),
                Dir::SW if actor.y + 1 < self.board.height && actor.x > 0 => Some(direction),
                _ => None,
            })
            .collect()
    }

    fn ai_turn_weight(&self) -> f64 {
        (self.reward_config.level_base as f64).powf(self.ai_turns as f64 * self.reward_config.turn_weight) / 2.0
    }

    pub(crate) fn accumulate_stair_reward(&mut self) {
        if self.reward_progress.is_none() {
            self.reward_progress = Some(vec![0.0; self.reward_actor_count()]);
        }
        let player_reward = self.reward_config.level_reward * self.ai_turn_weight();
        let rewards = self.reward_progress.as_mut().unwrap();
        rewards[YOU_ID] += player_reward;
        rewards[MON2Y_ID] -= player_reward;
    }

    pub(crate) fn accumulate_reward(&mut self) {
        if self.reward_progress.is_none() {
            self.reward_progress = Some(vec![0.0; self.reward_actor_count()]);
        }
        let player_reward = ((self.player_health_ratio() + self.reward_config.health_bias)
            * self.reward_config.health_weight)
            * self.ai_turn_weight();
        
        // Apply passivity penalty if player hasn't taken aggressive actions
        let passivity_penalty = self.turns_since_aggressive_action as f64 * self.reward_config.passivity_penalty;
        
        let rewards = self.reward_progress.as_mut().unwrap();
        rewards[YOU_ID] += player_reward - passivity_penalty;
        rewards[MON2Y_ID] -= player_reward - passivity_penalty;
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
        let cost_suicide_protected = next_actor.effective_allegiance() == next_actor.allegiance;

        let mut permitted_actions = Vec::with_capacity(board_permitted_moves.len() + 10); // Estimate capacity
        
        // Add move actions
        for &direction in &board_permitted_moves {
            let (x, y) = apply_dir(next_actor.x, next_actor.y, direction);
            if let Some(actor) = self.actor_in_space(x, y) {
                if next_actor.traits.contains(ActorTraits::MELEE)
                    && next_actor.effective_allegiance() != actor.effective_allegiance()
                {
                    permitted_actions.push(UtwidAction::Move(direction));
                }
            } else {
                permitted_actions.push(UtwidAction::Move(direction));
            }
        }
        
        // Add explode action if applicable
        if next_actor.traits.contains(ActorTraits::BOMB) {
            permitted_actions.push(UtwidAction::Explode);
        }

        if is_you {
            // Add conclusion actions
            for &direction in &board_permitted_moves {
                permitted_actions.push(UtwidAction::Conclusion(direction));
            }
            
            // Add stagnation moves
            let stagnation_moves = self
                .filter_directions_by_board_space(CARDINAL_DIRS.to_vec(), next_actor);
            for direction in stagnation_moves {
                permitted_actions.push(UtwidAction::Stagnation(direction));
            }
            
            // Add contention moves
            for (direction, _, _) in CARDINAL_DIRS.iter().chain(DIAGONAL_DIRS.iter()) {
                permitted_actions.push(UtwidAction::Contention(*direction));
            }
            
            permitted_actions.push(UtwidAction::Prescription);
            
            // Add multiplication moves
            let multiplication_moves = self
                .filter_directions_by_board_space(
                    CARDINAL_DIRS.iter().chain(DIAGONAL_DIRS.iter()).copied(),
                    next_actor,
                );
            for direction in multiplication_moves {
                permitted_actions.push(UtwidAction::Multiplication(direction));
            }
            
            // Add assumption moves
            for (direction, _, _) in CARDINAL_DIRS.iter().chain(DIAGONAL_DIRS.iter()) {
                if self.first_actor_in_direction(next_actor, *direction).is_some() {
                    permitted_actions.push(UtwidAction::Assumption(*direction));
                }
            }
        }

        let permitted_actions = if cost_suicide_protected {
            let hp = next_actor.health.unwrap();
            permitted_actions
                .into_iter()
                .filter(|action| action_cost(*action) < hp)
                .collect()
        } else {
            permitted_actions
        };

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
        match next_actor.effective_allegiance() {
            Allegiance::You => Actor::Player(0),
            Allegiance::Monty => Actor::Player(next_actor.mon2y.as_ref().unwrap().tree_id),
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
        let mut rewards = self
            .reward_progress
            .clone()
            .unwrap_or_else(|| vec![0.0; self.reward_actor_count()]);

        match self.game_state {
            GameState::Checkpoint => {
                let player_health_ratio = self.player_health_ratio();
                rewards[YOU_ID] += player_health_ratio;
                rewards[MON2Y_ID] += -player_health_ratio;
            }
            GameState::Mon2yShortcircuit => {
                let reward = 0.7 * (self.current_level as f64 / board::LEVEL_COUNT as f64);
                rewards[YOU_ID] += reward;
                rewards[MON2Y_ID] += -reward;
            }
            GameState::Lost => {
                rewards[YOU_ID] += self.reward_config.lose_reward * self.ai_turn_weight();
                rewards[MON2Y_ID] += self.reward_config.win_reward * self.ai_turn_weight();
            }
            GameState::Won => {
                rewards[YOU_ID] += self.reward_config.win_reward * self.ai_turn_weight();
                rewards[MON2Y_ID] += self.reward_config.lose_reward * self.ai_turn_weight();
            }
            GameState::Stalemate => {
                rewards[YOU_ID] += self.reward_config.stalemate_reward * self.ai_turn_weight();
                rewards[MON2Y_ID] += self.reward_config.stalemate_reward * self.ai_turn_weight();
            }
            _ => {}
        };
        rewards
    }

    fn round_hyperreward(&self) -> Self::GameHyperrewardType {
        ()
    }
}
