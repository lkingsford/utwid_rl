use super::game::{Action, Actor, State};
use super::node::Node;
use super::weighted_random::weighted_random;
use super::Reward;
use core::panic;
use log::trace;
use rand::prelude::*;
use std::sync::{Arc, RwLock};

#[derive(Debug, PartialEq)]
pub enum Selection<ActionType: Action> {
    FullyExplored,
    Selection(Vec<ActionType>),
}

pub struct Tree<StateType: State, ActionType: Action<StateType = StateType>> {
    pub root: Arc<RwLock<Node<StateType, ActionType>>>,
    pub constant: f64,
}

impl<StateType: State<ActionType = ActionType>, ActionType: Action<StateType = StateType>>
    Tree<StateType, ActionType>
where
    StateType: State<ActionType = ActionType>,
    ActionType: Action<StateType = StateType>,
{
    fn node_ref(root: Node<StateType, ActionType>) -> Arc<RwLock<Node<StateType, ActionType>>> {
        // Only doing this to keep it a little tidier
        Arc::new(RwLock::new(root))
    }

    pub fn new(root: Node<StateType, ActionType>) -> Tree<StateType, ActionType> {
        Tree {
            root: Tree::node_ref(root),
            constant: 2.0_f64.sqrt(),
        }
    }

    pub fn new_with_constant(
        root: Node<StateType, ActionType>,
        constant: f64,
    ) -> Tree<StateType, ActionType> {
        Tree {
            root: Tree::node_ref(root),
            constant,
        }
    }

    ///
    /// Returns a path to the current selection
    ///
    pub fn selection(&self) -> Selection<ActionType> {
        return Tree::select_from(self.root.clone(), self.constant);
    }

    fn select_from(
        node: Arc<RwLock<Node<StateType, ActionType>>>,
        constant: f64,
    ) -> Selection<ActionType> {
        let best_pick: Vec<_> = super::node::best_pick(&node, constant)
            .iter()
            .map(|x| x.0.clone())
            .collect();
        if best_pick.is_empty() {
            return Selection::FullyExplored;
        }
        for action in best_pick.iter() {
            let child = { node.read().unwrap().get_child(action.clone()) };
            let is_expanded = {
                let node = child.read().unwrap();
                if let Node::Expanded { .. } = &*node {
                    true
                } else {
                    false
                }
            };
            if is_expanded {
                let selection = Tree::select_from(child, constant);
                match selection {
                    // FullyExplored shouldn't normally happen here (because
                    // best_pick will handle it) - but with multithreading, it's
                    // possible to change the state between the two calls.
                    // Trust me.
                    // It's annoying.
                    Selection::FullyExplored => {
                        trace!("FullyExplored hit in selection");
                        continue;
                    }
                    Selection::Selection(selection) => {
                        // TBD if this would be faster with .insert or
                        // preallocation
                        let mut result_selection = vec![action.clone()];
                        result_selection.extend(selection);
                        return Selection::Selection(result_selection);
                    }
                }
            } else {
                return Selection::Selection(vec![action.clone()]);
            }
        }
        Selection::FullyExplored
    }

    pub fn expansion(
        &self,
        selection: &Selection<ActionType>,
    ) -> Vec<Arc<RwLock<Node<StateType, ActionType>>>> {
        trace!("Expansion: Selection: {:#?}", selection);
        let mut cur_node = self.root.clone();
        // This root is needed as part of the output to ensure that propagate can work
        // It was either here or selection. Could fit in either place.
        // Could also be in iterate, but that was going to result in more memory allocations.
        let mut result: Vec<Arc<RwLock<Node<StateType, ActionType>>>> = vec![self.root.clone()];

        if let Selection::Selection(selection) = selection {
            for action in selection.iter() {
                {
                    let child_node = {
                        let node = cur_node.read().unwrap();
                        if let Node::Expanded { .. } = &*node {
                            node.get_child(action.clone()).clone()
                        } else {
                            continue;
                        }
                    };

                    {
                        let cur_state = {
                            let node = cur_node.read().unwrap();
                            node.state().clone()
                        };

                        let expanded_child = {
                            let read_node = child_node.read().unwrap();
                            if let Node::Placeholder { .. } = &*read_node {
                                Some(read_node.expansion(action.clone(), &cur_state))
                            } else {
                                None
                            }
                        };

                        if let Some(expanded_child) = expanded_child {
                            cur_node
                                .write()
                                .unwrap()
                                .insert_child(action.clone(), expanded_child);
                        }
                    }

                    result.push(cur_node);
                    cur_node = child_node;
                }
            }
        }
        result
    }

    pub fn play_out(&self, state: StateType) -> Vec<Reward> {
        let mut rng = rand::rng();

        let mut cur_state = Box::new(state.clone());

        while !cur_state.terminal() {
            match cur_state.next_actor() {
                Actor::Player(_) => {
                    let permitted_actions = cur_state.permitted_actions();

                    let action: ActionType =
                        permitted_actions[rng.random_range(0..permitted_actions.len())].clone();
                    cur_state = Box::new(action.execute(&cur_state));
                }
                Actor::GameAction(actions) => {
                    let action = weighted_random(actions);
                    cur_state = Box::new(action.execute(&cur_state));
                }
            }
        }
        trace!("Reward is {:?}", cur_state.reward());
        cur_state.reward()
    }

    pub fn propagate_reward(
        &self,
        nodes: Vec<Arc<RwLock<Node<StateType, ActionType>>>>,
        reward: Vec<Reward>,
    ) {
        for node_arc in nodes.iter() {
            let (actor, is_expanded) = {
                let node = node_arc.read().unwrap();
                if let Node::Expanded { state, .. } = &*node {
                    (state.next_actor(), true)
                } else {
                    (Actor::Player(0), false) // Actor doesn't matter, won't be visited
                }
            };

            if is_expanded {
                let mut cur_node = node_arc.write().unwrap();
                cur_node.visit(match actor {
                    Actor::Player(player_id) => *reward.get(player_id as usize).unwrap_or(&0.0),
                    _ => 0.0,
                })
            }
        }
    }

    pub fn iterate(&self) -> Selection<ActionType> {
        let selection = self.selection();
        if let Selection::FullyExplored = selection {
            log::warn!("Iterate short circuited - fully explored");
            return Selection::FullyExplored;
        };
        let expanded_nodes = self.expansion(&selection);
        if let Selection::Selection(..) = selection {
            let reward = {
                self.play_out(
                    expanded_nodes
                        .last()
                        .unwrap()
                        .read()
                        .unwrap()
                        .state()
                        .clone(),
                )
            };
            self.propagate_reward(expanded_nodes, reward);
        }
        selection
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mon2y::node::create_expanded_node;
    use crate::test::injectable_game::{InjectableGameAction, InjectableGameState};
    use std::vec;

    ///
    /// Test that selection returns the unexplored path at the next node
    ///
    #[test]
    fn test_selection_basic() {
        let root_state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![
                InjectableGameAction::WinInXTurns(2),
                InjectableGameAction::WinInXTurns(3),
            ],
            player_count: 1,
            next_actor: Actor::Player(0),
        };

        let explored_state = InjectableGameAction::WinInXTurns(2).execute(&root_state);
        let mut root = create_expanded_node(root_state, None);

        let mut explored_node = create_expanded_node(explored_state, None);
        explored_node.visit(0.0f64);

        root.insert_child(InjectableGameAction::WinInXTurns(2), explored_node);
        root.insert_child(
            InjectableGameAction::WinInXTurns(3),
            Node::Placeholder { weight: None },
        );
        root.visit(0.0f64);
        let tree = Tree::new(root);

        assert_eq!(
            tree.selection(),
            Selection::Selection(vec![InjectableGameAction::WinInXTurns(3)])
        );
    }

    ///
    /// Test that selection returns the unexplored path at the next node
    ///
    #[test]
    fn test_selection_multiple_expanded() {
        let root_state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![
                InjectableGameAction::WinInXTurns(2),
                InjectableGameAction::WinInXTurns(3),
            ],
            player_count: 1,
            next_actor: Actor::Player(0),
        };

        let mut explored_state_1 = InjectableGameAction::WinInXTurns(2).execute(&root_state);
        explored_state_1.injected_permitted_actions = vec![InjectableGameAction::WinInXTurns(1)];
        let explored_state_2 = InjectableGameAction::WinInXTurns(3).execute(&root_state);
        let mut root = create_expanded_node(root_state, None);

        let mut explored_node_1 = create_expanded_node(explored_state_1, None);
        explored_node_1.visit(0.0f64);
        explored_node_1.insert_child(
            InjectableGameAction::WinInXTurns(1),
            Node::Placeholder { weight: None },
        );

        let mut explored_node_2 = create_expanded_node(explored_state_2, None);
        explored_node_2.visit(-1.0f64);
        explored_node_2.visit(0.0f64);

        root.insert_child(InjectableGameAction::WinInXTurns(2), explored_node_1);
        root.insert_child(InjectableGameAction::WinInXTurns(3), explored_node_2);
        root.visit(0.0f64);
        root.visit(0.0f64);
        root.visit(0.0f64);
        let tree = Tree::new(root);

        assert_eq!(
            tree.selection(),
            Selection::Selection(vec![
                InjectableGameAction::WinInXTurns(2),
                InjectableGameAction::WinInXTurns(1)
            ])
        );
    }

    #[test]
    fn test_expansion_basic() {
        let root_state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![
                InjectableGameAction::WinInXTurns(2),
                InjectableGameAction::WinInXTurns(3),
            ],
            player_count: 1,
            next_actor: Actor::Player(0),
        };
        let mut explored_state_1 = InjectableGameAction::WinInXTurns(2).execute(&root_state);
        explored_state_1.injected_permitted_actions =
            vec![InjectableGameAction::NextTurnInjectActionCount(5)];

        let explored_state_2 = InjectableGameAction::WinInXTurns(3).execute(&root_state);
        let mut root = create_expanded_node(root_state, None);

        let mut explored_node_1 = create_expanded_node(explored_state_1, None);
        explored_node_1.visit(0.0f64);
        explored_node_1.insert_child(
            InjectableGameAction::NextTurnInjectActionCount(5),
            Node::Placeholder { weight: None },
        );

        let mut explored_node_2 = create_expanded_node(explored_state_2, None);
        explored_node_2.visit(-1.0f64);
        explored_node_2.visit(0.0f64);

        root.insert_child(InjectableGameAction::WinInXTurns(2), explored_node_1);
        root.insert_child(InjectableGameAction::WinInXTurns(3), explored_node_2);

        let selection_path = vec![
            InjectableGameAction::WinInXTurns(2),
            InjectableGameAction::NextTurnInjectActionCount(5),
        ];
        let selection = Selection::Selection(selection_path.clone());

        let tree = Tree::new(root);
        tree.expansion(&selection);
        let node_path = tree.root.clone();
        let node_ref = node_path.read().unwrap().get_node_by_path(selection_path);
        let node = node_ref.read().unwrap();
        if let Node::Expanded { children, .. } = &*node {
            assert_eq!(children.len(), 5);
        } else {
            self::panic!("Node is not expanded");
        }
    }

    #[test]
    fn test_play_out() {
        let root_state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![InjectableGameAction::WinInXTurns(3)],
            player_count: 1,
            next_actor: Actor::Player(0),
        };

        let explored_state = InjectableGameAction::WinInXTurns(2).execute(&root_state);
        let root = create_expanded_node(root_state, None);
        let tree = Tree::new(root);
        let reward = tree.play_out(explored_state);

        assert_eq!(reward, vec![1.0]);
    }

    #[test]
    fn test_propagate_one_player() {
        let root_state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![
                InjectableGameAction::WinInXTurns(2),
                InjectableGameAction::WinInXTurns(3),
            ],
            player_count: 1,
            next_actor: Actor::Player(0),
        };

        let explored_state = InjectableGameAction::WinInXTurns(2).execute(&root_state);
        let mut root = create_expanded_node(root_state, None);

        let mut explored_node = create_expanded_node(explored_state, None);

        let mut child_node = create_expanded_node(
            InjectableGameAction::WinInXTurns(1).execute(&explored_node.state()),
            None,
        );

        let grandchild_state = InjectableGameAction::Win.execute(&child_node.state());
        let grandchild_node = create_expanded_node(grandchild_state, None);

        child_node.insert_child(InjectableGameAction::Win, grandchild_node);
        explored_node.insert_child(InjectableGameAction::WinInXTurns(1), child_node);
        root.insert_child(InjectableGameAction::WinInXTurns(2), explored_node);
        let tree = Tree::new(root);

        let path = vec![
            InjectableGameAction::WinInXTurns(2),
            InjectableGameAction::WinInXTurns(1),
            InjectableGameAction::Win,
        ];
        let owned_root = tree.root.clone();
        // Todo: Think about ways to tidy this.
        let nodes = vec![
            tree.root.clone(),
            owned_root
                .read()
                .unwrap()
                .get_child(InjectableGameAction::WinInXTurns(2))
                .clone(),
            owned_root
                .read()
                .unwrap()
                .get_child(InjectableGameAction::WinInXTurns(2))
                .read()
                .unwrap()
                .get_child(InjectableGameAction::WinInXTurns(1))
                .clone(),
            owned_root
                .read()
                .unwrap()
                .get_child(InjectableGameAction::WinInXTurns(2))
                .read()
                .unwrap()
                .get_child(InjectableGameAction::WinInXTurns(1))
                .read()
                .unwrap()
                .get_child(InjectableGameAction::Win)
                .clone(),
        ];

        let check_path = path.clone();
        const REWARD: f64 = 0.8;
        tree.propagate_reward(nodes, vec![REWARD]);

        for path_i in 1..=check_path.len() {
            let semi_path = check_path[0..path_i].to_vec();
            let node_ref = tree.root.read().unwrap().get_node_by_path(semi_path);
            let node = node_ref.read().unwrap();
            assert_eq!(node.value_sum(), REWARD);
            assert_eq!(node.visit_count(), 1);
        }
    }

    #[test]
    fn test_propagate_two_players() {
        let root_state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![
                InjectableGameAction::WinInXTurns(2),
                InjectableGameAction::WinInXTurns(3),
            ],
            player_count: 2,
            next_actor: Actor::Player(0),
        };

        let explored_state = InjectableGameAction::WinInXTurns(2).execute(&root_state);
        let mut root = create_expanded_node(root_state, None);

        let mut explored_node = create_expanded_node(explored_state, None);

        let mut child_node = create_expanded_node(
            InjectableGameAction::WinInXTurns(1).execute(&explored_node.state()),
            None,
        );

        let grandchild_state = InjectableGameAction::Win.execute(&child_node.state());
        let grandchild_node = create_expanded_node(grandchild_state, None);

        child_node.insert_child(InjectableGameAction::Win, grandchild_node);
        explored_node.insert_child(InjectableGameAction::WinInXTurns(1), child_node);
        root.insert_child(InjectableGameAction::WinInXTurns(2), explored_node);
        let tree = Tree::new(root);

        let path = vec![
            InjectableGameAction::WinInXTurns(2),
            InjectableGameAction::WinInXTurns(1),
            InjectableGameAction::Win,
        ];
        let owned_root = tree.root.clone();
        // Not super pleased with this here either
        let nodes = vec![
            tree.root.clone(),
            owned_root
                .read()
                .unwrap()
                .get_child(InjectableGameAction::WinInXTurns(2))
                .clone(),
            owned_root
                .read()
                .unwrap()
                .get_child(InjectableGameAction::WinInXTurns(2))
                .read()
                .unwrap()
                .get_child(InjectableGameAction::WinInXTurns(1))
                .clone(),
            owned_root
                .read()
                .unwrap()
                .get_child(InjectableGameAction::WinInXTurns(2))
                .read()
                .unwrap()
                .get_child(InjectableGameAction::WinInXTurns(1))
                .read()
                .unwrap()
                .get_child(InjectableGameAction::Win)
                .clone(),
        ];

        let check_path = path.clone();
        // Using slightly unusual rewards to just make more certain that it was actually this reward
        const REWARD: f64 = 0.8;
        const LOSS_REWARD: f64 = -0.6;
        tree.propagate_reward(nodes, vec![REWARD, LOSS_REWARD]);

        for path_i in 1..=check_path.len() {
            // This isn't the greatest way to do this - maybe we should be just looking it up in a
            // table.
            let semi_path = check_path[0..path_i].to_vec();
            let player_id = (path_i + 1) % 2;
            let node_ref = tree.root.read().unwrap().get_node_by_path(semi_path);
            let node = node_ref.read().unwrap();
            if player_id == 0 {
                assert_eq!(node.value_sum(), REWARD);
                assert_eq!(node.visit_count(), 1);
            } else {
                assert_eq!(node.value_sum(), LOSS_REWARD);
                assert_eq!(node.visit_count(), 1);
            }
        }
    }

    #[test]
    fn test_weighted_game_action_play_out() {
        let root_state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![],
            player_count: 1,
            next_actor: Actor::GameAction(vec![
                (InjectableGameAction::Lose, 1),
                (InjectableGameAction::Win, 2),
            ]),
        };

        let root = create_expanded_node(root_state.clone(), None);
        let tree = Tree::new(root);

        let mut weight_1_visits = 0;
        let mut weight_2_visits = 0;
        for _ in 0..1000 {
            let reward = tree.play_out(root_state.clone());
            if reward[0] < 0.0 {
                weight_1_visits += 1
            } else {
                weight_2_visits += 1
            };
        }

        let tolerance = 0.1;
        let ratio = weight_1_visits as f32 / weight_2_visits as f32;
        assert!(
            (ratio - (1.0 / 2.0)).abs() < tolerance,
            "Ratio was {}, expected {} +/- {}",
            ratio,
            1.0 / 2.0,
            tolerance
        );
    }
}
