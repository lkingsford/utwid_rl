import datetime
import json
import logging
import os
from typing import Any, Dict, Optional

from flask import Flask, jsonify, request
import optuna
import optuna.exceptions

app = Flask(__name__)
handler = logging.StreamHandler()
handler.setFormatter(logging.Formatter("%(asctime)s - %(levelname)s - %(message)s"))
if app.logger.hasHandlers():
    app.logger.handlers.clear()
app.logger.addHandler(handler)
app.logger.setLevel(logging.INFO)


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
    x86_wheel = data.get("x86_manylinux_wheel_s3")
    if x86_wheel:
        user_attrs["x86_manylinux_wheel_s3"] = x86_wheel

    arm_wheel = data.get("arm_manylinux_wheel_s3")
    if arm_wheel:
        user_attrs["arm_manylinux_wheel_s3"] = arm_wheel

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


if __name__ == "__main__":
    app.logger.info("Starting Flask development server.")
    app.run(debug=True)
