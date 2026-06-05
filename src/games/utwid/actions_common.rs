use std::collections::VecDeque;

use super::board::apply_dir;
use super::*;

impl UtwidAction {
    pub(super) fn execute_move(&self, state: &UtwidState) -> UtwidState {
        let direction = match self {
            UtwidAction::Move(direction) => *direction,
            _ => unreachable!("execute_move only handles Move actions"),
        };

        let actor = state.actor(state.to_act).unwrap();
        let (new_x, new_y) = apply_dir(actor.x, actor.y, direction);
        self.move_to(state, new_x, new_y)
    }

    pub(super) fn move_to(&self, state: &UtwidState, new_x: usize, new_y: usize) -> UtwidState {
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

    pub(crate) fn execute_stairs(&self, state: &UtwidState) -> UtwidState {
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

    pub(super) fn execute_win(&self, state: &UtwidState) -> UtwidState {
        let mut new_state = state.clone();
        new_state.game_state = GameState::Won;
        new_state
    }
}
