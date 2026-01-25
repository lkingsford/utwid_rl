import json
import os
from datetime import datetime
from typing import Dict, NamedTuple, Any
import sqlalchemy
from sqlalchemy import create_engine, text

# NamedTuple for individual runner statistics
class RunnerStats(NamedTuple):
    iterations: int
    process_count: int

# NamedTuple for the overall status output
class StatusOutput(NamedTuple):
    total_iterations: int
    runners: Dict[str, RunnerStats]

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
            t.datetime_complete,
            tua_runner_id.value_json AS runner_id_json,
            tua_process_id.value_json AS process_id_json,
            tua_iterations.value_json AS iterations_json
        FROM
            trials AS t
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

    runner_data: Dict[str, Dict[str, Any]] = {}
    total_iterations = 0

    with engine.connect() as connection:
        result = connection.execute(query, {"start_time": start_time, "end_time": end_time})
        for row in result:
            try:
                runner_id = json.loads(row.runner_id_json)
                process_id = json.loads(row.process_id_json)
                iterations = json.loads(row.iterations_json)
            except json.JSONDecodeError as e:
                # Log error and skip this row if JSON is invalid
                print(f"Error decoding JSON from trial_user_attributes: {e} for row: {row}")
                continue

            if runner_id not in runner_data:
                runner_data[runner_id] = {
                    "iterations": 0,
                    "process_ids": set()
                }
            
            runner_data[runner_id]["iterations"] += iterations
            runner_data[runner_id]["process_ids"].add(process_id)
            total_iterations += iterations

    runners_output: Dict[str, RunnerStats] = {}
    for runner_id, data in runner_data.items():
        runners_output[runner_id] = RunnerStats(
            iterations=data["iterations"],
            process_count=len(data["process_ids"])
        )
    
    return StatusOutput(total_iterations=total_iterations, runners=runners_output)
