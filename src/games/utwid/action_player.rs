use super::board::apply_dir;
use super::types::*;
use super::*;

impl UtwidAction {
    pub(super) fn execute_prescription(&self, state: &UtwidState) -> UtwidState {
        log::trace!("execute_prescription");
        let mut new_state = state.clone();
        new_state.prescription_turns = Some(PRESCRIPTION_TURNS);
        new_state
    }

    pub(super) fn execute_conclusion(&self, state: &UtwidState) -> (UtwidState, Vec<UtwidEvent>) {
        log::trace!("execute_conclusion");
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
            && !state.actors_iter().any(|(_, actor)| {
                !actor.traits.contains(ActorTraits::DEAD) && actor.x == new_x && actor.y == new_y
            })
        {
            (last_x, last_y) = (new_x, new_y);
            (new_x, new_y) = apply_dir(new_x, new_y, direction);
            damage_bonus += 1;
        }
        (new_x, new_y) = (last_x, last_y);

        let (mut new_state, mut events) = self.move_to(state, new_x, new_y);
        new_state.temporary_damage_bonus = Some(damage_bonus);

        if new_state
            .board
            .board_permitted_moves(new_x, new_y, true, true, true)
            .contains(&direction)
        {
            (new_x, new_y) = apply_dir(new_x, new_y, direction);
            let (final_state, more_events) = self.move_to(&new_state, new_x, new_y);
            events.extend(more_events);
            (final_state, events)
        } else {
            (new_state, events)
        }
    }

    pub(super) fn execute_stagnation(&self, state: &UtwidState) -> UtwidState {
        log::trace!("execute_stagnation");
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

    pub(super) fn execute_contention(&self, state: &UtwidState) -> (UtwidState, Vec<UtwidEvent>) {
        log::trace!("execute_contention");
        let direction = match self {
            UtwidAction::Contention(direction) => *direction,
            _ => unreachable!("execute_contention only handles Contention actions"),
        };
        let actor = state.actor(state.to_act).unwrap();

        let (_action, dx, dy) = CARDINAL_DIRS
            .iter()
            .chain(DIAGONAL_DIRS.iter())
            .find(|(action, _, _)| action == &direction)
            .unwrap()
            .clone();
        let (mut target_x, mut target_y) = (actor.x as isize + dx, actor.y as isize + dy);
        let mut _d = 0;
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
            _d += 1;
        }

        let dest_x = (target_x - CONTENTION_R_WIDTH as isize)
            .clamp(0, (BOARD_WIDTH - CONTENTION_NET_WIDTH) as isize) as usize;
        let dest_y = (target_y - CONTENTION_R_WIDTH as isize)
            .clamp(0, (BOARD_HEIGHT - CONTENTION_NET_WIDTH) as isize) as usize;

        // source is the area around the player
        // dest is the area being swapped with it
        let source_x = ((actor.x as isize) - CONTENTION_R_WIDTH as isize)
            .clamp(0, (BOARD_WIDTH - CONTENTION_NET_WIDTH) as isize)
            as usize;
        let source_y = ((actor.y as isize) - CONTENTION_R_WIDTH as isize)
            .clamp(0, (BOARD_HEIGHT - CONTENTION_NET_WIDTH) as isize)
            as usize;

        let source_geography: Vec<_> = (source_y..source_y + CONTENTION_NET_WIDTH)
            .flat_map(|y| {
                (source_x..source_x + CONTENTION_NET_WIDTH)
                    .map(move |x| state.board.geography[x + y * state.board.width].clone())
            })
            .collect();

        let dest_geography: Vec<_> = (dest_y..dest_y + CONTENTION_NET_WIDTH)
            .flat_map(|y| {
                (dest_x..dest_x + CONTENTION_NET_WIDTH)
                    .map(move |x| state.board.geography[x + y * state.board.width].clone())
            })
            .collect();

        let mut new_state = state.clone();

        for (i, tile) in source_geography.iter().enumerate() {
            let ix = i % CONTENTION_NET_WIDTH;
            let iy = i / CONTENTION_NET_WIDTH;
            new_state.board.geography[ix + dest_x + (iy + dest_y) * state.board.width] =
                tile.clone();
        }

        for (i, tile) in dest_geography.iter().enumerate() {
            let ix = i % CONTENTION_NET_WIDTH;
            let iy = i / CONTENTION_NET_WIDTH;
            new_state.board.geography[ix + source_x + (iy + source_y) * state.board.width] =
                tile.clone();
        }

        let has_goal_tile = new_state.board.geography.iter().any(|tile| {
            tile.traits.contains(TileTraits::STAIRS) || tile.traits.contains(TileTraits::WIN)
        });
        if !has_goal_tile {
            new_state.game_state = GameState::Stalemate;
        }

        (new_state, vec![UtwidEvent::Contention(dest_x, dest_y)])
    }

    pub(super) fn execute_multiplication(
        &self,
        state: &UtwidState,
    ) -> (UtwidState, Vec<UtwidEvent>) {
        log::trace!("execute_multiplication");
        let direction = match self {
            UtwidAction::Multiplication(direction) => *direction,
            _ => unreachable!("execute_multiplication only handles Multiplication actions"),
        };
        let mut new_state = state.clone();

        let actor = state.actor(state.to_act).unwrap();
        let (_action, dx, dy) = CARDINAL_DIRS
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
            && state.actors_iter().all(|(_, actor)| {
                actor.traits.contains(ActorTraits::DEAD)
                    || !(actor.x as isize == target_x && actor.y as isize == target_y)
            })
        {
            target_x += dx;
            target_y += dy;
        }
        target_x -= dx;
        target_y -= dy;

        let mut mult_actor = actor.clone();
        mult_actor.x = target_x as usize;
        mult_actor.y = target_y as usize;
        mult_actor.mon2y = Some(actor::you_mon2y_data());
        mult_actor.traits.remove(ActorTraits::HUMAN);
        mult_actor.assumed_turns = None;
        new_state.add_actor(mult_actor);

        (
            new_state,
            vec![UtwidEvent::Multiplication(
                target_x as usize,
                target_y as usize,
            )],
        )
    }

    pub(super) fn execute_assumption(&self, state: &UtwidState) -> (UtwidState, Vec<UtwidEvent>) {
        log::trace!("execute_assumption");
        let direction = match self {
            UtwidAction::Assumption(direction) => *direction,
            _ => unreachable!("execute_assumption only handles Assumption actions"),
        };

        let actor = state.actor(state.to_act).unwrap();
        let assumed_actor_id = state.first_actor_in_direction(actor, direction);
        let mut new_state = state.clone();

        if let Some(assumed_id) = assumed_actor_id {
            if let Some(assumed_actor) = new_state.actor(assumed_id).cloned() {
                if let Some(actor) = new_state.actor_mut(assumed_id) {
                    actor.assumed_turns = Some((ASSUMPTION_TURNS, Allegiance::You));
                }
                (new_state, vec![UtwidEvent::Assumed(assumed_actor)])
            } else {
                (new_state, vec![])
            }
        } else {
            (new_state, vec![])
        }
    }
}
