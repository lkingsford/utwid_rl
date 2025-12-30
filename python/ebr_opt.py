from functools import partial
import logging
import math
import os
from statistics import fmean
from typing import (
    Dict,
    List,
    TypedDict,
    NamedTuple,
    TypeVar,
    Generic,
    Tuple,
    Optional,
    Callable,
)

import numpy as np
import optuna
import pandas as pd

import mon2y

logging.basicConfig(
    format="%(asctime)s %(levelname)s %(message)s",
    datefmt="%Y-%m-%d %H:%M:%S",
    level=logging.INFO,
)

#####
# Optimization related consts
#####

MAX_ITERATIONS = 100000
TRIALS = 10000
# Experimentation showed more than 12 threads has minimum benefit (probably) due to locking
MAX_THREADS = 12
CPU_COUNT = os.cpu_count()

#####
# Rules related consts
#####

MAX_REVENUE = 50

MIN_BUILD_COST = 0
MAX_BUILD_COST = 20

MIN_ADDITIONAL_COST = 0
MAX_ADDITIONAL_COST = 20

MIN_BOND_COUNT = 1
MAX_BOND_COUNT = 50
MIN_BOND_FACE_STEP = 0
MAX_BOND_FACE_STEP = 10
MIN_BOND_COUPON_STEP = 0
MAX_BOND_COUPON_STEP = 10

MIN_PRIVATE_INITIAL_TREASURY = 0
MAX_PRIVATE_INITIAL_TREASURY = 50
MIN_PRIVATE_INITIAL_COUPON = 0
MAX_PRIVATE_INITIAL_COUPON = 25
MIN_STOCK_AVAILABLE = 1
MAX_STOCK_AVAILABLE = 6
MIN_TRACK_AVAILABLE = 1
MAX_TRACK_AVAILABLE = 15

MIN_INITIAL_CASH = 1
MAX_INITIAL_CASH = 60

MIN_WATER_COST = 0
MAX_WATER_COST = 40

MIN_NARROW_GAUGE_INITIAL = 1
MAX_NARROW_GAUGE_INITIAL = 40

MIN_MAX_BUILDS = 1
# Haw haw
MAX_MAX_BUILDS = 20

MIN_NARROW_TRACK_COST = 0
MAX_NARROW_TRACK_COST = 20

MIN_TAKE_RESOURCE_COST = 0
MAX_TAKE_RESOURCE_COST = 20

MIN_TAKE_DIVIDEND = 0
MAX_TAKE_DIVIDEND = 10

MIN_TAKE_TOWN_DELIVER_DIVIDEND = 0
MAX_TAKE_TOWN_DELIVER_DIVIDEND = 10

MIN_TAKE_PORT_DELIVER_DIVIDEND = 0
MAX_TAKE_PORT_DELIVER_DIVIDEND = 10

# Fixing player count, initially
PLAYER_COUNT = 3


#####
# Optimization Goal Related Consts/Types
#####


class Goal(NamedTuple):
    mean: float
    std_dev: float
    weight: float
    scalarize: Callable[[pd.DataFrame, EbrHyperparams], float]

    def loss(self, result_mean: float) -> float:
        z = (self.mean - result_mean) / self.std_dev
        return (1.0 - math.exp(z * z / -2)) * self.weight


DIFF_WEIGHT = 2

GOALS: Dict[str, Goal] = {
    "Bankruptcy": Goal(
        1 / 3, 1 / 6, 3, lambda df, _: len(df["end_game_reason"] == "Bankruptcy") / len(df)
    ),

    "Bond Ratio Taken": Goal (
        2 / 3, 2 / 3, 1, lambda df, hyper: df["total_bonds_issued"].mean() / len(hyper["bonds"])
    )
}

T = TypeVar("T")


class ModifiedSuggestion(Generic[T], NamedTuple):
    suggestion: T
    """Suggested result"""

    difference: float
    """OK - difference here is really 'normalized value from 0-1' that I can use to try to use to tend towards the original values"""


def calc_norm_diff(original, new, min_expected, max_expected) -> float:
    max_diff = max(abs(original - min_expected), abs(original - max_expected))
    return abs(original - new) / max_diff


def suggest_revenue(feature: str, trial: optuna.Trial) -> List[int]:
    # Revenue goes for two rounds each time
    return [
        trial.suggest_int(f"{feature}_rev_rnd_{i}", 0, MAX_REVENUE)
        for i in range(0, 3)
        for _ in (0, 1)
    ]


def diff_revenue(original: List[int], new: List[int]) -> float:
    return fmean([calc_norm_diff(o, n, 0, MAX_REVENUE) for o, n in zip(original, new)])


Terrain = TypedDict("Terrain", {"build_cost": int, "revenue": List[int]})


def suggest_modified_terrain(
    terrain: Terrain, terrain_type: str, trial: optuna.Trial
) -> ModifiedSuggestion[Terrain]:
    suggestion: Terrain = {
        "build_cost": trial.suggest_int(
            f"{terrain_type}_build_cost", MIN_BUILD_COST, MAX_BUILD_COST
        ),
        "revenue": suggest_revenue(terrain_type, trial),
    }

    diff = (
        diff_revenue(terrain["revenue"], suggestion["revenue"])
        + calc_norm_diff(
            terrain["build_cost"],
            suggestion["build_cost"],
            MIN_BUILD_COST,
            MAX_BUILD_COST,
        )
    ) / 2

    return ModifiedSuggestion(suggestion, diff)


Feature = TypedDict(
    "Feature",
    {
        "feature_type": str,
        "location_name": str,
        "revenue": List[int],
        "additional_cost": int,
    },
)


def suggest_modified_feature(
    feature: Feature, trial: optuna.Trial
) -> ModifiedSuggestion[Feature]:
    suggestion: Feature = {
        "feature_type": feature["feature_type"],
        "location_name": feature["location_name"],
        "revenue": suggest_revenue(feature["location_name"], trial),
        "additional_cost": trial.suggest_int(
            f"{feature["location_name"]}_additional_cost",
            MIN_ADDITIONAL_COST,
            MAX_ADDITIONAL_COST,
        ),
    }
    diff = (
        diff_revenue(feature["revenue"], suggestion["revenue"])
        + calc_norm_diff(
            feature["additional_cost"],
            suggestion["additional_cost"],
            MIN_ADDITIONAL_COST,
            MAX_ADDITIONAL_COST,
        )
    ) / 2

    return ModifiedSuggestion(suggestion, diff)


Bond = TypedDict("Bond", {"face_value": int, "coupon": int})


def suggest_bonds(
    original: List[Bond], trial: optuna.Trial
) -> ModifiedSuggestion[List[Bond]]:
    bond_count = trial.suggest_int("bond_count", MIN_BOND_COUNT, MAX_BOND_COUNT)

    suggested: List[Bond] = []
    face = 0
    coupon = 0
    for i in range(bond_count):
        face = trial.suggest_int(
            f"bond_face_{i}", max(1, face + MIN_BOND_FACE_STEP), face + MAX_BOND_FACE_STEP
        )
        coupon = trial.suggest_int(
            f"bond_coupon_{i}",
            coupon + MIN_BOND_COUPON_STEP,
            coupon + MAX_BOND_COUPON_STEP,
        )
        suggested.append({"face_value": face, "coupon": coupon})

    # Difference is a bit less clear (because amount of bonds might be different)
    # So - we're a few values:
    # - Bond count (weight: 1/3)
    # - Difference in max of face (weight: 1/6)
    # - Difference in min of face (weight: 1/6)
    # - Difference in mean ratio between face/coupon (weight: 1/3)
    count_diff = calc_norm_diff(
        len(original), bond_count, MIN_BOND_COUNT, MAX_BOND_COUNT
    )

    max_face_diff = calc_norm_diff(
        max([bond["face_value"] for bond in original]),
        max([bond["face_value"] for bond in suggested]),
        0,
        # TBD if this is the wise thing to do -
        # - but I need a way to make sure it's normalized to max of 1. This means the
        # ratio is different depending on bond_count, but I hope that's OK anyway.
        MAX_BOND_FACE_STEP * max(bond_count, len(original)),
    )

    min_face_diff = calc_norm_diff(
        min([bond["face_value"] for bond in original]),
        min([bond["face_value"] for bond in suggested]),
        0,
        # ... as above.
        MIN_BOND_FACE_STEP * max(bond_count, len(original)),
    )

    ratio_diff = calc_norm_diff(
        fmean([bond["coupon"] / bond["face_value"] for bond in original]),
        fmean([bond["coupon"] / bond["face_value"] for bond in suggested]),
        0,
        1,
    )

    diff = (
        count_diff * (1 / 3)
        + max_face_diff * (1 / 6)
        + min_face_diff * (1 / 6)
        + ratio_diff * (1 / 3)
    )

    return ModifiedSuggestion(suggested, diff)


CompanyFixedDetail = TypedDict(
    "CompanyFixedDetail",
    {
        "starting": List[int],
        "private": bool,
        "stock_available": int,
        "track_available": int,
        "initial_treasury": int,
        "initial_interest": int,
    },
)


def suggest_modified_company_fixed_detail(
    company_id: str, company_detail: CompanyFixedDetail, trial: optuna.Trial
) -> ModifiedSuggestion[CompanyFixedDetail]:
    suggestion: CompanyFixedDetail = {
        "private": company_detail["private"],
        "starting": company_detail["starting"],
        "initial_treasury": (
            trial.suggest_int(
                f"{company_id}_initial_treasury",
                MIN_PRIVATE_INITIAL_TREASURY,
                MAX_PRIVATE_INITIAL_TREASURY,
            )
            if company_detail["private"]
            else 0
        ),
        "initial_interest": trial.suggest_int(
            f"{company_id}_initial_interest",
            MIN_PRIVATE_INITIAL_COUPON,
            MAX_PRIVATE_INITIAL_COUPON,
        ),
        "stock_available": (
            1
            if company_detail["private"]
            else trial.suggest_int(
                f"{company_id}_stock_available",
                MIN_STOCK_AVAILABLE,
                MAX_STOCK_AVAILABLE,
            )
        ),
        "track_available": trial.suggest_int(
            f"{company_id}_track_available", MIN_TRACK_AVAILABLE, MAX_TRACK_AVAILABLE
        ),
    }

    diffs = []
    if company_detail["private"]:
        diffs.append(
            calc_norm_diff(
                company_detail["initial_treasury"],
                suggestion["initial_treasury"],
                MIN_PRIVATE_INITIAL_TREASURY,
                MAX_PRIVATE_INITIAL_TREASURY,
            )
        )
    diffs.append(
        calc_norm_diff(
            company_detail["initial_interest"],
            suggestion["initial_interest"],
            MIN_PRIVATE_INITIAL_COUPON,
            MAX_PRIVATE_INITIAL_COUPON,
        )
    )
    if not company_detail["private"]:
        diffs.append(
            calc_norm_diff(
                company_detail["stock_available"],
                suggestion["stock_available"],
                MIN_STOCK_AVAILABLE,
                MAX_STOCK_AVAILABLE,
            )
        )

    diffs.append(
        calc_norm_diff(
            company_detail["track_available"],
            suggestion["track_available"],
            MIN_TRACK_AVAILABLE,
            MAX_TRACK_AVAILABLE,
        )
    )
    return ModifiedSuggestion(suggestion, fmean(diffs) if diffs else 0)


def suggest_initial_cash(
    players: int, original: dict[str, int], trial: optuna.Trial
) -> ModifiedSuggestion[dict[str, int]]:
    # Diff wise, we're only changing (and caring about) the one for the current
    # amount of players.
    original_cash = original[str(players)]
    suggested_cash = trial.suggest_int(
        f"initial_cash_{players}p", MIN_INITIAL_CASH, MAX_INITIAL_CASH
    )
    modified = original.copy()
    modified[str(players)] = suggested_cash

    return ModifiedSuggestion(
        modified,
        calc_norm_diff(
            original_cash, suggested_cash, MIN_INITIAL_CASH, MAX_INITIAL_CASH
        ),
    )


class EbrHyperparams(TypedDict):
    terrain_attributes: dict[str, Terrain]
    features: List[Tuple[List[int], Feature]]
    water_features: List[Tuple[List[int], str]]
    bonds: List[Bond]
    initial_cash: dict[str, int]
    company_fixed_details: dict[str, CompanyFixedDetail]
    water_1_cost: int
    water_2_cost: int
    narrow_gauge_initial: int
    max_builds: int
    narrow_track_cost: int
    take_resource_cost: int
    take_dividend: int
    take_town_deliver_dividend: int
    take_port_deliver_dividend: int
    initial_resource_cubes: List[List[int]]


def suggest_for_trial(trial: optuna.Trial) -> ModifiedSuggestion[EbrHyperparams]:
    hyperparams: EbrHyperparams = mon2y.default_hyperparams(mon2y.Games.EBR)

    diffs = {}

    terrain_diff_sum = 0
    for terrain_type, terrain in hyperparams["terrain_attributes"].items():
        suggested = suggest_modified_terrain(terrain, terrain_type, trial)
        terrain_diff_sum += suggested.difference
        hyperparams["terrain_attributes"][terrain_type] = suggested.suggestion
    diffs["terrain_diff"] = (
        terrain_diff_sum / len(hyperparams["terrain_attributes"]),
        1,
    )

    feature_diff_sum = 0
    # A feature is a tuple of (coords, feature_details)
    for i, (coords, feature) in enumerate(hyperparams["features"]):
        suggested = suggest_modified_feature(feature, trial)
        feature_diff_sum += suggested.difference
        hyperparams["features"][i] = (coords, suggested.suggestion)
    diffs["feature_diff"] = (feature_diff_sum / len(hyperparams["features"]), 1)

    company_diff_sum = 0
    for company_id, details in hyperparams["company_fixed_details"].items():
        suggested = suggest_modified_company_fixed_detail(company_id, details, trial)
        company_diff_sum += suggested.difference
        hyperparams["company_fixed_details"][company_id] = suggested.suggestion
    diffs["company_diff"] = (
        (company_diff_sum / len(hyperparams["company_fixed_details"])),
        1,
    )

    suggested_bonds = suggest_bonds(hyperparams["bonds"], trial)
    hyperparams["bonds"] = suggested_bonds.suggestion
    diffs["suggested_bonds_diff"] = (suggested_bonds.difference, 1)

    initial_cash = suggest_initial_cash(
        PLAYER_COUNT, hyperparams["initial_cash"], trial
    )
    hyperparams["initial_cash"] = initial_cash.suggestion
    diffs["initial_cash_diff"] = (initial_cash.difference, 1)

    # Integer hyperparameters
    for key, min_const, max_const in [
        ("water_1_cost", MIN_WATER_COST, MAX_WATER_COST),
        ("water_2_cost", MIN_WATER_COST, MAX_WATER_COST),
        ("narrow_gauge_initial", MIN_NARROW_GAUGE_INITIAL, MAX_NARROW_GAUGE_INITIAL),
        ("max_builds", MIN_MAX_BUILDS, MAX_MAX_BUILDS),
        ("narrow_track_cost", MIN_NARROW_TRACK_COST, MAX_NARROW_TRACK_COST),
        ("take_resource_cost", MIN_TAKE_RESOURCE_COST, MAX_TAKE_RESOURCE_COST),
        ("take_dividend", MIN_TAKE_DIVIDEND, MAX_TAKE_DIVIDEND),
        (
            "take_town_deliver_dividend",
            MIN_TAKE_TOWN_DELIVER_DIVIDEND,
            MAX_TAKE_TOWN_DELIVER_DIVIDEND,
        ),
        (
            "take_port_deliver_dividend",
            MIN_TAKE_PORT_DELIVER_DIVIDEND,
            MAX_TAKE_PORT_DELIVER_DIVIDEND,
        ),
    ]:
        original_value = hyperparams[key]
        suggested_value = trial.suggest_int(key, min_const, max_const)
        hyperparams[key] = suggested_value
        diffs[f"{key}_diff"] = (
            calc_norm_diff(original_value, suggested_value, min_const, max_const),
            0.5,
        )
    
    diff_result = [diff[0] for diff in diffs.values()]
    diff_weight = [diff[1] for diff in diffs.values()]
    return ModifiedSuggestion(hyperparams, fmean(diff_result, diff_weight))


def most_trusted_hyperrewards(df: pd.DataFrame) -> pd.DataFrame:
    df["ratio"] = (df["turns"] - df["rwalk"]) / df["turns"]

    s = df["sum_diff_est_reward"]
    df["norm_sum_diff_est_reward"] = (s - s.min()) / (s.max() - s.min())

    df["trust"] = df["ratio"] * df["norm_sum_diff_est_reward"]

    t = df["trust"]
    df["norm_trust"] = (t - t.min()) / (t.max() - t.min())

    # Keep only top 1 std-dev of trust
    trust_mu = df["norm_trust"].mean()
    trust_sigma = df["norm_trust"].std()

    return pd.DataFrame(df[df["norm_trust"] >= trust_mu + trust_sigma])


def run_trial(
    trial: optuna.Trial,
    threads: Optional[int] = None,
    trials: Optional[int] = None,
    max_iterations: Optional[int] = None,
    force_iterations: Optional[int] = None
):
    trials = trials or TRIALS
    max_iterations = max_iterations or MAX_ITERATIONS
    explore_iterations = force_iterations or max_iterations * (
        1 - (math.log(max(1, trials - max(trial.number, 2))) / math.log(trials))
    )
    logging.info(f"Iterations: {explore_iterations}")

    suggested_hyperparams = suggest_for_trial(trial)

    raw_results = mon2y.explore(
        mon2y.Games.EBR,
        int(explore_iterations),
        threads or min([CPU_COUNT, MAX_THREADS]),
        hyperparams = suggested_hyperparams.suggestion,
    )
    logging.info("Explore Done - %s results", (len(raw_results),))

    df = pd.DataFrame(raw_results)
    logging.info("Dataframe Converted - %s items", (len(df),))

    # Currently using the top std-dev of trusted results for calculations
    # Desired improvement is to weight each result by trust instead
    trusted = most_trusted_hyperrewards(df)

    # Scalars is separate so they can be stored without recalculating
    logging.info("Trusted entries %s", (len(trusted),))
    goals_scalars = {
        goal_name: goal.scalarize(trusted, suggested_hyperparams.suggestion) for goal_name, goal in GOALS.items()
    }
    trial.set_user_attr("goal_scalars", goals_scalars)
    goals_loss = {
        goal_name: goal.loss(goals_scalars[goal_name])
        for goal_name, goal in GOALS.items()
    }

    return [suggested_hyperparams.difference * DIFF_WEIGHT] + list(goals_loss.values())


def start_study():
    objective = partial(run_trial, threads=4, trials=100, force_iterations=100)
    study = optuna.create_study(
        storage="sqlite:///db.sqlite3",  # Specify the storage URL here.
        #study_name="min_ebr_test_1",
        directions=["minimize"] * (1 + len(GOALS)),
    )
    study.optimize(objective, n_trials=100)
    print(f"Best value: {study.best_value} (params: {study.best_params})")


if __name__ == "__main__":
    start_study()
