import logging
import math
import os
import socket
import time
from typing import Optional, NamedTuple, List, Any

import requests

# Self-contained distribution definitions to avoid importing from the main package
class IntDistribution(NamedTuple):
    low: int
    high: int
    step: int = 1
    log: bool = False

logging.basicConfig(
    level=logging.DEBUG,
    format="%(asctime)s %(levelname)s %(filename)s:%(lineno)d %(message)s",
)

class FloatDistribution(NamedTuple):
    low: float
    high: float
    step: Optional[float] = None
    log: bool = False

class CategoricalDistribution(NamedTuple):
    choices: List[Any]

MSG_DONE = b"done"
MSG_TOLD = b"told"


def get_distributions():
    return {
        "int1": IntDistribution(low=1, high=10),
        "int2": IntDistribution(low=1, high=10),
        "float1": FloatDistribution(low=0.0, high=1.0),
        "float2": FloatDistribution(low=0.0, high=1.0),
        "cat1": CategoricalDistribution(choices=["a", "b", "c"]),
        "cat2": CategoricalDistribution(choices=[1, 2, 3]),
    }


def mock_trial_worker(
    comm_socket_fd: int,
    study_name: str,
    threads: int,
    force_iterations: int | None,
    params: dict,
    runner_id: Optional[str] = None,
    tell_socket_fd: Optional[int] = None,
):
    """A mock worker for integration tests."""
    process_id = os.getpid()
    logging.info(f"Mock worker started (PID: {process_id}) for study '{study_name}'")

    comm_socket = socket.fromfd(comm_socket_fd, socket.AF_UNIX, socket.SOCK_STREAM)
    comm_socket.setblocking(False)

    tell_socket = None
    if tell_socket_fd is not None:
        tell_socket = socket.fromfd(tell_socket_fd, socket.AF_UNIX, socket.SOCK_STREAM)

    dist_uri = os.environ.get("DIST_URI", "http://localhost:5000")
    distributions = get_distributions()

    stop_after = params.get("stop_after")
    ask_count = 0

    try:
        while True:
            # Check for shutdown message from daemon
            try:
                msg = comm_socket.recv(1024)
                if msg == MSG_DONE:
                    logging.info(f"Mock worker for study '{study_name}' received 'done'.")
                    break
            except BlockingIOError:
                pass  # No message, continue
            except Exception as e:
                logging.error(f"Error on comm_socket: {e}")
                break

            # If stop_after is set and we've reached the limit, go idle
            if stop_after is not None and ask_count >= stop_after:
                logging.info(f"Mock worker for study '{study_name}' reached stop_after={stop_after}. Going idle.")
                while True:
                    # Keep checking for shutdown message
                    try:
                        msg = comm_socket.recv(1024)
                        if msg == MSG_DONE:
                            logging.info(f"Mock worker for study '{study_name}' received 'done' while idle.")
                            break
                    except BlockingIOError:
                        pass
                    except Exception as e:
                        logging.error(f"Error on comm_socket while idle: {e}")
                        break
                    time.sleep(1) # Simulate idle work
                break # Exit the main loop after breaking from inner idle loop

            # Ask for a trial
            ask_payload = {
                "study_name": study_name,
                "distributions": {
                    k: {"name": v.__class__.__name__, "attributes": v._asdict()}
                    for k, v in distributions.items()
                },
            }
            try:
                logging.info(f"Mock worker asking for trial for study '{study_name}'")
                response = requests.post(f"{dist_uri}/ask", json=ask_payload, timeout=5)
                response.raise_for_status()
                ask_data = response.json()
                trial_number = ask_data["trial_number"]
                trial_params = ask_data["params"]
                ask_count += 1
                logging.debug(f"Mock worker ask_count: {ask_count}")
            except Exception as e:
                logging.error(f"Mock worker failed to /ask: {e}")
                time.sleep(1)
                continue

            # Calculate reward
            val = abs(
                math.sqrt(
                    trial_params["int1"] ** 2
                    + trial_params["int2"] ** 2
                    + trial_params["float1"] ** 2
                    + trial_params["float2"] ** 2
                )
                - 25
            )
            result = [val, val, val]

            # Tell the result
            tell_payload = {
                "study_name": study_name,
                "trial_number": trial_number,
                "status": "succeed",
                "result": result,
                "user_data": {"process_id": process_id}
            }
            try:
                logging.info(f"Mock worker telling result for trial {trial_number}")
                response = requests.post(f"{dist_uri}/tell", json=tell_payload, timeout=5)
                response.raise_for_status()
                if tell_socket:
                    tell_socket.send(MSG_TOLD)
            except Exception as e:
                logging.error(f"Mock worker failed to /tell: {e}")
                continue # Try again next loop
    
    finally:
        logging.info(f"Mock worker for study '{study_name}' shutting down.")
        comm_socket.close()
        if tell_socket:
            tell_socket.close()

