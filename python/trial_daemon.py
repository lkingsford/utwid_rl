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

try:
    import requests
except ImportError:
    subprocess.check_call([sys.executable, "-m", "pip", "install", "requests"])
    import requests
try:
    import boto3
except ImportError:
    subprocess.check_call([sys.executable, "-m", "pip", "install", "boto3"])
    import boto3


def get_cpu_arch() -> str | None:
    machine = platform.machine()
    if machine in ("x86_64", "i686"):
        return "x86"
    elif "aarch64" in machine or "arm" in machine:
        return "arm"
    else:
        return None


def download_from_s3(s3_path: str, target_dir: str) -> str | None:
    s3 = boto3.client("s3")
    bucket, key = s3_path.replace("s3://", "").split("/", 1)
    filename = os.path.basename(key)
    target_path = os.path.join(target_dir, filename)

    if os.path.exists(target_path):
        logging.info(f"Wheel {filename} already exists in {target_dir}.")
        return target_path

    try:
        logging.info(f"Downloading {s3_path} to {target_path}")
        s3.download_file(bucket, key, target_path)
        return target_path
    except Exception as e:
        logging.exception(f"Failed to download {s3_path}: {e}")
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
        "--player-count",
        type=int,
        default=3,
        help="Number of players in the game.",
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
        format="%(asctime)s %(levelname)s %(message)s",
        datefmt="%Y-%m-%d %H:%M:%S",
        level=log_level,
    )

    POLL_INTERVAL = 30
    WHEELS_DIR = "wheels"
    os.makedirs(WHEELS_DIR, exist_ok=True)

    DIST_SERVER = os.environ.get("DIST_SERVER", "http://localhost:5000")

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
                continue

            wheel_attr = f"{cpu_arch}_manylinux_wheel_s3"
            if wheel_attr not in study["user_attrs"]:
                logging.debug(f"No compatible wheel found for study '{study_name}' on {cpu_arch} architecture.")
                continue

            logging.info(f"Found new open study '{study_name}' with compatible wheel.")
            
            wheel_s3_path = study["user_attrs"][wheel_attr]
            wheel_path = download_from_s3(wheel_s3_path, WHEELS_DIR)
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
            
            # Use this file as the entry point for the child process.
            
            process = subprocess.Popen(
                [
                    python_executable,
                    "-c",
                    f"import mon2y.ebr_opt; mon2y.ebr_opt.trial_worker(comm_socket_fd={child_sock.fileno()}, study_name='{study_name}', player_count={args.player_count}, threads={args.threads}, force_iterations={args.force_iterations})",
                ],
                pass_fds=[child_sock.fileno()],
            )

            running_studies[study_name] = {"process": process, "socket": parent_sock}
            child_sock.close() 
            
            parent_sock.send(b"run")

        time.sleep(POLL_INTERVAL)
