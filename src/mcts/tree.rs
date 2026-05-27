use super::Reward;
use super::game_trait::{Action, Actor, State};
use super::node::Node;
use super::weighted_random::weighted_random;
use core::panic;
use log::trace;
use rand::Rng;
use std::sync::{Arc, RwLock};

#[derive(Debug, PartialEq, Clone)]
pub struct SelectionResult<ActionType: Action> {
    pub selection: Vec<ActionType>,
    pub random_walk_steps: u32,
    pub selected_steps: u32,
    pub sum_diff_est_reward: f64,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Selection<ActionType: Action> {
    FullyExplored,
    Selection(SelectionResult<ActionType>),
}
pub struct Tree<StateType: State, ActionType: Action<StateType = StateType>> {
    pub root: Arc<RwLock<Node<StateType, ActionType>>>,
    pub constant: f64,
    pub per: Option<u8>,
}

#[derive(Debug, PartialEq)]
pub struct PlayOutResult<GameHyperrewardType> {
    pub reward: Vec<Reward>,
    pub random_walk_steps: u32,
    pub round_hyperreward: GameHyperrewardType,
}

impl<StateType: State<ActionType = ActionType>, ActionType: Action<StateType = StateType>>
    Tree<StateType, ActionType>
where
    StateType: State<ActionType = ActionType>,
    ActionType: Action<StateType = StateType>,
{
    fn perspective_player(&self) -> u8 {
        if let Some(per) = self.per {
            return per;
        }

        let root = self.root.read().unwrap();
        match root.state().next_actor() {
            Actor::Player(player_id) => player_id,
            Actor::GameAction(_) => panic!("MCTS playout requires a player perspective"),
        }
    }

    fn node_ref(root: Node<StateType, ActionType>) -> Arc<RwLock<Node<StateType, ActionType>>> {
        // Only doing this to keep it a little tidier
        Arc::new(RwLock::new(root))
    }

    pub fn new(root: Node<StateType, ActionType>) -> Tree<StateType, ActionType> {
        Tree {
            root: Tree::node_ref(root),
            constant: 2.0_f64.sqrt(),
            per: None,
        }
    }

    pub fn new_with_constant(
        root: Node<StateType, ActionType>,
        constant: f64,
    ) -> Tree<StateType, ActionType> {
        Tree::new_with_constant_and_per(root, constant, None)
    }

    pub fn new_with_constant_and_per(
        root: Node<StateType, ActionType>,
        constant: f64,
        per: Option<u8>,
    ) -> Tree<StateType, ActionType> {
        Tree {
            root: Tree::node_ref(root),
            constant,
            per,
        }
    }

    ///
    /// Returns a path to the current selection
    ///
    pub fn selection(&self) -> Selection<ActionType> {
        return Tree::select_from(self.root.clone(), self.constant, 1);
    }

    fn select_from(
        node: Arc<RwLock<Node<StateType, ActionType>>>,
        constant: f64,
        depth: u32,
    ) -> Selection<ActionType> {
        let best_pick = super::node::best_pick(&node, constant);
        if best_pick.is_empty() {
            return Selection::FullyExplored;
        }

        let mean_reward = {
            let n = node.read().unwrap();
            match n.state().next_actor() {
                Actor::Player(player_id) => n.mean_child_est_reward_for_player(player_id),
                Actor::GameAction(_) => 0.0,
            }
        };

        for pick in best_pick.iter() {
            let child = { node.read().unwrap().get_child(&pick.action_to_take) };
            let is_expanded = {
                let node = child.read().unwrap();
                matches!(&*node, Node::Expanded { .. })
            };
            if is_expanded {
                let selection = Tree::select_from(child, constant, depth + 1);
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
                    Selection::Selection(selection_result) => {
                        // TBD if this would be faster with .insert or
                        // preallocation
                        let mut result_selection =
                            Vec::with_capacity(selection_result.selection.len() + 1);
                        result_selection.push(pick.action_to_take.clone());
                        result_selection.extend(selection_result.selection);

                        let diff_est_reward = pick.expected_value - mean_reward;

                        return Selection::Selection(SelectionResult {
                            selection: result_selection,
                            random_walk_steps: selection_result.random_walk_steps,
                            selected_steps: selection_result.selected_steps,
                            sum_diff_est_reward: selection_result.sum_diff_est_reward
                                + diff_est_reward,
                        });
                    }
                }
            } else {
                let diff_est_reward = pick.expected_value - mean_reward;
                return Selection::Selection(SelectionResult {
                    selection: vec![pick.action_to_take.clone()],
                    random_walk_steps: 0,
                    selected_steps: depth,
                    sum_diff_est_reward: diff_est_reward,
                });
            }
        }
        Selection::FullyExplored
    }

    pub fn expansion(
        &self,
        selection: &Selection<ActionType>,
    ) -> Vec<Arc<RwLock<Node<StateType, ActionType>>>> {
        trace!("Expansion: Selection: {:#?}", selection);
        let mut path: Vec<Arc<RwLock<Node<StateType, ActionType>>>> = vec![self.root.clone()];

        if let Selection::Selection(selection_result) = selection {
            if selection_result.selection.is_empty() {
                return path;
            }

            let mut parent_node = self.root.clone();
            for action in selection_result
                .selection
                .iter()
                .take(selection_result.selection.len() - 1)
            {
                let child = parent_node.read().unwrap().get_child(action);
                parent_node = child;
                path.push(parent_node.clone());
            }

            let leaf_action = selection_result.selection.last().unwrap();
            let leaf_node_arc = parent_node.read().unwrap().get_child(leaf_action);

            let is_placeholder = {
                let leaf_guard = leaf_node_arc.read().unwrap();
                matches!(&*leaf_guard, Node::Placeholder { .. })
            };

            if is_placeholder {
                let mut parent_guard = parent_node.write().unwrap();
                let current_leaf_arc = parent_guard.get_child(leaf_action);

                if Arc::ptr_eq(&leaf_node_arc, &current_leaf_arc) {
                    let parent_state = parent_guard.state().clone();
                    let expanded_node = current_leaf_arc.read().unwrap().expansion(
                        leaf_action.clone(),
                        &parent_state,
                        self.per,
                    );
                    parent_guard.insert_child(leaf_action.clone(), expanded_node);
                }
            }

            let final_leaf_node = parent_node.read().unwrap().get_child(leaf_action);
            path.push(final_leaf_node);
        }
        path
    }

    pub fn play_out(
        &self,
        state: StateType,
        per: u8,
    ) -> PlayOutResult<StateType::GameHyperrewardType> {
        let mut rng = rand::rng();

        let mut cur_state = state;

        let mut random_walk_steps = 0;

        while !cur_state.terminal() {
            random_walk_steps += 1;
            match cur_state.next_actor() {
                Actor::Player(_) => {
                    let permitted_actions = cur_state.permitted_actions(Some(per));

                    if permitted_actions.is_empty() {
                        log::warn!("Player has no permitted actions in a non-terminal state.");
                        break;
                    }

                    let action: ActionType =
                        permitted_actions[rng.random_range(0..permitted_actions.len())].clone();
                    cur_state = action.execute(&cur_state);
                }
                Actor::GameAction(actions) => {
                    let action = weighted_random(actions);
                    cur_state = action.execute(&cur_state);
                }
            }
        }
        trace!("Reward is {:?}", cur_state.reward());
        PlayOutResult {
            reward: cur_state.reward(),
            random_walk_steps: random_walk_steps,
            round_hyperreward: cur_state.round_hyperreward(),
        }
    }

    pub fn propagate_reward(
        &self,
        nodes: Vec<Arc<RwLock<Node<StateType, ActionType>>>>,
        reward: &[Reward],
    ) {
        for node in nodes.iter() {
            let mut cur_node = node.write().unwrap();
            cur_node.visit(reward);
        }
    }

    pub fn propagate_reward_filtered(
        &self,
        nodes: Vec<Arc<RwLock<Node<StateType, ActionType>>>>,
        reward: &[Reward],
    ) {
        for node in nodes.iter() {
            let mut cur_node = node.write().unwrap();
            cur_node.visit(reward);
        }
    }

    pub fn iterate(&self) -> Selection<ActionType> {
        let selection = self.selection();
        if let Selection::FullyExplored = selection {
            log::warn!("Iterate short circuited - fully explored");
            return Selection::FullyExplored;
        };
        let expanded_nodes = self.expansion(&selection);
        if let Selection::Selection(selection_result) = selection {
            let play_out_result = {
                self.play_out(
                    expanded_nodes
                        .last()
                        .unwrap()
                        .read()
                        .unwrap()
                        .state()
                        .clone(),
                    self.perspective_player(),
                )
            };
            self.propagate_reward(expanded_nodes, &play_out_result.reward);

            return Selection::Selection(SelectionResult {
                selection: selection_result.selection,
                random_walk_steps: play_out_result.random_walk_steps,
                selected_steps: selection_result.selected_steps,
                sum_diff_est_reward: selection_result.sum_diff_est_reward,
            });
        }
        panic!("Should be unreachable");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcts::node::create_expanded_node;
    use crate::test::injectable_game::{
        InjectableGameAction, InjectableGameState, TestHyperreward,
    };
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
            perceived_permitted_actions: Default::default(),
            player_count: 1,
            next_actor: Actor::Player(0),
            injected_hyperreward: Default::default(),
            terminal_hyperreward: Default::default(),
        };

        let explored_state = InjectableGameAction::WinInXTurns(2).execute(&root_state);
        let mut root = create_expanded_node(root_state, None, None);

        let mut explored_node = create_expanded_node(explored_state, None, None);
        explored_node.visit(&[0.0f64]);

        root.insert_child(InjectableGameAction::WinInXTurns(2), explored_node);
        root.insert_child(
            InjectableGameAction::WinInXTurns(3),
            Node::Placeholder { weight: None },
        );
        root.visit(&[0.0f64]);
        let tree = Tree::new(root);
        if let Selection::Selection(selection_result) = tree.selection() {
            assert_eq!(
                selection_result.selection,
                vec![InjectableGameAction::WinInXTurns(3)]
            )
        } else {
            self::panic!("Incorrect selection type")
        }
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
            perceived_permitted_actions: Default::default(),
            player_count: 1,
            next_actor: Actor::Player(0),
            injected_hyperreward: Default::default(),
            terminal_hyperreward: Default::default(),
        };

        let mut explored_state_1 = InjectableGameAction::WinInXTurns(2).execute(&root_state);
        explored_state_1.injected_permitted_actions = vec![InjectableGameAction::WinInXTurns(1)];
        let explored_state_2 = InjectableGameAction::WinInXTurns(3).execute(&root_state);
        let mut root = create_expanded_node(root_state, None, None);

        let mut explored_node_1 = create_expanded_node(explored_state_1, None, None);
        explored_node_1.visit(&[0.0f64]);
        explored_node_1.insert_child(
            InjectableGameAction::WinInXTurns(1),
            Node::Placeholder { weight: None },
        );

        let mut explored_node_2 = create_expanded_node(explored_state_2, None, None);
        explored_node_2.visit(&[-1.0f64]);
        explored_node_2.visit(&[0.0f64]);

        root.insert_child(InjectableGameAction::WinInXTurns(2), explored_node_1);
        root.insert_child(InjectableGameAction::WinInXTurns(3), explored_node_2);
        root.visit(&[0.0f64]);
        root.visit(&[0.0f64]);
        root.visit(&[0.0f64]);

        let tree = Tree::new(root);
        if let Selection::Selection(selection_result) = tree.selection() {
            assert_eq!(
                selection_result.selection,
                vec![
                    InjectableGameAction::WinInXTurns(2),
                    InjectableGameAction::WinInXTurns(1)
                ]
            )
        } else {
            self::panic!("Incorrect selection type")
        }
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
            perceived_permitted_actions: Default::default(),
            player_count: 1,
            next_actor: Actor::Player(0),
            injected_hyperreward: Default::default(),
            terminal_hyperreward: Default::default(),
        };
        let mut explored_state_1 = InjectableGameAction::WinInXTurns(2).execute(&root_state);
        explored_state_1.injected_permitted_actions =
            vec![InjectableGameAction::NextTurnInjectActionCount(5)];

        let explored_state_2 = InjectableGameAction::WinInXTurns(3).execute(&root_state);
        let mut root = create_expanded_node(root_state, None, None);

        let mut explored_node_1 = create_expanded_node(explored_state_1, None, None);
        explored_node_1.visit(&[0.0f64]);
        explored_node_1.insert_child(
            InjectableGameAction::NextTurnInjectActionCount(5),
            Node::Placeholder { weight: None },
        );

        let mut explored_node_2 = create_expanded_node(explored_state_2, None, None);
        explored_node_2.visit(&[-1.0f64]);
        explored_node_2.visit(&[0.0f64]);

        root.insert_child(InjectableGameAction::WinInXTurns(2), explored_node_1);
        root.insert_child(InjectableGameAction::WinInXTurns(3), explored_node_2);

        let selection_path = vec![
            InjectableGameAction::WinInXTurns(2),
            InjectableGameAction::NextTurnInjectActionCount(5),
        ];
        let selection = Selection::Selection(SelectionResult {
            selection: selection_path.clone(),
            selected_steps: 2,
            random_walk_steps: 0,
            sum_diff_est_reward: 0.0,
        });

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
    fn test_expansion_returns_selected_child_path() {
        let root_state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![
                InjectableGameAction::WinInXTurns(2),
                InjectableGameAction::WinInXTurns(3),
            ],
            perceived_permitted_actions: Default::default(),
            player_count: 1,
            next_actor: Actor::Player(0),
            injected_hyperreward: Default::default(),
            terminal_hyperreward: Default::default(),
        };
        let mut explored_state_1 = InjectableGameAction::WinInXTurns(2).execute(&root_state);
        explored_state_1.injected_permitted_actions =
            vec![InjectableGameAction::NextTurnInjectActionCount(5)];

        let explored_state_2 = InjectableGameAction::WinInXTurns(3).execute(&root_state);
        let mut root = create_expanded_node(root_state, None, None);

        let mut explored_node_1 = create_expanded_node(explored_state_1, None, None);
        explored_node_1.visit(&[0.0f64]);
        explored_node_1.insert_child(
            InjectableGameAction::NextTurnInjectActionCount(5),
            Node::Placeholder { weight: None },
        );

        let mut explored_node_2 = create_expanded_node(explored_state_2, None, None);
        explored_node_2.visit(&[-1.0f64]);
        explored_node_2.visit(&[0.0f64]);

        root.insert_child(InjectableGameAction::WinInXTurns(2), explored_node_1);
        root.insert_child(InjectableGameAction::WinInXTurns(3), explored_node_2);

        let selection = Selection::Selection(SelectionResult {
            selection: vec![
                InjectableGameAction::WinInXTurns(2),
                InjectableGameAction::NextTurnInjectActionCount(5),
            ],
            selected_steps: 2,
            random_walk_steps: 0,
            sum_diff_est_reward: 0.0,
        });

        let tree = Tree::new(root);
        let expanded_nodes = tree.expansion(&selection);
        let owned_root = tree.root.clone();
        let expected_first = owned_root
            .read()
            .unwrap()
            .get_child(&InjectableGameAction::WinInXTurns(2));
        let expected_last = expected_first
            .read()
            .unwrap()
            .get_child(&InjectableGameAction::NextTurnInjectActionCount(5));

        assert_eq!(expanded_nodes.len(), 3);
        assert!(std::sync::Arc::ptr_eq(&expanded_nodes[0], &tree.root));
        assert!(std::sync::Arc::ptr_eq(&expanded_nodes[1], &expected_first));
        assert!(std::sync::Arc::ptr_eq(&expanded_nodes[2], &expected_last));
    }

    #[test]
    fn test_play_out() {
        let root_state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![InjectableGameAction::WinInXTurns(3)],
            perceived_permitted_actions: Default::default(),
            player_count: 1,
            next_actor: Actor::Player(0),
            injected_hyperreward: Default::default(),
            terminal_hyperreward: Default::default(),
        };

        let explored_state = InjectableGameAction::WinInXTurns(2).execute(&root_state);
        let root = create_expanded_node(root_state, None, None);
        let tree = Tree::new(root);
        let reward = tree.play_out(explored_state, 0);

        assert_eq!(reward.reward, vec![1.0]);
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
            perceived_permitted_actions: Default::default(),
            player_count: 1,
            next_actor: Actor::Player(0),
            injected_hyperreward: Default::default(),
            terminal_hyperreward: Default::default(),
        };

        let explored_state = InjectableGameAction::WinInXTurns(2).execute(&root_state);
        let mut root = create_expanded_node(root_state, None, None);

        let mut explored_node = create_expanded_node(explored_state, None, None);

        let mut child_node = create_expanded_node(
            InjectableGameAction::WinInXTurns(1).execute(&explored_node.state()),
            None,
            None,
        );

        let grandchild_state = InjectableGameAction::Win.execute(&child_node.state());
        let grandchild_node = create_expanded_node(grandchild_state, None, None);

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
                .get_child(&InjectableGameAction::WinInXTurns(2))
                .clone(),
            owned_root
                .read()
                .unwrap()
                .get_child(&InjectableGameAction::WinInXTurns(2))
                .read()
                .unwrap()
                .get_child(&InjectableGameAction::WinInXTurns(1))
                .clone(),
            owned_root
                .read()
                .unwrap()
                .get_child(&InjectableGameAction::WinInXTurns(2))
                .read()
                .unwrap()
                .get_child(&InjectableGameAction::WinInXTurns(1))
                .read()
                .unwrap()
                .get_child(&InjectableGameAction::Win)
                .clone(),
        ];

        let check_path = path.clone();
        const REWARD: f64 = 0.8;
        tree.propagate_reward(nodes, &[REWARD]);

        {
            let root = tree.root.read().unwrap();
            assert_eq!(root.value_sum_for_player(0), REWARD);
            assert_eq!(root.visit_count(), 1);
        }

        for path_i in 1..=check_path.len() {
            let semi_path = check_path[0..path_i].to_vec();
            let node_ref = tree.root.read().unwrap().get_node_by_path(semi_path);
            let node = node_ref.read().unwrap();
            assert_eq!(node.value_sum_for_player(0), REWARD);
            assert_eq!(node.visit_count(), 1);
        }
    }

    #[test]
    fn test_propagate_two_players() {
        let root_state = InjectableGameState {
            injected_reward: vec![0.0, 0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![
                InjectableGameAction::WinInXTurns(2),
                InjectableGameAction::WinInXTurns(3),
            ],
            perceived_permitted_actions: Default::default(),
            player_count: 2,
            next_actor: Actor::Player(0),
            injected_hyperreward: Default::default(),
            terminal_hyperreward: Default::default(),
        };

        let explored_state = InjectableGameAction::WinInXTurns(2).execute(&root_state);
        let mut root = create_expanded_node(root_state, None, None);

        let mut explored_node = create_expanded_node(explored_state, None, None);

        let mut child_node = create_expanded_node(
            InjectableGameAction::WinInXTurns(1).execute(&explored_node.state()),
            None,
            None,
        );

        let grandchild_state = InjectableGameAction::Win.execute(&child_node.state());
        let grandchild_node = create_expanded_node(grandchild_state, None, None);

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
                .get_child(&InjectableGameAction::WinInXTurns(2))
                .clone(),
            owned_root
                .read()
                .unwrap()
                .get_child(&InjectableGameAction::WinInXTurns(2))
                .read()
                .unwrap()
                .get_child(&InjectableGameAction::WinInXTurns(1))
                .clone(),
            owned_root
                .read()
                .unwrap()
                .get_child(&InjectableGameAction::WinInXTurns(2))
                .read()
                .unwrap()
                .get_child(&InjectableGameAction::WinInXTurns(1))
                .read()
                .unwrap()
                .get_child(&InjectableGameAction::Win)
                .clone(),
        ];

        let check_path = path.clone();
        // Using slightly unusual rewards to just make more certain that it was actually this reward
        const REWARD: f64 = 0.8;
        const LOSS_REWARD: f64 = -0.6;
        tree.propagate_reward(nodes, &[REWARD, LOSS_REWARD]);

        {
            let root = tree.root.read().unwrap();
            assert_eq!(root.value_sum_for_player(0), REWARD);
            assert_eq!(root.value_sum_for_player(1), LOSS_REWARD);
            assert_eq!(root.visit_count(), 1);
        }

        for path_i in 1..=check_path.len() {
            // This isn't the greatest way to do this - maybe we should be just looking it up in a
            // table.
            let semi_path = check_path[0..path_i].to_vec();
            let node_ref = tree.root.read().unwrap().get_node_by_path(semi_path);
            let node = node_ref.read().unwrap();
            assert_eq!(node.value_sum_for_player(0), REWARD);
            assert_eq!(node.value_sum_for_player(1), LOSS_REWARD);
            assert_eq!(node.visit_count(), 1);
        }
    }

    #[test]
    fn test_weighted_game_action_play_out() {
        let root_state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![],
            perceived_permitted_actions: Default::default(),
            player_count: 1,
            next_actor: Actor::GameAction(vec![
                (InjectableGameAction::Lose, 1),
                (InjectableGameAction::Win, 2),
            ]),
            injected_hyperreward: Default::default(),
            terminal_hyperreward: Default::default(),
        };

        let root = create_expanded_node(root_state.clone(), None, None);
        let tree = Tree::new(root);

        let mut weight_1_visits = 0;
        let mut weight_2_visits = 0;
        for _ in 0..1000 {
            let reward = tree.play_out(root_state.clone(), 0).reward;
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

    #[test]
    fn test_iterate_hyperreward() {
        use crate::test::injectable_game::TestHyperreward;

        let root_state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![InjectableGameAction::Win],
            perceived_permitted_actions: Default::default(),
            player_count: 1,
            next_actor: Actor::Player(0),
            injected_hyperreward: TestHyperreward { value: 1 },
            terminal_hyperreward: TestHyperreward { value: 100 },
        };

        let root = create_expanded_node(root_state, None, None);
        let tree = Tree::new(root);
        let play_out_result = tree.play_out(tree.root.read().unwrap().state().clone(), 0);
        assert_eq!(
            play_out_result.round_hyperreward,
            TestHyperreward { value: 100 }
        );
    }

    #[test]
    fn test_sum_diff_est_reward() {
        let mut root_node = create_expanded_node(
            InjectableGameState {
                injected_reward: vec![0.0f64],
                injected_terminal: false,
                injected_permitted_actions: vec![
                    InjectableGameAction::WinInXTurns(1),
                    InjectableGameAction::WinInXTurns(2),
                    InjectableGameAction::WinInXTurns(3),
                ],
                perceived_permitted_actions: Default::default(),
                player_count: 1,
                injected_hyperreward: TestHyperreward { value: 0 },
                next_actor: Actor::Player(0),
                terminal_hyperreward: TestHyperreward { value: 1 },
            },
            None,
            None,
        );

        let mut child1 = create_expanded_node(
            InjectableGameState {
                injected_reward: vec![0.0f64],
                injected_terminal: false,
                injected_permitted_actions: vec![],
                perceived_permitted_actions: Default::default(),
                player_count: 1,
                next_actor: Actor::Player(0),
                injected_hyperreward: TestHyperreward { value: 0 },
                terminal_hyperreward: TestHyperreward { value: 1 },
            },
            None,
            None,
        );
        child1.visit(&[10.0]); // est_reward = 10.0

        let mut child2 = create_expanded_node(
            InjectableGameState {
                injected_reward: vec![0.0f64],
                injected_terminal: false,
                injected_permitted_actions: vec![],
                perceived_permitted_actions: Default::default(),
                player_count: 1,
                next_actor: Actor::Player(0),
                injected_hyperreward: TestHyperreward { value: 0 },
                terminal_hyperreward: TestHyperreward { value: 1 },
            },
            None,
            None,
        );
        child2.visit(&[20.0]);
        child2.visit(&[4.0]); // est_reward = 12.0

        root_node.insert_child(InjectableGameAction::WinInXTurns(1), child1);
        root_node.insert_child(InjectableGameAction::WinInXTurns(2), child2);
        root_node.insert_child(
            InjectableGameAction::WinInXTurns(3),
            Node::Placeholder { weight: None },
        );

        let tree = Tree::new(root_node);
        if let Selection::Selection(selection_result) = tree.selection() {
            // mean_child_est_reward is (10.0 + 12.0) / 2 = 11.0 for the root node's expanded children.
            // best_pick is WinInXTurns(3) which has an est_reward of 0.
            // dWe should be 0 - 11.0 = -11.0
            assert_eq!(selection_result.sum_diff_est_reward, -11.0);
        } else {
            self::panic!("Incorrect selection type")
        }
    }
}
