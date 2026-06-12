use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use log::{debug, trace};

use super::BestTurnPolicy;
use super::game_trait::{Action, Actor, State};
use super::node::{Node, create_expanded_node};
use super::tree::{Selection, Tree};

/// Run multiple iterations of the MCTS algorithm on a state.
pub fn run_mcts_iterations<
    StateType: State<ActionType = ActionType> + Sync + Send + 'static,
    ActionType: Action<StateType = StateType> + Sync + Send + 'static,
>(
    tree: Arc<Tree<StateType, ActionType>>,
    iterations: usize,
    time_limit: Option<std::time::Duration>,
    thread_count: usize,
) {
    let mut threads = vec![];

    let finished_iterations: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

    for _ in 0..thread_count {
        let tree_clone: Arc<Tree<StateType, ActionType>> = Arc::clone(&tree);
        let finished_iterations_clone: Arc<AtomicUsize> = Arc::clone(&finished_iterations);
        let time_started = std::time::Instant::now();
        threads.push(std::thread::spawn(move || {
            loop {
                {
                    debug!(
                        "Starting iteration {}",
                        finished_iterations_clone.load(std::sync::atomic::Ordering::SeqCst)
                    );
                    let result = tree_clone.iterate();
                    let current_iterations =
                        finished_iterations_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    trace!("Finished iteration {}", current_iterations);
                    if current_iterations >= iterations
                        || matches!(result, Selection::FullyExplored)
                        || time_started.elapsed() > time_limit.unwrap_or(std::time::Duration::MAX)
                    {
                        break;
                    }
                }
            }
        }));
    }

    for thread in threads {
        if let Err(e) = thread.join() {
            log::error!("A worker thread panicked: {:?}", e);
        }
    }

    log::debug!(
        "Completed {} iterations",
        finished_iterations.load(std::sync::atomic::Ordering::SeqCst)
    );
}

/// Run multiple iterations of the MCTS algorithm on a state.
pub fn calculate_best_turn<
    StateType: State<ActionType = ActionType> + Sync + Send + 'static,
    ActionType: Action<StateType = StateType> + Sync + Send + 'static,
>(
    iterations: usize,
    time_limit: Option<std::time::Duration>,
    thread_count: usize,
    state: StateType,
    policy: BestTurnPolicy,
    exploration_constant: f64,
    log_children: bool,
    existing_tree: Option<Arc<Tree<StateType, ActionType>>>,
) -> (
    <StateType as State>::ActionType,
    Option<Arc<Tree<StateType, ActionType>>>,
)
where
    StateType: State<ActionType = ActionType>,
    ActionType: Action<StateType = StateType>,
{
    log::debug!("Starting next turn");
    let per = match state.next_actor() {
        Actor::Player(player_id) => Some(player_id),
        Actor::GameAction(_) => None,
    };
    let root_node = create_expanded_node(state, None, per);
    if let Node::Expanded { children, .. } = &root_node {
        if children.is_empty() {
            panic!("calculate_best_turn called with a root state that has no available actions");
        }
        if children.len() == 1 {
            log::debug!("Short circuited - only one option");
            return (children.keys().next().unwrap().clone(), None);
        }
    }

    let tree = match existing_tree {
        Some(existing_tree) => existing_tree,
        None => Arc::new(Tree::new_with_constant_and_per(
            root_node,
            exploration_constant,
            per,
        )),
    };

    run_mcts_iterations(tree.clone(), iterations, time_limit, thread_count);

    if log::log_enabled!(log::Level::Trace) || log_children {
        tree.root.clone().read().unwrap().log_children(0);
    }
    let root_ref = tree.root.clone();
    match policy {
        BestTurnPolicy::Ucb0 => {
            let node = root_ref.read().unwrap();
            let root_player = match node.state().next_actor() {
                Actor::Player(player_id) => player_id,
                Actor::GameAction(_) => {
                    panic!("BestTurnPolicy::Ucb0 expects a player turn at root")
                }
            };
            // This bit of logic is reimplemented due to crashing when tree is fully explored
            let mut picks = match &*node {
                Node::Expanded { children, .. } => children
                    .iter()
                    // TODO: Add random factor
                    .map(|(action, child)| {
                        let child = child.read().unwrap();
                        (
                            action.clone(),
                            child.value_sum_for_player(root_player)
                                / if child.visit_count() > 0 {
                                    child.visit_count() as f64
                                } else {
                                    f64::INFINITY
                                },
                        )
                    })
                    .collect::<Vec<_>>(),
                _ => panic!("Root should be parent"),
            };
            picks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            log::debug!("Action, UCB0: {:?}", picks);
            (picks[0].0.clone(), Some(tree))
        }

        BestTurnPolicy::MostVisits => {
            let root = root_ref.read().unwrap();
            if let Node::Expanded { children, .. } = &*root {
                log::debug!(
                    "Action, Visits, Value: {:?}",
                    children
                        .iter()
                        .map(|(action, node)| {
                            let node = node.read().unwrap();
                            (
                                action.clone(),
                                node.visit_count(),
                                node.value_sums_ref()
                                    .iter()
                                    .map(|value| value.value_sum)
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect::<Vec<_>>()
                );
                // Short circuit on a winning move
                // Implemented because (I think) the UCB formula doesn't end up prioritizing
                // certainly winning moves, because they're already explored. Dunno if this
                // is a cludge though.
                let winning_moves: Vec<ActionType> = children
                    .iter()
                    .filter_map(|(action, node)| {
                        let node = node.read().unwrap();
                        if let Node::Placeholder { .. } = &*node {
                            return None;
                        }
                        if node.state().terminal() {
                            let actor = root.state().next_actor();
                            if let Actor::Player(player_id) = actor {
                                if let Some((index, _)) =
                                    // Annoying - but necessary because I was dumb enough to use f64
                                    // (otherwise, it'd be max_by_key)
                                    node.state().reward().iter().enumerate().max_by(
                                            |(_, a), (_, b)| {
                                                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Less)
                                            },
                                        )
                                {
                                    if index == player_id as usize {
                                        return Some(action.clone());
                                    }
                                }
                            }
                        }
                        None
                    })
                    .collect();
                if let Some(action) = winning_moves.first() {
                    return (action.clone(), Some(tree));
                }

                (
                    children
                        .iter()
                        .max_by_key(|(_, node)| node.read().unwrap().visit_count())
                        .unwrap()
                        .0
                        .clone(),
                    Some(tree),
                )
            } else {
                panic!("Expected root to be an expanded node")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcts::game_trait::Actor;
    use crate::mcts::node::Node;
    use crate::test::injectable_game::{
        InjectableGameAction, InjectableGameState, TestHyperreward,
    };
    use std::collections::HashMap;

    #[test]
    fn calculate_best_turn_does_not_expand_hidden_opponent_win() {
        let real_actions = vec![
            InjectableGameAction::Win,
            InjectableGameAction::WinInXTurns(2),
            InjectableGameAction::WinInXTurns(3),
        ];
        let hidden_actions = vec![
            InjectableGameAction::WinInXTurns(2),
            InjectableGameAction::WinInXTurns(3),
        ];
        let scout_action = InjectableGameAction::NextTurnGameAction(real_actions.clone());

        let state = InjectableGameState {
            injected_reward: vec![0.0, 0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![scout_action.clone(), InjectableGameAction::Lose],
            perceived_permitted_actions: HashMap::from([
                ((1, 0), hidden_actions.clone()),
                ((1, 1), real_actions.clone()),
            ]),
            player_count: 2,
            next_actor: Actor::Player(0),
            injected_hyperreward: TestHyperreward { value: 0 },
            terminal_hyperreward: TestHyperreward { value: 1 },
        };

        let (_, tree) = calculate_best_turn(
            10,
            None,
            1,
            state,
            BestTurnPolicy::MostVisits,
            2.0_f64.sqrt(),
            false,
            None,
        );

        let tree = tree.expect("tree should be returned when the root has multiple actions");
        let root = tree.root.read().unwrap();
        let opponent_node = root.get_child(&scout_action);
        let opponent_node = opponent_node.read().unwrap();

        match &*opponent_node {
            Node::Expanded { children, .. } => {
                assert!(!children.contains_key(&InjectableGameAction::Win));
                assert!(children.contains_key(&InjectableGameAction::WinInXTurns(2)));
                assert!(children.contains_key(&InjectableGameAction::WinInXTurns(3)));
            }
            Node::Placeholder { .. } => panic!("expected opponent node to be expanded"),
        }
    }

    #[test]
    fn perceived_actions_can_still_include_hidden_win_for_owner() {
        let real_actions = vec![
            InjectableGameAction::Win,
            InjectableGameAction::WinInXTurns(2),
            InjectableGameAction::WinInXTurns(3),
        ];

        let state = InjectableGameState {
            injected_reward: vec![0.0, 0.0],
            injected_terminal: false,
            injected_permitted_actions: real_actions.clone(),
            perceived_permitted_actions: HashMap::from([
                (
                    (1, 0),
                    vec![
                        InjectableGameAction::WinInXTurns(2),
                        InjectableGameAction::WinInXTurns(3),
                    ],
                ),
                ((1, 1), real_actions.clone()),
            ]),
            player_count: 2,
            next_actor: Actor::Player(1),
            injected_hyperreward: TestHyperreward { value: 0 },
            terminal_hyperreward: TestHyperreward { value: 1 },
        };

        assert_eq!(
            state.permitted_actions(Some(0)),
            vec![
                InjectableGameAction::WinInXTurns(2),
                InjectableGameAction::WinInXTurns(3),
            ]
        );
        assert_eq!(state.permitted_actions(Some(1)), real_actions);
    }
}
