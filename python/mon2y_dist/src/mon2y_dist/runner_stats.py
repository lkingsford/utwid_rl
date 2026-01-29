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
        lambda: {
            "total_iterations": 0,
            "total_trials": 0,
            "processes": set(),
            "total_ask_time": 0.0,
            "num_asks": 0,
        }
    )

    query = """
    SELECT
        runner_id_attr.value_json AS runner_id,
        iterations_attr.value_json AS iterations,
        process_id_attr.value_json AS process_id,
        ask_time_attr.value_json AS ask_time_ms
    FROM trials
    JOIN trial_user_attributes AS runner_id_attr ON trials.trial_id = runner_id_attr.trial_id AND runner_id_attr.key = 'runner_id'
    JOIN trial_user_attributes AS iterations_attr ON trials.trial_id = iterations_attr.trial_id AND iterations_attr.key = 'iterations'
    LEFT JOIN trial_user_attributes AS process_id_attr ON trials.trial_id = process_id_attr.trial_id AND process_id_attr.key = 'process_id'
    LEFT JOIN trial_user_attributes AS ask_time_attr ON trials.trial_id = ask_time_attr.trial_id AND ask_time_attr.key = 'ask_reply_time_ms'
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
            runner_id = json.loads(row.runner_id)
            if not runner_id:
                continue

            stats = runner_stats[runner_id]
            stats["total_iterations"] += json.loads(row.iterations or "0")
            stats["total_trials"] += 1

            if row.process_id:
                stats["processes"].add(json.loads(row.process_id))
            
            if row.ask_time_ms:
                stats["total_ask_time"] += json.loads(row.ask_time_ms) / 1000.0  # Convert ms to seconds
                stats["num_asks"] += 1


    processed_stats = {
        runner_id: RunnerStats(
            total_iterations=stats["total_iterations"],
            total_trials=stats["total_trials"],
            num_processes=len(stats["processes"]),
            total_ask_time=stats["total_ask_time"],
            num_asks=stats["num_asks"],
        )
        for runner_id, stats in runner_stats.items()
    }

    return RunnerStatus(runners=processed_stats)
