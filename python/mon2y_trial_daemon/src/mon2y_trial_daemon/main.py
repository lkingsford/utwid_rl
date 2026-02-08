import argparse
import logging
import os
import sys

from .daemon import TrialDaemon


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
    parser.add_argument(
        "--halt_after_idle_time",
        type=int,
        default=0,
        help="Time in minutes that the daemon can be idle before the system is halted. Default to 0 (don't halt).",
    )
    parser.add_argument(
        "--halt-after",
        type=int,
        default=0,
        help="Time in minutes that the daemon can run before the system is halted. Default to 0 (don't halt).",
    )
    parser.add_argument(
        "--halt-grace",
        type=int,
        default=120,
        help="Time in seconds to wait for runners to finish before halting. Default to 120.",
    )
    parser.add_argument(
        "--halt-instance",
        type=bool,
        default=False,
        help="Halt EC2 instance after shutting down",
    )
    parser.add_argument(
        "--treat-worker-as-idle-after",
        type=int,
        default=0,
        help="Time in minutes without a 'tell' to treat a worker as idle. Default to 0 (don't treat as idle).",
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

    daemon = TrialDaemon(args, log_level)
    daemon.run()


if __name__ == "__main__":
    main()
