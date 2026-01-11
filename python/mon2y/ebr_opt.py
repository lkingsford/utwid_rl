from functools import partial
from itertools import accumulate
from multiprocessing import Pool
import logging
import math
import os
from statistics import fmean
from typing import (
    Any,
    Dict,
    List,
    TypedDict,
    NamedTuple,
    TypeVar,
    Generic,
    Tuple,
    Optional,
    Callable,
    Mapping,
    ChainMap,
)
import argparse

import numpy as np
import optuna
from optuna.distributions import (
    IntDistribution,
    FloatDistribution,
    CategoricalDistribution,
    BaseDistribution,
)
import pandas as pd

import mon2y

#####
# Optimization related consts
#####

MAX_ITERATIONS = 1_000_000
MIN_ITERATIONS = 10_000
TRIALS = 10000
# Experimentation showed more than 12 threads has minimum benefit (probably) due to locking
MAX_THREADS = 4
CPU_COUNT = os.cpu_count() or 8

#####
# Rules related consts
#####

MAX_REVENUE = 25

MIN_BUILD_COST = 0
MAX_BUILD_COST = 20

MIN_ADDITIONAL_COST = 0
MAX_ADDITIONAL_COST = 20

MIN_BOND_COUNT = 5
MAX_BOND_COUNT = 20
MIN_BOND_FACE = 1
MAX_BOND_FACE = 50
MIN_BOND_COUPON_RATIO = 0.1
MAX_BOND_COUPON_RATIO = 0.5

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


#####
# Optimization Goal Related Consts/Types
#####


class GoalAspect(NamedTuple):
    mean: float
    std_dev: float
    weight: float
    scalarize: Callable[[pd.DataFrame, EbrHyperparams], float]

    def loss(self, result_mean: float) -> float:
        z = (self.mean - result_mean) / self.std_dev
        return (1.0 - math.exp(z * z / -2)) * self.weight


class EbrMeta(NamedTuple):
    terrain_type_keys: List[str]
    feature_keys: List[str]
    private_company_keys: List[str]
    public_company_keys: List[str]


EBR_META: Optional[EbrMeta] = None


DIFF_WEIGHT = 10
SMALL_LOSS_WEIGHT = 10

GOALS: Dict[str, Dict[str, GoalAspect]] = {
    "Game End Reasons": {
        "Bankruptcy": GoalAspect(
            1 / 3,
            1 / 6,
            3,
            lambda df, _: (
                (df["end_game_reason"] == "Bankruptcy").sum() / len(df)
                if not df.empty
                else 0.0
            ),
        ),
        "Dividends": GoalAspect(
            1 / 3,
            1 / 6,
            1,
            lambda df, _: (
                (df["end_game_reason"] == "Dividends").sum() / len(df)
                if not df.empty
                else 0.0
            ),
        ),
        "Shares": GoalAspect(
            1 / 6,
            1 / 6,
            1,
            lambda df, _: (
                (df["end_game_reason"] == "Shares").sum() / len(df)
                if not df.empty
                else 0.0
            ),
        ),
        "Bonds": GoalAspect(
            1 / 5 / 3,
            1 / 6,
            1,
            lambda df, _: (
                (df["end_game_reason"] == "Bonds").sum() / len(df)
                if not df.empty
                else 0.0
            ),
        ),
        "Track": GoalAspect(
            1 / 5 / 3,
            1 / 6,
            1,
            lambda df, _: (
                (df["end_game_reason"] == "Track").sum() / len(df)
                if not df.empty
                else 0.0
            ),
        ),
        "Resources": GoalAspect(
            1 / 5 / 3,
            1 / 6,
            1,
            lambda df, _: (
                (df["end_game_reason"] == "Resources").sum() / len(df)
                if not df.empty
                else 0.0
            ),
        ),
        "Stalemate": GoalAspect(
            1 / 5 / 3,
            1 / 6,
            1,
            lambda df, _: (
                (df["end_game_reason"] == "Stalemate").sum() / len(df)
                if not df.empty
                else 0.0
            ),
        ),
    },
    "Utilization": {
        "Bond Ratio Taken": GoalAspect(
            2 / 3,
            2 / 3,
            1,
            lambda df, hyper: (
                np.nan_to_num(subset["total_bonds_issued"].mean()) / len(hyper["bonds"])
                if not (subset := df.query("completed_dividend_rounds == 6")).empty
                else 0.0
            ),
        ),
        "Resources Remaining": GoalAspect(
            2,
            1,
            1,
            lambda df, _: (
                np.nan_to_num(subset["remaining_resource_cubes"].median())
                if not (subset := df.query("completed_dividend_rounds == 6")).empty
                else 0.0
            ),
        ),
    },
    "Desired Map Shape": {
        "If all dividends paid, TMLC or LW connected to Hobart and Launceston": GoalAspect(
            1 / 2,
            1 / 3,
            2,
            lambda df, _: len(
                df.query(
                    "completed_dividend_rounds == 6 and ((lw_connected_to_launceston and lw_connected_to_hobart) or (tmlc_connected_to_launceston and tmlc_connected_to_hobart))"
                )
            )
            / max(0.1, len(df.query("completed_dividend_rounds == 6"))),
        ),
        "Overall Track Ratio": GoalAspect(
            1.0,
            0.5,
            0.5,
            lambda df, _: (
                subset["overall_track_ratio"].mean()
                if not (subset := df.query("completed_dividend_rounds == 6")).empty
                else 0.0
            ),
        ),
        "Average Terrain Track Ratio": GoalAspect(
            1.0,
            0.5,
            0.5,
            lambda df, _: (
                subset["terrain_track_ratios"]
                .apply(lambda x: fmean(x.values()) if x else 0.0)
                .mean()
                if not (subset := df.query("completed_dividend_rounds == 6")).empty
                and "terrain_track_ratios" in subset.columns
                else 0.0
            ),
        ),
    },
    "Bias": {
        "IPO EBRC Winner Bias": GoalAspect(
            1 / 4,
            1 / 2,
            1,
            lambda df, _: (
                len(df.query("ebrc_auction_winner == winning_player_id")) / len(df)
                if not df.empty
                else 0.0
            ),
        ),
        "IPO LW Winner Bias": GoalAspect(
            1 / 4,
            1 / 2,
            1,
            lambda df, _: (
                len(df.query("lw_auction_winner == winning_player_id")) / len(df)
                if not df.empty
                else 0.0
            ),
        ),
        "IPO TMLC Winner Bias": GoalAspect(
            1 / 4,
            1 / 2,
            1,
            lambda df, _: (
                len(df.query("tmlc_auction_winner == winning_player_id")) / len(df)
                if not df.empty
                else 0.0
            ),
        ),
        "IPO GT Winner Bias": GoalAspect(
            1 / 4,
            1 / 2,
            1,
            lambda df, _: (
                len(df.query("gt_auction_winner == winning_player_id")) / len(df)
                if not df.empty
                else 0.0
            ),
        ),
    },
}

T = TypeVar("T")


class ModifiedSuggestion(Generic[T], NamedTuple):
    suggestion: T
    """Suggested result"""

    difference: float
    """OK - difference here is really 'normalized value from 0-1' that I can use to try to use to tend towards the original values"""

    small_loss: float
    """Bias towards smaller numbers"""


def calc_norm_diff(original, new, min_expected, max_expected) -> float:
    max_diff = max(abs(original - min_expected), abs(original - max_expected))
    return abs(original - new) / max_diff


def bias_small_loss(val: float) -> float:
    """
    Calculates a loss that biases towards smaller numbers.
    log10(max(1, val)) - 1
    """
    return math.pow(math.log10(max(1, val)), 2) - 1.0


def get_revenue_from_params(prefix: str, params: Dict[str, Any]) -> List[int]:
    """Converts from {prefix}_rev_0, {prefix}_rev_1, {prefix}_rev_2 keys to a 6-element revenue list."""
    return [params[f"{prefix}_rev_{i}"] for i in range(3) for _ in (0, 1)]


def diff_revenue(original: List[int], new: List[int]) -> float:
    return fmean([calc_norm_diff(o, n, 0, MAX_REVENUE) for o, n in zip(original, new)])


Terrain = TypedDict("Terrain", {"build_cost": int, "revenue": List[int]})


def suggest_modified_terrain(
    terrain: Terrain,
    terrain_type: str,
    params: Dict[str, Any],
) -> ModifiedSuggestion[Terrain]:
    logging.debug(f"Suggesting for terrain: {terrain_type}")

    suggestion: Terrain = {
        "build_cost": params[f"{terrain_type}_build_cost"],
        "revenue": get_revenue_from_params(terrain_type, params),
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

    weight = 3.0 if terrain_type == "plain" else 1
    small_loss = (
        bias_small_loss(fmean(suggestion["revenue"]))
        + bias_small_loss(suggestion["build_cost"])
    ) * weight

    return ModifiedSuggestion(suggestion, diff, small_loss)


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
    feature: Feature, params: Dict[str, Any]
) -> ModifiedSuggestion[Feature]:
    logging.debug(f"Suggesting for feature: {feature['location_name']}")

    suggestion: Feature = {
        "feature_type": feature["feature_type"],
        "location_name": feature["location_name"],
        "revenue": get_revenue_from_params(feature["location_name"], params),
        "additional_cost": params[f"{feature['location_name']}_additional_cost"],
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

    feature_type = feature["feature_type"]
    if feature_type == "town":
        weight = 1.0
    elif feature_type == "port":
        weight = 0.75
    else:
        weight = 0.5

    small_loss = (
        bias_small_loss(fmean(suggestion["revenue"]))
        + bias_small_loss(suggestion["additional_cost"])
    ) * weight

    return ModifiedSuggestion(suggestion, diff, small_loss)


Bond = TypedDict("Bond", {"face_value": int, "coupon": int})


def suggest_bonds(
    original: List[Bond], params: Dict[str, Any]
) -> ModifiedSuggestion[List[Bond]]:
    """
    Ignoring bonds for now as per user request.
    This function will just return the original bonds.
    """
    logging.debug("Suggesting bonds (ignored, returning original)")
    return ModifiedSuggestion(original, 0.0, 0.0)


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
    company_id: str,
    company_detail: CompanyFixedDetail,
    params: Dict[str, Any],
) -> ModifiedSuggestion[CompanyFixedDetail]:
    logging.debug(f"Suggesting for company: {company_id}")

    suggestion: CompanyFixedDetail = {
        "private": company_detail["private"],
        "starting": company_detail["starting"],
        "initial_treasury": (
            params[f"{company_id}_initial_treasury"]
            if company_detail["private"]
            else 0
        ),
        "initial_interest": (
            params[f"{company_id}_initial_interest"]
            if company_detail["private"]
            else 0
        ),
        "stock_available": (
            1
            if company_detail["private"]
            else params[f"{company_id}_stock_available"]
        ),
        "track_available": params[f"{company_id}_track_available"],
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
    diff = fmean(diffs) if diffs else 0

    small_loss = bias_small_loss(suggestion["track_available"])
    if suggestion["private"]:
        small_loss += bias_small_loss(suggestion["initial_interest"] * 2)
        small_loss += bias_small_loss(suggestion["initial_treasury"])
    else:
        small_loss += bias_small_loss(suggestion["stock_available"])

    return ModifiedSuggestion(suggestion, diff, small_loss)


def suggest_initial_cash(
    players: int,
    original: dict[str, int],
    params: Dict[str, Any],
) -> ModifiedSuggestion[dict[str, int]]:
    logging.debug(f"Suggesting initial cash for {players} players")
    # Diff wise, we're only changing (and caring about) the one for the current
    # amount of players.
    original_cash = original[str(players)]
    suggested_cash = params["initial_cash"]
    modified = original.copy()
    modified[str(players)] = suggested_cash

    diff = calc_norm_diff(
        original_cash, suggested_cash, MIN_INITIAL_CASH, MAX_INITIAL_CASH
    )
    small_loss = bias_small_loss(suggested_cash) * 4

    return ModifiedSuggestion(modified, diff, small_loss)


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


# Small loss is a bias towards smaller numbers.
# If each component of small loss is 1 (which means the value is 100), the total would be 31.
# See gemini_investigation.py for details
SMALL_LOSS_NORMALIZATION_FACTOR = 34.0


def suggest_for_trial(
    params: Dict[str, Any], player_count: int = 3
) -> ModifiedSuggestion[EbrHyperparams]:
    hyperparams: EbrHyperparams = mon2y.default_hyperparams(mon2y.Games.EBR)

    diffs = {}
    small_losses = []

    terrain_diff_sum = 0
    for terrain_type, terrain in hyperparams["terrain_attributes"].items():
        if terrain_type in ("town", "port", "nothing"):
            continue
        suggested = suggest_modified_terrain(terrain, terrain_type, params)
        terrain_diff_sum += suggested.difference
        hyperparams["terrain_attributes"][terrain_type] = suggested.suggestion
        small_losses.append(suggested.small_loss)
    diffs["terrain_diff"] = (
        terrain_diff_sum / len(hyperparams["terrain_attributes"]),
        1,
    )

    feature_diff_sum = 0
    # A feature is a tuple of (coords, feature_details)
    for i, (coords, feature) in enumerate(hyperparams["features"]):
        suggested = suggest_modified_feature(feature, params)
        feature_diff_sum += suggested.difference
        hyperparams["features"][i] = (coords, suggested.suggestion)
        small_losses.append(suggested.small_loss)
    diffs["feature_diff"] = (feature_diff_sum / len(hyperparams["features"]), 1)

    company_diff_sum = 0
    for company_id, details in hyperparams["company_fixed_details"].items():
        suggested = suggest_modified_company_fixed_detail(
            company_id, details, params
        )
        company_diff_sum += suggested.difference
        hyperparams["company_fixed_details"][company_id] = suggested.suggestion
        small_losses.append(suggested.small_loss)

    diffs["company_diff"] = (
        (company_diff_sum / len(hyperparams["company_fixed_details"])),
        1,
    )

    suggested_bonds = suggest_bonds(hyperparams["bonds"], params)
    hyperparams["bonds"] = suggested_bonds.suggestion
    diffs["suggested_bonds_diff"] = (suggested_bonds.difference, 1)
    small_losses.append(suggested_bonds.small_loss)

    initial_cash = suggest_initial_cash(
        player_count, hyperparams["initial_cash"], params
    )
    hyperparams["initial_cash"] = initial_cash.suggestion
    diffs["initial_cash_diff"] = (initial_cash.difference, 1)
    small_losses.append(initial_cash.small_loss)

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
        suggested_value = params[key]
        hyperparams[key] = suggested_value

        diffs[f"{key}_diff"] = (
            calc_norm_diff(original_value, suggested_value, min_const, max_const),
            0.5,
        )

    diff_result = [diff[0] for diff in diffs.values()]
    diff_weight = [diff[1] for diff in diffs.values()]

    total_small_loss = sum(small_losses) / SMALL_LOSS_NORMALIZATION_FACTOR

    return ModifiedSuggestion(
        hyperparams,
        fmean(diff_result, diff_weight) if diff_result else 0.0,
        total_small_loss,
    )
    

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


_dists: Optional[Dict[str, optuna.distributions.BaseDistribution]] = None


def _rev_dist(prefix: str, min: int, max: int) -> dict[str, IntDistribution]:
    return {f"{prefix}_rev_{i}": IntDistribution(min, max) for i in range(3)}


def dists() -> Dict[str, optuna.distributions.BaseDistribution]:
    global _dists

    if _dists:
        return _dists

    simple_dists: Dict[str, BaseDistribution] = {
        "initial_cash": IntDistribution(MIN_INITIAL_CASH, MAX_INITIAL_CASH),
        "water_1_cost": IntDistribution(MIN_WATER_COST, MAX_WATER_COST),
        "water_2_cost": IntDistribution(MIN_WATER_COST, MAX_WATER_COST),
        "narrow_gauge_initial": IntDistribution(
            MIN_NARROW_GAUGE_INITIAL, MAX_NARROW_GAUGE_INITIAL
        ),
        "max_builds": IntDistribution(MIN_MAX_BUILDS, MAX_MAX_BUILDS),
        "narrow_track_cost": IntDistribution(
            MIN_NARROW_TRACK_COST, MAX_NARROW_TRACK_COST
        ),
        "take_resource_cost": IntDistribution(
            MIN_TAKE_RESOURCE_COST, MAX_TAKE_RESOURCE_COST
        ),
        "take_dividend": IntDistribution(MIN_TAKE_DIVIDEND, MAX_TAKE_DIVIDEND),
        "take_town_deliver_dividend": IntDistribution(
            MIN_TAKE_TOWN_DELIVER_DIVIDEND, MAX_TAKE_TOWN_DELIVER_DIVIDEND
        ),
        "take_port_deliver_dividend": IntDistribution(
            MIN_TAKE_PORT_DELIVER_DIVIDEND, MAX_TAKE_PORT_DELIVER_DIVIDEND
        ),
    }

    default_hp = mon2y.default_hyperparams(mon2y.Games.EBR)

    feature_dists = ChainMap(
        *[
            _rev_dist(feature['location_name'], 0, MAX_REVENUE)
            | {
                f"{feature['location_name']}_additional_cost": IntDistribution(
                    MIN_ADDITIONAL_COST, MAX_ADDITIONAL_COST
                )
            }
            for _, feature in default_hp["features"]
        ]
    )

    terrains_dists = ChainMap(
        *[
            {
                f"{terrain_type}_build_cost": IntDistribution(
                    MIN_BUILD_COST, MAX_BUILD_COST
                )
            }
            | _rev_dist(terrain_type, 0, MAX_REVENUE)
            for terrain_type, terrain in default_hp["terrain_attributes"].items()
            if terrain_type not in ["town", "port", "nothing"]
        ]
    )

    company_dists = ChainMap(
        *[
            (
                {
                    f"{company_id}_track_available": IntDistribution(
                        MIN_TRACK_AVAILABLE, MAX_TRACK_AVAILABLE
                    ),
                }
                | (
                    {
                        f"{company_id}_initial_treasury": IntDistribution(
                            MIN_PRIVATE_INITIAL_TREASURY,
                            MAX_PRIVATE_INITIAL_TREASURY,
                        ),
                        f"{company_id}_initial_interest": IntDistribution(
                            MIN_PRIVATE_INITIAL_COUPON, MAX_PRIVATE_INITIAL_COUPON
                        ),
                    }
                    if detail["private"]
                    else {
                        f"{company_id}_stock_available": IntDistribution(
                            MIN_STOCK_AVAILABLE, MAX_STOCK_AVAILABLE
                        ),
                    }
                )
            )
            for company_id, detail in default_hp["company_fixed_details"].items()
        ]
    )

    bond_dists: Mapping[str, BaseDistribution] = {
        "bond_count": IntDistribution(MIN_BOND_COUNT, MAX_BOND_COUNT),
        "max_bond_face": IntDistribution(MIN_BOND_FACE, MAX_BOND_FACE),
    } | {
        f"bond_{i:0{len(str(MAX_BOND_COUNT))}}_{key}": FloatDistribution(0, 1)
        for i in range(MAX_BOND_COUNT)
        for key in ("coupon_ratio", "face")
    }

    _dists = dict(
        simple_dists | bond_dists | company_dists | terrains_dists | feature_dists
    )
    return _dists


def start_trial(
    params: Dict[str, Any],
    threads: Optional[int] = None,
    trials: Optional[int] = None,
    max_iterations: Optional[int] = None,
    force_iterations: Optional[int] = None,
    player_count: int = 3,
) -> Dict[str, Any]:
    logging.debug("Starting run trial")
    trials = trials or TRIALS
    max_iterations = max_iterations or MAX_ITERATIONS
    explore_iterations = force_iterations or MIN_ITERATIONS
    logging.info(f"Iterations: {explore_iterations}")

    logging.debug("Suggesting hyperparams")
    suggested_hyperparams = suggest_for_trial(params, player_count=player_count)

    logging.debug("Starting explore")
    raw_results = mon2y.explore(
        mon2y.Games.EBR,
        int(explore_iterations),
        threads or min([CPU_COUNT, MAX_THREADS]),
        hyperparams=suggested_hyperparams.suggestion,
        player_count=player_count,
    )
    logging.info("Explore Done - %s results", (len(raw_results),))
    
    results: Dict[str, Any] = {"iterations": len(raw_results)}

    df = pd.DataFrame(raw_results)
    logging.info("Dataframe Converted - %s items", (len(df),))

    # Currently using the top std-dev of trusted results for calculations
    # Desired improvement is to weight each result by trust instead
    trusted = most_trusted_hyperrewards(df)

    # Scalars is separate so they can be stored without recalculating
    logging.info("Trusted entries %s", (len(trusted),))

    losses = [
        suggested_hyperparams.difference * DIFF_WEIGHT,
        suggested_hyperparams.small_loss * SMALL_LOSS_WEIGHT,
    ]
    results["diff_loss"] = suggested_hyperparams.difference
    results["small_loss"] = suggested_hyperparams.small_loss

    for goal in GOALS.values():
        goal_scalars = {
            goal_name: goal.scalarize(trusted, suggested_hyperparams.suggestion)
            for goal_name, goal in goal.items()
        }
        for goal_name, scalar in goal_scalars.items():
            results[f"{goal_name}"] = scalar
        goals_loss = {
            goal_name: goal.loss(goal_scalars[goal_name])
            for goal_name, goal in goal.items()
        }

        losses.append(sum(goals_loss.values()))

    results["total_loss"] = sum(losses)
    results["losses"] = losses
    return results
