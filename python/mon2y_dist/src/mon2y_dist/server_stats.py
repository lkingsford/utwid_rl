import logging
import datetime
import os
import threading
import time
from collections import namedtuple
from typing import List

try:
    import psutil
except ImportError:
    psutil = None

from sqlalchemy import (
    create_engine,
    Column,
    DateTime,
    Float,
    Integer,
    MetaData,
    Table,
    desc,
)
from sqlalchemy.orm import sessionmaker
from sqlalchemy.orm.scoping import scoped_session

from . import op_queue

LOGGER = logging.getLogger()

# --- Database Setup ---
STORAGE_URL = os.environ.get("OPTUNA_STORAGE") or "sqlite:///db.sqlite3"
_engine = create_engine(STORAGE_URL)
_session_factory = sessionmaker(bind=_engine)
Session = scoped_session(_session_factory)
_metadata = MetaData()


# Define tables
server_status_table = Table(
    "server_status",
    _metadata,
    Column("timestamp", DateTime, primary_key=True, index=True),
    Column("cpu_usage_percent", Float),
    Column("ram_usage_percent", Float),
)

process_status_table = Table(
    "process_status",
    _metadata,
    Column("timestamp", DateTime, primary_key=True, index=True),
    Column("ops_queue_size", Integer),
    Column("ops_calls_last_minute", Integer),
    Column("dropped_ops_last_minute", Integer),
)

# --- In-memory Stats Tracking ---
_ops_calls: List[datetime.datetime] = []
_dropped_calls: List[datetime.datetime] = []
_lock = threading.Lock()
_init_lock = threading.Lock()
_started = False


def op_called():
    """Record that an operation was called."""
    init()
    with _lock:
        LOGGER.debug("os_called")
        _ops_calls.append(datetime.datetime.now(datetime.timezone.utc))


def op_dropped():
    """Record that an operation was dropped."""
    init()
    with _lock:
        LOGGER.debug("os_dropped")
        _dropped_calls.append(datetime.datetime.now(datetime.timezone.utc))


def _get_ops_last_minute() -> int:
    """Return the number of operations called in the last minute."""
    return len(_ops_calls)


def _get_ops_dropped_last_minute() -> int:
    """Return the number of dropped operations in the last minute."""
    return len(_dropped_calls)


# --- Background Tasks ---


def _tidy_ops():
    """Periodically cleans up old entries from _ops_calls and _dropped_calls."""
    while True:
        time.sleep(1)
        one_minute_ago = datetime.datetime.now(
            datetime.timezone.utc
        ) - datetime.timedelta(minutes=1)
        with _lock:
            while _ops_calls and _ops_calls[0] < one_minute_ago:
                _ops_calls.pop(0)
            while _dropped_calls and _dropped_calls[0] < one_minute_ago:
                _dropped_calls.pop(0)


def _monitor_poll():
    """Periodically records server and process stats to the database."""
    while True:
        time.sleep(30)

        session = Session()
        try:
            now = datetime.datetime.now(datetime.timezone.utc)

            # --- Record Server Status ---
            if psutil:
                cpu_usage = psutil.cpu_percent()
                ram_usage = psutil.virtual_memory().percent

                stmt = server_status_table.insert().values(
                    timestamp=now,
                    cpu_usage_percent=cpu_usage,
                    ram_usage_percent=ram_usage,
                )
                session.execute(stmt)

            # --- Record Process Status ---
            queue_size = op_queue.op_queue.qsize()

            stmt = process_status_table.insert().values(
                timestamp=now,
                ops_queue_size=queue_size,
                ops_calls_last_minute=_get_ops_last_minute(),
                dropped_ops_last_minute=_get_ops_dropped_last_minute(),
            )
            session.execute(stmt)

            session.commit()
        except Exception:
            session.rollback()
            # In a real application, this should be logged.
        finally:
            Session.remove()


# --- Public API ---


def init():
    """Initializes the server stats module. Safe to call multiple times."""
    global _started

    with _init_lock:
        if _started:
            return
        _started = True

    LOGGER.info("Creating tables")
    _metadata.create_all(_engine, checkfirst=True)

    monitor_thread = threading.Thread(target=_monitor_poll, daemon=True)
    monitor_thread.start()

    tidy_thread = threading.Thread(target=_tidy_ops, daemon=True)
    tidy_thread.start()


ServerStats = namedtuple(
    "ServerStats", ["timestamp", "cpu_usage_percent", "ram_usage_percent"]
)


def server_stats(entries_count: int) -> List[ServerStats]:
    """Returns the most recent rows from the server_status table."""
    init()
    session = Session()
    try:
        rows = (
            session.query(server_status_table)
            .order_by(desc(server_status_table.c.timestamp))
            .limit(entries_count)
            .all()
        )
        return [
            ServerStats(
                timestamp=r.timestamp.isoformat(),
                cpu_usage_percent=r.cpu_usage_percent,
                ram_usage_percent=r.ram_usage_percent,
            )
            for r in rows
        ]
    finally:
        Session.remove()


ProcessStats = namedtuple(
    "ProcessStats",
    ["timestamp", "ops_queue_size", "ops_calls_last_minute", "dropped_ops_last_minute"],
)


def process_stats(entries_count: int) -> List[ProcessStats]:
    """Returns the most recent rows from the process_status table."""
    init()
    session = Session()
    try:
        rows = (
            session.query(process_status_table)
            .order_by(desc(process_status_table.c.timestamp))
            .limit(entries_count)
            .all()
        )
        return [
            ProcessStats(
                timestamp=r.timestamp.isoformat(),
                ops_queue_size=r.ops_queue_size,
                ops_calls_last_minute=r.ops_calls_last_minute,
                dropped_ops_last_minute=r.dropped_ops_last_minute,
            )
            for r in rows
        ]
    finally:
        Session.remove()
