import uuid
import argparse
import logging
import os
import platform
import socket
import subprocess
import sys
import tempfile
import time
import venv
from typing import Any, Dict, Optional, NamedTuple, List, Set
from enum import Enum
from urllib.parse import urlparse

try:
    import requests
except ImportError:
    subprocess.check_call([sys.executable, "-m", "pip", "install", "requests"])
    import requests


def get_cpu_arch() -> str | None:
    machine = platform.machine()
    if machine in ("x86_64", "i686"):
        return "x86"
    elif "aarch64" in machine or "arm" in machine:
        return "arm"
    else:
        return None


def download_from_url(url: str, target_dir: str) -> str | None:
    try:
        parsed_url = urlparse(url)
        s3_key = parsed_url.path.lstrip("/")
        filename = os.path.basename(s3_key)
    except Exception as e:
        raise ValueError(f"Could not parse wheel filename from URL {url}: {e}")

    target_path = os.path.join(target_dir, filename)

    if os.path.exists(target_path):
        logging.info(f"Wheel {filename} already exists in {target_dir}.")
        return target_path

    try:
        logging.info(f"Downloading {filename} to {target_path}")
        with requests.get(url, stream=True) as r:
            r.raise_for_status()
            with open(target_path, "wb") as f:
                for chunk in r.iter_content(chunk_size=8192):
                    f.write(chunk)
        return target_path
    except requests.RequestException as e:
        logging.exception(f"Failed to download from {url}: {e}")
        return None


MSG_DONE = b"done"


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

    def __del__(self):
        self._parent_sock.close()
        self._child_sock.close()

    def __init__(
        self,
        python_executable: str,
        runner_details: RunnerDetails,
        log_level: int,
    ):
        self._parent_sock, self._child_sock = socket.socketpair()
        self._stop_sent: bool = False
        self._started: bool = False

        log_level_str = logging.getLevelName(log_level)
        command = (
            f"import logging; logging.basicConfig("
            f"format='%(asctime)s %(levelname)s %(process)d %(message)s', "
            f"datefmt='%Y-%m-%d %H:%M:%S', level='{log_level_str}'); "
            f"import {runner_details.module}; {runner_details.module}.{runner_details.function}("
            f"comm_socket_fd={self._child_sock.fileno()}, "
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
            pass_fds=[self._child_sock.fileno()],
        )
        self._started = True


class StudyError(RuntimeError):
    pass


class NoWheelException(StudyError):
    pass


class Study:
    def __init__(
        self,
        wheel_uri: Optional[str],
        runner_details: RunnerDetails,
        use_current_env: bool = False,
        log_level: int = logging.INFO,
    ):
        self.wheel_uri = wheel_uri
        self.use_current_env = use_current_env
        self._executable: Optional[str] = None
        self.runner_details = runner_details
        self.log_level = log_level
        if not wheel_uri and not use_current_env:
            raise ValueError(
                "wheel_uri must be provided when not using current [virtual] environment"
            )
        self._runners: List[TrialRunner] = []

    def executable(self, wheels_dir: str) -> str:
        """Gets the python interpreter to use, creating a new venv if needed"""
        if self._executable:
            return self._executable

        if self.use_current_env:
            logging.info("Using current virtual environment.")
            self._executable = sys.executable

        else:
            wheel_path = download_from_url(self.wheel_uri, wheels_dir)
            if not wheel_path:
                raise NoWheelException(
                    f"Failed to download wheel from {self.wheel_uri}"
                )
            venv_dir = tempfile.mkdtemp()
            logging.info(f"Creating virtual environment in {venv_dir}")
            venv.create(venv_dir, with_pip=True)

            pip_executable = os.path.join(venv_dir, "bin", "pip")
            logging.info(f"Installing wheel {wheel_path} into virtual environment.")
            subprocess.check_call([pip_executable, "install", wheel_path])

            self._executable = os.path.join(venv_dir, "bin", "python")

        return self._executable

    def current_running(self):
        return [
            runner
            for runner in self._runners
            if runner.status() in [TrialRunnerState.STARTING, TrialRunnerState.RUNNING]
        ]

    def scale(self, desired_processes: int, wheels_dir: str):
        """Scale until at correct number of processes.

        Shutting down instances are not included in the count.
        """
        current_running = self.current_running()

        if len(current_running) == desired_processes:
            return

        elif len(current_running) > desired_processes:
            to_remove = len(current_running) - desired_processes
            logging.info(
                f"{self.runner_details.study_name} - Shutting down {to_remove} runners"
            )
            for runner in current_running[0:to_remove]:
                runner.shutdown()

        else:
            to_add = desired_processes - len(current_running)
            logging.info(
                f"{self.runner_details.study_name} - Scaling up {to_add} runners"
            )
            for _ in range(to_add):
                self._runners.append(
                    TrialRunner(
                        self.executable(wheels_dir), self.runner_details, self.log_level
                    )
                )

    def cleanup(self):
        """Manually clean up runners to free the sockets"""

        pre_cleanup_len = len(self._runners)
        self._runners = [
            runner
            for runner in self._runners
            if runner.status() != TrialRunnerState.STOPPED
        ]

        cleaned_up_runners = pre_cleanup_len - len(self._runners)
        if cleaned_up_runners > 0:
            logging.info(
                f"{self.runner_details.study_name} - Cleaning up {cleaned_up_runners} runners"
            )


def main():
    # Experimentation showed more than 12 threads has minimum benefit (probably) due to locking
    MAX_THREADS = 4

    parser = argparse.ArgumentParser(
        description="EBR hyperparameter optimization daemon."
    )
    parser.add_argument(
        "-v",
        "--verbose",
        action="count",
        default=0,
        help="Increase verbosity level: -v for INFO, -vv for DEBUG.",
    )
    parser.add_argument(
        "--threads",
        type=int,
        default=MAX_THREADS,
        help="Number of threads per process.",
    )
    parser.add_argument(
        "--force-iterations",
        type=int,
        help="Force the number of iterations for each trial. Useful for debugging.",
    )
    parser.add_argument(
        "--current_venv",
        action="store_true",
        help="If set, do not create a new virtual environment, use the current one.",
    )

    # Determine default number of processes
    default_processes = os.cpu_count() if os.cpu_count() is not None else 4

    parser.add_argument(
        "--processes",
        type=int,
        default=default_processes,
        help="Trial runner processes",
    )
    args = parser.parse_args()

    if args.verbose == 0:
        log_level = logging.WARNING
    elif args.verbose == 1:
        log_level = logging.INFO
    else:
        log_level = logging.DEBUG

    logging.basicConfig(
        format="%(asctime)s %(levelname)s %(process)d %(message)s",
        datefmt="%Y-%m-%d %H:%M:%S",
        level=log_level,
    )

    logging.info(f"Starting trial daemon with PID {os.getpid()}")

    runner_id = str(uuid.uuid4())[:8]

    POLL_INTERVAL = 30
    WHEELS_DIR = "wheels"
    os.makedirs(WHEELS_DIR, exist_ok=True)

    cpu_arch = get_cpu_arch()
    if not cpu_arch:
        logging.error(f"Unsupported CPU architecture: {platform.machine()}")
        sys.exit(1)
    logging.info(f"Detected CPU architecture: {cpu_arch}")
    wheel_url_attr = f"{cpu_arch}_manylinux_wheel_url"

    studies: Dict[str, Study] = {}
    noted_as_incompatible: Set[str] = set()

    dist_uri = os.environ.get("DIST_URI", "http://localhost:5000")
    while True:
        logging.info("Polling for open studies...")
        try:
            response = requests.get(f"{dist_uri}/open", timeout=10)
            response.raise_for_status()
            open_studies = response.json()
        except requests.RequestException as e:
            logging.error(f"Failed to get open studies: {e}")
            time.sleep(POLL_INTERVAL)
            continue

        open_studies_by_name = {s["study_name"]: s for s in open_studies}
        open_study_names = set(open_studies_by_name)

        for study_name in open_study_names:
            if study_name not in studies:
                study_info = open_studies_by_name[study_name]
                user_attrs = study_info["user_attrs"]

                if wheel_url_attr not in user_attrs and not args.current_venv:
                    if study_name not in noted_as_incompatible:
                        logging.debug(
                            f"No compatible wheel found for study '{study_name}' on {cpu_arch} architecture."
                        )
                        noted_as_incompatible.add(study_name)
                    continue

                wheel_url = user_attrs.get(wheel_url_attr)
                if wheel_url:
                    logging.info(
                        f"Found new open study '{study_name}' with compatible wheel."
                    )
                else:
                    logging.info(f"Found new open study '{study_name}'.")

                runner_details = RunnerDetails(
                    study_name=study_name,
                    module=user_attrs.get("module"),
                    function=user_attrs.get("function"),
                    threads=args.threads,
                    force_iterations=args.force_iterations,
                    params=user_attrs.get("params", {}),
                    runner_id=runner_id,
                )

                try:
                    dist_uri = os.environ.get("DIST_URI", "http://localhost:5000")

                    studies[study_name] = Study(
                        wheel_uri=wheel_url,
                        runner_details=runner_details,
                        use_current_env=args.current_venv,
                        log_level=log_level,
                    )
                except (ValueError, StudyError) as e:
                    logging.exception(e)
                    logging.info(f"Continuing. Study {study_name} not created.")

        for study in studies.values():
            study.cleanup()

        logging.info(
            f"Active runner #: { {study_name: len(study.current_running())  for study_name, study in studies.items() if study_name in open_study_names} }"
        )

        running_study_names = {
            study_name
            for study_name, study in studies.items()
            if len(study.current_running()) > 0
        }

        studies_to_stop = running_study_names - open_study_names
        for study_name in studies_to_stop:
            if study_name in studies:
                studies[study_name].scale(0, WHEELS_DIR)

        active_available_studies = set(studies.keys()) & open_study_names

        if active_available_studies:
            scale_to_set = args.processes // len(active_available_studies)
        else:
            scale_to_set = 0

        for study_name in active_available_studies:
            studies[study_name].scale(scale_to_set, WHEELS_DIR)

        time.sleep(POLL_INTERVAL)


if __name__ == "__main__":
    main()
