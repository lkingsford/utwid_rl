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
        if let Some(&actor_id) = new_state.spatial_hashmap.get(&(new_x, new_y)) {
            let mut actor_died = false;
            let mut was_human = false;
            
            if let Some(actor) = new_state.actor_mut(actor_id) {
                if !actor.traits.contains(ActorTraits::DEAD) {
                    was_human = actor.traits.contains(ActorTraits::HUMAN);
                    events.push(UtwidEvent::DamageTaken(actor.clone(), damage));
                    let was_alive = !actor.traits.contains(ActorTraits::DEAD);
                    actor.modify_health(damage);
                    if actor.traits.contains(ActorTraits::DEAD) && was_alive {
                        actor_died = true;
                    }
                    // Record aggressive action if player dealt damage
                    if let Some(attacker) = new_state.actor(new_state.to_act) {
                        if attacker.actor_type == ACTOR_TYPE_YOU {
                            new_state.turns_since_aggressive_action = 0;
                        }
                    }
                }
            }
            
            if actor_died {
                new_state.spatial_hashmap.remove(&(new_x, new_y));
                if was_human {
                    human_died = true;
                }
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

        if new_state.actor_in_space(new_x, new_y).is_none()
            && new_state
                .board
                .get(new_x, new_y)
                .traits
                .contains(TileTraits::WALKABLE)
        {
            let actor = new_state.actor_mut(actor_id).unwrap();
            let old_x = actor.x;
            let old_y = actor.y;
            (actor.x, actor.y) = (new_x, new_y);
            new_state.update_actor_position(actor_id, old_x, old_y, new_x, new_y);
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
        new_state.spatial_hashmap.clear();
        new_state.spatial_hashmap.insert((1, 3), 0);
        new_state.turn_order = VecDeque::from([0]);
        new_state.to_act = 0;
        new_state.actor_id_counter = 1;
        new_state.game_state = GameState::Checkpoint;
        new_state.reward_progress = None;
        new_state.turns_since_aggressive_action = 0;
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
