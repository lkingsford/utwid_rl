use super::game::{Action, Actor, State};
use core::panic;
use log::{trace, warn};
use rand::rngs::ThreadRng;
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
    value_sum: f64,
    visit_count: u32,
    parent_visit_count: u32,
}

#[derive(Debug)]
pub enum Node<StateType: State, ActionType: Action<StateType = StateType>> {
    Expanded {
        state: StateType,
        children: HashMap<ActionType, Arc<RwLock<Node<StateType, ActionType>>>>,
        visit_count: u32,
        /// Sum of rewards for this player
        value_sum: f64,
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
                let child_nodes: Vec<Arc<RwLock<Node<StateType, ActionType>>>> =
                    { children.values().cloned().collect() };
                let fully_explored = child_nodes.is_empty()
                    || child_nodes.iter().all(|child| {
                        let child = child.clone();
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

    pub fn value_sum(&self) -> f64 {
        match self {
            Node::Expanded { value_sum, .. } => *value_sum,
            Node::Placeholder { .. } => 0.0,
        }
    }

    pub fn weight(&self) -> u32 {
        match self {
            Node::Expanded { weight, .. } => weight.unwrap_or(1),
            Node::Placeholder { weight, .. } => weight.unwrap_or(1),
        }
    }

    pub fn visit(&mut self, reward: f64) {
        match self {
            Node::Expanded {
                visit_count,
                value_sum,
                cached_fully_explored,
                ..
            } => {
                *visit_count += 1;
                *value_sum += reward as f64;
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

    pub fn cache_ucb(&self, ucb: f64, value_sum: f64, visit_count: u32, parent_visit_count: u32) {
        match self {
            Node::Expanded { cached_ucb, .. } => {
                if let Ok(mut cached_ucb_ref) = cached_ucb.try_write() {
                    *cached_ucb_ref = Some(CachedUcb {
                        ucb,
                        value_sum,
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
        value_sum: f64,
        visit_count: u32,
        parent_visit_count: u32,
    ) -> Option<f64> {
        match self {
            Node::Expanded { cached_ucb, .. } => {
                let ucb = cached_ucb.read().unwrap();
                match *ucb {
                    Some(CachedUcb {
                        ucb: cached_ucb,
                        value_sum: cached_value_sum,
                        visit_count: cached_visit_count,
                        parent_visit_count: cached_parent_visit_count,
                    }) => {
                        if cached_value_sum == value_sum
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
    ) -> Node<StateType, <StateType as State>::ActionType> {
        match self {
            Node::Expanded { .. } => {
                panic!("Expanding an expanded node");
            }
            Node::Placeholder { weight, .. } => {
                let state = action.execute(parent_state);
                Self::new_expanded(state, *weight)
            }
        }
    }

    pub fn state(&self) -> &StateType {
        match self {
            Node::Expanded { state, .. } => state,
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

    pub fn get_child(&self, action: ActionType) -> Arc<RwLock<Node<StateType, ActionType>>> {
        if let Node::Expanded { children, .. } = self {
            children.get(&action).unwrap().clone()
        } else {
            panic!("Getting child from placeholder");
        }
    }

    pub fn new_expanded(
        state: StateType,
        weight: Option<u32>,
    ) -> Node<StateType, <StateType as State>::ActionType> {
        create_expanded_node(state, weight)
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
                node = Some(self.get_child(action));
            } else {
                node = Some(node.unwrap().read().unwrap().get_child(action).clone());
            }
        }
        node.unwrap()
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
                                "{} {:.6} {}",
                                "         | ".repeat(level),
                                child_node.value_sum(),
                                child_node.visit_count()
                            );
                            log::info!(
                                "{} {:.6}",
                                "         | ".repeat(level),
                                child_node.value_sum() / (child_node.visit_count() as f64)
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

pub fn best_pick<StateType, ActionType>(
    node_lock: &RwLock<Node<StateType, ActionType>>,
    constant: f64,
) -> Vec<(ActionType, f64)>
where
    StateType: State<ActionType = ActionType>,
    ActionType: Action<StateType = StateType>,
{
    let children: HashMap<ActionType, Arc<RwLock<Node<StateType, ActionType>>>> = {
        let node = node_lock.read().unwrap();
        match &*node {
            Node::Expanded { children, .. } => children
                .iter()
                .map(|(action, child)| (action.clone(), child.clone()))
                .collect(),
            Node::Placeholder { .. } => {
                return vec![];
            }
        }
    };
    // Using a minimum of 1 here, because it's possible (can reproduce 1 in every few thousand iterations) that
    // parent_visit_count is 0 but the value sum is non-zero meaning (I think) that another selector has clashed.
    // This is faster than additional locks.
    // The issue is that ln(0) == NaN. So - yeah.
    let (game_action, parent_visit_count) = {
        let node = node_lock.read().unwrap();
        let parent_visit_count = std::cmp::max(node.visit_count(), 1);
        (node.game_action(), parent_visit_count)
    };

    let mut ucbs: Vec<(ActionType, f64)> = children
                    .iter()
                    .filter_map(|(action, child_node)| {
                        let (visit_count, value_sum) = {
                            let child_ref = child_node.clone();
                            let child_node = child_ref.read().unwrap();
                            if child_node.fully_explored() {
                                log::trace!("Select short circuited - fully explored");
                                return None;
                            }
                            let cached_ucb = child_node.cached_ucb(
                                child_node.value_sum(), child_node.visit_count(), parent_visit_count);
                            if let Some(ucb) = cached_ucb {
                                return Some((action.clone(), ucb));
                            }
                            if game_action {
                                (child_node.visit_count() as f64 / child_node.weight() as f64, 1.0)
                            } else {
                                (child_node.visit_count() as f64, child_node.value_sum())
                            }
                        };
                        let parent_visits = parent_visit_count as f64;
                        if visit_count == 0.0 {
                            return Some((action.clone(), f64::INFINITY));
                        }
                        let q: f64 = value_sum / visit_count;
                        let u: f64 = (parent_visits.ln() / visit_count).sqrt();
                        // Random used to break ties
                        // Todo: Cache the rng
                        let r: f64 = rand::thread_rng().gen::<f64>() * RANDOM_FACTOR;
                        let ucb: f64 = q + constant * u + r;
                        trace!(
                            "UCB action: {:?}, value_sum: {}, visit_count: {}, parent_visits: {}, q: {}, u: {}, c: {} ucb: {}",
                            action,
                            value_sum,
                            visit_count,
                            parent_visits,
                            q,
                            u,
                            constant,
                            ucb
                        );
                        Some((action.clone(), ucb))
                    })
                    .collect();

    for (action, ucb) in ucbs.iter_mut() {
        let node = children.get(action).unwrap();
        let read_node = node.read().unwrap();
        read_node.cache_ucb(
            *ucb,
            read_node.value_sum(),
            read_node.visit_count(),
            parent_visit_count,
        );
    }
    ucbs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    trace!("UCBS action, ucb: {:?}", ucbs.iter().collect::<Vec<_>>());
    ucbs
}

pub fn create_expanded_node<StateType>(
    state: StateType,
    weight: Option<u32>,
) -> Node<StateType, StateType::ActionType>
where
    StateType: State,
{
    // Used here so can be used outside of an instance of Node
    // (I think the Node::new_expanded should be able to work? But my rust brain
    // is still learning and couldn't figure out syntax that the type checker
    // was happy with)
    let mut children: HashMap<
        StateType::ActionType,
        Arc<RwLock<Node<StateType, StateType::ActionType>>>,
    > = HashMap::new();
    let game_action = match state.next_actor() {
        Actor::Player(_) => {
            for action in state.permitted_actions() {
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
    };

    Node::Expanded {
        state,
        children,
        visit_count: 0,
        value_sum: 0.0,
        cached_ucb: RwLock::new(None),
        cached_fully_explored: RwLock::new(None),
        game_action,
        weight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::injectable_game::{InjectableGameAction, InjectableGameState};

    #[test]
    fn test_create_expanded_node() {
        let state = InjectableGameState {
            injected_reward: vec![0.0],
            injected_terminal: false,
            injected_permitted_actions: vec![InjectableGameAction::Win],
            player_count: 1,
            next_actor: Actor::Player(0),
        };
        let node = create_expanded_node(state, None);
        assert_eq!(node.visit_count(), 0);
        assert_eq!(node.value_sum(), 0.0);
    }

    #[test]
    fn test_best_pick_weighted_visits() {
        // Maybe this being parameterized would be better?
        // But, it's probably going to look messy, so this will do as a minimum check
        // Low effort test - create a node with weight 1 and weight 2, give them unexpanded children too,
        // check that the next pick is from the weight 2 node

        let mut root_node = create_expanded_node(
            InjectableGameState {
                injected_reward: vec![0.0f64],
                injected_terminal: false,
                injected_permitted_actions: vec![],
                player_count: 1,
                next_actor: Actor::GameAction(vec![
                    (InjectableGameAction::WinInXTurns(1), 1),
                    (InjectableGameAction::WinInXTurns(2), 2),
                ]),
            },
            None,
        );

        let mut win_in_x_turns_1 = create_expanded_node(
            InjectableGameState {
                injected_reward: vec![0.0f64],
                injected_terminal: false,
                injected_permitted_actions: vec![],
                player_count: 1,
                next_actor: Actor::Player(0),
            },
            Some(1),
        );

        let mut win_in_x_turns_2 = create_expanded_node(
            InjectableGameState {
                injected_reward: vec![0.0f64],
                injected_terminal: false,
                injected_permitted_actions: vec![],
                player_count: 1,
                next_actor: Actor::Player(0),
            },
            Some(2),
        );

        root_node.visit(0.0f64);

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
            let child = root_node_ref.get_child(InjectableGameAction::WinInXTurns(2));
            let mut child_write = child.write().unwrap();
            child_write.visit(0.0f64);
        }
        // Weight 2 visited, weight 1 not, check that weight 1 is next
        {
            let best_pick = best_pick(&locked_node, 2.0_f64.sqrt());
            assert_eq!(
                best_pick.first().unwrap().0,
                InjectableGameAction::WinInXTurns(1)
            );
        }

        {
            let root_node_ref = locked_node.read().unwrap();
            let child = root_node_ref.get_child(InjectableGameAction::WinInXTurns(1));
            let mut child_write = child.write().unwrap();
            child_write.visit(0.0f64);
        }

        let best_pick = best_pick(&locked_node, 2.0_f64.sqrt());
        // We're checking for 2 - because it's the first node from the root (and best-pick isn't
        // iterative down the tree, selection is)
        assert_eq!(
            best_pick.first().unwrap().0,
            InjectableGameAction::WinInXTurns(2)
        );
    }
}
