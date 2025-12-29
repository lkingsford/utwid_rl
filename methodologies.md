# mon2y_rs Tree Optimization - The Theory

## Purpose of this document

There's a _heck_ of a lot going on right now. I'm writing down what everything is so I can hopefully understand it later.

# Introduction

## Problem

The practical problem that I am trying to solve is that I want to improve my ability to playtest and balanace board game designs. I want to assess the most likely outcomes for a given set of rules and constants in the game, so I can use an optimizer to help me shape the game.

## Likely outcomes

The Monte Carlo Tree Search (**MCTS**) is normally used for picking the best path for an agent, normally by picking its best action. My usage is novel, because mon2y_rs is not trying to pick the best action, instead, it's using it to explore the tree for likely outcomes.

In short - I expand my use of the MCTS Reward Function beyond 'win or lose', to try to incorporate actions that a player might be more likely to take. 

As there are more iterations, the MCTS will better explore 'likely' parts of the tree.

## Brief technical background

mon2y_rs is implemented in Rust.

In antipation of using existing optimization tooling

- The explore functions
- The games
- The hyperparam types
- The hyperreward types

are exposed to Python as a Python library.

Any games must be implemented in Rust for speed's sake.


# The Model

## Introduction

The Model being interrogated is a stochastic game. It is modelled as a pure function `F (state[n-1], action) -> [state[n], allowed actions, R]` (where `R` is a vector of rewards).


## Hyperparameters

Hyperparameters are the parameters of the model itself that can be changed to change the function itself, rather than the iteration of it. 


### Implementation

A dictionary of metadata on a game's Hyperparameters is exposed to Python. That metadata includes:

- The default value
- The type (Float, Int, Uint, Bool, Enum)
- the range, if appropriate

A dictionary was used to due to ease of interfacing with.

## Hyperrewards

Hyperrewards are the outcomes of the model itself that might be optimized for.

For example, that could be:

- Whether a game finished in bankruptcy
- How many times an action was taken
- How close the final game results were

Most optimizers require this as a scalar value, where mon2y_rs outputs all of the values separately. The weighting and scalarization needs to be handled in Python before calling the optimizer. 

### Implementation

A game specific struct of hyperrewards is exposed in python, as well as (for the path to the final state):

- The total number of turns
- The number of random walk turns
- `sum(dWc[t..])` (see **Trust of iteration**)

A dictionary was used due to the high speed dataframe integrations available with a dictionary (that were significantly slower when exposing a class)


# Trust of iteration

## Problem statement

Problem statement is: 'how much do I trust the hyperreward from this iteration?'.

Conditions include: 

 - I'm not outputting the whole out stupid tree from Rust to Python
 - I'm trying to expose trustworthiness as we go rather than at the end

Assumptions are:

 - Each actor is working to maximize their reward
 - Therefore the higher the sum of all estimated rewards is an analogue for likelihood that game would be actually played. Big assumption there, I know.

## Proposed solution

*This is not a correctly written proof, or good formal definitions yet.*

- `Hyperreward :=` a measure of the system as a whole
- `Reward :=` the reward that each individual agent is trying to optimize
- `T :=` Total Turns
- `R :=` Random Walk Steps
- `R' := UCB Chosen Steps (and `R + R' = T`)
- `We[t, A] :=` estimated reward for choice A in a turn when the node was explored
- `Wc[t, A] :=` the theoretical estimated reward for a choice in a turn once fully explored (and assume as iterations -> inf. We trends towards Wc).
- `dWe[t, A] := We[t, A] - mean(We[t, A'])`. So - difference in est reward between action A and all the actions.
- `dWc[t, A] := Wc[t, A] - mean(Wc[t, A'])`.

Therefore, `sum(dWc[t[0]..t[n], A])` is a good way of measuring 'they picked the best move for them'.

Thesis that I did some tests for was was: 

`τ := (R'/T) * sum(dWe[t..])` - which we can do continuously, rather than wait until the end - is correlated to `sum(dWc[t..])`, and so can be an indicator of 'trustworthiness' of the hyperreward.

Experimental analysis shows:

 - `dWe` trends towards `dWc` as `R'/T -> 1`
 - `τ` is correlated with `sum(dWc[t..])`


### Implementation

`T`, `R` and `sum(dWc[t..[)` are exposed to Python to perform the τ calculation there. 


# Optimizer

Currently experimenting with using Optuna.

I need further education to using the trust variable effectively. Currently, I sort results by τ, and only use the top stddev worth of results for the figures.

I'm needing more stats guidance to best use those hyperrewards (for instance - to quantify a number of booleans with different τ into 'targetting that X% of iterations are true'.


# Possible Additional Steps


## Weighting of Random Walk Ratio in Trustworthiness Calculation

Further experimentation needs to be done to consider if the multiplier `'R/T` should be transformed before multiplying against the estimate.

## Length Bias

Further experimentation and analysis needs to be done to consider if longer games carry a higher trustworthiness value due to the use of sum instead of mean when scalarizing the estimated diffs.

## Include random action likelihood

Some actions of games include a random component (such as a dice roll). We need to consider how to apply this to weighting when considering trustworthiness, or whether we do at all given it already effecting the estimate value.

## Incorporate Trust Metric in diff_est_reward itself

When calculating the diff_est_reward for an action, use the `r'/t` to weight the values of the mean.

This needs to be explored: it may be overexposing the `r'/t` variable, and may result in a real worse outcome.

## Stanza types in Hyperparameters

This is almost certainly necessary.

This is to expose complex types as Hyperparameters - so, for instance, you would be able to optimize a list of numbers, or a list of dictionaries of hyperparemeters (for instance, the contents of a deck of cards).

