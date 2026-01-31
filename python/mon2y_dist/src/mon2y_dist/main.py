import datetime
import json
import logging
import os
import subprocess
import sys
import threading
import time
from datetime import timedelta
from typing import Any, Dict, Optional

import optuna
import optuna.exceptions
import sqlalchemy
from flask import Flask, jsonify, request

import mon2y_dist.runner_stats
import mon2y_dist.server_stats
from mon2y_dist.db_models import study_run_details_table
from mon2y_dist.op_queue import (MAX_OP_CONNECTIONS, OP_POLL_MS, Ask, OpResult,
                                 Tell, op_queue)

try:
    import psutil
except ImportError:
    psutil = None


try:
    import boto3
except ImportError:
    subprocess.check_call([sys.executable, "-m", "pip", "install", "boto3"])
    import boto3

app = Flask(__name__)

handler = logging.StreamHandler()
handler.setFormatter(logging.Formatter("%(asctime)s - %(levelname)s - %(message)s"))
if app.logger.hasHandlers():
    app.logger.handlers.clear()
app.logger.addHandler(handler)
app.logger.setLevel(logging.INFO)

app.logger.info(f"__name__ is {__name__}")

S3_BUCKET = os.environ.get("S3_BUCKET", "mon2y")
S3_REGION = os.environ.get("S3_REGION", "ap-southeast-2")

DEFAULT_ITERATIONS = 10000

STORAGE_URL = os.environ.get("OPTUNA_STORAGE") or "sqlite:///db.sqlite3"

_storage = None


def storage():
    global _storage
    if not _storage:
        _storage = optuna.storages.RDBStorage(
            url=STORAGE_URL,
            engine_kwargs={
                "pool_size": MAX_OP_CONNECTIONS,  # Must be >= MAX_OP_CONNECTIONS
                "max_overflow": 10,
                "pool_pre_ping": True,  # Essential for Postgres to recover from idle disconnects
            },
        )
    return _storage


_ensure_tables_called = False


def ensure_tables_if_needed():
    """
    Ensures that the additional tables are created in the database,
    but only runs the check once per process.
    """
    global _ensure_tables_called
    if not _ensure_tables_called:
        app.logger.info("Running first-time table existence check...")
        mon2y_dist.runner_stats.ensure_additional_tables_exist(storage().engine)
        _ensure_tables_called = True
        app.logger.info("Table existence check complete.")


studies: Dict[str, optuna.Study] = {}


def get_study(study_name) -> Optional[optuna.Study]:
    app.logger.info(f"Loading study '{study_name}' from storage")
    try:
        # Always load from storage to prevent memory leaks from caching Study objects.
        # This is the most direct fix for the observed memory leak.
        study = optuna.load_study(study_name=study_name, storage=storage())
        app.logger.info(f"Study '{study_name}' loaded successfully from storage.")
        return study
    except KeyError:
        app.logger.warning(f"Study '{study_name}' not found in storage.")
        return None
    except Exception as e:
        app.logger.exception(f"An error occurred while loading study '{study_name}'")
        return None


DIRECTION = {"min": "minimize", "max": "maximize"}


@app.route("/create_study", methods=["POST"])
def create_study():
    app.logger.info(f"Received /create_study request from {request.remote_addr}")
    data = request.json
    if not data:
        app.logger.error("/create_study: Invalid request, expected JSON body")
        return jsonify({"error": "Invalid request, expected JSON body"}), 400

    study_name = data.get("study_name")
    if not study_name:
        app.logger.error("/create_study: 'study_name' is required")
        return jsonify({"error": "study_name is required"}), 400
    app.logger.info(f"/create_study: study_name='{study_name}'")

    directions_str = data.get("direction", "min")
    directions = [d.strip() for d in directions_str.split(",")]
    for d in directions:
        if d not in DIRECTION:
            msg = f"Invalid direction '{d}', must be 'min' or 'max'"
            app.logger.error(f"/create_study: {msg}")
            return (
                jsonify({"error": msg}),
                400,
            )
    converted_directions = [DIRECTION[d] for d in directions]
    app.logger.info(f"/create_study: directions={converted_directions}")

    iterations = data.get("iterations", DEFAULT_ITERATIONS)
    if not isinstance(iterations, int) or iterations <= 0:
        app.logger.error("/create_study: 'iterations' must be a positive integer")
        return jsonify({"error": "'iterations' must be a positive integer"}), 400
    app.logger.info(f"/create_study: iterations={iterations}")

    try:
        app.logger.info(
            f"/create_study: Creating/loading study '{study_name}' with directions {converted_directions}"
        )
        study = optuna.create_study(
            study_name=study_name,
            storage=storage(),
            directions=converted_directions if len(converted_directions) > 1 else None,
            direction=(
                converted_directions[0] if len(converted_directions) == 1 else None
            ),
            load_if_exists=True,
        )
        app.logger.info(
            f"/create_study: Study '{study_name}' created/loaded successfully."
        )
    except Exception as e:
        app.logger.exception(
            f"/create_study: Failed to create or load study '{study_name}'"
        )
        return jsonify({"error": f"Failed to create or load study: {e}"}), 500

    x86_s3_path = None
    x86_wheel_filename = data.get("x86_manylinux_wheel_s3")
    if x86_wheel_filename:
        x86_s3_path = f"s3://{S3_BUCKET}/{x86_wheel_filename}"
        app.logger.info(f"Constructed x86 wheel S3 path: {x86_s3_path}")

    arm_s3_path = None
    arm_wheel_filename = data.get("arm_manylinux_wheel_s3")
    if arm_wheel_filename:
        arm_s3_path = f"s3://{S3_BUCKET}/{arm_wheel_filename}"
        app.logger.info(f"Constructed arm wheel S3 path: {arm_s3_path}")

    module = data.get("module")
    if not module:
        app.logger.error("/create_study: Invalid Request, expected 'module'")
        return jsonify({"error": "Invalid Request, expected 'module'"}), 400

    function = data.get("function")
    if not function:
        app.logger.error("/create_study: Invalid Request, expected 'function'")
        return jsonify({"error": "Invalid Request, expected 'function'"}), 400

    params = data.get("params")

    # study.set_user_attr calls have been removed as per the "user_attr is dead" directive

    ensure_tables_if_needed()
    # Upsert the study details into our new cache table
    details = {
        "study_id": study._study_id,
        "study_name": study_name,
        "status": "open",
        "x86_manylinux_wheel_s3": x86_s3_path,
        "arm_manylinux_wheel_s3": arm_s3_path,
        "iterations": iterations,
        "module": module,
        "function": function,
        "params": json.dumps(params) if params else None,
    }

    # Use dialect-specific insert for cross-DB compatibility (SQLite/Postgres)
    dialect = storage().engine.dialect.name
    if dialect == "postgresql":
        from sqlalchemy.dialects.postgresql import insert as pg_insert

        stmt = pg_insert(study_run_details_table).values(details)
        stmt = stmt.on_conflict_do_update(index_elements=["study_id"], set_=details)
    else:  # Assume sqlite
        from sqlalchemy.dialects.sqlite import insert as sqlite_insert

        stmt = sqlite_insert(study_run_details_table).values(details)
        stmt = stmt.on_conflict_do_update(index_elements=["study_id"], set_=details)


    try:
        with storage().engine.connect() as connection:
            # Use a transaction with a commit
            trans = connection.begin()
            try:
                connection.execute(stmt)
                trans.commit()
            except Exception:
                trans.rollback()
                raise

        app.logger.info(
            f"Upserted details for study '{study_name}' into study_run_details."
        )
    except Exception as e:
        app.logger.exception(
            f"Failed to upsert study details for '{study_name}': {e}"
        )
        # Continue anyway, as this is just a cache
        pass

    return jsonify({"status": "ok", "study_name": study_name})


@app.route("/set_status", methods=["POST"])
def set_status():
    app.logger.info(f"Received /set_status request from {request.remote_addr}")
    data = request.json

    study_name = data.get("study_name")
    if not study_name:
        app.logger.error("/set_status: 'study_name' is required")
        return jsonify({"error": "'study_name' is required"}), 400

    status = data.get("status")
    PERMITTED_STATUSES = ["open", "done"]
    if not status:
        app.logger.error("/set_status: 'status' is required")
        return jsonify({"error": "'statu' is required"}), 400
    if status not in PERMITTED_STATUSES:
        return jsonify({"error": f"'status' must be one of {PERMITTED_STATUSES}"}), 400

    study = get_study(study_name)
    if study is None:
        app.logger.error(f"/set_status: Study '{study_name}' not found")
        return jsonify({"error": f"Study '{study_name}' not found"}), 404

    ensure_tables_if_needed()
    try:
        app.logger.info(
            f"Setting status for study '{study_name}' to '{status}' in study_run_details."
        )
        stmt = (
            sqlalchemy.update(study_run_details_table)
            .where(study_run_details_table.c.study_id == study._study_id)
            .values(status=status)
        )
        with storage().engine.connect() as connection:
            trans = connection.begin()
            try:
                connection.execute(stmt)
                trans.commit()
            except Exception:
                trans.rollback()
                raise
    except Exception as e:
        app.logger.exception(f"Failed to set status for study '{study_name}': {e}")
        return jsonify({"error": f"Failed to set status: {e}"}), 500

    return jsonify({"status": "ok"})


@app.route("/ask", methods=["POST"])
def ask():
    app.logger.info(f"__name__ is {__name__}")
    start_workers_if_needed()
    app.logger.info(f"Received /ask request from {request.remote_addr}")
    data = request.json
    if not data:
        app.logger.error("/ask: Invalid request, expected JSON body")
        return jsonify({"error": "Invalid request, expected JSON body"}), 400

    study_name = data.get("study_name")
    if not study_name:
        app.logger.error("/ask: 'study_name' is required")
        return jsonify({"error": "study_name is required"}), 400
    app.logger.info(f"/ask: study_name='{study_name}'")

    distributions_json = data.get("distributions")
    if not distributions_json:
        app.logger.error("/ask: 'distributions' is required")
        return jsonify({"error": "distributions is required"}), 400

    try:
        distributions = {
            param_name: optuna.distributions.json_to_distribution(
                json.dumps(param_json)
            )
            for param_name, param_json in distributions_json.items()
        }
    except Exception as e:
        app.logger.exception(
            f"/ask: Failed to parse distributions for study '{study_name}'"
        )
        return jsonify({"error": f"Failed to parse distributions: {e}"}), 400

    op_result = OpResult()
    ask_op = Ask(study_name, distributions, op_result)
    op_queue.put(ask_op)
    mon2y_dist.server_stats.op_called()

    # Poll for the result
    start_time = time.time()
    # Using a 30-second timeout as a safeguard
    while not op_result.is_complete:
        if time.time() - start_time > 30:
            mon2y_dist.server_stats.op_dropped()
            app.logger.error(f"Request timed out for study '{study_name}'")
            return jsonify({"error": "Request timed out"}), 504  # Gateway Timeout

        time.sleep(OP_POLL_MS / 1000.0)

    if op_result.error:
        mon2y_dist.server_stats.op_dropped()
        app.logger.error(
            f"/ask: Operation failed for study '{study_name}': {op_result.error}"
        )
        return jsonify({"error": f"Failed to ask for trial: {op_result.error}"}), 500

    # "user_attr is dead", so get iterations from our new cache table.
    try:
        from sqlalchemy import select

        stmt = select(study_run_details_table.c.iterations).where(
            study_run_details_table.c.study_name == study_name
        )
        with storage().engine.connect() as connection:
            iterations = connection.execute(stmt).scalar_one_or_none()
        op_result.result["iterations"] = iterations
        if iterations is None:
            app.logger.warning(
                f"Iterations for study '{study_name}' was NULL in the study_run_details table."
            )

    except Exception as e:
        app.logger.exception(
            f"Failed to get iterations for study '{study_name}' from cache table."
        )
        return jsonify({"error": f"Failed to get iterations for study: {e}"}), 500

    app.logger.info(
        f"/ask: Processed trial {op_result.result['trial_number']} for study '{study_name}' with params: {op_result.result['params']}"
    )
    return jsonify(op_result.result)


@app.route("/tell", methods=["POST"])
def tell():
    start_workers_if_needed()
    app.logger.info(f"Received /tell request from {request.remote_addr}")
    data = request.json
    if not data:
        app.logger.error("/tell: Invalid request, expected JSON body")
        return jsonify({"error": "Invalid request, expected JSON body"}), 400

    study_name = data.get("study_name")
    if not study_name:
        app.logger.error("/tell: 'study_name' is required")
        return jsonify({"error": "study_name is required"}), 400
    app.logger.info(f"/tell: study_name='{study_name}'")

    if data.get("trial_number") is None:
        app.logger.error("/tell: 'trial_number' is required")
        return jsonify({"error": "trial_number is required"}), 400

    op_result = OpResult()
    # The Tell class will handle the full logic based on the request data
    tell_op = Tell(study_name, data, op_result)
    op_queue.put(tell_op)
    mon2y_dist.server_stats.op_called()

    # Poll for the result
    return jsonify({"status": "enqueued"})


@app.route("/heartbeat", methods=["POST"])
def heartbeat():
    app.logger.info(f"Received /heartbeat request from {request.remote_addr}")
    data = request.json
    if not data:
        app.logger.error("/heartbeat: Invalid request, expected JSON body")
        return jsonify({"error": "Invalid request, expected JSON body"}), 400

    study_name = data.get("study_name")
    if not study_name:
        app.logger.error("/heartbeat: 'study_name' is required")
        return jsonify({"error": "study_name is required"}), 400
    app.logger.info(f"/heartbeat: study_name='{study_name}'")

    trial_number = data.get("trial_number")
    if trial_number is None:
        app.logger.error("/heartbeat: 'trial_number' is required")
        return jsonify({"error": "trial_number is required"}), 400
    app.logger.info(f"/heartbeat: trial_number='{trial_number}'")

    try:
        study = get_study(study_name)
        if study is None:
            app.logger.error(f"/heartbeat: Study '{study_name}' not found")
            return jsonify({"error": f"Study '{study_name}' not found"}), 404
    except Exception as e:
        app.logger.exception(f"/heartbeat: Failed to load study '{study_name}'")
        return jsonify({"error": f"Failed to load study: {e}"}), 500

    storage_obj = study._storage
    if not isinstance(storage_obj, optuna.storages.RDBStorage):
        app.logger.error(
            f"/heartbeat: Heartbeat is only supported for RDBStorage, but got {type(storage_obj)}"
        )
        return jsonify({"error": "Heartbeat is only supported for RDBStorage"}), 501

    trial_id = storage_obj.get_trial_id_from_study_id_trial_number(
        study._study_id, trial_number
    )

    if trial_id is None:
        app.logger.error(
            f"/heartbeat: Trial number {trial_number} not found in study {study_name}"
        )
        return (
            jsonify(
                {
                    "error": f"Trial number {trial_number} not found in study {study_name}"
                }
            ),
            404,
        )

    try:
        app.logger.info(
            f"/heartbeat: Recording heartbeat for trial {trial_number} (id: {trial_id}) in study '{study_name}'"
        )
        storage_obj.record_heartbeat(trial_id)
        return jsonify({"status": "ok"})
    except Exception as e:
        app.logger.exception(
            f"/heartbeat: Failed to record heartbeat for trial {trial_number} in study '{study_name}'"
        )
        return jsonify({"error": f"Failed to record heartbeat: {e}"}), 500


@app.route("/update_wheel", methods=["POST"])
def update_wheel():
    app.logger.info(f"Received /update_wheel request from {request.remote_addr}")

    study_name = request.args.get("study_name")
    if not study_name:
        app.logger.error("/update_wheel: 'study_name' is required")
        return jsonify({"error": "'study_name' is required"}), 400

    platform = request.args.get("platform")
    if not platform:
        app.logger.error("/update_wheel: 'platform' is required")
        return jsonify({"error": "'platform' is required"}), 400
    if platform not in ["x86_manylinux", "arm_manylinux"]:
        app.logger.error(f"/update_wheel: Invalid 'platform' {platform}")
        return (
            jsonify({"error": "platform must be 'x86_manylinux' or 'arm_manylinux'"}),
            400,
        )

    filename = request.args.get("filename")
    if not filename:
        app.logger.error("/update_wheel: 'filename' is required")
        return jsonify({"error": "'filename' is required"}), 400

    wheel_data = request.data
    if not wheel_data:
        app.logger.error("/update_wheel: POST data is empty")
        return jsonify({"error": "POST data is empty"}), 400

    if not S3_BUCKET:
        app.logger.error("/update_wheel: S3_BUCKET environment variable not set")
        return jsonify({"error": "Server is not configured for S3 uploads"}), 500

    study = get_study(study_name)
    if study is None:
        app.logger.error(f"/update_wheel: Study '{study_name}' not found")
        return jsonify({"error": f"Study '{study_name}' not found"}), 404

    s3 = boto3.client("s3", region_name=S3_REGION)
    s3_key = f"{study_name}/{filename}"
    s3_path = f"s3://{S3_BUCKET}/{s3_key}"

    try:
        app.logger.info(f"Uploading wheel to {s3_path}")
        s3.put_object(Bucket=S3_BUCKET, Key=s3_key, Body=wheel_data)
    except Exception as e:
        app.logger.exception(f"Failed to upload to S3: {e}")
        return jsonify({"error": f"Failed to upload to S3: {e}"}), 500

    ensure_tables_if_needed()
    user_attr_key = f"{platform}_wheel_s3"
    try:
        app.logger.info(
            f"Updating study_run_details for '{study_name}' with {user_attr_key} = {s3_path}"
        )
        stmt = (
            sqlalchemy.update(study_run_details_table)
            .where(study_run_details_table.c.study_id == study._study_id)
            .values({user_attr_key: s3_path})
        )
        with storage().engine.connect() as connection:
            trans = connection.begin()
            try:
                connection.execute(stmt)
                trans.commit()
            except Exception:
                trans.rollback()
                raise
    except Exception as e:
        app.logger.exception(f"Failed to update study_run_details: {e}")
        return jsonify({"error": f"Failed to update study_run_details: {e}"}), 500

    return jsonify({"status": "ok", "s3_path": s3_path})


@app.route("/remove_wheel", methods=["POST"])
def remove_wheel():
    app.logger.info(f"Received /remove_wheel request from {request.remote_addr}")

    study_name = request.args.get("study_name")
    if not study_name:
        app.logger.error("/remove_wheel: 'study_name' is required")
        return jsonify({"error": "'study_name' is required"}), 400

    platform = request.args.get("platform")
    if not platform:
        app.logger.error("/remove_wheel: 'platform' is required")
        return jsonify({"error": "'platform' is required"}), 400
    if platform not in ["x86_manylinux", "arm_manylinux"]:
        app.logger.error(f"/remove_wheel: Invalid 'platform' {platform}")
        return (
            jsonify({"error": "platform must be 'x86_manylinux' or 'arm_manylinux'"}),
            400,
        )

    study = get_study(study_name)
    if study is None:
        app.logger.error(f"/remove_wheel: Study '{study_name}' not found")
        return jsonify({"error": f"Study '{study_name}' not found"}), 404

    ensure_tables_if_needed()
    user_attr_key = f"{platform}_wheel_s3"
    try:
        app.logger.info(
            f"Updating study_run_details for '{study_name}', setting {user_attr_key} to NULL"
        )
        stmt = (
            sqlalchemy.update(study_run_details_table)
            .where(study_run_details_table.c.study_id == study._study_id)
            .values({user_attr_key: None})
        )
        with storage().engine.connect() as connection:
            trans = connection.begin()
            try:
                connection.execute(stmt)
                trans.commit()
            except Exception:
                trans.rollback()
                raise
    except Exception as e:
        app.logger.exception(f"Failed to update study_run_details: {e}")
        return jsonify({"error": f"Failed to update study_run_details: {e}"}), 500

    return jsonify({"status": "ok"})


@app.route("/open", methods=["GET"])
def get_open_studies():
    app.logger.info("Received /open request")
    ensure_tables_if_needed()
    try:
        s3 = boto3.client("s3", region_name=S3_REGION)

        # Query our fast cache table instead of slow get_all_study_summaries
        stmt = sqlalchemy.select(study_run_details_table).where(
            study_run_details_table.c.status == "open"
        )
        with storage().engine.connect() as connection:
            open_studies_details = connection.execute(stmt).fetchall()
        app.logger.info(
            f"Found {len(open_studies_details)} open studies in study_run_details."
        )

        open_studies = []
        for row in open_studies_details:
            # Build the user_attrs dict directly from our cache table,
            # completely avoiding the slow Study object and its user_attrs.
            user_attrs = {
                "dist-status": row.status,
                "iterations": row.iterations,
                "module": row.module,
                "function": row.function,
                "params": json.loads(row.params) if row.params else None,
            }

            # Generate presigned URLs from the S3 paths in our table
            for attr_key, s3_path in [
                ("x86_manylinux_wheel_s3", row.x86_manylinux_wheel_s3),
                ("arm_manylinux_wheel_s3", row.arm_manylinux_wheel_s3),
            ]:
                if s3_path:
                    if not s3_path.startswith("s3://"):
                        app.logger.warning(
                            f"Attribute '{attr_key}' for study '{row.study_name}' does not contain a valid S3 path: {s3_path}. Skipping presigned URL generation."
                        )
                        continue
                    try:
                        bucket, key = s3_path.replace("s3://", "").split("/", 1)
                        presigned_url = s3.generate_presigned_url(
                            "get_object",
                            Params={"Bucket": bucket, "Key": key},
                            ExpiresIn=3600,  # 1 hour
                        )
                        app.logger.info(f"Presigned url is {presigned_url}")
                        url_attr_key = attr_key.replace("_s3", "_url")
                        user_attrs[url_attr_key] = presigned_url
                    except Exception as e:
                        app.logger.error(
                            f"Failed to create presigned URL for {s3_path}: {e}"
                        )

            open_studies.append(
                {
                    "study_id": row.study_id,
                    "study_name": row.study_name,
                    "user_attrs": user_attrs,
                }
            )
        return jsonify(open_studies)
    except Exception as e:
        app.logger.exception("Failed to get open studies")
        return jsonify({"error": f"Failed to get open studies: {e}"}), 500


def namedtuple_to_dict(obj):
    if isinstance(obj, tuple) and hasattr(obj, "_asdict"):
        return {k: namedtuple_to_dict(v) for k, v in obj._asdict().items()}
    elif isinstance(obj, dict):
        return {k: namedtuple_to_dict(v) for k, v in obj.items()}
    elif isinstance(obj, list):
        return [namedtuple_to_dict(elem) for elem in obj]
    else:
        return obj


@app.route("/runner_status", methods=["GET"])
def get_runner_status():
    app.logger.info("Received /runner_status request")
    try:
        iso_start_time = request.args.get("start_time")
        iso_end_time = request.args.get("end_time")

        if iso_start_time and iso_end_time:
            try:
                start_time = datetime.datetime.fromisoformat(iso_start_time)
                end_time = datetime.datetime.fromisoformat(iso_end_time)
            except ValueError:
                return (
                    jsonify({"error": "Invalid ISO format for start_time or end_time"}),
                    400,
                )
        else:
            time_seconds = int(request.args.get("time_seconds", 600))
            end_time = datetime.datetime.now()
            start_time = end_time - timedelta(seconds=time_seconds)

        status_output = mon2y_dist.runner_stats.runner_status(
            start_time, end_time, STORAGE_URL
        )

        return jsonify(namedtuple_to_dict(status_output))
    except Exception as e:
        app.logger.exception("Failed to get runner status")
        return jsonify({"error": f"Failed to get runner status: {e}"}), 500


@app.route("/dist_status", methods=["GET"])
def dist_status():
    app.logger.info("Received /dist_status request")
    try:
        entries = int(request.args.get("entries", 1))

        server_stats_data = mon2y_dist.server_stats.server_stats(entries)
        process_stats_data = mon2y_dist.server_stats.process_stats(entries)

        return jsonify(
            {
                "server_stats": namedtuple_to_dict(server_stats_data),
                "process_stats": namedtuple_to_dict(process_stats_data),
            }
        )
    except Exception as e:
        app.logger.exception("Failed to get dist status")
        return jsonify({"error": f"Failed to get dist status: {e}"}), 500


def worker_thread_main():
    """Main loop for worker threads processing Optuna operations."""
    while True:
        op = op_queue.get()
        try:
            study = get_study(op.study_name)
            if study is None:
                op.result_container.complete(
                    error=ValueError(f"Study '{op.study_name}' not found")
                )
                continue

            # The 'run' method on the op object will handle the specific logic
            # for ask/tell and populate the result container.
            op.run(study, storage())
        except Exception as e:
            app.logger.exception(f"Error processing operation in worker thread: {e}")
            op.result_container.complete(error=e)


_workers_started = False
_worker_start_lock = threading.Lock()


def start_workers_if_needed():
    """Starts the worker threads if they haven't been started for this process."""
    global _workers_started
    with _worker_start_lock:
        if not _workers_started:
            app.logger.info(
                f"Starting {MAX_OP_CONNECTIONS} worker threads for PID {os.getpid()}."
            )
            for i in range(MAX_OP_CONNECTIONS):
                thread = threading.Thread(
                    target=worker_thread_main, daemon=True, name=f"Worker-{i}"
                )
                thread.start()
            _workers_started = True


def main():
    # Initialize the server stats module (now without queue parameter)
    mon2y_dist.server_stats.init()
    mon2y_dist.runner_stats.ensure_additional_tables_exist(storage().engine)

    # Note: Worker threads are now started lazily on the first request
    # by start_workers_if_needed() to support pre-fork servers like Gunicorn.

    app.run(host="0.0.0.0", port=5000)


if __name__ == "__main__":
    main()
