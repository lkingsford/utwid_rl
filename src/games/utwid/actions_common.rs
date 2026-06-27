use std::collections::VecDeque;

use super::board::apply_dir;
use super::types::*;
use super::*;

impl UtwidAction {
    pub(super) fn execute_move(&self, state: UtwidState) -> (UtwidState, Vec<UtwidEvent>) {
        log::trace!("execute_move");
        let direction = match self {
            UtwidAction::Move(direction) => *direction,
            _ => unreachable!("execute_move only handles Move actions"),
        };

        let (new_x, new_y) = {
            let actor = state.actor(state.to_act).unwrap();
            apply_dir(actor.x, actor.y, direction)
        };
        self.move_to(state, new_x, new_y)
    }

    pub(super) fn move_to(
        &self,
        mut new_state: UtwidState,
        new_x: usize,
        new_y: usize,
    ) -> (UtwidState, Vec<UtwidEvent>) {
        log::trace!("move_to");
        let mut events: Vec<UtwidEvent> = vec![];
        let actor_id = new_state.to_act;

        let damage = {
            let actor = new_state.actor(actor_id).unwrap();
            actor.attack_damage.unwrap_or(0) as isize * -1
        } - new_state.temporary_damage_bonus.unwrap_or_default() as isize;

        let mut human_died = false;
        for (_, actor) in new_state.actors_iter_mut().filter(|(_, actor)| {
            !actor.traits.contains(ActorTraits::DEAD) && actor.x == new_x && actor.y == new_y
        }) {
            events.push(UtwidEvent::DamageTaken(actor.clone(), damage));
            actor.modify_health(damage);
            if actor.traits.contains(ActorTraits::DEAD) && actor.traits.contains(ActorTraits::HUMAN)
            {
                human_died = true;
            }
        }

        if human_died {
            for (_, actor) in new_state.actors_iter_mut().filter(|(_, actor)| {
                actor.actor_type == ACTOR_TYPE_YOU
                    && !actor.traits.contains(ActorTraits::HUMAN)
                    && !actor.traits.contains(ActorTraits::DEAD)
            }) {
                actor.traits.insert(ActorTraits::HUMAN)
            }
        }

        let tile = new_state.board.get_mut(new_x, new_y);
        if tile.health.is_some() {
            tile.modify_health(damage);
        }

        if new_state
            .actors_iter()
            .map(|(_, actor)| actor)
            .find(|actor| {
                !actor.traits.contains(ActorTraits::DEAD) && actor.x == new_x && actor.y == new_y
            })
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

        let actor_clone = new_state.actor(actor_id).unwrap().clone();

        if actor_clone.actor_type == ACTOR_TYPE_YOU {
            let tile = new_state.board.get(actor_clone.x, actor_clone.y);
            if tile.traits.contains(TileTraits::STAIRS) {
                (self.execute_stairs(actor_clone, new_state), events)
            } else if tile.traits.contains(TileTraits::WIN) {
                let (win_state, mut win_events) = self.execute_win(new_state);
                events.append(&mut win_events);
                (win_state, events)
            } else {
                (new_state, events)
            }
        } else {
            (new_state, events)
        }
    }

    pub(crate) fn execute_stairs(
        &self,
        acting_character: GameActor,
        mut new_state: UtwidState,
    ) -> UtwidState {
        log::trace!("execute_stairs");
        new_state.current_level += 1;
        let mut board_rng = new_state.board.rng.clone();
        new_state.board = Board::new(new_state.current_level, &mut board_rng);
        let mut you = acting_character;
        you.x = 1;
        you.y = 3;
        you.traits.remove(ActorTraits::DEAD);
        you.traits.insert(ActorTraits::HUMAN);
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

    pub(super) fn execute_win(&self, mut new_state: UtwidState) -> (UtwidState, Vec<UtwidEvent>) {
        log::trace!("execute_win");
        new_state.game_state = GameState::Won;
        (new_state, vec![UtwidEvent::Won])
    }
}
