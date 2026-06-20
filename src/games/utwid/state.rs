use std::collections::VecDeque;

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
        self.actors.get_mut(actor_id).and_then(Option::take)
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
        self.actors
            .iter()
            .filter_map(|actor| actor.as_ref())
            .filter(|actor| !actor.traits.contains(ActorTraits::DEAD))
            .find(|actor| actor.x == x && actor.y == y)
    }

    pub(crate) fn actor_id_in_space(&self, x: usize, y: usize) -> Option<ActorId> {
        self.actors_iter()
            .find(|(_, actor)| {
                !actor.traits.contains(ActorTraits::DEAD) && actor.x == x && actor.y == y
            })
            .map(|(id, _)| id)
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

            if let Some(actor_id) = self.actor_id_in_space(target_x_usize, target_y_usize) {
                return Some(actor_id);
            }

            target_x += dx;
            target_y += dy;
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
                        && next_actor.effective_allegiance() != actor.effective_allegiance()
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
            let stagnation_moves = self
                .filter_directions_by_board_space(CARDINAL_DIRS.to_vec(), next_actor)
                .into_iter()
                .map(UtwidAction::Stagnation);
            permitted_actions.extend(stagnation_moves);

            let contention_moves = CARDINAL_DIRS
                .iter()
                .chain(DIAGONAL_DIRS.iter())
                .map(|d| d.0)
                .map(UtwidAction::Contention);
            permitted_actions.extend(contention_moves);

            permitted_actions.push(UtwidAction::Prescription);

            let multiplication_moves = self
                .filter_directions_by_board_space(
                    CARDINAL_DIRS.iter().chain(DIAGONAL_DIRS.iter()).copied(),
                    next_actor,
                )
                .into_iter()
                .map(UtwidAction::Multiplication);
            permitted_actions.extend(multiplication_moves);

            let assumption_moves = CARDINAL_DIRS
                .iter()
                .chain(DIAGONAL_DIRS.iter())
                .filter_map(|(direction, _, _)| {
                    self.first_actor_in_direction(next_actor, *direction)
                        .is_some()
                        .then_some(*direction)
                })
                .map(UtwidAction::Assumption);
            permitted_actions.extend(assumption_moves);
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
            GameState::Stalemate => {
                rewards[YOU_ID] = -0.25;
                rewards[MON2Y_ID] = -0.25;
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
