import logging
import os
import platform
import subprocess
import sys
import time
import uuid
from typing import Dict, Optional, Set

try:
    import boto3
except ImportError:
    subprocess.check_call([sys.executable, "-m", "pip", "install", "boto3"])
    import boto3

try:
    import requests
except ImportError:
    subprocess.check_call([sys.executable, "-m", "pip", "install", "requests"])
    import requests

from .runner import RunnerDetails
from .study import Study, StudyError


def get_cpu_arch() -> str | None:
    machine = platform.machine()
    if machine in ("x86_64", "i686"):
        return "x86"
    elif "aarch64" in machine or "arm" in machine:
        return "arm"
    else:
        return None


def uri_from_ec2() -> Optional[str]:
    ec2 = boto3.client("ec2", region_name="us-east-1")

    response = ec2.describe_instances(
        Filters=[
            {"Name": "tag:Role", "Values": ["dist"]},
            {"Name": "instance-state-name", "Values": ["running"]},
        ]
    )

    private_ips = []

    for reservation in response["Reservations"]:
        for instance in reservation["Instances"]:
            private_ips.append(instance["PrivateIpAddress"])

    if not (private_ips):
        logging.warn("Distribution server not found")
        return None

    return f"http://{private_ips[0]}:5000/"


class TrialDaemon:
    def __init__(self, args, log_level):
        self.args = args
        self.log_level = log_level
        self.runner_id = str(uuid.uuid4())[:8]
        self.studies: Dict[str, Study] = {}
        self.noted_as_incompatible: Set[str] = set()
        self.last_activity_time = time.time()

    def _halt_instance(self):
        """Terminates the EC2 instance this daemon is running on."""
        logging.info("Halting instance due to idle time.")
        try:
            # Get instance ID from metadata
            instance_id_url = "http://169.254.169.254/latest/meta-data/instance-id"
            response = requests.get(instance_id_url, timeout=5)
            response.raise_for_status()
            instance_id = response.text

            # Get region from metadata
            region_url = "http://169.254.169.254/latest/meta-data/placement/region"
            response = requests.get(region_url, timeout=5)
            response.raise_for_status()
            region = response.text

            logging.info(f"Terminating instance {instance_id} in region {region}.")
            ec2 = boto3.client("ec2", region_name=region)
            ec2.terminate_instances(InstanceIds=[instance_id])

        except requests.RequestException as e:
            logging.error(f"Could not get EC2 metadata: {e}")
        except Exception as e:
            logging.error(f"Failed to terminate instance: {e}")

    def run(self):
        logging.info(f"Starting trial daemon with PID {os.getpid()}")

        POLL_INTERVAL = 30
        WHEELS_DIR = "wheels"
        os.makedirs(WHEELS_DIR, exist_ok=True)

        cpu_arch = get_cpu_arch()
        if not cpu_arch:
            logging.error(f"Unsupported CPU architecture: {platform.machine()}")
            sys.exit(1)
        logging.info(f"Detected CPU architecture: {cpu_arch}")
        wheel_url_attr = f"{cpu_arch}_manylinux_wheel_url"

        dist_uri = (
            os.environ.get("DIST_URI") or uri_from_ec2() or "http://localhost:5000"
        )

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
                if study_name not in self.studies:
                    study_info = open_studies_by_name[study_name]
                    user_attrs = study_info["user_attrs"]

                    if wheel_url_attr not in user_attrs and not self.args.current_venv:
                        if study_name not in self.noted_as_incompatible:
                            logging.debug(
                                f"No compatible wheel found for study '{study_name}' on {cpu_arch} architecture."
                            )
                            self.noted_as_incompatible.add(study_name)
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
                        threads=self.args.threads,
                        force_iterations=self.args.force_iterations,
                        params=user_attrs.get("params", {}),
                        runner_id=self.runner_id,
                    )

                    try:
                        self.studies[study_name] = Study(
                            wheel_uri=wheel_url,
                            runner_details=runner_details,
                            use_current_env=self.args.current_venv,
                            log_level=self.log_level,
                        )
                    except (ValueError, StudyError) as e:
                        logging.exception(e)
                        logging.info(f"Continuing. Study {study_name} not created.")

            for study in self.studies.values():
                study.cleanup()

            logging.info(
                f"Active runner #: { {study_name: len(study.current_running())  for study_name, study in self.studies.items() if study_name in open_study_names} }"
            )

            running_study_names = {
                study_name
                for study_name, study in self.studies.items()
                if len(study.current_running()) > 0
            }

            studies_to_stop = running_study_names - open_study_names
            for study_name in studies_to_stop:
                if study_name in self.studies:
                    self.studies[study_name].scale(0, WHEELS_DIR)

            active_available_studies = set(self.studies.keys()) & open_study_names

            if active_available_studies:
                scale_to_set = max(
                    1, self.args.processes // len(active_available_studies)
                )
            else:
                scale_to_set = 0

            for study_name in active_available_studies:
                self.studies[study_name].scale(scale_to_set, WHEELS_DIR)

            # Check for idle and halt if necessary
            total_running_workers = sum(
                len(study.current_running()) for study in self.studies.values()
            )

            if total_running_workers > 0:
                self.last_activity_time = time.time()
            elif self.args.halt_after_idle_time > 0:
                idle_time_seconds = time.time() - self.last_activity_time
                idle_time_minutes = idle_time_seconds / 60
                logging.debug(f"System idle for {idle_time_minutes:.2f} minutes.")
                if idle_time_minutes > self.args.halt_after_idle_time:
                    self._halt_instance()
                    # We can break here as the instance is terminating
                    break

            time.sleep(POLL_INTERVAL)
