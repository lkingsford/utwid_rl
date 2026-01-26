import json
import os
from datetime import datetime
from typing import Dict, NamedTuple, Any
import sqlalchemy
from sqlalchemy import create_engine, text

# NamedTuple for individual process statistics within a study
class ProcessStats(NamedTuple):
    trials_completed: int
    iterations: int
    sum_total_trial_time_ms: float
    avg_total_trial_time_ms: float
    sum_explore_time_ms: float
    avg_explore_time_ms: float
    sum_ask_reply_time_ms: float
    avg_ask_reply_time_ms: float


# NamedTuple for statistics of a single study within a runner
class StudyStats(NamedTuple):
    total_trials: int
    total_iterations: int
    sum_ask_reply_time_ms: float
    avg_ask_reply_time_ms: float
    processes: Dict[int, ProcessStats]  # Keyed by process_id


# NamedTuple for statistics of a single runner
class RunnerOutput(NamedTuple):
    total_trials: int
    total_iterations: int
    sum_ask_reply_time_ms: float
    avg_ask_reply_time_ms: float
    studies: Dict[str, StudyStats]  # Keyed by study_name


# NamedTuple for the overall status output
class StatusOutput(NamedTuple):
    total_completed_trials: int  # New field
    total_iterations: int
    sum_ask_reply_time_ms: float
    avg_ask_reply_time_ms: float
    runners: Dict[str, RunnerOutput]  # Keyed by runner_id


_engine = None


def _get_engine():
    global _engine
    if _engine is None:
        storage_url = os.environ.get("OPTUNA_STORAGE") or "sqlite:///db.sqlite3"
        _engine = create_engine(storage_url)
    return _engine


def runner_status(start_time: datetime, end_time: datetime) -> StatusOutput:
    engine = _get_engine()

    # Query to get relevant trial user attributes for completed trials within the time range
    query = text("""
        SELECT
            t.trial_id,
            s.study_name,
            tua_runner_id.value_json AS runner_id_json,
            tua_process_id.value_json AS process_id_json,
            tua_iterations.value_json AS iterations_json,
            tua_total_trial_time_ms.value_json AS total_trial_time_ms_json,
            tua_explore_time_ms.value_json AS explore_time_ms_json,
            tua_ask_reply_time_ms.value_json AS ask_reply_time_ms_json
        FROM
            trials AS t
        JOIN
            studies AS s
            ON t.study_id = s.study_id
        JOIN
            trial_user_attributes AS tua_runner_id
            ON t.trial_id = tua_runner_id.trial_id AND tua_runner_id."key" = 'runner_id'
        JOIN
            trial_user_attributes AS tua_process_id
            ON t.trial_id = tua_process_id.trial_id AND tua_process_id."key" = 'process_id'
        JOIN
            trial_user_attributes AS tua_iterations
            ON t.trial_id = tua_iterations.trial_id AND tua_iterations."key" = 'iterations'
        JOIN
            trial_user_attributes AS tua_total_trial_time_ms
            ON t.trial_id = tua_total_trial_time_ms.trial_id AND tua_total_trial_time_ms."key" = 'total_trial_time_ms'
        JOIN
            trial_user_attributes AS tua_explore_time_ms
            ON t.trial_id = tua_explore_time_ms.trial_id AND tua_explore_time_ms."key" = 'explore_time_ms'
        JOIN
            trial_user_attributes AS tua_ask_reply_time_ms
            ON t.trial_id = tua_ask_reply_time_ms.trial_id AND tua_ask_reply_time_ms."key" = 'ask_reply_time_ms'
        WHERE
            t.state = 'COMPLETE'
            AND t.datetime_complete BETWEEN :start_time AND :end_time
    """)

    # Structure to hold aggregated data:
    # { runner_id: { study_name: { process_id: {
    #   trials: int, iterations: int,
    #   sum_total_trial_time_ms: float, count_total_trial_time_ms: int,
    #   sum_explore_time_ms: float, count_explore_time_ms: int,
    #   sum_ask_reply_time_ms: float, count_ask_reply_time_ms: int
    # } } } }
    aggregated_data: Dict[str, Dict[str, Dict[int, Dict[str, Any]]]] = {}
    overall_total_iterations = 0
    overall_total_trials = 0
    overall_sum_ask_reply_time_ms = 0.0
    overall_count_ask_reply_time_ms = 0

    with engine.connect() as connection:
        result = connection.execute(query, {"start_time": start_time, "end_time": end_time})
        for row in result:
            try:
                runner_id = json.loads(row.runner_id_json)
                process_id = json.loads(row.process_id_json)
                iterations = json.loads(row.iterations_json)
                study_name = row.study_name
                total_trial_time_ms = json.loads(row.total_trial_time_ms_json)
                explore_time_ms = json.loads(row.explore_time_ms_json)
                ask_reply_time_ms = json.loads(row.ask_reply_time_ms_json)

                # Log the deserialized timing values - Removed as per user request to avoid clutter
                # print(f"DEBUG runner_stats: Deserialized timings for trial {row.trial_id}: total={total_trial_time_ms}, explore={explore_time_ms}, ask_reply={ask_reply_time_ms}")
            except json.JSONDecodeError as e:
                print(f"Error decoding JSON from trial_user_attributes: {e} for row: {row}")
                continue
            except Exception as e:
                print(f"Error processing row: {e} for row: {row}")
                continue

            if runner_id not in aggregated_data:
                aggregated_data[runner_id] = {}

            if study_name not in aggregated_data[runner_id]:
                aggregated_data[runner_id][study_name] = {}

            if process_id not in aggregated_data[runner_id][study_name]:
                aggregated_data[runner_id][study_name][process_id] = {
                    "trials": 0, "iterations": 0,
                    "sum_total_trial_time_ms": 0.0, "count_total_trial_time_ms": 0,
                    "sum_explore_time_ms": 0.0, "count_explore_time_ms": 0,
                    "sum_ask_reply_time_ms": 0.0, "count_ask_reply_time_ms": 0
                }

            process_agg = aggregated_data[runner_id][study_name][process_id]
            process_agg["trials"] += 1
            process_agg["iterations"] += iterations
            process_agg["sum_total_trial_time_ms"] += total_trial_time_ms
            process_agg["count_total_trial_time_ms"] += 1
            process_agg["sum_explore_time_ms"] += explore_time_ms
            process_agg["count_explore_time_ms"] += 1
            process_agg["sum_ask_reply_time_ms"] += ask_reply_time_ms
            process_agg["count_ask_reply_time_ms"] += 1

            overall_total_iterations += iterations
            overall_total_trials += 1
            overall_sum_ask_reply_time_ms += ask_reply_time_ms
            overall_count_ask_reply_time_ms += 1

    runners_output: Dict[str, RunnerOutput] = {}
    for runner_id, studies_data in aggregated_data.items():
        runner_total_trials = 0
        runner_total_iterations = 0
        runner_sum_ask_reply_time_ms = 0.0
        runner_count_ask_reply_time_ms = 0
        studies_dict: Dict[str, StudyStats] = {}
        for study_name, processes_data in studies_data.items():
            study_total_trials = 0
            study_total_iterations = 0
            study_sum_ask_reply_time_ms = 0.0
            study_count_ask_reply_time_ms = 0
            processes_dict: Dict[int, ProcessStats] = {}
            for process_id, data in processes_data.items():
                avg_total_trial_time_ms = data["sum_total_trial_time_ms"] / data["count_total_trial_time_ms"] if data["count_total_trial_time_ms"] > 0 else 0.0
                avg_explore_time_ms = data["sum_explore_time_ms"] / data["count_explore_time_ms"] if data["count_explore_time_ms"] > 0 else 0.0
                avg_ask_reply_time_ms = data["sum_ask_reply_time_ms"] / data["count_ask_reply_time_ms"] if data["count_ask_reply_time_ms"] > 0 else 0.0

                processes_dict[process_id] = ProcessStats(
                    trials_completed=data["trials"],
                    iterations=data["iterations"],
                    sum_total_trial_time_ms=data["sum_total_trial_time_ms"],
                    avg_total_trial_time_ms=avg_total_trial_time_ms,
                    sum_explore_time_ms=data["sum_explore_time_ms"],
                    avg_explore_time_ms=avg_explore_time_ms,
                    sum_ask_reply_time_ms=data["sum_ask_reply_time_ms"],
                    avg_ask_reply_time_ms=avg_ask_reply_time_ms
                )
                study_total_trials += data["trials"]
                study_total_iterations += data["iterations"]
                study_sum_ask_reply_time_ms += data["sum_ask_reply_time_ms"]
                study_count_ask_reply_time_ms += data["count_ask_reply_time_ms"]

            avg_study_ask_reply_time_ms = study_sum_ask_reply_time_ms / study_count_ask_reply_time_ms if study_count_ask_reply_time_ms > 0 else 0.0
            studies_dict[study_name] = StudyStats(
                total_trials=study_total_trials,
                total_iterations=study_total_iterations,
                sum_ask_reply_time_ms=study_sum_ask_reply_time_ms,
                avg_ask_reply_time_ms=avg_study_ask_reply_time_ms,
                processes=processes_dict
            )
            runner_total_trials += study_total_trials
            runner_total_iterations += study_total_iterations
            runner_sum_ask_reply_time_ms += study_sum_ask_reply_time_ms
            runner_count_ask_reply_time_ms += study_count_ask_reply_time_ms

        avg_runner_ask_reply_time_ms = runner_sum_ask_reply_time_ms / runner_count_ask_reply_time_ms if runner_count_ask_reply_time_ms > 0 else 0.0
        runners_output[runner_id] = RunnerOutput(
            total_trials=runner_total_trials,
            total_iterations=runner_total_iterations,
            sum_ask_reply_time_ms=runner_sum_ask_reply_time_ms,
            avg_ask_reply_time_ms=avg_runner_ask_reply_time_ms,
            studies=studies_dict
        )

    avg_overall_ask_reply_time_ms = overall_sum_ask_reply_time_ms / overall_count_ask_reply_time_ms if overall_count_ask_reply_time_ms > 0 else 0.0
    return StatusOutput(
        total_completed_trials=overall_total_trials,
        total_iterations=overall_total_iterations,
        sum_ask_reply_time_ms=overall_sum_ask_reply_time_ms,
        avg_ask_reply_time_ms=avg_overall_ask_reply_time_ms,
        runners=runners_output
    )
