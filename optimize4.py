import mon2y
import optuna
import math
import pandas as pd
import numpy as np
import logging
from IPython.display import display

logging.basicConfig(
    format='%(asctime)s %(levelname)s %(message)s',
    datefmt='%Y-%m-%d %H:%M:%S',
    level=logging.INFO
)

target_game_turns = 25
max_iterations = 250000
trials = 4000

study = optuna.create_study(
    storage="sqlite:///db.sqlite3",  # Specify the storage URL here.
    study_name="connect4_25_turns_6",
    directions = ["minimize", "minimize", "minimize"],
    load_if_exists = True,
)


def objective(trial: optuna.Trial):
    board_width = trial.suggest_int("board_width", 4, 50)
    board_height = trial.suggest_int("board_height", 4, 50)

    explore_iterations = max_iterations * (
        1 - (math.log(max(1, trials - max(trial.number, 2))) / math.log(trials))
    )
    logging.info(
        f"iterations: {explore_iterations} "
        f"board_width: {board_width} board_height: {board_height}"
    )

    raw_results = mon2y.explore(
        mon2y.Games.C4,
        int(explore_iterations),
        4,
        hyperparams={"board_width": board_width, "board_height": board_height},
    )
    logging.info("  explore done")

    df = pd.DataFrame(raw_results)
    logging.info("  dataframe converted")

    # ---- Trust construction ----
    df["ratio"] = (df["turns"] - df["rwalk"]) / df["turns"]

    s = df["sum_diff_est_reward"]
    df["norm_sum_diff_est_reward"] = (s - s.min()) / (s.max() - s.min())

    df["trust"] = df["ratio"] * df["norm_sum_diff_est_reward"]

    t = df["trust"]
    df["norm_trust"] = (t - t.min()) / (t.max() - t.min())

    # ---- Keep only top 1 std-dev of trust ----
    trust_mu = df["norm_trust"].mean()
    trust_sigma = df["norm_trust"].std()

    df_t = df[df["norm_trust"] >= trust_mu + trust_sigma]

    # Guard against pathological filtering
    if len(df_t) < 2:
        return float("inf"), float("inf"), float("inf")

    Neff = len(df_t)

    # ---- Turns ----
    mu_T = df_t["turns"].mean()
    var_T = df_t["turns"].var(ddof=0)
    se_T = np.sqrt(var_T / Neff)
    z_T = (mu_T - target_game_turns) / se_T if se_T > 0 else 0.0

    trial.set_user_attr("mu_T", mu_T)
    trial.set_user_attr("var_T", var_T)

    # ---- Win-rate ----
    wins = (df_t["winning_player"] == 1).astype(float)
    p_hat = wins.mean()
    se_p = np.sqrt(p_hat * (1 - p_hat) / Neff) if Neff > 0 else 0.0
    z_p = (p_hat - 0.5) / se_p if se_p > 0 else 0.0

    trial.set_user_attr("p_hat", p_hat)

    logging.info(
        f"Filtered Neff={Neff} mu_T={mu_T:.2f} p_hat={p_hat:.3f}"
    )

    return (
        abs(z_T),
        10 * abs(z_p),
        ((abs(board_width - 8) + abs(board_height - 8)))
    )


study.optimize(objective, n_trials=trials)
print(f"Best value: {study.best_value} (params: {study.best_params})")
