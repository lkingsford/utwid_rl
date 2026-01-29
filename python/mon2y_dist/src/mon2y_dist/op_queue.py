import logging
import os
import queue
from dataclasses import dataclass, field
from typing import Any, Dict, Optional, List

import optuna

LOGGER = logging.getLogger()

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
            trial_number = self.tell_data.get("trial_number")
            status = self.tell_data.get("status")
            user_data = self.tell_data.get("user_data")

            trial_id = storage.get_trial_id_from_study_id_trial_number(
                study._study_id, trial_number
            )

            if user_data:
                for key, value in user_data.items():
                    storage.set_trial_user_attr(trial_id, key, value)

            LOGGER.debug(
                f"Starting tell for {self.study_name} trial {trial_number}: {user_data}"
            )

            if status == "succeed":
                result = self.tell_data.get("result")
                if result is None:
                    raise ValueError("'result' is required for status 'succeed'")
                study.tell(trial_number, values=result)
                self.result_container.complete(result={"status": "ok"})

            elif status == "fail":
                study.tell(trial_number, state=optuna.trial.TrialState.FAIL)
                self.result_container.complete(result={"status": "ok"})

            else:
                raise ValueError("status must be 'succeed' or 'fail'")
                LOGGER.info(
                    f"Tell for {self.study_name} trial {trial_number} completed"
                )

        except Exception as e:
            self.result_container.complete(error=e)

