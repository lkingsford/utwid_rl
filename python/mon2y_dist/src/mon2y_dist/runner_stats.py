import datetime
from typing import Dict, List, NamedTuple, Optional
import boto3
from collections import defaultdict, Counter


class RunnerStats(NamedTuple):
    total_iterations: int
    total_trials: int
    num_processes: int
    total_ask_time: float
    num_asks: int


class RunnerStatus(NamedTuple):
    runners: Dict[str, RunnerStats]


def runner_status(
    start_time: datetime.datetime, end_time: datetime.datetime
) -> RunnerStatus:
    """
    Calculates runner status within a given time window.

    Args:
        start_time: The start of the time window.
        end_time: The end of the time window.

    Returns:
        A RunnerStatus object containing statistics for each runner.
    """
    log_group_name = "/ecs/mon2y-trial-daemon"
    runner_stats = defaultdict(
        lambda: {
            "total_iterations": 0,
            "total_trials": 0,
            "processes": set(),
            "total_ask_time": 0.0,
            "num_asks": 0,
        }
    )

    client = boto3.client("logs")
    paginator = client.get_paginator("filter_log_events")

    # This is not going to be performant, but it'll do for now
    try:
        page_iterator = paginator.paginate(
            logGroupName=log_group_name,
            startTime=int(start_time.timestamp() * 1000),
            endTime=int(end_time.timestamp() * 1000),
            filterPattern="INFO",
        )

        for page in page_iterator:
            for event in page["events"]:
                message = event["message"]
                if "Runner" in message and "processed" in message:
                    try:
                        parts = message.split()
                        runner_id = None
                        iterations = 0
                        for i, part in enumerate(parts):
                            if part == "Runner":
                                runner_id = parts[i + 1].strip("'")
                            elif part == "iterations:":
                                iterations = int(parts[i + 1])

                        if runner_id:
                            runner_stats[runner_id]["total_iterations"] += iterations
                            runner_stats[runner_id]["total_trials"] += 1
                            runner_stats[runner_id]["processes"].add(
                                event["logStreamName"]
                            )
                    except (ValueError, IndexError) as e:
                        print(f"Error parsing log message: {message} - {e}")
                        continue
                elif "Ask for study" in message and "took" in message:
                    try:
                        parts = message.split()
                        study_name = None
                        ask_time = 0.0
                        for i, part in enumerate(parts):
                            if part == "study":
                                study_name = parts[i + 1]
                            elif part == "took":
                                ask_time = float(parts[i - 1])

                        if study_name:
                            runner_stats[study_name]["total_ask_time"] += ask_time
                            runner_stats[study_name]["num_asks"] += 1
                    except (ValueError, IndexError) as e:
                        print(f"Error parsing log message: {message} - {e}")
                        continue

    except client.exceptions.ResourceNotFoundException:
        print(f"Log group {log_group_name} not found.")
        return RunnerStatus(runners={})

    processed_stats = {
        runner_id: RunnerStats(
            total_iterations=stats["total_iterations"],
            total_trials=stats["total_trials"],
            num_processes=len(stats["processes"]),
            total_ask_time=stats["total_ask_time"],
            num_asks=stats["num_asks"],
        )
        for runner_id, stats in runner_stats.items()
    }

    return RunnerStatus(runners=processed_stats)
