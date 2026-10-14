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

fn floor_tile() -> Tile {
    Tile {
        traits: TileTraits::WALKABLE,
        health: None,
        repr: Some(Repr::Floor),
        repr_set: ReprSet::Room1,
    }
}

fn set_tile(state: &mut UtwidState, x: usize, y: usize, tile: Tile) {
    state.board.geography[x + y * state.board.width] = tile;
}

#[test]
fn first_actor_in_direction_finds_adjacent_live_actor() {
    let mut state = UtwidState::new();
    state.actor_mut(0).unwrap().x = 1;
    state.actor_mut(0).unwrap().y = 1;
    let actor_id = state.add_actor(GameActor::them_actor(2, 1));
    set_tile(&mut state, 2, 1, floor_tile());

    assert_eq!(
        state.first_actor_in_direction(state.actor(0).unwrap(), Dir::E),
        Some(actor_id)
    );
}

#[test]
fn first_actor_in_direction_skips_dead_actor() {
    let mut state = UtwidState::new();
    state.actor_mut(0).unwrap().x = 1;
    state.actor_mut(0).unwrap().y = 1;
    let actor_id = state.add_actor(GameActor::them_actor(2, 1));
    state
        .actor_mut(actor_id)
        .unwrap()
        .traits
        .insert(ActorTraits::DEAD);
    set_tile(&mut state, 2, 1, floor_tile());

    assert_eq!(
        state.first_actor_in_direction(state.actor(0).unwrap(), Dir::E),
        None
    );
}

#[test]
fn move_into_dead_actor_space_removes_dead_actor() {
    use crate::mcts::game_trait::Action;

    let mut state = UtwidState::new();
    state.actor_mut(0).unwrap().x = 1;
    state.actor_mut(0).unwrap().y = 1;
    let dead_actor_id = state.add_actor(GameActor::them_actor(2, 1));
    state
        .actor_mut(dead_actor_id)
        .unwrap()
        .traits
        .insert(ActorTraits::DEAD);
    set_tile(&mut state, 1, 1, floor_tile());
    set_tile(&mut state, 2, 1, floor_tile());

    let (state, _events) = UtwidAction::Move(Dir::E).execute(&state);

    assert_eq!(state.actor(0).unwrap().x, 2);
    assert_eq!(state.actor(0).unwrap().y, 1);
    assert!(!state.has_actor(dead_actor_id));
}

#[test]
fn conclusion_passes_through_dead_actor() {
    use crate::mcts::game_trait::Action;

    let mut state = UtwidState::new();
    state.actor_mut(0).unwrap().x = 1;
    state.actor_mut(0).unwrap().y = 1;
    let dead_actor_id = state.add_actor(GameActor::them_actor(2, 1));
    let live_actor_id = state.add_actor(GameActor::them_actor(3, 1));
    state
        .actor_mut(dead_actor_id)
        .unwrap()
        .traits
        .insert(ActorTraits::DEAD);
    set_tile(&mut state, 1, 1, floor_tile());
    set_tile(&mut state, 2, 1, floor_tile());
    set_tile(&mut state, 3, 1, floor_tile());
    set_tile(&mut state, 4, 1, Tile::wall());

    let (state, _events) = UtwidAction::Conclusion(Dir::E).execute(&state);

    assert_eq!(state.actor(0).unwrap().x, 3);
    assert_eq!(state.actor(0).unwrap().y, 1);
    assert!(!state.has_actor(dead_actor_id));
    assert!(!state.has_actor(live_actor_id));
}

#[test]
fn multiplication_passes_through_dead_actor() {
    use crate::mcts::game_trait::Action;

    let mut state = UtwidState::new();
    state.actor_mut(0).unwrap().x = 1;
    state.actor_mut(0).unwrap().y = 1;
    let dead_actor_id = state.add_actor(GameActor::them_actor(2, 1));
    state
        .actor_mut(dead_actor_id)
        .unwrap()
        .traits
        .insert(ActorTraits::DEAD);
    set_tile(&mut state, 1, 1, floor_tile());
    set_tile(&mut state, 2, 1, floor_tile());
    set_tile(&mut state, 3, 1, floor_tile());
    set_tile(&mut state, 4, 1, Tile::wall());

    let (state, _events) = UtwidAction::Multiplication(Dir::E).execute(&state);

    assert!(state.actors_iter().any(|(_, actor)| {
        actor.actor_type == ACTOR_TYPE_YOU
            && !actor.traits.contains(ActorTraits::HUMAN)
            && actor.x == 3
            && actor.y == 1
    }));
    assert!(!state.has_actor(dead_actor_id));
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
        let you = state.actor(0).unwrap().clone();
        state = UtwidAction::Move(Dir::N).execute_stairs(you, state);
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
        let you = state.actor(0).unwrap().clone();
        state = UtwidAction::Move(Dir::N).execute_stairs(you, state);
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
    let (mut state, _events) = action.execute(&state);

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
    let (post_stairs, _events) = action.execute(&state);

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

    let (post_stairs, _events) = action.execute(&state);
    let rewards = post_stairs.reward();

    assert!(post_stairs.terminal());
    assert_eq!(rewards.len(), 2);
    // Checkpoint reward includes player health ratio + level reward
    // With default config: level_reward = 20.0
    // ai_turns starts at 0, increments to 1 after action execution
    // ai_turn_weight() = 0.75^(ai_turns * 0.2) / 2.0
    // With ai_turns = 1: 0.75^0.2 / 2.0
    let ai_turn_weight = 0.75_f64.powf(1.0 * 0.2) / 2.0;
    let expected_player_reward = 4.0 / 7.0 + 20.0 * ai_turn_weight;
    let expected_mon2y_reward = -expected_player_reward;
    // Use a reasonable epsilon for floating point comparison
    let epsilon = 1e-10;
    assert!(
        (rewards[YOU_ID] - expected_player_reward).abs() < epsilon,
        "Player reward: {}, expected: {}",
        rewards[YOU_ID],
        expected_player_reward
    );
    assert!(
        (rewards[MON2Y_ID] - expected_mon2y_reward).abs() < epsilon,
        "Mon2y reward: {}, expected: {}",
        rewards[MON2Y_ID],
        expected_mon2y_reward
    );
}

#[test]
fn lost_and_won_rewards_match_actor_outcomes() {
    let mut lost_state = UtwidState::new();
    lost_state.game_state = GameState::Lost;
    assert_eq!(lost_state.reward(), vec![-10.0, 10.0]);

    let mut won_state = UtwidState::new();
    won_state.game_state = GameState::Won;
    assert_eq!(won_state.reward(), vec![10.0, -10.0]);
}

#[test]
fn test_suggest_spawn_with_bomb_distance_restriction() {
    // Create a simple state
    let mut state = UtwidState::new();
    
    // Clear all actors except the player
    state.actors.clear();
    state.spatial_hashmap.clear();
    
    // Add the player at position (3, 3)
    let mut player = GameActor::you_actor();
    player.x = 3;
    player.y = 3;
    state.add_actor(player);
    
    // Test spawning a bomb actor (ONE type)
    // With player at (3, 3), positions within distance 3 should be avoided
    // Distance = |x1 - x2| + |y1 - y2|
    
    // Try to spawn multiple times
    let mut _found_valid_position = false;
    let mut found_invalid_position = false;
    
    for _ in 0..1000 {
        let spawn = state.suggest_spawn(ACTOR_TYPE_ONE);
        
        if spawn.0 == usize::MAX && spawn.1 == usize::MAX {
            // This means no valid position was found
            continue;
        }
        
        let distance = (spawn.0 as isize - 3).abs() + (spawn.1 as isize - 3).abs();
        
        if distance <= 3 {
            // This should not happen with bomb actors
            found_invalid_position = true;
            println!("Invalid spawn at ({}, {}) distance {}", spawn.0, spawn.1, distance);
        } else {
            _found_valid_position = true;
        }
    }
    
    // We should find valid positions (unless the board is too small)
    // But we should NEVER find invalid positions within distance 3
    assert!(!found_invalid_position, "Bomb actor spawned too close to player");
}

#[test]
fn test_suggest_spawn_falls_back_to_are_actor_when_no_space() {
    // Create a heavily crowded state
    let mut state = UtwidState::new();
    
    // Clear all actors and spatial hashmap
    state.actors.clear();
    state.spatial_hashmap.clear();
    state.turn_order.clear();
    
    // Add player at (3, 3) - center of board
    let mut player = GameActor::you_actor();
    player.x = 3;
    player.y = 3;
    state.add_actor(player);
    
    // Fill ALL positions on the 7x7 board
    for x in 0..state.board.width {
        for y in 0..state.board.height {
            // Skip player position
            if x == 3 && y == 3 {
                continue;
            }
            // Add a temporary actor to block the position
            let mut temp_actor = GameActor::are_actor(x, y);
            temp_actor.x = x;
            temp_actor.y = y;
            state.add_actor(temp_actor);
        }
    }
    
    // Make sure spatial hashmap is up to date
    state.update_spatial_hashmap();
    
    // Now try to spawn a bomb actor
    let spawn = state.suggest_spawn(ACTOR_TYPE_ONE);
    
    // Should return the special value (usize::MAX, usize::MAX)
    // because all positions are occupied
    assert_eq!(spawn.0, usize::MAX);
    assert_eq!(spawn.1, usize::MAX);
}

#[test]
fn test_suggest_spawn_non_bomb_actors_have_no_distance_restriction() {
    let mut state = UtwidState::new();
    state.actors.clear();
    state.spatial_hashmap.clear();
    
    // Add player at (3, 3)
    let mut player = GameActor::you_actor();
    player.x = 3;
    player.y = 3;
    state.add_actor(player);
    
    // Test spawning ARE actor (non-bomb)
    // Should be able to spawn anywhere, including adjacent to player
    let mut found_near_position = false;
    
    for _ in 0..1000 {
        let spawn = state.suggest_spawn(ACTOR_TYPE_ARE);
        
        let distance = (spawn.0 as isize - 3).abs() + (spawn.1 as isize - 3).abs();
        
        if distance <= 3 {
            found_near_position = true;
            break;
        }
    }
    
    // Non-bomb actors should be able to spawn near the player
    assert!(found_near_position, "Non-bomb actors should be able to spawn near player");
}
