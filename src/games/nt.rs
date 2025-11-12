use std::collections::HashMap;

use crate::game::Game;
use crate::mcts::game_trait::{Action, Actor, State};

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum NTAction {
    Take,
    NoThanks,
    Draw(u8),
}

impl Action for NTAction {
    type StateType = NTState;
    fn execute(&self, state: &Self::StateType) -> Self::StateType {
        match self {
            NTAction::Take => {
                let mut cards = state.cards.clone();
                cards.insert(
                    state.current_card.unwrap(),
                    CardState::Taken(state.next_player),
                );
                let mut tokens = state.tokens.clone();
                tokens.insert(
                    state.next_player,
                    state.tokens[&state.next_player] + state.tokens_on_card,
                );
                NTState {
                    cards,
                    tokens,
                    next_player: state.next_player,
                    to_draw: true,
                    current_card: state.current_card,
                    tokens_on_card: 0,
                }
            }
            NTAction::NoThanks => {
                let mut tokens = state.tokens.clone();
                tokens.insert(state.next_player, state.tokens[&state.next_player] - 1);
                NTState {
                    cards: state.cards.clone(),
                    tokens,
                    next_player: (state.next_player + 1) % state.tokens.len() as u8,
                    to_draw: false,
                    current_card: state.current_card,
                    tokens_on_card: state.tokens_on_card + 1,
                }
            }
            NTAction::Draw(card) => NTState {
                cards: state.cards.clone(),
                tokens: state.tokens.clone(),
                next_player: state.next_player,
                to_draw: false,
                current_card: Some(*card),
                tokens_on_card: 0,
            },
        }
    }
}

#[derive(Clone, PartialEq)]
enum CardState<Actor> {
    Drawable,
    Taken(Actor),
}

#[derive(Clone)]
pub struct NTState {
    cards: HashMap<u8, CardState<u8>>,
    tokens: HashMap<u8, u8>,
    next_player: u8,
    to_draw: bool,
    current_card: Option<u8>,
    tokens_on_card: u8,
}

impl NTState {
    fn scores(&self) -> Vec<f64> {
        let mut scores = self
            .tokens
            .iter()
            .map(|(_, tokens)| -1.0 * *tokens as f64)
            .collect::<Vec<_>>();
        // I know this could be functional, but maybe later.
        for (card, card_state) in &self.cards {
            if let CardState::Taken(player) = card_state {
                if (*card == 3)
                    || (*card > 0
                        && !matches!(self.cards.get(&(*card - 1)), Some(CardState::Taken(owned)) if owned == player))
                {
                    scores[*player as usize] += *card as f64;
                }
            }
        }
        scores
    }
}

impl State for NTState {
    type ActionType = NTAction;

    fn next_actor(&self) -> Actor<NTAction> {
        match self.to_draw {
            false => Actor::Player(self.next_player),
            true => Actor::GameAction(self.possible_non_player_actions()),
        }
    }

    fn permitted_actions(&self) -> Vec<Self::ActionType> {
        if self.tokens[&self.next_player] > 0 {
            vec![NTAction::Take, NTAction::NoThanks]
        } else {
            vec![NTAction::Take]
        }
    }

    fn possible_non_player_actions(&self) -> Vec<(Self::ActionType, u32)> {
        self.cards
            .iter()
            .filter(|(_, card_state)| matches!(card_state, CardState::Drawable))
            .map(|(card, _)| (NTAction::Draw(*card), 1))
            .collect()
    }

    fn terminal(&self) -> bool {
        self.cards
            .iter()
            .filter(|(_, card_state)| matches!(card_state, CardState::Taken(_)))
            .count()
            >= 24
    }

    fn reward(&self) -> Vec<f64> {
        // Lowest score wins
        // Distributing reward linearly between -1 and 1 based on position, not score
        let mut scores: Vec<(usize, f64)> = self.scores().into_iter().enumerate().collect();
        scores.sort_unstable_by(|(_, a_score), (_, b_score)| a_score.partial_cmp(b_score).unwrap());

        let player_count = scores.len();

        let interval = 1.0 / (player_count as f64 - 1.0);
        let mut reward: Vec<f64> = vec![0.0; player_count];

        for (pos, (i, _)) in scores.into_iter().enumerate() {
            reward[i] = 1.0 - (interval * pos as f64) + (if i == 0 { 1.0 } else { 0.0 })
        }

        log::trace!("Scores: {:?}", self.scores());
        log::trace!("Reward: {:?}", reward);
        reward
    }
}

pub struct NT {
    pub player_count: u8,
}

impl Game for NT {
    type StateType = NTState;
    type ActionType = NTAction;
    type HyperparamsType = ();

    fn visualise_state(&self, state: &Self::StateType) {
        for i in 0..self.player_count {
            let mut cards = state
                .cards
                .iter()
                .filter(|(_, card)| matches!(card, CardState::Taken(taken_i) if *taken_i == i))
                .collect::<Vec<_>>();
            cards.sort_by(|a, b| a.0.cmp(b.0));
            println!(
                "Player {}: ({} tokens) - {}",
                i,
                state.tokens[&i],
                cards
                    .iter()
                    .map(|(&card, _)| card.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        println!(
            "Current card: {:?}, with {} tokens",
            state.current_card, state.tokens_on_card
        );
        println!("Active player: {}", state.next_player);

        if state.terminal() {
            println!("Scores: {:?}", state.scores());
        }
    }

    fn init_game(&self, _hyperparams: &Self::HyperparamsType) -> Self::StateType {
        NTState {
            cards: (3..35)
                .map(|card| (card, CardState::Drawable))
                .collect::<HashMap<_, _>>(),
            tokens: (0..self.player_count)
                .map(|player_id| {
                    (
                        player_id,
                        match self.player_count {
                            0..=5 => 11,
                            6 => 9,
                            _ => 7,
                        },
                    )
                })
                .collect::<HashMap<_, _>>(),
            current_card: None,
            next_player: 0,
            to_draw: true,
            tokens_on_card: 0,
        }
    }
}
