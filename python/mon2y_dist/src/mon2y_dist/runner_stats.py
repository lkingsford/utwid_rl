import datetime
import json
import logging
import sys
from collections import defaultdict
from typing import Dict, NamedTuple, Optional

import sqlalchemy
from mon2y_dist.db_models import (ensure_additional_tables_exist,
                                  trial_additional_data_table)
from sqlalchemy.engine import Engine, make_url

logger = logging.getLogger()


class RunnerStats(NamedTuple):
    total_iterations: int
    total_trials: int
    num_processes: int
    total_ask_time: float
    num_asks: int


class RunnerStatus(NamedTuple):
    runners: Dict[str, RunnerStats]


# Cache the engine to avoid recreating pools, but allow for reset
_global_engine: Optional[Engine] = None


def get_engine(storage_url: str) -> Engine:
    """
    Returns a configured SQLAlchemy engine with timeouts and health checks
    to prevent freezing.
    """
    global _global_engine
    if _global_engine:
        return _global_engine

    logger.info("Initializing new Database Engine...")

    url = make_url(storage_url)
    connect_args = {}

    # Set a connection timeout (10s) so we error out instead of hanging forever
    if url.get_backend_name() == "postgresql":
        connect_args = {"options": "-csearch_path=public", "connect_timeout": 10}

    # pool_pre_ping=True: The most important fix. It pings the DB before
    # handing out a connection. If the DB closed the socket, it reconnects
    # automatically instead of freezing/crashing.
    _global_engine = sqlalchemy.create_engine(
        url, connect_args=connect_args, pool_pre_ping=True, pool_recycle=3600
    )
    return _global_engine


def runner_status(
    start_time: datetime.datetime, end_time: datetime.datetime, storage_url: str
) -> RunnerStatus:
    """
    Calculates runner status using an optimized aggregation query.
    """
    logger.info(f"Querying runner status from {start_time} to {end_time}")

    try:
        engine = get_engine(storage_url)

        # --- 2. Optimized SQL Query ---
        # We now retrieve data directly from the trial_additional_data table.
        query = """
        SELECT
            tad.runner_id,
            tad.process_id,
            SUM(tad.iterations) AS total_iterations_sum,
            SUM(tad.ask_reply_time_ms) AS total_ask_time_sum,
            COUNT(t.trial_id) AS trial_count,
            COUNT(CASE WHEN tad.ask_reply_time_ms IS NOT NULL THEN 1 ELSE NULL END) AS num_asks_count
        FROM trials t
        JOIN trial_additional_data tad ON t.trial_id = tad.trial_id
        WHERE
            t.datetime_complete BETWEEN :start_time AND :end_time
            AND t.state = 'COMPLETE'
        GROUP BY tad.runner_id, tad.process_id
        """

        with engine.connect() as connection:
            rows = connection.execute(
                sqlalchemy.text(query),
                {"start_time": start_time, "end_time": end_time},
            ).fetchall()

        logger.info(
            f"Database query returned {len(rows)} aggregated process stats. Processing final stats..."
        )

        runner_stats_data = defaultdict(
            lambda: {
                "total_iterations": 0,
                "total_trials": 0,
                "processes": set(),
                "total_ask_time": 0.0,
                "num_asks": 0,
            }
        )

        for row in rows:
            try:
                runner_id = row.runner_id
                if not runner_id:
                    continue

                stats = runner_stats_data[runner_id]
                stats["total_iterations"] += row.total_iterations_sum or 0
                stats["total_trials"] += row.trial_count or 0

                if row.process_id:
                    stats["processes"].add(row.process_id)

                stats["total_ask_time"] += row.total_ask_time_sum or 0.0
                stats["num_asks"] += row.num_asks_count or 0

            except Exception as e:
                logger.warning(
                    f"Error processing aggregated row for runner_id {row.runner_id}, process_id {row.process_id}: {e}"
                )
                continue

        processed_stats = {
            rid: RunnerStats(
                total_iterations=data["total_iterations"],
                total_trials=data["total_trials"],
                num_processes=len(data["processes"]),
                total_ask_time=data["total_ask_time"] / 1000.0,  # Convert ms to seconds
                num_asks=data["num_asks"],
            )
            for rid, data in runner_stats_data.items()
        }

        logger.info("Runner status processing complete.")
        return RunnerStatus(runners=processed_stats)

    except sqlalchemy.exc.OperationalError as e:
        logger.error(f"Database Operational Error (Connection lost?): {e}")
        # Explicitly dispose engine to force a reconnect next time
        if _global_engine:
            _global_engine.dispose()
        raise
    except Exception as e:
        logger.exception(f"Unexpected error in runner_status: {e}")
        raise
