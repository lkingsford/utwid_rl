import logging
import os
import subprocess
import sys
import tempfile
import venv
from typing import List, Optional
from urllib.parse import urlparse

try:
    import requests
except ImportError:
    subprocess.check_call([sys.executable, "-m", "pip", "install", "requests"])
    import requests

from .runner import RunnerDetails, TrialRunner, TrialRunnerState


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
