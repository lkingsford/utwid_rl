use super::game_trait::{Action, Actor, State};
use core::panic;
use log::{trace, warn};
use rand::Rng;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

#[cfg(not(test))]
const RANDOM_FACTOR: f64 = 1e-6;
#[cfg(test)]
const RANDOM_FACTOR: f64 = 0.0;

#[derive(Debug)]
pub struct CachedUcb {
    ucb: f64,
    player_value_sum: f64,
    visit_count: u32,
    parent_visit_count: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VisitCountValue {
    pub visit_count: u32,
    pub value_sum: f64,
}

#[derive(Debug)]
pub enum Node<StateType: State, ActionType: Action<StateType = StateType>> {
    Expanded {
        state: Arc<StateType>,
        children: HashMap<ActionType, Arc<RwLock<Node<StateType, ActionType>>>>,
        visit_count: u32,
        value_sums: Vec<VisitCountValue>,
        cached_ucb: RwLock<Option<CachedUcb>>,
        cached_fully_explored: RwLock<Option<bool>>,
        game_action: bool,
        weight: Option<u32>,
    },
    Placeholder {
        weight: Option<u32>,
    },
}

impl<StateType: State, ActionType: Action<StateType = StateType>> Node<StateType, ActionType> {
    pub fn fully_explored(&self) -> bool {
        match self {
            Node::Expanded {
                children,
                cached_fully_explored,
                ..
            } => {
                if let Ok(cached_fully_explored_read) = cached_fully_explored.try_read() {
                    if let Some(cached_fully_explored_value) = *cached_fully_explored_read {
                        //log::error!("CACHE HIT");
                        return cached_fully_explored_value;
                    }
                }
                //log::error!("CACHE MISS");
                let fully_explored = children.is_empty()
                    || children.values().all(|child| {
                        let child_node = child.read().unwrap();
                        match *child_node {
                            Node::Expanded { .. } => child_node.fully_explored(),
                            Node::Placeholder { .. } => false,
                        }
                    });
                if let Ok(mut cached_fully_explored) = cached_fully_explored.try_write() {
                    *cached_fully_explored = Some(fully_explored);
                    // log::error!("CACHE WRITE");
                };
                fully_explored
            }
            Node::Placeholder { .. } => false,
        }
    }

    pub fn visit_count(&self) -> u32 {
        match self {
            Node::Expanded { visit_count, .. } => *visit_count,
            Node::Placeholder { .. } => 0,
        }
    }

    pub fn game_action(&self) -> bool {
        match self {
            Node::Expanded { game_action, .. } => *game_action,
            Node::Placeholder { .. } => false,
        }
    }

    pub fn value_sums(&self) -> Vec<VisitCountValue> {
        match self {
            Node::Expanded { value_sums, .. } => value_sums.clone(),
            Node::Placeholder { .. } => vec![],
        }
    }

    pub fn value_sums_ref(&self) -> &[VisitCountValue] {
        match self {
            Node::Expanded { value_sums, .. } => value_sums,
            Node::Placeholder { .. } => &[],
        }
    }

    pub fn value_sum_for_player(&self, player_id: u8) -> f64 {
        match self {
            Node::Expanded { value_sums, .. } => value_sums
                .get(player_id as usize)
                .map(|value| value.value_sum)
                .unwrap_or(0.0),
            Node::Placeholder { .. } => 0.0,
        }
    }

    pub fn est_reward_for_player(&self, player_id: u8) -> f64 {
        match self {
            Node::Expanded { value_sums, .. } => {
                if let Some(value_sum) = value_sums.get(player_id as usize) {
                    if value_sum.visit_count == 0 {
                        0.0
                    } else {
                        value_sum.value_sum / value_sum.visit_count as f64
                    }
                } else {
                    0.0
                }
            }
            Node::Placeholder { .. } => 0.0,
        }
    }

    pub fn weight(&self) -> u32 {
        match self {
            Node::Expanded { weight, .. } => weight.unwrap_or(1),
            Node::Placeholder { weight, .. } => weight.unwrap_or(1),
        }
    }

    pub fn visit(&mut self, reward: &[f64]) {
        match self {
            Node::Expanded {
                visit_count,
                value_sums,
                cached_fully_explored,
                ..
            } => {
                *visit_count += 1;
                if value_sums.len() < reward.len() {
                    value_sums.resize(reward.len(), VisitCountValue::default());
                }
                for (i, reward_component) in reward.iter().enumerate() {
                    value_sums[i].visit_count += 1;
                    value_sums[i].value_sum += reward_component;
                }
                if let Ok(mut cached_fully_explored) = cached_fully_explored.write() {
                    *cached_fully_explored = None;
                } else {
                    panic!("Can't write cached fully explored");
                }
            }
            Node::Placeholder { .. } => {
                warn!("Visiting placeholder node");
            }
        }
    }

    pub fn cache_ucb(
        &self,
        ucb: f64,
        player_value_sum: f64,
        visit_count: u32,
        parent_visit_count: u32,
    ) {
        match self {
            Node::Expanded { cached_ucb, .. } => {
                if let Ok(mut cached_ucb_ref) = cached_ucb.try_write() {
                    *cached_ucb_ref = Some(CachedUcb {
                        ucb,
                        player_value_sum,
                        visit_count,
                        parent_visit_count,
                    });
                }
            }
            Node::Placeholder { .. } => {}
        }
    }

    pub fn cached_ucb(
        &self,
        player_value_sum: f64,
        visit_count: u32,
        parent_visit_count: u32,
    ) -> Option<f64> {
        match self {
            Node::Expanded { cached_ucb, .. } => {
                let ucb = cached_ucb.read().unwrap();
                match *ucb {
                    Some(CachedUcb {
                        ucb: cached_ucb,
                        player_value_sum: cached_player_value_sum,
                        visit_count: cached_visit_count,
                        parent_visit_count: cached_parent_visit_count,
                    }) => {
                        if cached_player_value_sum == player_value_sum
                            && cached_visit_count == visit_count
                            && cached_parent_visit_count == parent_visit_count
                        {
                            Some(cached_ucb)
                        } else {
                            None
                        }
                    }
                    None => None,
                }
            }
            Node::Placeholder { .. } => None,
        }
    }

    pub fn expansion(
        &self,
        action: ActionType,
        parent_state: &<ActionType as Action>::StateType,
        per: Option<u8>,
    ) -> Node<StateType, <StateType as State>::ActionType> {
        match self {
            Node::Expanded { .. } => {
                panic!("Expanding an expanded node");
            }
            Node::Placeholder { weight, .. } => {
                let (state, _events) = action.execute(parent_state);
                Self::new_expanded(state, *weight, per)
            }
        }
    }

    pub fn state(&self) -> &StateType {
        match self {
            Node::Expanded { state, .. } => &**state,
            Node::Placeholder { .. } => panic!("Placeholder node has no state"),
        }
    }

    pub fn insert_child(&mut self, action: ActionType, child: Node<StateType, ActionType>) {
        if let Node::Expanded { children, .. } = self {
            children.insert(action, Arc::new(RwLock::new(child)));
        } else {
            panic!("Inserting child into placeholder");
        }
    }

    pub fn get_child(&self, action: &ActionType) -> Arc<RwLock<Node<StateType, ActionType>>> {
        if let Node::Expanded { children, .. } = self {
            children.get(action).unwrap().clone()
        } else {
            panic!("Getting child from placeholder");
        }
    }

    pub fn new_expanded(
        state: StateType,
        weight: Option<u32>,
        per: Option<u8>,
    ) -> Node<StateType, <StateType as State>::ActionType> {
        create_expanded_node(state, weight, per)
    }

    pub fn reset_visits(&mut self) {
        match self {
            Node::Expanded {
                visit_count,
                value_sums,
                cached_ucb,
                cached_fully_explored,
                children,
                ..
            } => {
                *visit_count = 0;
                for vs in value_sums.iter_mut() {
                    vs.visit_count = 0;
                    vs.value_sum = 0.0;
                }
                *cached_ucb = RwLock::new(None);
                *cached_fully_explored = RwLock::new(None);
                for child in children.values_mut() {
                    child.write().unwrap().reset_visits();
                }
            }
            Node::Placeholder { .. } => {}
        }
    }

    pub fn mean_child_est_reward_for_player(&self, player_id: u8) -> f64 {
        match self {
            Node::Expanded { children, .. } => {
                let mut sum = 0.0;
                let mut count = 0;
                for child_lock in children.values() {
                    let child = child_lock.read().unwrap();
                    if matches!(*child, Node::Expanded { .. }) {
                        sum += child.est_reward_for_player(player_id);
                        count += 1;
                    }
                }

                if count == 0 { 0.0 } else { sum / count as f64 }
            }
            Node::Placeholder { .. } => 0.0,
        }
    }

    pub fn deep_clone(&self, depth_limit: Option<usize>) -> Self {
        match self {
            Node::Expanded {
                state,
                children,
                visit_count,
                value_sums,
                game_action,
                weight,
                ..
            } => {
                let cloned_children = if let Some(limit) = depth_limit {
                    if limit <= 1 {
                        children
                            .iter()
                            .map(|(action, _)| {
                                (
                                    action.clone(),
                                    Arc::new(RwLock::new(Node::Placeholder { weight: None })),
                                )
                            })
                            .collect()
                    } else {
                        children
                            .iter()
                            .map(|(action, child)| {
                                let cloned = child.read().unwrap().deep_clone(Some(limit - 1));
                                (action.clone(), Arc::new(RwLock::new(cloned)))
                            })
                            .collect()
                    }
                } else {
                    children
                        .iter()
                        .map(|(action, child)| {
                            let cloned = child.read().unwrap().deep_clone(None);
                            (action.clone(), Arc::new(RwLock::new(cloned)))
                        })
                        .collect()
                };
                Node::Expanded {
                    state: state.clone(),
                    children: cloned_children,
                    visit_count: *visit_count,
                    value_sums: value_sums.clone(),
                    cached_ucb: RwLock::new(None),
                    cached_fully_explored: RwLock::new(None),
                    game_action: *game_action,
                    weight: *weight,
                }
            }
            Node::Placeholder { weight } => Node::Placeholder { weight: *weight },
        }
    }

    pub fn get_node_by_path(
        &self,
        path: Vec<ActionType>,
    ) -> Arc<RwLock<Node<StateType, ActionType>>> {
        if path.is_empty() {
            panic!("Can't return empty path")
        }
        let mut node = None;
        for action in path {
            if node.is_none() {
                node = Some(self.get_child(&action));
            } else {
                node = Some(node.unwrap().read().unwrap().get_child(&action));
            }
        }
        node.unwrap()
    }

    pub fn node_merge(&self, other: &Self) -> Self {
        // Subtlety: merging trees with different root states will produce garbage.
        // Callers should ensure both nodes originate from the same state (Arc::ptr_eq).
        match (self, other) {
            (Node::Placeholder { weight: _ }, Node::Expanded { .. }) => other.deep_clone(None),
            (Node::Expanded { .. }, Node::Placeholder { weight: _ }) => self.deep_clone(None),
            (Node::Placeholder { weight }, Node::Placeholder { weight: weight_b }) => {
                Node::Placeholder {
                    weight: weight.or(*weight_b),
                }
            }
            (
                Node::Expanded {
                    state,
                    children,
                    visit_count,
                    value_sums,
                    game_action,
                    weight,
                    ..
                },
                Node::Expanded {
                    children: children_b,
                    visit_count: visit_count_b,
                    value_sums: value_sums_b,
                    game_action: game_action_b,
                    weight: weight_b,
                    ..
                },
            ) => {
                let merged_visits = *visit_count + *visit_count_b;
                let merged_values: Vec<VisitCountValue> = value_sums
                    .iter()
                    .zip(value_sums_b.iter())
                    .map(|(a, b)| VisitCountValue {
                        visit_count: a.visit_count + b.visit_count,
                        value_sum: a.value_sum + b.value_sum,
                    })
                    .collect();

                let mut merged_children: HashMap<
                    ActionType,
                    Arc<RwLock<Node<StateType, ActionType>>>,
                > = HashMap::new();
                for (action, child) in children.iter() {
                    merged_children.insert(action.clone(), child.clone());
                }
                for (action, child_b) in children_b.iter() {
                    if let Some(existing) = merged_children.get(action) {
                        let merged = {
                            let a = existing.read().unwrap();
                            let b = child_b.read().unwrap();
                            a.node_merge(&b)
                        };
                        merged_children.insert(action.clone(), Arc::new(RwLock::new(merged)));
                    } else {
                        let cloned = child_b.read().unwrap().deep_clone(None);
                        merged_children.insert(action.clone(), Arc::new(RwLock::new(cloned)));
                    }
                }

                Node::Expanded {
                    state: state.clone(),
                    children: merged_children,
                    visit_count: merged_visits,
                    value_sums: merged_values,
                    cached_ucb: RwLock::new(None),
                    cached_fully_explored: RwLock::new(None),
                    game_action: *game_action,
                    weight: weight.or(*weight_b),
                }
            }
        }
    }

    pub fn log_children(&self, level: usize) {
        if level == 0 {
            log::info!("--- TREE ---");
        }
        match self {
            Node::Expanded { children, .. } => {
                for (action, child) in children.iter() {
                    let cloned_child = child.clone();
                    let child_node = cloned_child.read().unwrap();
                    match *child_node {
                        Node::Expanded { .. } => {
                            let action_name = format!("{:?}", action);
                            log::info!("{} {}", "         |-".repeat(level), action_name);
                            log::info!(
                                "{} {:?} {}",
                                "         | ".repeat(level),
                                child_node
                                    .value_sums()
                                    .into_iter()
                                    .map(|value| value.value_sum)
                                    .collect::<Vec<_>>(),
                                child_node.visit_count()
                            );
                            log::info!(
                                "{} {:.6}",
                                "         | ".repeat(level),
                                child_node.value_sum_for_player(0)
                                    / (child_node.visit_count() as f64)
                            );
                            child_node.log_children(level + 1);
                        }
                        Node::Placeholder { .. } => {
                            let action_name = format!("({:?})", action);
                            log::info!("{} {}", "         |-".repeat(level), action_name);
                        }
                    }
                }
            }
            Node::Placeholder { .. } => return,
        }
    }
}

#[derive(Debug)]
pub struct BestPickEntry<ActionType> {
    pub action_to_take: ActionType,
    pub ucb: f64,
    pub expected_value: f64,
}

pub fn best_pick<StateType, ActionType>(
    node_lock: &RwLock<Node<StateType, ActionType>>,
    constant: f64,
) -> Vec<BestPickEntry<ActionType>>
where
    StateType: State<ActionType = ActionType>,
    ActionType: Action<StateType = StateType>,
{
    // Using a minimum of 1 here, because it's possible (can reproduce 1 in every few thousand iterations) that
    // parent_visit_count is 0 but the value sum is non-zero meaning (I think) that another selector has clashed.
    // This is faster than additional locks.
    // The issue is that ln(0) == NaN. So - yeah.
    let node = node_lock.read().unwrap();
    let (children, game_action, parent_visit_count, player_id) = match &*node {
        Node::Expanded {
            children,
            game_action,
            ..
        } => {
            if children.is_empty() {
                return vec![];
            }
            let parent_visit_count = std::cmp::max(node.visit_count(), 1);
            let player_id = match node.state().next_actor() {
                Actor::Player(player_id) => Some(player_id),
                Actor::GameAction(_) => None,
            };
            (children, *game_action, parent_visit_count, player_id)
        }
        Node::Placeholder { .. } => return vec![],
    };

    let parent_visits = parent_visit_count as f64;
    let mut rng = rand::rng();
    let mut ucbs: Vec<BestPickEntry<ActionType>> = Vec::with_capacity(children.len());
    for (action, child_node) in children.iter() {
        let child_node = child_node.read().unwrap();
        if child_node.fully_explored() {
            log::trace!("Select short circuited - fully explored");
            continue;
        }

        let child_visit_count = child_node.visit_count();
        let cached_player_value_sum = player_id
            .map(|player_id| child_node.value_sum_for_player(player_id))
            .unwrap_or(1.0);
        if let Some(ucb) = child_node.cached_ucb(
            cached_player_value_sum,
            child_visit_count,
            parent_visit_count,
        ) {
            let q = if child_visit_count == 0 {
                0.0
            } else {
                player_id
                    .map(|player_id| child_node.est_reward_for_player(player_id))
                    .unwrap_or(0.0)
            };
            ucbs.push(BestPickEntry {
                action_to_take: action.clone(),
                ucb,
                expected_value: q,
            });
            continue;
        }

        let (visit_count, player_value_sum) = if game_action {
            (child_visit_count as f64 / child_node.weight() as f64, 1.0)
        } else {
            (child_visit_count as f64, cached_player_value_sum)
        };

        if visit_count == 0.0 {
            ucbs.push(BestPickEntry {
                action_to_take: action.clone(),
                ucb: f64::INFINITY,
                expected_value: 0.0,
            });
            continue;
        }

        let q = player_value_sum / visit_count;
        let u = (parent_visits.ln() / visit_count).sqrt();
        let r = rng.random::<f64>() * RANDOM_FACTOR;
        let ucb = q + constant * u + r;
        trace!(
            "UCB action: {:?}, value_sum: {}, visit_count: {}, parent_visits: {}, q: {}, u: {}, c: {} ucb: {}",
            action, player_value_sum, visit_count, parent_visits, q, u, constant, ucb
        );
        child_node.cache_ucb(
            ucb,
            cached_player_value_sum,
            child_visit_count,
            parent_visit_count,
        );
        ucbs.push(BestPickEntry {
            action_to_take: action.clone(),
            ucb,
            expected_value: q,
        });
    }
    ucbs.sort_by(|a, b| b.ucb.partial_cmp(&a.ucb).unwrap());
    trace!("UCBS action, ucb: {:?}", ucbs.iter().collect::<Vec<_>>());
    ucbs
}

pub fn create_expanded_node<StateType>(
    state: StateType,
    weight: Option<u32>,
    per: Option<u8>,
) -> Node<StateType, StateType::ActionType>
where
    StateType: State,
{
    let state = Arc::new(state);
    let reward_len = state.reward().len();
    let mut children: HashMap<
        StateType::ActionType,
        Arc<RwLock<Node<StateType, StateType::ActionType>>>,
    > = HashMap::new();
    let game_action = if state.terminal() {
        false
    } else {
        match state.next_actor() {
            Actor::Player(_) => {
                for action in state.permitted_actions(per) {
                    children.insert(
                        action,
                        Arc::new(RwLock::new(Node::Placeholder { weight: None })),
                    );
                }
                false
            }
            Actor::GameAction(actions) => {
                for action in actions {
                    children.insert(
                        action.0,
                        Arc::new(RwLock::new(Node::Placeholder {
                            weight: Some(action.1),
                        })),
                    );
                }
                true
            }
        }
    };

    Node::Expanded {
        state,
        children,
        visit_count: 0,
        value_sums: vec![VisitCountValue::default(); reward_len],
        cached_ucb: RwLock::new(None),
        cached_fully_explored: RwLock::new(None),
        game_action,
        weight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::injectable_game::{
        InjectableGameAction, InjectableGameState, TestHyperreward,
    };

    #[test]
    fn test_create_expanded_node() {
        let state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![InjectableGameAction::Win],
            perceived_permitted_actions: Default::default(),
            player_count: 1,
            next_actor: Actor::Player(0),
            injected_hyperreward: TestHyperreward { value: 0 },
            terminal_hyperreward: TestHyperreward { value: 1 },
        };
        let node = create_expanded_node(state, None, None);
        assert_eq!(node.visit_count(), 0);
        assert_eq!(node.value_sums().len(), 1);
        assert_eq!(node.value_sums()[0].value_sum, 0.0);
    }

    #[test]
    fn test_create_expanded_terminal_node_has_no_children() {
        let state = InjectableGameState {
            injected_reward: vec![1.0],
            injected_terminal: true,
            injected_permitted_actions: vec![InjectableGameAction::Win],
            perceived_permitted_actions: Default::default(),
            player_count: 1,
            next_actor: Actor::Player(0),
            injected_hyperreward: TestHyperreward { value: 0 },
            terminal_hyperreward: TestHyperreward { value: 1 },
        };
        let node = create_expanded_node(state, None, None);
        match node {
            Node::Expanded { children, .. } => assert!(children.is_empty()),
            Node::Placeholder { .. } => std::panic!("Expected expanded node"),
        }
    }

    #[test]
    fn test_best_pick_weighted_visits() {
        // Maybe this being parameterized would be better?
        // But, it's probably going to look messy, so this will do as a minimum check
        // Low effort test - create a node with weight 1 and weight 2, give them unexpanded children too,
        // check that the next pick is from the weight 2 node

        let mut root_node = create_expanded_node(
            InjectableGameState {
                terminal_hyperreward: TestHyperreward { value: 1 },
                injected_reward: vec![0.0f64],
                injected_terminal: false,
                injected_permitted_actions: vec![],
                perceived_permitted_actions: Default::default(),
                player_count: 1,
                next_actor: Actor::GameAction(vec![
                    (InjectableGameAction::WinInXTurns(1), 1),
                    (InjectableGameAction::WinInXTurns(2), 2),
                ]),
                injected_hyperreward: TestHyperreward { value: 0 },
            },
            None,
            None,
        );

        let mut win_in_x_turns_1 = create_expanded_node(
            InjectableGameState {
                terminal_hyperreward: TestHyperreward { value: 1 },
                injected_reward: vec![0.0f64],
                injected_terminal: false,
                injected_permitted_actions: vec![],
                perceived_permitted_actions: Default::default(),
                player_count: 1,
                next_actor: Actor::Player(0),
                injected_hyperreward: TestHyperreward { value: 0 },
            },
            Some(1),
            None,
        );

        let mut win_in_x_turns_2 = create_expanded_node(
            InjectableGameState {
                terminal_hyperreward: TestHyperreward { value: 1 },
                injected_reward: vec![0.0f64],
                injected_terminal: false,
                injected_permitted_actions: vec![],
                perceived_permitted_actions: Default::default(),
                player_count: 1,
                next_actor: Actor::Player(0),
                injected_hyperreward: TestHyperreward { value: 0 },
            },
            Some(2),
            None,
        );

        root_node.visit(&[0.0f64]);

        let win_in_x_turns_1_child_3 = Node::Placeholder { weight: Some(3) };
        let win_in_x_turns_1_child_4 = Node::Placeholder { weight: Some(4) };
        let win_in_x_turns_2_child_5 = Node::Placeholder { weight: Some(5) };
        let win_in_x_turns_2_child_6 = Node::Placeholder { weight: Some(6) };
        win_in_x_turns_1.insert_child(
            InjectableGameAction::WinInXTurns(3),
            win_in_x_turns_1_child_3,
        );
        win_in_x_turns_1.insert_child(
            InjectableGameAction::WinInXTurns(4),
            win_in_x_turns_1_child_4,
        );
        win_in_x_turns_2.insert_child(
            InjectableGameAction::WinInXTurns(5),
            win_in_x_turns_2_child_5,
        );
        win_in_x_turns_2.insert_child(
            InjectableGameAction::WinInXTurns(6),
            win_in_x_turns_2_child_6,
        );
        root_node.insert_child(InjectableGameAction::WinInXTurns(1), win_in_x_turns_1);
        root_node.insert_child(InjectableGameAction::WinInXTurns(2), win_in_x_turns_2);

        let locked_node = RwLock::new(root_node);

        // No visits, get the weight 2 node
        // TODO: do that. Currently, it visits the inf+ nodes in a random order.
        // {
        //    let best_pick = best_pick(&locked_node, 2.0_f64.sqrt());
        //    assert_eq!(
        //        best_pick.first().unwrap().0,
        //        InjectableGameAction::WinInXTurns(2)
        //    );
        // }

        {
            let root_node_ref = locked_node.read().unwrap();
            let child = root_node_ref.get_child(&InjectableGameAction::WinInXTurns(2));
            let mut child_write = child.write().unwrap();
            child_write.visit(&[0.0f64]);
        }
        // Weight 2 visited, weight 1 not, check that weight 1 is next
        {
            let best_pick = best_pick(&locked_node, 2.0_f64.sqrt());
            assert_eq!(
                best_pick.first().unwrap().action_to_take,
                InjectableGameAction::WinInXTurns(1)
            );
        }

        {
            let root_node_ref = locked_node.read().unwrap();
            let child = root_node_ref.get_child(&InjectableGameAction::WinInXTurns(1));
            let mut child_write = child.write().unwrap();
            child_write.visit(&[0.0f64]);
        }

        let best_pick = best_pick(&locked_node, 2.0_f64.sqrt());
        // We're checking for 2 - because it's the first node from the root (and best-pick isn't
        // iterative down the tree, selection is)
        assert_eq!(
            best_pick.first().unwrap().action_to_take,
            InjectableGameAction::WinInXTurns(2)
        );
    }

    #[test]
    fn test_best_pick_expected_value() {
        let mut root_node = create_expanded_node(
            InjectableGameState {
                injected_reward: vec![0.0f64],
                injected_terminal: false,
                injected_permitted_actions: vec![
                    InjectableGameAction::WinInXTurns(1),
                    InjectableGameAction::WinInXTurns(2),
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
                injected_permitted_actions: vec![InjectableGameAction::Win],
                perceived_permitted_actions: Default::default(),
                player_count: 1,
                next_actor: Actor::Player(0),
                injected_hyperreward: TestHyperreward { value: 0 },
                terminal_hyperreward: TestHyperreward { value: 1 },
            },
            None,
            None,
        );
        child1.visit(&[10.0]);
        child1.visit(&[0.0]);

        let child2 = create_expanded_node(
            InjectableGameState {
                injected_reward: vec![0.0f64],
                injected_terminal: false,
                injected_permitted_actions: vec![InjectableGameAction::Win],
                perceived_permitted_actions: Default::default(),
                player_count: 1,
                next_actor: Actor::Player(0),
                injected_hyperreward: TestHyperreward { value: 0 },
                terminal_hyperreward: TestHyperreward { value: 1 },
            },
            None,
            None,
        );

        root_node.insert_child(InjectableGameAction::WinInXTurns(1), child1);
        root_node.insert_child(InjectableGameAction::WinInXTurns(2), child2);
        root_node.visit(&[0.0]);

        let locked_node = RwLock::new(root_node);
        let best_picks = best_pick(&locked_node, 2.0_f64.sqrt());

        assert_eq!(best_picks.len(), 2);

        let pick1 = best_picks
            .iter()
            .find(|p| p.action_to_take == InjectableGameAction::WinInXTurns(1))
            .unwrap();
        assert_eq!(pick1.expected_value, 5.0);

        let pick2 = best_picks
            .iter()
            .find(|p| p.action_to_take == InjectableGameAction::WinInXTurns(2))
            .unwrap();
        assert_eq!(pick2.expected_value, 0.0);
    }

    #[test]
    fn test_est_reward() {
        let state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![],
            perceived_permitted_actions: Default::default(),
            player_count: 1,
            next_actor: Actor::Player(0),
            injected_hyperreward: TestHyperreward { value: 0 },
            terminal_hyperreward: TestHyperreward { value: 1 },
        };
        let mut node = create_expanded_node(state, None, None);
        assert_eq!(node.est_reward_for_player(0), 0.0);
        node.visit(&[10.0]);
        assert_eq!(node.est_reward_for_player(0), 10.0);
        node.visit(&[5.0]);
        assert_eq!(node.est_reward_for_player(0), 7.5);
    }

    #[test]
    fn test_mean_child_est_reward() {
        let mut root_node = create_expanded_node(
            InjectableGameState {
                injected_reward: vec![0.0f64],
                injected_terminal: false,
                injected_permitted_actions: vec![
                    InjectableGameAction::WinInXTurns(1),
                    InjectableGameAction::WinInXTurns(2),
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

        // Mean of 10.0 and 12.0 should be 11.0
        assert_eq!(root_node.mean_child_est_reward_for_player(0), 11.0);
    }
}
