import time
import pytest
import requests
import optuna
from optuna.trial import Trial, TrialState
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


def _setup_daemon_test_env(
    dist_server,
    study_name_prefix,
    monkeypatch, # Added monkeypatch fixture
    daemon_args_override=None,
    create_payload_extra_params=None,
):
    study_name = f"{study_name_prefix}_{int(time.time())}"

    # 1. Create the study
    create_payload = {
        "study_name": study_name,
        "direction": "min,min,min",
        "module": "mock_runner",
        "function": "mock_trial_worker",
    }
    if create_payload_extra_params:
        create_payload.update(create_payload_extra_params)

    response = requests.post(f"{dist_server}/create_study", json=create_payload)
    assert response.status_code == 200, f"Failed to create study: {response.text}"

    # 2. Setup and run the trial daemon in a thread
    base_args = Namespace(
        processes=2,
        threads=2,
        current_venv=True,
        halt_after_idle_time=0,
        halt_after=0,
        halt_grace=5,
        halt_instance_after_stopping=False,
        treat_worker_as_idle_after=0,
        force_iterations=10,  # Keep trials fast
        poll_interval=1,
    )
    if daemon_args_override:
        for key, value in daemon_args_override.items():
            setattr(base_args, key, value)

    # Ensure the test_integration directory is on the PYTHONPATH for daemon workers
    # This allows the mock_runner module to be found by subprocesses
    test_integration_path = os.path.abspath(os.path.dirname(__file__))
    current_pythonpath = os.environ.get("PYTHONPATH", "")
    if test_integration_path not in current_pythonpath:
        if current_pythonpath:
            monkeypatch.setenv("PYTHONPATH", f"{test_integration_path}:{current_pythonpath}")
        else:
            monkeypatch.setenv("PYTHONPATH", test_integration_path)

    daemon = TrialDaemon(base_args, logging.DEBUG, dist_server)
    daemon_thread = threading.Thread(target=daemon.run, daemon=True)
    return daemon, daemon_thread, study_name


def _teardown_daemon_test_env(daemon, daemon_thread):
    daemon.force_shutdown()
    if daemon_thread.is_alive():
        daemon_thread.join(timeout=30)
        if daemon_thread.is_alive():
            logging.critical(
                "Daemon thread is still alive after teardown timeout. This indicates a shutdown issue."
            )


def test_halt_after_mechanism(dist_server, tmp_db_url, monkeypatch):
    """
    Tests that the daemon halts cleanly after 'halt_after' minutes.
    """
    halt_duration = 20 / 60
    wait_buffer = 5
    daemon, daemon_thread, study_name = _setup_daemon_test_env(
        dist_server,
        "test_halt_after",
        monkeypatch,
        daemon_args_override={"halt_after": halt_duration, "poll_interval": 1},
    )
    daemon_thread.start()
    start_time = time.time()
    # Wait for halt_duration + buffer to ensure it has time to shut down
    test_wait_time = (
        (halt_duration * 60) + daemon.args.halt_grace + 5
    )  # Add an extra 5 second buffer for robustness
    time.sleep(test_wait_time)

    end_time = time.time()
    elapsed_time = end_time - start_time
    logging.info(f"Test finished waiting. Elapsed time: {elapsed_time:.2f}s")

    try:
        # Check that the daemon thread is no longer alive, meaning it halted
        assert not daemon_thread.is_alive(), (
            f"Daemon thread is still alive after {elapsed_time:.2f}s, "
            f"expected halt after {halt_duration*60}s."
        )
        # Check that it halted approximately at the expected time
        assert elapsed_time >= (halt_duration * 60), "Daemon halted too early."
        assert (
            elapsed_time < (halt_duration * 60) + daemon.args.halt_grace + 10
        ), f"Daemon took too long to halt. Expected <{(halt_duration * 60) + daemon.args.halt_grace + 10}s, got {elapsed_time:.2f}s."
        logging.info(f"Daemon halted successfully after {elapsed_time:.2f}s.")

    finally:
        _teardown_daemon_test_env(daemon, daemon_thread)


def test_treat_worker_as_idle_after_halts_daemon(dist_server, tmp_db_url, monkeypatch):
    """
    Tests that the daemon halts when workers become idle and
    treat_worker_as_idle_after is set.
    """
    idle_after_minutes = 5 / 60
    halt_after_minutes = 30 / 60
    stop_after_asks = 3  # Mock worker will stop asking after 3 asks

    daemon, daemon_thread, study_name = _setup_daemon_test_env(
        dist_server,
        "test_idle_after",
        monkeypatch,
        daemon_args_override={
            "treat_worker_as_idle_after": idle_after_minutes,
            "halt_after_idle_time": idle_after_minutes,
            "halt_after": halt_after_minutes,
            "poll_interval": 1,  # Ensure frequent polling
        },
        create_payload_extra_params={"params": {"stop_after": stop_after_asks}},
    )

    daemon_thread.start()

    start_time = time.time()

    try:
        # Expected timing:
        # - workers become idle after treat_worker_as_idle_after
        # - daemon waits halt_after_idle_time
        # - next poll triggers halt
        expected_min_minutes = idle_after_minutes
        expected_max_minutes = (
            idle_after_minutes
            + daemon.args.halt_after_idle_time
            + (daemon.args.poll_interval / 60)
            + (daemon.args.halt_grace / 60)
            + 0.2  # scheduling tolerance
        )

        timeout_for_test = expected_max_minutes * 60 + 5  # buffer

        daemon_thread.join(timeout=timeout_for_test)

        end_time = time.time()
        elapsed_time = (end_time - start_time) / 60
        logging.info(f"Test finished waiting. Elapsed time: {elapsed_time:.2f}m")

        # 1. Daemon must have halted
        assert not daemon_thread.is_alive(), (
            f"Daemon thread is still alive after {elapsed_time:.2f}m "
            f"(timeout was {timeout_for_test / 60:.2f}m)."
        )

        # 2. It should halt due to idle workers, within the expected idle window
        assert expected_min_minutes <= elapsed_time <= expected_max_minutes, (
            f"Daemon halted at {elapsed_time:.2f}m; expected between "
            f"{expected_min_minutes:.2f}m and {expected_max_minutes:.2f}m "
            f"due to idle workers."
        )

        # 3. It must halt faster than the global halt_after limit
        assert elapsed_time < halt_after_minutes, (
            f"Daemon halted at {elapsed_time:.2f}m, which is not faster than "
            f"halt_after {halt_after_minutes:.2f}m."
        )

        logging.info(
            f"Daemon halted successfully due to idle workers in {elapsed_time:.2f}m."
        )

    finally:
        _teardown_daemon_test_env(daemon, daemon_thread)


def test_basic_run_10_tells(dist_server, tmp_db_url, monkeypatch):

    """
    Tests the basic operation in-process with threads:

    - A study is created.
    - The trial daemon is started in a thread.
    - The daemon runs workers that complete at least 10 trials.
    - The daemon is stopped cleanly.
    """

    daemon, daemon_thread, study_name = _setup_daemon_test_env(
        dist_server, "test_basic_run_threaded", monkeypatch
    )
    daemon_thread.start()
    print(
        f"Daemon thread for {study_name} started. Is alive: {daemon_thread.is_alive()} #GEMADD"
    )
    time.sleep(0.1)  # Give it a moment to potentially die
    print(
        f"Daemon thread for {study_name} after 0.1s sleep. Is alive: {daemon_thread.is_alive()} #GEMADD"
    )

    try:
        # 3. Poll the database and wait for 10 trials
        storage = optuna.storages.RDBStorage(url=tmp_db_url)
        completed_trials = []
        for _ in range(100):  # 10 seconds total, polling every 0.1s
            try:
                study_id = storage.get_study_id_from_name(study_name)
                all_trials = storage.get_all_trials(study_id, deepcopy=False)
                completed_trials = [
                    trial for trial in all_trials if trial.state == TrialState.COMPLETE
                ]
                if len(completed_trials) >= 10:
                    break
            except KeyError:
                pass  # Study might not exist yet

            time.sleep(0.1)

        # 4. Final assertion

        assert (
            len(completed_trials) >= 10
        ), f"Expected at least 10 completed trials, but found {len(completed_trials)}"

    finally:

        # 5. Stop the daemon and clean up

        _teardown_daemon_test_env(daemon, daemon_thread)
