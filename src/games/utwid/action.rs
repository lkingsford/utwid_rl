use crate::mcts::game_trait::Action;

use super::types::*;
use super::*;

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

    Conclusion(Dir),     // Jump to a position
    Redemption,          // Jump through a line of actors, injuring all
    Contemplation(Dir),  // Hit adjacent into wall
    Stagnation(Dir),     // Create a wall
    Prescription,        // Take multiple moves in a row
    Attention,           // Pull a whole direction closer
    Multiplication(Dir), // Create a duplicate player
    Contention(Dir),     // Glitch the map
    Assumption,          // Take over a person
}

pub fn action_cost(action: UtwidAction) -> usize {
    match action {
        UtwidAction::Conclusion(_) => 1,
        UtwidAction::Redemption => 1,
        UtwidAction::Attention => 1,
        UtwidAction::Stagnation(_) => 2,
        UtwidAction::Prescription => 2,
        UtwidAction::Contemplation(_) => 2,
        UtwidAction::Multiplication(_) => 3,
        UtwidAction::Contention(_) => 3,
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
            UtwidAction::Prescription => self.execute_prescription(state),
            UtwidAction::Conclusion(_) => self.execute_conclusion(state),
            UtwidAction::Stagnation(_) => self.execute_stagnation(state),
            UtwidAction::Contention(_) => self.execute_contention(state),
            UtwidAction::Multiplication(_) => self.execute_multiplication(state),
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
