import logging
import socket
import subprocess
import time
from enum import Enum
from typing import Any, Dict, NamedTuple, Optional

MSG_DONE = b"done"
MSG_TOLD = b"told"


class RunnerDetails(NamedTuple):
    study_name: str
    module: str
    function: str
    threads: int
    force_iterations: Optional[int]
    params: Dict[str, Any]
    runner_id: Optional[str]


class TrialRunnerState(Enum):
    STARTING = 0
    RUNNING = 1
    SHUTTING_DOWN = 2
    STOPPED = 3


class TrialRunner:
    def status(self) -> TrialRunnerState:
        if not self._started:
            return TrialRunnerState.STARTING
        elif self._process.poll() is not None:
            return TrialRunnerState.STOPPED
        elif self._stop_sent:
            return TrialRunnerState.SHUTTING_DOWN
        return TrialRunnerState.RUNNING

    def shutdown(self):
        # Sends the socket message to finish at a convenient time
        self._parent_sock.send(MSG_DONE)
        self._stop_sent = True

    def kill(self):
        self._process.kill()

    def check_for_tell(self):
        """Checks for messages on the tell-reporting socket and updates the last_tell_time."""
        try:
            # Read all available data from the socket to clear the buffer
            while self.tell_report_sock.recv(1024):
                self.last_tell_time = time.time()
        except BlockingIOError:
            # No data to read
            pass
        except Exception:
            logging.exception("Error checking for tell from runner.")

    def is_active(self, idle_threshold_seconds: int) -> bool:
        """Determines if the runner is active based on the last tell time."""
        if idle_threshold_seconds <= 0:
            return True  # Feature is disabled, so always consider active
        return (time.time() - self.last_tell_time) < idle_threshold_seconds

    def __del__(self):
        self._parent_sock.close()
        self._child_sock.close()
        self.tell_report_sock.close()
        self.child_tell_report_sock.close()

    def __init__(
        self,
        python_executable: str,
        runner_details: RunnerDetails,
        log_level: int,
    ):
        self._parent_sock, self._child_sock = socket.socketpair()
        self.tell_report_sock, self.child_tell_report_sock = socket.socketpair()
        self.tell_report_sock.setblocking(False)

        self._stop_sent: bool = False
        self._started: bool = False
        self.last_tell_time = time.time()

        log_level_str = logging.getLevelName(log_level)
        command = (
            f"import logging; logging.basicConfig("
            f"format='%(asctime)s %(levelname)s %(process)d %(message)s', "
            f"datefmt='%Y-%m-%d %H:%M:%S', level='{log_level_str}'); "
            f"import {runner_details.module}; {runner_details.module}.{runner_details.function}("
            f"comm_socket_fd={self._child_sock.fileno()}, "
            f"tell_socket_fd={self.child_tell_report_sock.fileno()}, "
            f"study_name='{runner_details.study_name}', "
            f"threads={runner_details.threads},"
            f"force_iterations={runner_details.force_iterations}, "
            f"params={runner_details.params},"
            f"runner_id='{runner_details.runner_id}')"
        )
        logging.debug(
            f"Executing command for study '{runner_details.study_name}': {command}"
        )
        self._process = subprocess.Popen(
            [
                python_executable,
                "-c",
                command,
            ],
            pass_fds=[self._child_sock.fileno(), self.child_tell_report_sock.fileno()],
        )
        self._started = True
