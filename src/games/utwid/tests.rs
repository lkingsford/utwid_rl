use super::types::*;
use super::*;
use crate::mcts::game_trait::{Action, Actor, State};
use crate::mcts::node::create_expanded_node;
use std::collections::VecDeque;

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
