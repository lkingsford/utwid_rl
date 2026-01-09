from dataclasses import asdict, dataclass
import json
import os
from typing import Any, Dict, List, NamedTuple

from flask import Flask, jsonify, request
import optuna

from .ebr_opt import EbrHyperparams, suggest_for_trial, GOALS

app = Flask(__name__)

STORAGE_URL = os.environ.get("OPTUNA_STORAGE") or "sqlite:///db.sqlite3"

studies: Dict[str, optuna.Study] = {}

@dataclass
class AskResponse:
    study_name: str
    trial_number: int
    hyperparams: EbrHyperparams

    def to_dict(self) -> Dict[str, Any]:
        return {
            "study_name": self.study_name,
            "trial_number": self.trial_number,
            "hyperparams": self.hyperparams.to_dict(),
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "AskResponse":
        return cls(
            study_name=data["study_name"],
            trial_number=data["trial_number"],
            hyperparams=EbrHyperparams.from_dict(data["hyperparams"]),
        )

    def serialize(self) -> str:
        return json.dumps(self.to_dict())

    @classmethod
    def deserialize(cls, json_str: str) -> "AskResponse":
        return cls.from_dict(json.loads(json_str))


def get_study(study_name) -> optuna.Study:
    if study_name not in studies:
        studies[study_name] = optuna.LoadStudy(study_name)
    return studies[study_name]


@app.route("/ask")
def ask() -> str:
    study = get_study()
    trial = study.ask()
    modified_suggestion = suggest_for_trial(trial, use_defaults=False, player_count=3)

    response = AskResponse(
        study_name=STUDY_NAME,
        trial_number=trial.number,
        hyperparams=modified_suggestion.suggestion,
    )
    return response.serialize()


@app.route("/tell")
def tell(study_name: str, ):
    data = request.json
    if not data:
        return jsonify({"error": "Invalid request"}), 400

    study_name = data.get("study_name")
    trial_number = data.get("trial_number")
    results = data.get("results")

    if not all([study_name, isinstance(trial_number, int), results]):
        return jsonify({"error": "Missing required fields"}), 400
    
    if study_name != STUDY_NAME:
        return jsonify({"error": f"Invalid study name, expected {STUDY_NAME}"}), 400

    study = get_study()
    
    # Optuna doesn't have a direct way to get a trial by number and then tell it.
    # We have to re-create the conditions to report the results.
    # This is not ideal, but it's how Optuna's distributed system works without using their built-in mechanisms.
    # A better approach would be to have workers report directly to the Optuna database.
    # For this simulation, we will just mark it as complete.
    try:
        study.tell(trial_number, results)
        return jsonify({"status": "ok"})
    except Exception as e:
        # This can happen if the trial is already finished or doesn't exist.
        return jsonify({"error": str(e)}), 500


class OpenStudy(NamedTuple):
    hash: str 
    """Hash of mon2y being used"""


@app.route("/open")
def open_studies() -> Dict[str, dict[str, Any]]:
    all_studies = optuna.get_all_study_summaries()
    open_studies = [study for study in all_studies if study.user_parameters.user_attrs("dist_status") == "open"]
    return {
        study.study_name: OpenStudy(
    }


if __name__ == "__main__":
    app.run(debug=True)
