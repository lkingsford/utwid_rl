import argparse
import logging
import math
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

def start_trial(study: optuna.Study, dists: dict):
    """Ask for a trial, evaluate it, and tell the study."""
    logging.info("Starting trial.")
    
    trial = study.ask(dists)
    logging.info(f"Trial params: {trial.params}")
    
    result = evaluate(trial.params)
    logging.info(f"Trial {trial.number} result: {result}")
    
    study.tell(trial, result)
    logging.info("Finished trial.")

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
    
    logging.info(f"Starting {args.trials} trials.")
    for _ in range(args.trials):
        start_trial(study, dists)
    logging.info("Finished all trials.")
    
    logging.info("Finished main function.")

if __name__ == "__main__":
    main()
