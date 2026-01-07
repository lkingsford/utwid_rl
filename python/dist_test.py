import argparse
import logging
import math
import time
from datetime import datetime

import optuna


def setup_logging():
    """Configures logging to show date and default to INFO level."""
    logging.basicConfig(
        level=logging.INFO,
        format='%(asctime)s - %(levelname)s - %(message)s',
    )

def define(n: int) -> dict:
    """Create a dictionary of distributions for the study."""
    logging.info(f"Defining {n} distributions.")
    dists = {
        f"v{i}": optuna.distributions.FloatDistribution(1e-7, 1.0)
        for i in range(n)
    }
    logging.info("Finished defining distributions.")
    return dists

def evaluate(params: dict) -> float:
    """Evaluate the given parameters."""
    logging.info(f"Evaluating parameters.")
    val = abs(1 - math.sqrt(sum([i**2 for i in params.values()])))
    logging.info(f"Evaluation result: {val}")
    return val

def start_trial(study: optuna.Study, dists: dict) -> float:
    """Ask for a trial, evaluate it, and tell the study."""
    logging.info("Starting trial.")
    
    start_time = time.perf_counter()
    trial = study.ask(dists)
    end_time = time.perf_counter()
    duration_ms = (end_time - start_time) * 1000
    logging.info(f"study.ask took {duration_ms:.2f} ms")
    
    logging.info(f"Trial params: {trial.params}")
    
    result = evaluate(trial.params)
    logging.info(f"Trial {trial.number} result: {result}")
    
    study.tell(trial, result)
    logging.info("Finished trial.")
    return duration_ms

def main():
    """Main function to run the optimization."""
    setup_logging()
    logging.info("Starting main function.")
    
    parser = argparse.ArgumentParser(description="Optuna distribution test.")
    parser.add_argument(
        "--storage",
        type=str,
        default="sqlite:///db.sqlite3",
        help="Database storage URL.",
    )
    parser.add_argument("--n", type=int, default=20, help="Number of dimensions.")
    parser.add_argument("--trials", type=int, default=100, help="Number of trials.")
    args = parser.parse_args()

    logging.info(f"Arguments: {args}")

    study = optuna.create_study(
        study_name="disttest",
        storage=args.storage,
        load_if_exists=True,
        direction="minimize"
    )

    dists = define(args.n)
    
    ask_timings = []
    logging.info(f"Starting {args.trials} trials.")
    for _ in range(args.trials):
        duration = start_trial(study, dists)
        ask_timings.append(duration)
    logging.info("Finished all trials.")
    
    total_time_ms = sum(ask_timings)
    if ask_timings:
        mean_time_per_ask_ms = total_time_ms / len(ask_timings)
        mean_time_per_ask_per_n_ms = mean_time_per_ask_ms / args.n
    else:
        mean_time_per_ask_ms = 0
        mean_time_per_ask_per_n_ms = 0

    print("--- Timing Metrics ---")
    print(f"Total study.ask time: {total_time_ms:.2f} ms")
    print(f"Mean time per study.ask: {mean_time_per_ask_ms:.2f} ms")
    print(f"Mean time per study.ask / n: {mean_time_per_ask_per_n_ms:.2f} ms")
    
    logging.info("Finished main function.")

if __name__ == "__main__":
    main()
