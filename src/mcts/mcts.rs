use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use log::trace;

use crate::hyper::Hyperrewards;

use super::game_trait::{Action, Actor, State};
use super::node::{create_expanded_node, Node};
use super::sender::{MctsSender, NoopSender};
use super::tree::{Selection, Tree};
use super::BestTurnPolicy;

/// Run multiple iterations of the MCTS algorithm on a state.
pub fn run_mcts_iterations<
    StateType: State<ActionType = ActionType> + Sync + Send + 'static,
    ActionType: Action<StateType = StateType> + Sync + Send + 'static,
>(
    tree: Arc<Tree<StateType, ActionType>>,
    iterations: usize,
    time_limit: Option<std::time::Duration>,
    thread_count: usize,
    sender: Box<dyn MctsSender<Hyperrewards<StateType::GameHyperrewardType>>>,
) {
    let mut threads = vec![];

    let finished_iterations: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

    for _ in 0..thread_count {
        let tree_clone: Arc<Tree<StateType, ActionType>> = Arc::clone(&tree);
        let finished_iterations_clone: Arc<AtomicUsize> = Arc::clone(&finished_iterations);
        let time_started = std::time::Instant::now();
        let sender_clone = sender.clone_sender();
        threads.push(std::thread::spawn(move || loop {
            {
                trace!(
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
                    // If the tree is fully explored, we want to notify the sender
                    if let Selection::Selection(selection_result) = result {
                        let hyperrewards_to_send = Hyperrewards {
                            // TBD if this should be adding here, or just the 'selected turns' and
                            // add it later.
                            turns: selection_result.random_walk_steps
                                + selection_result.selected_steps,
                            rwalk: selection_result.random_walk_steps as u32,
                            sum_diff_est_reward: selection_result.sum_diff_est_reward,
                            game_hrs: selection_result.round_hyperreward.unwrap(),
                        };
                        if sender_clone.send(hyperrewards_to_send).is_err() {
                            trace!("Receiver has been dropped, not sending final rewards.");
                        }
                    }
                    break;
                }
                if let Selection::Selection(selection_result) = result {
                    // Send the round_hyperreward
                    let hyperrewards_to_send = Hyperrewards {
                        // Like above - should figure out if we expose selected turns or overall
                        turns: selection_result.random_walk_steps + selection_result.selected_steps,
                        rwalk: selection_result.random_walk_steps as u32,
                        sum_diff_est_reward: selection_result.sum_diff_est_reward,
                        game_hrs: selection_result.round_hyperreward.unwrap(),
                    };
                    if sender_clone.send(hyperrewards_to_send).is_err() {
                        trace!("Receiver has been dropped, exiting thread.");
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
    existing_tree: Option<Tree<StateType, ActionType>>,
) -> (
    <StateType as State>::ActionType,
    Option<Tree<StateType, ActionType>>,
)
where
    StateType: State<ActionType = ActionType>,
    ActionType: Action<StateType = StateType>,
    <StateType as State>::GameHyperrewardType: Clone,
{
    log::debug!("Starting next turn");
    let root_node = create_expanded_node(state, None);
    if let Node::Expanded { children, .. } = &root_node {
        if children.is_empty() {
            panic!("calculate_best_turn called with a root state that has no available actions");
        }
        if children.len() == 1 {
            log::debug!("Short circuited - only one option");
            return (children.keys().next().unwrap().clone(), None);
        }
    }

    let tree = Arc::new(match existing_tree {
        Some(existing_tree) => existing_tree,
        None => Tree::new_with_constant(root_node, exploration_constant),
    });

    let noop_sender = Box::new(NoopSender::new());
    run_mcts_iterations(
        tree.clone(),
        iterations,
        time_limit,
        thread_count,
        noop_sender,
    );

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
            (picks[0].0.clone(), (Arc::try_unwrap(tree).ok()))
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
                                node.value_sums_ref().to_vec(),
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
                    return (action.clone(), (Arc::try_unwrap(tree).ok()));
                }

                (
                    children
                        .iter()
                        .max_by_key(|(_, node)| node.read().unwrap().visit_count())
                        .unwrap()
                        .0
                        .clone(),
                    (Arc::try_unwrap(tree).ok()),
                )
            } else {
                panic!("Expected root to be an expanded node")
            }
        }
    }
}

/// Explore a tree to get the hyperrewards
pub fn explore_tree<
    StateType: State<ActionType = ActionType> + Sync + Send + 'static,
    ActionType: Action<StateType = StateType> + Sync + Send + 'static,
>(
    iterations: usize,
    time_limit: Option<std::time::Duration>,
    thread_count: usize,
    state: StateType,
    exploration_constant: f64,
) -> Vec<Hyperrewards<StateType::GameHyperrewardType>>
where
    StateType: State<ActionType = ActionType>,
    ActionType: Action<StateType = StateType>,
    <StateType as State>::GameHyperrewardType: Clone,
{
    log::debug!("Starting explore tree");
    let (tx, rx) = std::sync::mpsc::channel();
    let root_node = create_expanded_node(state, None);
    if let Node::Expanded { children, .. } = &root_node {
        if children.len() == 1 {
            log::debug!("Short circuited - only one option");
            return vec![];
        }
    }

    let tree = Arc::new(Tree::new_with_constant(root_node, exploration_constant));
    let sender = Box::new(tx);
    run_mcts_iterations(tree.clone(), iterations, time_limit, thread_count, sender);

    rx.iter().collect()
}
