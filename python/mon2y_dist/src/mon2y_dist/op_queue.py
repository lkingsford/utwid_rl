import json
import logging
import os
import queue
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

import flask
import optuna
from optuna.storages import RDBStorage

from mon2y_dist.db_models import trial_additional_data_table

LOGGER = flask.Flask("__main__.op_queue").logger
LOGGER.setLevel(logging.INFO)  # Revert to INFO, was DEBUG for prior debugging


# --- Queue Configuration ---

# Maximum number of concurrent Optuna operations.
# Defaults to 16, can be overridden by environment variable.
MAX_OP_CONNECTIONS = int(os.environ.get("MAX_OP_CONNECTIONS", 16))

# Time in milliseconds for web requests to wait between checking for task completion.
# Defaults to 100ms, can be overridden by environment variable.
OP_POLL_MS = int(os.environ.get("OP_POLL_MS", 100))


# --- Shared Queue and Result Container ---

# The shared queue for pending Ask/Tell operations.
op_queue = queue.Queue()


@dataclass
class OpResult:
    """A simple, mutable container for the result of a queued operation."""

    result: Optional[Dict[str, Any]] = None
    error: Optional[Exception] = None
    is_complete: bool = False

    def complete(
        self, result: Optional[Dict[str, Any]] = None, error: Optional[Exception] = None
    ):
        self.result = result
        self.error = error
        self.is_complete = True


# --- Operation Classes ---


class Ask:
    """An operation to get a new trial from a study (study.ask)."""

    def __init__(
        self, study_name: str, distributions: Dict, result_container: OpResult
    ):
        self.study_name = study_name
        self.distributions = distributions
        self.result_container = result_container

    def run(self, study: optuna.Study, storage: optuna.storages.BaseStorage):
        """Executes the ask operation and places the result in the container."""
        LOGGER.info("Run called with {}", self.distributions)
        try:
            trial = study.ask(self.distributions)
            # Serialize the FrozenTrial object into a JSON-friendly dictionary
            result_data = {
                "trial_number": trial.number,
                "params": trial.params,
                "distributions": {k: str(v) for k, v in trial.distributions.items()},
                "user_attrs": trial.user_attrs,
                "system_attrs": trial.system_attrs,
                "datetime_start": (
                    trial.datetime_start.isoformat() if trial.datetime_start else None
                ),
            }
            self.result_container.complete(result=result_data)
        except Exception as e:
            self.result_container.complete(error=e)


class Tell:
    """An operation to report the result of a trial (study.tell)."""

    def __init__(self, study_name: str, tell_data: Dict, result_container: OpResult):
        self.study_name = study_name
        self.tell_data = tell_data
        self.result_container = result_container

    def run(self, study: optuna.Study, storage: optuna.storages.BaseStorage):
        """Executes the tell operation and places the result in the container."""
        try:
            trial_number = self.tell_data.get("trial_number") or -1
            status = self.tell_data.get("status")
            user_data = self.tell_data.get("user_data") or {}

            LOGGER.debug(
                f"Starting tell for {self.study_name} trial {trial_number}: {user_data}"
            )

            # --- Extract data for trial_additional_data table ---
            _process_id = user_data.pop("process_id", None)
            _iterations = user_data.pop("iterations", None)
            _runner_id = user_data.pop("runner_id", None)
            _ask_reply_time_ms = user_data.pop("ask_reply_time_ms", None)
            _user_data_json = json.dumps(
                user_data
            )  # Remaining user_data as JSON string

            # Ensure storage is RDBStorage to access _engine and other methods
            if not isinstance(storage, RDBStorage):
                error_msg = "Additional trial data can only be stored with RDBStorage."
                LOGGER.error(error_msg)
                self.result_container.complete(error=ValueError(error_msg))
                return

            # Get trial_id from trial_number
            try:
                # Use main.get_study to ensure study._study_id is correctly set (might not be if only `study` is passed)
                # Or, if study is guaranteed to be a RDBStudy, then study._study_id is directly available.
                # Assuming study._study_id is available here from the passed study object.
                trial_id = storage.get_trial_id_from_study_id_trial_number(
                    study._study_id, trial_number
                )
                if trial_id is None:
                    error_msg = f"Trial {trial_number} not found in study {self.study_name} for additional data storage."
                    LOGGER.error(error_msg)
                    self.result_container.complete(error=ValueError(error_msg))
                    return
            except Exception as e:
                LOGGER.error(f"Failed to get trial_id for trial {trial_number}: {e}")
                self.result_container.complete(error=e)
                return

            # --- Insert into trial_additional_data table ---
            with storage.engine.connect() as connection:
                insert_stmt = trial_additional_data_table.insert().values(
                    trial_id=trial_id,
                    process_id=_process_id,
                    iterations=_iterations,
                    runner_id=_runner_id,
                    ask_reply_time_ms=_ask_reply_time_ms,
                    user_data_json=_user_data_json,
                )
                connection.execute(insert_stmt)
                connection.commit()  # Commit the insert
                LOGGER.debug(
                    f"Inserted additional data for trial {trial_id} into trial_additional_data."
                )

            # --- Original study.tell logic ---
            if status == "succeed":
                study.tell(trial_number, values=self.tell_data.get("result"))
            else:
                study.tell(trial_number, state=optuna.trial.TrialState.FAIL)
            LOGGER.debug(f"study.tell completed for trial {trial_number}.")

        except Exception as e:
            self.result_container.complete(error=e)
            raise
