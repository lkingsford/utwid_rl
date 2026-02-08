import time
import pytest
import requests
import optuna
import threading
import logging
from argparse import Namespace
import os

# Configure logging to include filename
logging.basicConfig(
    level=logging.DEBUG,
    format="%(asctime)s %(levelname)s %(filename)s:%(lineno)d %(message)s",
)

from mon2y_trial_daemon.daemon import TrialDaemon

def test_basic_run_10_tells(dist_server, tmp_db_url):
    """
    Tests the basic operation in-process with threads:
    - A study is created.
    - The trial daemon is started in a thread.
    - The daemon runs workers that complete at least 10 trials.
    - The daemon is stopped cleanly.
    """
    study_name = "test_basic_run_threaded"
    os.environ["DIST_URI"] = dist_server

    # 1. Create the study
    create_payload = {
        "study_name": study_name,
        "direction": "min,min,min",
        "module": "tests.integration.mock_runner",
        "function": "mock_trial_worker",
    }
    response = requests.post(f"{dist_server}/create_study", json=create_payload)
    assert response.status_code == 200, f"Failed to create study: {response.text}"

    # 2. Setup and run the trial daemon in a thread
    args = Namespace(
        processes=2,
        threads=2,
        current_venv=True,
        halt_after_idle_time=0,
        halt_after=0,
        halt_grace=5,
        treat_worker_as_idle_after=0,
        force_iterations=10, # Keep trials fast
    )
    daemon = TrialDaemon(args, logging.DEBUG)
    daemon_thread = threading.Thread(target=daemon.run, daemon=True)
    daemon_thread.start()

    try:
        # 3. Poll the database and wait for 10 trials
        storage = optuna.storages.RDBStorage(url=tmp_db_url)
        completed_trials = []
        for _ in range(45):  # 45-second timeout
            try:
                study_id = storage.get_study_id_from_name(study_name)
                all_trials = storage.get_all_trials(study_id, deepcopy=False)
                completed_trials = [t for t in all_trials if t.state == optuna.trial.TrialState.COMPLETE]
                if len(completed_trials) >= 10:
                    break
            except KeyError:
                pass # Study might not exist yet
            time.sleep(1)
        
        # 4. Final assertion
        assert len(completed_trials) >= 10, f"Expected at least 10 completed trials, but found {len(completed_trials)}"

    finally:
        # 5. Stop the daemon and clean up
        daemon.stop()
        daemon_thread.join(timeout=10)
        if daemon_thread.is_alive():
            pytest.fail("Trial daemon thread did not shut down cleanly.")