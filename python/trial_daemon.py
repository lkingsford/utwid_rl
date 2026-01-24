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
from typing import Any, Dict
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


def download_from_url(url: str, filename: str, target_dir: str) -> str | None:
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


if __name__ == "__main__":
    # Experimentation showed more than 12 threads has minimum benefit (probably) due to locking
    MAX_THREADS = 4
    
    parser = argparse.ArgumentParser(description="EBR hyperparameter optimization daemon.")
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

    POLL_INTERVAL = 30
    WHEELS_DIR = "wheels"
    os.makedirs(WHEELS_DIR, exist_ok=True)

    DIST_SERVER = os.environ.get("DIST_SERVER", "http://127.0.0.1:5000")

    cpu_arch = get_cpu_arch()
    if not cpu_arch:
        logging.error(f"Unsupported CPU architecture: {platform.machine()}")
        sys.exit(1)
    logging.info(f"Detected CPU architecture: {cpu_arch}")

    running_studies: Dict[str, Dict[str, Any]] = {}

    while True:
        logging.info("Polling for open studies...")
        try:
            response = requests.get(f"{DIST_SERVER}/open", timeout=10)
            response.raise_for_status()
            open_studies = response.json()
        except requests.RequestException as e:
            logging.error(f"Failed to get open studies: {e}")
            time.sleep(POLL_INTERVAL)
            continue

        open_study_names = {s["study_name"] for s in open_studies}

        # Find and handle stopped studies
        stopped_study_names = set(running_studies.keys()) - open_study_names
        for study_name in stopped_study_names:
            logging.info(f"Study '{study_name}' is no longer open. Shutting down worker.")
            worker_info = running_studies.pop(study_name)
            worker_info["socket"].send(b"done")
            worker_info["socket"].close()
            worker_info["process"].terminate()

        # Find and handle new studies
        for study in open_studies:
            study_name = study["study_name"]
            if study_name in running_studies:
                logging.debug(f"Study '{study_name}' is already running. Skipping.")
                continue

            wheel_url_attr = f"{cpu_arch}_manylinux_wheel_url"

            user_attrs = study["user_attrs"]
            if wheel_url_attr not in user_attrs:
                logging.debug(
                    f"No compatible wheel found for study '{study_name}' on {cpu_arch} architecture."
                )
                continue

            logging.info(f"Found new open study '{study_name}' with compatible wheel.")

            wheel_url = user_attrs[wheel_url_attr]

            try:
                parsed_url = urlparse(wheel_url)
                s3_key = parsed_url.path.lstrip("/")
                filename = os.path.basename(s3_key)
            except Exception as e:
                logging.error(f"Could not parse wheel filename from URL {wheel_url}: {e}")
                continue

            wheel_path = download_from_url(wheel_url, filename, WHEELS_DIR)
            if not wheel_path:
                continue

            venv_dir = tempfile.mkdtemp()
            logging.info(f"Creating virtual environment in {venv_dir}")
            venv.create(venv_dir, with_pip=True)
            
            pip_executable = os.path.join(venv_dir, "bin", "pip")
            logging.info(f"Installing wheel {wheel_path} into virtual environment.")
            subprocess.check_call([pip_executable, "install", wheel_path])
            
            parent_sock, child_sock = socket.socketpair()

            python_executable = os.path.join(venv_dir, "bin", "python")

            user_attrs = study["user_attrs"]
            module = user_attrs.get("module")
            function = user_attrs.get("function")

            if not module or not function:
                logging.error(
                    f"Study '{study_name}' is missing 'module' or 'function' in user_attrs."
                )
                continue

            params = user_attrs.get("params", {})
            logging.info(f"Using params for study '{study_name}': {params}")

            # Use this file as the entry point for the child process.

            log_level_str = logging.getLevelName(log_level)
            force_iterations_arg = (
                f", force_iterations={args.force_iterations}"
                if args.force_iterations is not None
                else ""
            )
            command = (
                f"import logging; logging.basicConfig("
                f"format='%(asctime)s %(levelname)s %(process)d %(message)s', "
                f"datefmt='%Y-%m-%d %H:%M:%S', level='{log_level_str}'); "
                f"import {module}; {module}.{function}("
                f"comm_socket_fd={child_sock.fileno()}, "
                f"study_name='{study_name}', "
                f"threads={args.threads}"
                f"{force_iterations_arg}, "
                f"params={params})"
            )
            logging.debug(f"Executing command for study '{study_name}': {command}")

            process = subprocess.Popen(
                [
                    python_executable,
                    "-c",
                    command,
                ],
                pass_fds=[child_sock.fileno()],
            )

            running_studies[study_name] = {"process": process, "socket": parent_sock}
            child_sock.close() 
            
            parent_sock.send(b"run")

        time.sleep(POLL_INTERVAL)
