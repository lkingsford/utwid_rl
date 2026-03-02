use crate::mon2y::game::{Action, State};
use std::io;

pub trait Game {
    type StateType: State<ActionType = Self::ActionType> + 'static + Send + Sync;
    type ActionType: Action<StateType = Self::StateType> + 'static + Send + Sync;
    fn get_human_turn(&self, state: &Self::StateType) -> Self::ActionType {
        for (i, action) in state.permitted_actions().iter().enumerate() {
            println!("{} {:?}", i, action);
        }
        loop {
            let mut input = String::new();
            print!("Input for player {:?}: ", state.next_actor());
            if io::stdin().read_line(&mut input).is_err() {
                println!("Failed to read line. Please try again.");
                continue;
            }
            match input.trim().parse::<usize>() {
                Ok(action) => {
                    if action < state.permitted_actions().len() {
                        return state.permitted_actions()[action].clone();
                    } else {
                        println!("Action {} is out of range. Please try again.", action);
                        continue;
                    }
                }
                Err(_) => {
                    println!("Failed to parse action. Please enter a valid number.");
                    continue;
                }
            }
        }
    }
    fn visualise_state(&self, state: &Self::StateType);
    fn init_game(&self) -> Self::StateType;
}
