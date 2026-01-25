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

# NamedTuple for statistics of a single study within a runner
class StudyStats(NamedTuple):
    total_trials: int
    total_iterations: int
    processes: Dict[int, ProcessStats]  # Keyed by process_id

# NamedTuple for statistics of a single runner
class RunnerOutput(NamedTuple):
    total_trials: int
    total_iterations: int
    studies: Dict[str, StudyStats]  # Keyed by study_name

# NamedTuple for the overall status output
class StatusOutput(NamedTuple):
    total_completed_trials: int  # New field
    total_iterations: int
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
            tua_iterations.value_json AS iterations_json
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
        WHERE
            t.state = 'COMPLETE'
            AND t.datetime_complete BETWEEN :start_time AND :end_time
    """)

    # Structure to hold aggregated data:
    # { runner_id: { study_name: { process_id: { trials: int, iterations: int } } } }
    aggregated_data: Dict[str, Dict[str, Dict[int, Dict[str, int]]]] = {}
    overall_total_iterations = 0
    overall_total_trials = 0

    with engine.connect() as connection:
        result = connection.execute(query, {"start_time": start_time, "end_time": end_time})
        for row in result:
            try:
                runner_id = json.loads(row.runner_id_json)
                process_id = json.loads(row.process_id_json)
                iterations = json.loads(row.iterations_json)
                study_name = row.study_name  # Directly from trials table
            except json.JSONDecodeError as e:
                print(f"Error decoding JSON from trial_user_attributes: {e} for row: {row}")
                continue
            except Exception as e:  # Catch other potential errors, e.g. study_name not found
                print(f"Error processing row: {e} for row: {row}")
                continue

            if runner_id not in aggregated_data:
                aggregated_data[runner_id] = {}

            if study_name not in aggregated_data[runner_id]:
                aggregated_data[runner_id][study_name] = {}

            if process_id not in aggregated_data[runner_id][study_name]:
                aggregated_data[runner_id][study_name][process_id] = {"trials": 0, "iterations": 0}

            aggregated_data[runner_id][study_name][process_id]["trials"] += 1
            aggregated_data[runner_id][study_name][process_id]["iterations"] += iterations
            overall_total_iterations += iterations
            overall_total_trials += 1

    runners_output: Dict[str, RunnerOutput] = {}
    for runner_id, studies_data in aggregated_data.items():
        runner_total_trials = 0
        runner_total_iterations = 0
        studies_dict: Dict[str, StudyStats] = {}
        for study_name, processes_data in studies_data.items():
            study_total_trials = 0
            study_total_iterations = 0
            processes_dict: Dict[int, ProcessStats] = {}
            for process_id, data in processes_data.items():
                processes_dict[process_id] = ProcessStats(
                    trials_completed=data["trials"],
                    iterations=data["iterations"]
                )
                study_total_trials += data["trials"]
                study_total_iterations += data["iterations"]
            studies_dict[study_name] = StudyStats(
                total_trials=study_total_trials,
                total_iterations=study_total_iterations,
                processes=processes_dict
            )
            runner_total_trials += study_total_trials
            runner_total_iterations += study_total_iterations
        runners_output[runner_id] = RunnerOutput(
            total_trials=runner_total_trials,
            total_iterations=runner_total_iterations,
            studies=studies_dict
        )

    return StatusOutput(
        total_completed_trials=overall_total_trials,
        total_iterations=overall_total_iterations,
        runners=runners_output
    )
