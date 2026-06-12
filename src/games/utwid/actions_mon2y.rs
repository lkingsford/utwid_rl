use super::*;

fn neighborhood_range(center: usize, max: usize) -> std::ops::Range<usize> {
    center.saturating_sub(1)..center.saturating_add(2).min(max)
}

impl UtwidAction {
    pub(super) fn execute_explode(&self, state: &UtwidState) -> UtwidState {
        log::trace!("execute_explode");
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
                for (_, actor) in new_state.actors_iter_mut().filter(|(_, actor)| {
                    !actor.traits.contains(ActorTraits::DEAD) && actor.x == ix && actor.y == iy
                }) {
                    actor.modify_health(damage);
                }
            }
        }
        new_state
    }
}
