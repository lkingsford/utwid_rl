import datetime
import json
import logging
import os
import sys
import subprocess
from typing import Any, Dict, Optional

from flask import Flask, jsonify, request
import optuna
import optuna.exceptions

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

S3_BUCKET = os.environ.get("S3_BUCKET", "mon2y")
S3_REGION = os.environ.get("S3_REGION", "ap-southeast-2")

STORAGE_URL = os.environ.get("OPTUNA_STORAGE") or "sqlite:///db.sqlite3"

_storage = None


def storage():
    global _storage
    if not _storage:
        _storage = optuna.storages.get_storage(STORAGE_URL)
    return _storage


studies: Dict[str, optuna.Study] = {}


def get_study(study_name) -> Optional[optuna.Study]:
    app.logger.info(f"Accessing study '{study_name}'")
    if study_name in studies:
        app.logger.info(f"Study '{study_name}' found in memory cache.")
        return studies[study_name]

    app.logger.info(
        f"Study '{study_name}' not in memory cache, attempting to load from storage: {STORAGE_URL}"
    )
    if study_name not in optuna.get_all_study_names(STORAGE_URL):
        app.logger.warning(f"Study '{study_name}' not found in storage.")
        return None
    study = optuna.load_study(study_name=study_name, storage=STORAGE_URL)
    studies[study_name] = study
    app.logger.info(f"Study '{study_name}' loaded from storage and cached.")
    return study


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

    try:
        app.logger.info(
            f"/create_study: Creating/loading study '{study_name}' with directions {converted_directions}"
        )
        if len(directions) == 1:
            study = optuna.create_study(
                study_name=study_name,
                storage=STORAGE_URL,
                direction=converted_directions[0],
                load_if_exists=True,
            )
        else:
            study = optuna.create_study(
                study_name=study_name,
                storage=STORAGE_URL,
                directions=converted_directions,
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

    user_attrs = {
        "dist-status": "open",
    }


    x86_wheel_filename = data.get("x86_manylinux_wheel_s3")
    if x86_wheel_filename:
        s3_path = f"s3://{S3_BUCKET}/{study_name}/{x86_wheel_filename}"
        user_attrs["x86_manylinux_wheel_s3"] = s3_path
        app.logger.info(f"Constructed x86 wheel S3 path: {s3_path}")

    arm_wheel_filename = data.get("arm_manylinux_wheel_s3")
    if arm_wheel_filename:
        s3_path = f"s3://{S3_BUCKET}/{study_name}/{arm_wheel_filename}"
        user_attrs["arm_manylinux_wheel_s3"] = s3_path
        app.logger.info(f"Constructed arm wheel S3 path: {s3_path}")

    module = data.get("module")
    if not module:
        app.logger.error("/create_study: Invalid Request, expected 'module'")
        return jsonify({"error": "Invalid Request, expected 'module'"}), 400
    user_attrs["module"] = module

    function = data.get("function")
    if not function:
        app.logger.error("/create_study: Invalid Request, expected 'function'")
        return jsonify({"error": "Invalid Request, expected 'function'"}), 400
    user_attrs["function"] = function

    params = data.get("params")
    if params:
        user_attrs["params"] = params

    try:
        for key, value in user_attrs.items():
            study.set_user_attr(key, value)
        app.logger.info(
            f"/create_study: Set user attributes for study '{study_name}': {user_attrs}"
        )
    except Exception as e:
        app.logger.exception(
            f"/create_study: Failed to set user attributes for study '{study_name}'"
        )
        return jsonify({"error": f"Failed to set user attributes: {e}"}), 500

    return jsonify({"status": "ok", "study_name": study_name})


@app.route("/ask", methods=["POST"])
def ask():
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
        study = get_study(study_name)
        if study is None:
            app.logger.error(f"/ask: Study '{study_name}' not found")
            return jsonify({"error": f"Study '{study_name}' not found"}), 404
    except Exception as e:
        app.logger.exception(
            f"/ask: Failed to load study '{study_name}' due to an unexpected error"
        )
        return jsonify({"error": f"Failed to load study: {e}"}), 500

    try:
        app.logger.info(f"/ask: Parsing distributions for study '{study_name}'")
        distributions = {
            # This is lazy, but I can't be bothered rewriting part of the library code to support this bit
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

    try:
        trial = study.ask(distributions)
        app.logger.info(
            f"/ask: Generated trial {trial.number} for study '{study_name}' with params: {trial.params}"
        )
        return jsonify({"trial_number": trial.number, "params": trial.params})
    except Exception as e:
        app.logger.exception(f"/ask: Study.ask failed for study '{study_name}'")
        return jsonify({"error": f"Failed to ask for trial: {e}"}), 500


@app.route("/tell", methods=["POST"])
def tell():
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

    trial_number = data.get("trial_number")
    if trial_number is None:  # trial_number can be 0
        app.logger.error("/tell: 'trial_number' is required")
        return jsonify({"error": "trial_number is required"}), 400
    app.logger.info(f"/tell: trial_number='{trial_number}'")

    status = data.get("status")
    if not status:
        app.logger.error("/tell: 'status' is required")
        return jsonify({"error": "status is required"}), 400
    app.logger.info(f"/tell: status='{status}'")

    try:
        study = get_study(study_name)
        if study is None:
            app.logger.error(f"/tell: Study '{study_name}' not found")
            return jsonify({"error": f"Study '{study_name}' not found"}), 404
    except Exception as e:
        app.logger.exception(f"/tell: Failed to load study '{study_name}'")
        return jsonify({"error": f"Failed to load study: {e}"}), 500

    user_data = data.get("user_data")

    trial_id = storage().get_trial_id_from_study_id_trial_number(
        study._study_id, trial_number
    )

    if user_data:
        try:
            app.logger.info(
                f"/tell: Setting user data for trial {trial_number} in study '{study_name}': {user_data}"
            )
            for key, value in user_data.items():
                storage().set_trial_user_attr(trial_id, key, value)
        except Exception as e:
            app.logger.exception(
                f"/tell: Failed to set user data for trial {trial_number} in study '{study_name}'"
            )
            return jsonify({"error": f"Failed to set user data: {e}"}), 500

    if status == "succeed":
        result = data.get("result")
        if result is None:
            app.logger.error("/tell: 'result' is required for status 'succeed'")
            return jsonify({"error": "result is required for status 'succeed'"}), 400
        try:
            app.logger.info(
                f"/tell: Reporting success for trial {trial_number} in study '{study_name}' with result: {result}"
            )
            study.tell(trial_number, values=result)
            return jsonify({"status": "ok"})
        except Exception as e:
            app.logger.exception(
                f"/tell: Failed to tell study '{study_name}' for trial {trial_number}"
            )
            return jsonify({"error": f"Failed to tell study: {e}"}), 500

    elif status == "fail":
        try:
            app.logger.info(
                f"/tell: Reporting fail for trial {trial_number} in study '{study_name}'"
            )
            study.tell(trial_number, state=optuna.trial.TrialState.FAIL)
            return jsonify({"status": "ok"})
        except Exception as e:
            app.logger.exception(
                f"/tell: Failed to tell study '{study_name}' for trial {trial_number}"
            )
            return jsonify({"error": f"Failed to tell study: {e}"}), 500

    else:
        app.logger.error(f"/tell: Invalid status '{status}'")
        return jsonify({"error": "status must be 'succeed' or 'fail'"}), 400


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

    storage = study._storage
    if not isinstance(storage, optuna.storages.RDBStorage):
        app.logger.error(
            f"/heartbeat: Heartbeat is only supported for RDBStorage, but got {type(storage)}"
        )
        return jsonify({"error": "Heartbeat is only supported for RDBStorage"}), 501

    trial_id = storage.get_trial_id_from_study_id_trial_number(
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
        storage.record_heartbeat(trial_id)
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
        return jsonify({"error": "platform must be 'x86_manylinux' or 'arm_manylinux'"}), 400

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
    
    user_attr_key = f"{platform}_wheel_s3"
    try:
        app.logger.info(f"Setting user attr '{user_attr_key}' to '{s3_path}'")
        study.set_user_attr(user_attr_key, s3_path)
    except Exception as e:
        app.logger.exception(f"Failed to set user attribute: {e}")
        return jsonify({"error": f"Failed to set user attribute: {e}"}), 500

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
        return jsonify({"error": "platform must be 'x86_manylinux' or 'arm_manylinux'"}), 400

    study = get_study(study_name)
    if study is None:
        app.logger.error(f"/remove_wheel: Study '{study_name}' not found")
        return jsonify({"error": f"Study '{study_name}' not found"}), 404

    user_attr_key = f"{platform}_wheel_s3"
    try:
        app.logger.info(f"Setting user attr '{user_attr_key}' to None")
        study.set_user_attr(user_attr_key, None)
    except Exception as e:
        app.logger.exception(f"Failed to set user attribute: {e}")
        return jsonify({"error": f"Failed to set user attribute: {e}"}), 500
        
    return jsonify({"status": "ok"})


@app.route("/open", methods=["GET"])
def get_open_studies():
    app.logger.info("Received /open request")
    try:
        s3 = boto3.client("s3", region_name=S3_REGION)
        all_studies = optuna.study.get_all_study_summaries(
            storage=STORAGE_URL, include_best_trial=False
        )
        open_studies = []
        for study_summary in all_studies:
            if study_summary.user_attrs.get("dist-status") == "open":
                user_attrs = study_summary.user_attrs.copy()
                for attr_key in ("x86_manylinux_wheel_s3", "arm_manylinux_wheel_s3"):
                    if s3_path := user_attrs.get(attr_key):
                        if not s3_path.startswith("s3://"):
                            app.logger.warning(
                                f"Attribute '{attr_key}' for study '{study_summary.study_name}' does not contain a valid S3 path: {s3_path}. Skipping presigned URL generation."
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
                            del user_attrs[attr_key]
                        except Exception as e:
                            app.logger.error(
                                f"Failed to create presigned URL for {s3_path}: {e}"
                            )

                open_studies.append(
                    {
                        "study_id": study_summary._study_id,
                        "study_name": study_summary.study_name,
                        "user_attrs": user_attrs,
                    }
                )
        return jsonify(open_studies)
    except Exception as e:
        app.logger.exception("Failed to get open studies")
        return jsonify({"error": f"Failed to get open studies: {e}"}), 500


