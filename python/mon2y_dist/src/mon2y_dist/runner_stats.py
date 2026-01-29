import datetime
import json
from typing import Dict, NamedTuple
from collections import defaultdict
import sqlalchemy


class RunnerStats(NamedTuple):
    total_iterations: int
    total_trials: int
    num_processes: int
    total_ask_time: float
    num_asks: int


class RunnerStatus(NamedTuple):
    runners: Dict[str, RunnerStats]


def runner_status(
    start_time: datetime.datetime, end_time: datetime.datetime, storage_url: str
) -> RunnerStatus:
    """
    Calculates runner status within a given time window by querying the Optuna database.

    Args:
        start_time: The start of the time window.
        end_time: The end of the time window.
        storage_url: The connection string for the Optuna database.

    Returns:
        A RunnerStatus object containing statistics for each runner.
    """
    engine = sqlalchemy.create_engine(storage_url)
    runner_stats = defaultdict(
        lambda: {"total_iterations": 0, "total_trials": 0}
    )

    query = """
    SELECT
        runner_id_attr.value_json AS runner_id,
        iterations_attr.value_json AS iterations
    FROM trials
    JOIN trial_user_attributes AS runner_id_attr ON trials.trial_id = runner_id_attr.trial_id AND runner_id_attr.key = 'runner_id'
    JOIN trial_user_attributes AS iterations_attr ON trials.trial_id = iterations_attr.trial_id AND iterations_attr.key = 'iterations'
    WHERE
        trials.datetime_complete BETWEEN :start_time AND :end_time
        AND trials.state = 'COMPLETE'
    """

    with engine.connect() as connection:
        result = connection.execute(
            sqlalchemy.text(query),
            {"start_time": start_time, "end_time": end_time},
        )
        for row in result:
            # runner_id and iterations are stored as JSON strings (e.g., '"runner-123"' or '"1000"')
            runner_id = json.loads(row.runner_id)
            iterations = json.loads(row.iterations)
            
            if runner_id:
                runner_stats[runner_id]["total_iterations"] += int(iterations)
                runner_stats[runner_id]["total_trials"] += 1

    processed_stats = {
        runner_id: RunnerStats(
            total_iterations=stats["total_iterations"],
            total_trials=stats["total_trials"],
            # These stats are no longer available from the database, defaulting to 0
            num_processes=0,
            total_ask_time=0.0,
            num_asks=0,
        )
        for runner_id, stats in runner_stats.items()
    }

    return RunnerStatus(runners=processed_stats)
