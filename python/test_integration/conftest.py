import os
import tempfile
import time
import pytest
import requests
import socket
from contextlib import closing
import threading
import logging
import queue

from mon2y_dist.main import app as dist_app
import mon2y_dist.main
from mon2y_dist.op_queue import op_queue


@pytest.fixture(scope="function")
def tmp_db_url():
    """Creates a temporary sqlite db file for a test and yields the URL."""
    with tempfile.NamedTemporaryFile(suffix=".sqlite3", delete=False) as tmp:
        db_path = tmp.name

    db_url = f"sqlite:///{db_path}"
    yield db_url

    # Cleanup the file after the test
    if os.path.exists(db_path):
        os.remove(db_path)


import queue
from mon2y_dist.op_queue import op_queue


@pytest.fixture(scope="function")
def dist_server(tmp_db_url, monkeypatch):
    """Starts the mon2y_dist server in a thread for a test."""

    # Clear the queue before starting to prevent state leakage between tests
    while not op_queue.empty():
        try:
            op_queue.get_nowait()
        except queue.Empty:
            break

    # Find a free port
    with closing(socket.socket(socket.AF_INET, socket.SOCK_STREAM)) as s:
        s.bind(("", 0))
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        port = s.getsockname()[1]

    server_url = f"http://localhost:{port}"

    # Reset global singletons in mon2y_dist.main to ensure fresh state for each test
    monkeypatch.setattr(mon2y_dist.main, "_storage", None)
    monkeypatch.setattr(mon2y_dist.main, "_ensure_tables_called", False)
    mon2y_dist.main.get_study.cache_clear()
    # Set app config for this test's database
    monkeypatch.setitem(mon2y_dist.main.app.config, "STORAGE_URL", tmp_db_url)

    def run_app():
        # Use a real WSGI server for better control if needed, but for now, this is fine
        dist_app.run(host="0.0.0.0", port=port, debug=False)

    logging.info("Starting server thread")
    logging.info("Server storage: {}", tmp_db_url)
    thread = threading.Thread(target=run_app, daemon=True)
    thread.start()

    # Wait for the server to become responsive
    for _ in range(20):  # Wait up to 10 seconds
        try:
            response = requests.get(f"{server_url}/open", timeout=2.0)
            if response.status_code == 200:
                break
        except requests.ConnectionError:
            time.sleep(0.5)
    else:
        pytest.fail("Distribution server failed to start in a thread.")

    yield server_url

    # Teardown is implicit because the thread is a daemon
    # A more robust server would have a shutdown endpoint
    thread.join(timeout=5)
