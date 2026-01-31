import logging

import sqlalchemy

logger = logging.getLogger(__name__)
logger.setLevel(logging.INFO)

if not logger.handlers:
    handler = logging.StreamHandler()
    formatter = logging.Formatter(
        "[%(asctime)s] [PID %(process)d] %(levelname)s - %(message)s"
    )
    handler.setFormatter(formatter)
    logger.addHandler(handler)

# --- Additional Database Tables Definition ---
metadata = sqlalchemy.MetaData()

trial_additional_data_table = sqlalchemy.Table(
    "trial_additional_data",
    metadata,
    sqlalchemy.Column(
        "trial_id",
        sqlalchemy.Integer,
        # sqlalchemy.ForeignKey("trials.trial_id"),
        primary_key=True,
    ),
    sqlalchemy.Column("process_id", sqlalchemy.String),
    sqlalchemy.Column("iterations", sqlalchemy.Integer),
    sqlalchemy.Column(
        "runner_id", sqlalchemy.String
    ),  # New column to directly store runner_id
    sqlalchemy.Column("ask_reply_time_ms", sqlalchemy.Float),  # New column for ask time
    sqlalchemy.Column(
        "user_data_json", sqlalchemy.Text
    ),  # Use Text for JSON string to be compatible across DBs
)

study_run_details_table = sqlalchemy.Table(
    "study_run_details",
    metadata,
    sqlalchemy.Column("study_id", sqlalchemy.Integer, primary_key=True),
    sqlalchemy.Column("study_name", sqlalchemy.String(255), nullable=False, unique=True, index=True),
    sqlalchemy.Column("status", sqlalchemy.String(50), nullable=False, index=True),
    sqlalchemy.Column("x86_manylinux_wheel_s3", sqlalchemy.String(1024)),
    sqlalchemy.Column("arm_manylinux_wheel_s3", sqlalchemy.String(1024)),
)


def ensure_additional_tables_exist(engine: sqlalchemy.engine.Engine):
    """Ensures the additional tables (like trial_additional_data) are created."""
    logger.info("Ensuring additional tables exist...")
    metadata.create_all(engine, checkfirst=True)
    logger.info("Additional tables ensured.")
