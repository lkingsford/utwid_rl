import os
from typing import Any, Dict

from flask import Flask, jsonify, request
import optuna

app = Flask(__name__)

STORAGE_URL = os.environ.get("OPTUNA_STORAGE") or "sqlite:///db.sqlite3"

studies: Dict[str, optuna.Study] = {}


def get_study(study_name) -> optuna.Study:
    if study_name not in studies:
        studies[study_name] = optuna.load_study(
            study_name=study_name, storage=STORAGE_URL
        )
    return studies[study_name]


@app.route("/create_study", methods=["POST"])
def create_study():
    data = request.json
    if not data:
        return jsonify({"error": "Invalid request, expected JSON body"}), 400

    study_name = data.get("study_name")
    if not study_name:
        return jsonify({"error": "study_name is required"}), 400

    directions_str = data.get("direction", "min")
    directions = [d.strip() for d in directions_str.split(",")]
    for d in directions:
        if d not in ["min", "max"]:
            return (
                jsonify(
                    {"error": f"Invalid direction '{d}', must be 'min' or 'max'"}
                ),
                400,
            )

    try:
        study = optuna.create_study(
            study_name=study_name,
            storage=STORAGE_URL,
            directions=directions,
            load_if_exists=True,
        )
    except Exception as e:
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

    for key, value in user_attrs.items():
        study.set_user_attr(key, value)

    return jsonify({"status": "ok", "study_name": study_name})



@app.route("/ask", methods=["POST"])
def ask():
    data = request.json
    if not data:
        return jsonify({"error": "Invalid request, expected JSON body"}), 400

    study_name = data.get("study_name")
    if not study_name:
        return jsonify({"error": "study_name is required"}), 400

    distributions_json = data.get("distributions")
    if not distributions_json:
        return jsonify({"error": "distributions is required"}), 400

    try:
        study = get_study(study_name)
    except Exception as e:
        return jsonify({"error": f"Failed to load study: {e}"}), 500

    try:
        distributions = {
            param_name: optuna.distributions.json_to_distribution(param_json)
            for param_name, param_json in distributions_json.items()
        }
    except Exception as e:
        return jsonify({"error": f"Failed to parse distributions: {e}"}), 400

    trial = study.ask(search_space=distributions)
    return jsonify({"trial_number": trial.number, "params": trial.params})


@app.route("/tell", methods=["POST"])
def tell():
    data = request.json
    if not data:
        return jsonify({"error": "Invalid request, expected JSON body"}), 400

    study_name = data.get("study_name")
    if not study_name:
        return jsonify({"error": "study_name is required"}), 400

    trial_number = data.get("trial_number")
    if trial_number is None:  # trial_number can be 0
        return jsonify({"error": "trial_number is required"}), 400

    status = data.get("status")
    if not status:
        return jsonify({"error": "status is required"}), 400

    try:
        study = get_study(study_name)
    except Exception as e:
        return jsonify({"error": f"Failed to load study: {e}"}), 500

    user_data = data.get("user_data")
    if user_data:
        try:
            for key, value in user_data.items():
                study.set_trial_user_attr(trial_number, key, value)
        except Exception as e:
            return jsonify({"error": f"Failed to set user data: {e}"}), 500

    if status == "succeed":
        result = data.get("result")
        if result is None:
            return jsonify({"error": "result is required for status 'succeed'"}), 400
        try:
            study.tell(trial_number, values=result)
            return jsonify({"status": "ok"})
        except Exception as e:
            return jsonify({"error": f"Failed to tell study: {e}"}), 500

    elif status == "fail":
        try:
            study.tell(trial_number, state=optuna.trial.TrialState.FAIL)
            return jsonify({"status": "ok"})
        except Exception as e:
            return jsonify({"error": f"Failed to tell study: {e}"}), 500

    else:
        return jsonify({"error": "status must be 'succeed' or 'fail'"}), 400


if __name__ == "__main__":
    app.run(debug=True)