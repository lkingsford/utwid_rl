use crate::mcts::game_trait::{Action, Actor, State};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct TestHyperreward {
    pub value: i32,
}

///
/// A generic test game that can have injected reward, terminal state, and permitted actions
/// to test tree and node related things.
///
#[derive(Clone, Debug)]
pub struct InjectableGameState {
    pub injected_reward: Vec<f64>,
    pub injected_terminal: bool,
    pub injected_permitted_actions: Vec<InjectableGameAction>,
    pub perceived_permitted_actions: HashMap<(u8, u8), Vec<InjectableGameAction>>,
    pub player_count: u8,
    pub next_actor: Actor<InjectableGameAction>,
    pub injected_hyperreward: TestHyperreward,
    pub terminal_hyperreward: TestHyperreward,
}

impl State for InjectableGameState {
    type ActionType = InjectableGameAction;
    type GameHyperrewardType = TestHyperreward;

    fn permitted_actions(&self, per: Option<u8>) -> Vec<Self::ActionType> {
        if let (Some(per), Actor::Player(actor_id)) = (per, &self.next_actor)
            && let Some(actions) = self.perceived_permitted_actions.get(&(*actor_id, per))
        {
            return actions.clone();
        }

        self.injected_permitted_actions.clone()
    }
    fn next_actor(&self) -> Actor<Self::ActionType> {
        self.next_actor.clone()
    }
    fn reward(&self) -> Vec<f64> {
        return self.injected_reward.clone();
    }

    fn terminal(&self) -> bool {
        return self.injected_terminal;
    }
    fn round_hyperreward(&self) -> Self::GameHyperrewardType {
        if self.terminal() {
            self.terminal_hyperreward.clone()
        } else {
            self.injected_hyperreward.clone()
        }
    }
}

#[derive(Hash, Clone, Eq, PartialEq, Debug)]
pub enum InjectableGameAction {
    Win,
    Lose,
    WinInXTurns(u8),
    NextTurnInjectActionCount(u8),
    Nothing,
    NextTurnGameAction(Vec<InjectableGameAction>),
}
impl Action for InjectableGameAction {
    type StateType = InjectableGameState;
    type EventType = ();

    fn execute(&self, state: &Self::StateType) -> (Self::StateType, Vec<Self::EventType>) {
        let next_actor = if let Actor::Player(player_id) = state.next_actor() {
            Actor::Player((player_id + 1) % state.player_count)
        } else {
            Actor::Player(0)
        };
        (
            match self {
                InjectableGameAction::NextTurnInjectActionCount(c) => InjectableGameState {
                    injected_permitted_actions: (0..*c)
                        .map(|i| InjectableGameAction::WinInXTurns(i))
                        .collect(),
                    perceived_permitted_actions: HashMap::new(),
                    next_actor,
                    ..state.clone()
                },
                InjectableGameAction::WinInXTurns(turns) => InjectableGameState {
                    injected_permitted_actions: {
                        if *turns > 0 {
                            vec![InjectableGameAction::WinInXTurns(turns - 1)]
                        } else {
                            vec![InjectableGameAction::Win]
                        }
                    },
                    perceived_permitted_actions: HashMap::new(),
                    next_actor,
                    ..state.clone()
                },
                InjectableGameAction::Win => InjectableGameState {
                    injected_terminal: true,
                    injected_reward: vec![1.0],
                    perceived_permitted_actions: HashMap::new(),
                    next_actor,
                    ..state.clone()
                },
                InjectableGameAction::Lose => InjectableGameState {
                    injected_terminal: true,
                    injected_reward: vec![-1.0],
                    perceived_permitted_actions: HashMap::new(),
                    next_actor,
                    ..state.clone()
                },
                InjectableGameAction::Nothing => InjectableGameState {
                    perceived_permitted_actions: HashMap::new(),
                    next_actor,
                    ..state.clone()
                },
                InjectableGameAction::NextTurnGameAction(actions) => InjectableGameState {
                    injected_permitted_actions: actions.clone(),
                    perceived_permitted_actions: state.perceived_permitted_actions.clone(),
                    next_actor,
                    ..state.clone()
                },
            },
            vec![],
        )
    }
}
