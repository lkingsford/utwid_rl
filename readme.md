# mon2y_rs

# Description

A project for me to learn rust, by implementing a generic monty carlo tree search algorithm with some games. Please don't judge the code too harshly: it literally is a learning project.

# Building

## Dependencies

### Rust

Rust needs to be installed. Use the normal rustup.

### Rust dependencies

... will be installed correctly when you build

### Python

Python needs to be installed, ideally in a virtualenv.

Tested with python 3.14 on Linux

### Python dependencies

`pip install maturin==1.9.6`

## Building


### Binaries

Build Arena with `cargo build arena`

Build Bench with `cargo build bench`


### Python Library

Build python library in development mode (installed to current virtual env) with `maturin develop`

## Calling in Python

After building with `maturin develop`, you can import and use the library in Python.

```python
import mon2y
```

### `explore()`

Runs the Monte Carlo Tree Search for a given game.

```python
mon2y.explore(
    game: mon2y.Games,
    iterations: int,
    thread_count: int,
    time_limit_secs: int | None = None,
    exploration_constant: float | None = None,
    hyperparams: dict | None = None
) -> list[dict]
```

-   **`game`**: The game to run. An enum from `mon2y.Games` (e.g., `mon2y.Games.C4`).
-   **`iterations`**: The number of MCTS iterations to perform.
-   **`thread_count`**: The number of threads to use for the search.
-   **`time_limit_secs`**: Optional time limit in seconds. The search will stop after this duration. Defaults to `None`.
-   **`exploration_constant`**: The UCB1 exploration constant. Defaults to `sqrt(2)`.
-   **`hyperparams`**: Optional dictionary of game-specific hyperparameters.

### `set_log_level()`

Sets the log level for the Rust backend.

```python
mon2y.set_log_level(level: str)
```

-   **`level`**: A string representing the log level (e.g., `"info"`, `"warn"`, `"error"`).

### `get_hyperparam_meta()`

Returns metadata about the available hyperparameters for a given game.

```python
mon2y.get_hyperparam_meta(game: mon2y.Games) -> dict
```

-   **`game`**: The game to get hyperparameter metadata for.

### `get_hyperreward_meta()`

Returns metadata about the available hyperrewards for a given game.

```python
mon2y.get_hyperreward_meta(game: mon2y.Games) -> dict
```

-   **`game`**: The game to get hyperreward metadata for.

### `Games` Enum

An enum representing the available games.

-   `mon2y.Games.C4`
-   `mon2y.Games.NT`
-   `mon2y.Games.CS`
-   `mon2y.Games.EBR`


