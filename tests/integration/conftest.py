import os
import tempfile
import time
import pytest
import requests
import socket
from contextlib import closing
import threading
from mon2y_dist.main import app as dist_app

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

@pytest.fixture(scope="function")
def dist_server(tmp_db_url):
    """Starts the mon2y_dist server in a thread for a test."""
    
    # Find a free port
    with closing(socket.socket(socket.AF_INET, socket.SOCK_STREAM)) as s:
        s.bind(('', 0))
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        port = s.getsockname()[1]
    
    server_url = f"http://localhost:{port}"
    
    # Set env var for the app factory to use
    os.environ["OPTUNA_STORAGE"] = tmp_db_url

    def run_app():
        # Use a real WSGI server for better control if needed, but for now, this is fine
        dist_app.run(host="0.0.0.0", port=port, debug=False)

    thread = threading.Thread(target=run_app, daemon=True)
    thread.start()

    # Wait for the server to become responsive
    for _ in range(20):  # Wait up to 10 seconds
        try:
            response = requests.get(f"{server_url}/open", timeout=0.5)
            if response.status_code == 200:
                break
        except requests.ConnectionError:
            time.sleep(0.5)
    else:
        pytest.fail("Distribution server failed to start in a thread.")
        
    yield server_url
    
    # Teardown is implicit because the thread is a daemon
    # A more robust server would have a shutdown endpoint

