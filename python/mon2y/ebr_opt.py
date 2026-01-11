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


def suggest_fixed(trial: optuna.Trial, name: str, value: Any, *args) -> Any:
    """Suggests a fixed value for a parameter."""
    if isinstance(value, int):
        return trial.suggest_int(name, value, value)
    elif isinstance(value, float):
        return trial.suggest_float(name, value, value)
    elif isinstance(value, str):
        return trial.suggest_categorical(name, [value])
    else:
        raise TypeError(f"Unsupported type for suggest_fixed: {type(value)}")


def suggest_revenue(
    feature: str,
    trial: optuna.Trial,
    use_defaults: bool = False,
    default: Optional[List[int]] = None,
) -> List[int]:
    logging.debug(f"Suggesting revenue for {feature}")
    if use_defaults:
        assert default is not None
        return [
            suggest_fixed(trial, f"{feature}_rev_rnd_{i}", default[i * 2])
            for i in range(0, 3)
            for _ in (0, 1)
        ]
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
    terrain: Terrain,
    terrain_type: str,
    trial: optuna.Trial,
    use_defaults: bool = False,
) -> ModifiedSuggestion[Terrain]:
    logging.debug(f"Suggesting for terrain: {terrain_type}")
    s_int = partial(suggest_fixed, trial) if use_defaults else trial.suggest_int

    suggestion: Terrain = {
        "build_cost": s_int(
            f"{terrain_type}_build_cost",
            terrain["build_cost"] if use_defaults else MIN_BUILD_COST,
            terrain["build_cost"] if use_defaults else MAX_BUILD_COST,
        ),
        "revenue": [trial.suggest_int(f"{terrain_type}_rev", 0, MAX_REVENUE)] * 6,
    }

    if use_defaults:
        return ModifiedSuggestion(suggestion, 0.0, 0.0)

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
    feature: Feature, trial: optuna.Trial, use_defaults: bool = False
) -> ModifiedSuggestion[Feature]:
    logging.debug(f"Suggesting for feature: {feature['location_name']}")
    s_int = partial(suggest_fixed, trial) if use_defaults else trial.suggest_int

    suggestion: Feature = {
        "feature_type": feature["feature_type"],
        "location_name": feature["location_name"],
        "revenue": suggest_revenue(
            feature["location_name"], trial, use_defaults, feature["revenue"]
        ),
        "additional_cost": s_int(
            f"{feature["location_name"]}_additional_cost",
            feature["additional_cost"] if use_defaults else MIN_ADDITIONAL_COST,
            feature["additional_cost"] if use_defaults else MAX_ADDITIONAL_COST,
        ),
    }

    if use_defaults:
        return ModifiedSuggestion(suggestion, 0.0, 0.0)

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
    original: List[Bond], trial: optuna.Trial, use_defaults: bool = False
) -> ModifiedSuggestion[List[Bond]]:
    logging.debug("Suggesting bonds")
    s_int = partial(suggest_fixed, trial) if use_defaults else trial.suggest_int

    if use_defaults:
        bond_count = s_int("bond_count", len(original), len(original))
        suggested: List[Bond] = []
        for i in range(bond_count):
            face = s_int(f"bond_face_{i}", original[i]["face_value"])
            coupon = s_int(f"bond_coupon_{i}", original[i]["coupon"])
            suggested.append({"face_value": face, "coupon": coupon})
        return ModifiedSuggestion(suggested, 0.0, 0.0)

    # `use_defaults` is false from here
    bond_count = trial.suggest_int("bond_count", MIN_BOND_COUNT, MAX_BOND_COUNT)

    min_face = trial.suggest_int("min_bond_face", MIN_BOND_FACE, MAX_BOND_FACE)
    max_face = trial.suggest_int("max_bond_face", min_face, MAX_BOND_FACE)

    suggested: List[Bond] = []
    for i in range(bond_count):
        face = trial.suggest_int(f"bond_face_{i}", min_face, max_face)
        min_coupon = max(0, math.ceil(face * MIN_BOND_COUPON_RATIO))
        max_coupon = max(min_coupon, math.floor(face * MAX_BOND_COUPON_RATIO))
        coupon = trial.suggest_int(f"bond_coupon_{i}", int(min_coupon), int(max_coupon))
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

    original_faces = [bond["face_value"] for bond in original]
    original_max_face = max(original_faces) if original_faces else 0
    original_min_face = min(original_faces) if original_faces else 0

    suggested_faces = [bond["face_value"] for bond in suggested]
    suggested_max_face = max(suggested_faces) if suggested_faces else 0
    suggested_min_face = min(suggested_faces) if suggested_faces else 0

    max_face_diff = calc_norm_diff(
        original_max_face,
        suggested_max_face,
        MIN_BOND_FACE,
        MAX_BOND_FACE,
    )

    min_face_diff = calc_norm_diff(
        original_min_face,
        suggested_min_face,
        MIN_BOND_FACE,
        MAX_BOND_FACE,
    )

    original_ratios = [
        b["coupon"] / b["face_value"] for b in original if b["face_value"] > 0
    ]
    suggested_ratios = [
        b["coupon"] / b["face_value"] for b in suggested if b["face_value"] > 0
    ]

    ratio_diff = calc_norm_diff(
        fmean(original_ratios) if original_ratios else 0,
        fmean(suggested_ratios) if suggested_ratios else 0,
        0,
        1,
    )

    diff = (
        count_diff * (1 / 3)
        + max_face_diff * (1 / 6)
        + min_face_diff * (1 / 6)
        + ratio_diff * (1 / 3)
    )

    if not suggested:
        small_loss = 0
    else:
        avg_face = fmean([b["face_value"] for b in suggested])
        avg_coupon = fmean([b["coupon"] for b in suggested])
        small_loss = bias_small_loss(avg_face) + bias_small_loss(avg_coupon * 3)

    return ModifiedSuggestion(suggested, diff, small_loss)


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
    trial: optuna.Trial,
    use_defaults: bool = False,
) -> ModifiedSuggestion[CompanyFixedDetail]:
    logging.debug(f"Suggesting for company: {company_id}")
    s_int = partial(suggest_fixed, trial) if use_defaults else trial.suggest_int

    suggestion: CompanyFixedDetail = {
        "private": company_detail["private"],
        "starting": company_detail["starting"],
        "initial_treasury": (
            s_int(
                f"{company_id}_initial_treasury",
                (
                    company_detail["initial_treasury"]
                    if use_defaults
                    else MIN_PRIVATE_INITIAL_TREASURY
                ),
                (
                    company_detail["initial_treasury"]
                    if use_defaults
                    else MAX_PRIVATE_INITIAL_TREASURY
                ),
            )
            if company_detail["private"]
            else 0
        ),
        "initial_interest": (
            s_int(
                f"{company_id}_initial_interest",
                (
                    company_detail["initial_interest"]
                    if use_defaults
                    else MIN_PRIVATE_INITIAL_COUPON
                ),
                (
                    company_detail["initial_interest"]
                    if use_defaults
                    else MAX_PRIVATE_INITIAL_COUPON
                ),
            )
            if company_detail["private"]
            else 0
        ),
        "stock_available": (
            1
            if company_detail["private"]
            else s_int(
                f"{company_id}_stock_available",
                (
                    company_detail["stock_available"]
                    if use_defaults
                    else MIN_STOCK_AVAILABLE
                ),
                (
                    company_detail["stock_available"]
                    if use_defaults
                    else MAX_STOCK_AVAILABLE
                ),
            )
        ),
        "track_available": s_int(
            f"{company_id}_track_available",
            company_detail["track_available"] if use_defaults else MIN_TRACK_AVAILABLE,
            company_detail["track_available"] if use_defaults else MAX_TRACK_AVAILABLE,
        ),
    }

    if use_defaults:
        return ModifiedSuggestion(suggestion, 0.0, 0.0)

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
    trial: optuna.Trial,
    use_defaults: bool = False,
) -> ModifiedSuggestion[dict[str, int]]:
    logging.debug(f"Suggesting initial cash for {players} players")
    s_int = partial(suggest_fixed, trial) if use_defaults else trial.suggest_int
    # Diff wise, we're only changing (and caring about) the one for the current
    # amount of players.
    original_cash = original[str(players)]
    suggested_cash = s_int(
        f"initial_cash_{players}p",
        original_cash if use_defaults else MIN_INITIAL_CASH,
        original_cash if use_defaults else MAX_INITIAL_CASH,
    )
    modified = original.copy()
    modified[str(players)] = suggested_cash

    if use_defaults:
        return ModifiedSuggestion(modified, 0.0, 0.0)

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
    trial: optuna.Trial, use_defaults: bool = False, player_count: int = 3
) -> ModifiedSuggestion[EbrHyperparams]:
    hyperparams: EbrHyperparams = mon2y.default_hyperparams(mon2y.Games.EBR)
    s_int = partial(suggest_fixed, trial) if use_defaults else trial.suggest_int

    diffs = {}
    small_losses = []

    terrain_diff_sum = 0
    for terrain_type, terrain in hyperparams["terrain_attributes"].items():
        if terrain_type in ("town", "port", "nothing"):
            continue
        suggested = suggest_modified_terrain(terrain, terrain_type, trial, use_defaults)
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
        suggested = suggest_modified_feature(feature, trial, use_defaults)
        feature_diff_sum += suggested.difference
        hyperparams["features"][i] = (coords, suggested.suggestion)
        small_losses.append(suggested.small_loss)
    diffs["feature_diff"] = (feature_diff_sum / len(hyperparams["features"]), 1)

    company_diff_sum = 0
    # We're calculating the average of private company losses, so we need to collect them
    private_company_losses = []
    for company_id, details in hyperparams["company_fixed_details"].items():
        suggested = suggest_modified_company_fixed_detail(
            company_id, details, trial, use_defaults
        )
        company_diff_sum += suggested.difference
        hyperparams["company_fixed_details"][company_id] = suggested.suggestion

        # The small loss for companies is a bit more complex.
        # From the prompt:
        # - Sum of (small loss of stock available for all non private companies)
        # - Sum of (small loss of track available for all companies)
        # - Small loss of (average of all private company initial interest * 2)
        # - Small loss of (average of all private company initial treasury)
        # The suggest_modified_company_fixed_detail returns the loss for a single company.
        # I will sum them here.
        small_losses.append(suggested.small_loss)

    diffs["company_diff"] = (
        (company_diff_sum / len(hyperparams["company_fixed_details"])),
        1,
    )

    suggested_bonds = suggest_bonds(hyperparams["bonds"], trial, use_defaults)
    hyperparams["bonds"] = suggested_bonds.suggestion
    diffs["suggested_bonds_diff"] = (suggested_bonds.difference, 1)
    small_losses.append(suggested_bonds.small_loss)

    initial_cash = suggest_initial_cash(
        player_count, hyperparams["initial_cash"], trial, use_defaults
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
        suggested_value = s_int(
            key,
            original_value if use_defaults else min_const,
            original_value if use_defaults else max_const,
        )
        hyperparams[key] = suggested_value

        if not use_defaults:
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
        "take_town_deliver_dividend": IntDistribution(
            MIN_TAKE_TOWN_DELIVER_DIVIDEND, MAX_TAKE_TOWN_DELIVER_DIVIDEND
        ),
        "take_port_deliver_dividend": IntDistribution(
            MIN_TAKE_PORT_DELIVER_DIVIDEND, MAX_TAKE_PORT_DELIVER_DIVIDEND
        ),
    }

    default_hp = mon2y.default_hyperparams(mon2y.Games.EBR)

    features = default_hp["features"]
    terrain_types = default_hp["terrain_attributes"].keys()

    private_companies = [
        key
        for key, detail in default_hp["company_fixed_details"].items()
        if not detail["private"]
    ]

    company_dists = ChainMap(
        [
            (
                {
                    f"public_{company_id}_track_available": IntDistribution(
                        MIN_TRACK_AVAILABLE, MAX_TRACK_AVAILABLE
                    ),
                }
                | {
                    f"public_{company_id}_initial_treasury": IntDistribution(
                        MIN_PRIVATE_INITIAL_TREASURY, MAX_PRIVATE_INITIAL_TREASURY
                    ),
                    f"public_{company_id}_initial_coupon": IntDistribution(
                        MIN_PRIVATE_INITIAL_COUPON, MAX_PRIVATE_INITIAL_COUPON
                    ),
                }
                if detail.private
                else {
                    f"public_{company_id}_stock_available": IntDistribution(
                        MIN_STOCK_AVAILABLE, MAX_STOCK_AVAILABLE
                    ),
                }
            )
            for company_id, detail in default_hp["company_fixed_details"].items()
            if not detail["private"]
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

    return simple_dists | bond_dists | company_dists


def run_trial(
    trial: optuna.Trial,
    threads: Optional[int] = None,
    trials: Optional[int] = None,
    max_iterations: Optional[int] = None,
    force_iterations: Optional[int] = None,
    single: bool = False,
    use_defaults: bool = False,
    player_count: int = 3,
):
    logging.debug("Starting run trial")
    trials = trials or TRIALS
    max_iterations = max_iterations or MAX_ITERATIONS
    explore_iterations = force_iterations or max_iterations * (
        1 - (math.log(max(1, trials - max(trial.number, 2))) / math.log(trials))
    )
    explore_iterations = max(explore_iterations, MIN_ITERATIONS)
    logging.info(f"Iterations: {explore_iterations}")

    logging.debug("Suggesting hyperparams")
    suggested_hyperparams = suggest_for_trial(
        trial, use_defaults=use_defaults, player_count=player_count
    )

    logging.debug("Starting explore")
    raw_results = mon2y.explore(
        mon2y.Games.EBR,
        int(explore_iterations),
        threads or min([CPU_COUNT, MAX_THREADS]),
        hyperparams=suggested_hyperparams.suggestion,
        player_count=player_count,
    )
    logging.info("Explore Done - %s results", (len(raw_results),))
    trial.set_user_attr(f"iterations", len(raw_results))

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
    trial.set_user_attr(f"diff_loss", suggested_hyperparams.difference)
    trial.set_user_attr(f"small_loss", suggested_hyperparams.small_loss)

    for goal in GOALS.values():
        goal_scalars = {
            goal_name: goal.scalarize(trusted, suggested_hyperparams.suggestion)
            for goal_name, goal in goal.items()
        }
        for goal_name, scalar in goal_scalars.items():
            trial.set_user_attr(f"{goal_name}", scalar)
        goals_loss = {
            goal_name: goal.loss(goal_scalars[goal_name])
            for goal_name, goal in goal.items()
        }

        losses.append(sum(goals_loss.values()))

    return losses if not single else (sum(losses))


def start_study(
    worker_idx: int,
    study_name: str,
    n_trials: int,
    storage: str,
    threads: int,
    single: bool,
    include_first: bool,
    player_count: int,
    force_iterations: Optional[int],
):
    """Start a study, potentially running the first trial with defaults."""
    global EBR_META
    if EBR_META is None:
        logging.debug("Populating EbrMeta")
        hyperparams = mon2y.default_hyperparams(mon2y.Games.EBR)
        terrain_type_keys = [
            key
            for key in hyperparams["terrain_attributes"].keys()
            if key not in ("town", "port", "nothing")
        ]
        feature_keys = [
            feature["location_name"] for _, feature in hyperparams["features"]
        ]
        private_company_keys = [
            cid
            for cid, details in hyperparams["company_fixed_details"].items()
            if details["private"]
        ]
        public_company_keys = [
            cid
            for cid, details in hyperparams["company_fixed_details"].items()
            if not details["private"]
        ]
        EBR_META = EbrMeta(
            terrain_type_keys=terrain_type_keys,
            feature_keys=feature_keys,
            private_company_keys=private_company_keys,
            public_company_keys=public_company_keys,
        )

    logging.debug("Starting study")
    if single:
        study = optuna.create_study(
            storage=storage,
            study_name=study_name,
            direction="minimize",
            load_if_exists=True,
        )
    else:
        study = optuna.create_study(
            storage=storage,
            study_name=study_name,
            directions=["minimize"] * (2 + len(GOALS)),
            load_if_exists=True,
        )

    if include_first and worker_idx == 0:
        # This worker will run the default trial first
        # Only run if no trials exist
        if len(study.get_trials(deepcopy=False)) == 0:
            trial = study.ask()
            try:
                result = run_trial(
                    trial,
                    threads=threads,
                    single=single,
                    trials=n_trials,
                    use_defaults=True,
                    player_count=player_count,
                    force_iterations=force_iterations,
                )
                study.tell(trial, result)
            except Exception:
                # If the default trial fails, we still want to continue with the study
                study.tell(trial, state=optuna.trial.TrialState.FAIL)
            logging.info("Default trial complete")

    for _ in range(n_trials):
        trial = study.ask()
        try:
            result = run_trial(
                trial,
                threads=threads,
                single=single,
                trials=n_trials,
                use_defaults=False,
                player_count=player_count,
                force_iterations=force_iterations,
            )
            study.tell(trial, result)
        except Exception:
            study.tell(trial, state=optuna.trial.TrialState.FAIL)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Optimize EBR hyperparameters.")
    parser.add_argument(
        "-v",
        "--verbose",
        action="count",
        default=0,
        help="Increase verbosity level: -v for INFO, -vv for DEBUG.",
    )
    parser.add_argument(
        "--processes", type=int, default=CPU_COUNT, help="Number of processes to use."
    )
    parser.add_argument(
        "--threads",
        type=int,
        default=MAX_THREADS,
        help="Number of threads per process.",
    )
    parser.add_argument(
        "--single-study", action="store_true", help="Run a single-objective study."
    )
    parser.add_argument(
        "--study-name", type=str, default="ebr_study_4", help="Name of the study."
    )
    parser.add_argument(
        "--n-trials", type=int, default=1000, help="Number of trials to run."
    )
    parser.add_argument(
        "--storage",
        type=str,
        default="sqlite:///db.sqlite3",
        help="Database storage URL.",
    )
    parser.add_argument(
        "--include-first",
        action="store_true",
        help="Include a first trial with default settings.",
    )
    parser.add_argument(
        "--player-count",
        type=int,
        default=3,
        help="Number of players in the game.",
    )
    parser.add_argument(
        "--force-iterations",
        type=int,
        help="Force the number of iterations for each trial. Useful for debugging.",
    )

    args = parser.parse_args()

    # Configure logging
    if args.verbose == 0:
        log_level = logging.WARNING
    elif args.verbose == 1:
        log_level = logging.INFO
    else:  # >= 2
        log_level = logging.DEBUG

    logging.basicConfig(
        format="%(asctime)s %(levelname)s %(message)s",
        datefmt="%Y-%m-%d %H:%M:%S",
        level=log_level,
    )

    study_name = args.study_name if args.single_study else f"{args.study_name}_multi"

    runner = partial(
        start_study,
        study_name=study_name,
        n_trials=args.n_trials,
        storage=args.storage,
        threads=args.threads,
        single=args.single_study,
        include_first=args.include_first,
        player_count=args.player_count,
        force_iterations=args.force_iterations,
    )

    if args.processes > 1:
        with Pool(processes=args.processes) as pool:
            pool.map(runner, range(args.processes))
    else:
        runner(0)
