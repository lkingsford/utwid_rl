import datetime
import json
import logging
import sys
from collections import defaultdict
from typing import Dict, NamedTuple, Optional

import sqlalchemy
from sqlalchemy.engine import Engine, make_url

from mon2y_dist.db_models import (ensure_additional_tables_exist,
                                  trial_additional_data_table)

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
            t.trial_id,
            tad.runner_id,
            tad.iterations,
            tad.process_id,
            tad.ask_reply_time_ms
        FROM trials t
        JOIN trial_additional_data tad ON t.trial_id = tad.trial_id
        WHERE
            t.datetime_complete BETWEEN :start_time AND :end_time
            AND t.state = 'COMPLETE'
        """

        with engine.connect() as connection:
            rows = connection.execute(
                sqlalchemy.text(query),
                {"start_time": start_time, "end_time": end_time},
            ).fetchall()

        logger.info(f"Database query returned {len(rows)} trials. Processing stats...")

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
                stats["total_trials"] += 1

                # Iterations
                if row.iterations is not None:  # Use is not None as 0 is a valid value
                    stats["total_iterations"] += row.iterations

                # Process ID
                if row.process_id:
                    stats["processes"].add(row.process_id)

                # Ask Time
                if row.ask_reply_time_ms is not None:
                    stats["total_ask_time"] += row.ask_reply_time_ms / 1000.0
                    stats["num_asks"] += 1

            except Exception as e:
                logger.warning(f"Error processing trial_id {row.trial_id}: {e}")
                continue
            
            return RunnerStats(stats['total_iterations'], stats['total_trials'], stats['processes'], stats['total_ask_time'], stats['num_asks'])



    except sqlalchemy.exc.OperationalError as e:
        logger.error(f"Database Operational Error (Connection lost?): {e}")
        # Explicitly dispose engine to force a reconnect next time
        if _global_engine:
            _global_engine.dispose()
        raise
    except Exception as e:
        logger.exception(f"Unexpected error in runner_status: {e}")
        raise
