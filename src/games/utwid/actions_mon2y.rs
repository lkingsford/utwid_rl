use super::*;
use crate::games::utwid::types::{ACTOR_TYPE_YOU, ActorId};

fn neighborhood_range(center: usize, max: usize) -> std::ops::Range<usize> {
    center.saturating_sub(1)..center.saturating_add(2).min(max)
}

impl UtwidAction {
    pub(super) fn execute_explode(&self, mut new_state: UtwidState) -> (UtwidState, Vec<UtwidEvent>) {
        log::trace!("execute_explode");
        let mut events: Vec<UtwidEvent> = vec![];
        let actor_id = new_state.to_act;
        let (x0, y0, damage) = {
            let actor = new_state.actor(actor_id).unwrap();
            (
                actor.x,
                actor.y,
                actor.attack_damage.unwrap_or_default() as isize * -1,
            )
        };
        let is_player_attack = new_state.actor(actor_id)
            .map(|attacker| attacker.actor_type == ACTOR_TYPE_YOU)
            .unwrap_or(false);
        
        for ix in neighborhood_range(x0, new_state.board.width) {
            for iy in neighborhood_range(y0, new_state.board.height) {
                let tile = new_state.board.get_mut(ix, iy);
                tile.modify_health(damage);
                // Collect actor IDs in this position before modifying them
                let actor_ids_in_position: Vec<ActorId> = new_state.actors_iter()
                    .filter(|(_, actor)| {
                        !actor.traits.contains(ActorTraits::DEAD) && actor.x == ix && actor.y == iy
                    })
                    .map(|(id, _)| id)
                    .collect();
                
                for actor_id_target in actor_ids_in_position {
                    if let Some(actor) = new_state.actor_mut(actor_id_target) {
                        events.push(UtwidEvent::DamageTaken(actor.clone(), damage));
                        let was_alive = !actor.traits.contains(ActorTraits::DEAD);
                        actor.modify_health(damage);
                        if actor.traits.contains(ActorTraits::DEAD) && was_alive && is_player_attack {
                            // Increment player kills if attacker was the player
                            new_state.player_kills += 1;
                        }
                    }
                }
            }
        }
        // Record aggressive action if player used explode
        if is_player_attack {
            new_state.turns_since_aggressive_action = 0;
        }
        (new_state, events)
    }
}
