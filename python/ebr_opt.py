from statistics import fmean
from typing import List, TypedDict, NamedTuple, TypeVar, Generic

import optuna

import mon2y

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

T = TypeVar("T")

class ModifiedSuggestion(Generic[T], NamedTuple):
    suggestion: T
    """Suggested result"""

    difference: float
    """OK - difference here is really 'normalized value from 0-1' that I can use to try to use to tend towards the original values"""



def calc_norm_diff(original, new, min, max) -> float:
    max_diff = max(abs(original - min), abs(original - max))
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

    diff = (diff_revenue(terrain["revenue"], suggestion["revenue"]) + calc_norm_diff(terrain["build_cost"], suggestion["build_cost"], MIN_BUILD_COST, MAX_BUILD_COST))/2

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


def suggest_modified_feature(feature: Feature, trial: optuna.Trial) -> ModifiedSuggestion[Feature]:
    suggestion: Feature = {
        "feature_type": feature["feature_type"],
        "location_name": feature["location_name"],
        "revenue": suggest_revenue(feature["location_name"], trial),
        "additional_cost": trial.suggest_int(
            f"{feature["location_name"]}_additional_cost", MIN_ADDITIONAL_COST, MAX_ADDITIONAL_COST
        ),
    }
    diff = (diff_revenue(feature["revenue"], suggestion["revenue"]) +
            calc_norm_diff(feature["additional_cost"], suggestion["additional_cost"], MIN_ADDITIONAL_COST, MAX_ADDITIONAL_COST))/2

    return ModifiedSuggestion(suggestion, diff)

Bond = TypedDict("Bond", {"face_value": int, "coupon": int})


def suggest_bonds(original: List[Bond], trial: optuna.Trial) -> ModifiedSuggestion[List[Bond]]:
    bond_count = trial.suggest_int("bond_count", MIN_BOND_COUNT, MAX_BOND_COUNT)

    suggested: List[Bond] = []
    face = 0
    coupon = 0
    for i in range(bond_count):
        face = trial.suggest_int(
            f"bond_face_{i}", face + MIN_BOND_FACE_STEP, face + MAX_BOND_FACE_STEP
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
    count_diff = calc_norm_diff(len(original), bond_count, MIN_BOND_COUNT, MAX_BOND_COUNT)

    max_face_diff = calc_norm_diff(
            max([bond["face_value"] for bond in original]),
            max([bond["face_value"] for bond in suggested]),
            0,
            # TBD if this is the wise thing to do -
            # - but I need a way to make sure it's normalized to max of 1. This means the
            # ratio is different depending on bond_count, but I hope that's OK anyway.
            MAX_BOND_FACE_STEP * max(bond_count, len(original))
    )

    min_face_diff = calc_norm_diff(
            min([bond["face_value"] for bond in original]),
            min([bond["face_value"] for bond in suggested]),
            0,
            #... as above.
            MIN_BOND_FACE_STEP * max(bond_count, len(original))
    )

    ratio_diff = calc_norm_diff(
        fmean([bond["coupon"] / bond["face_value"] for bond in original]),
        fmean([bond["coupon"] / bond["face_value"] for bond in suggested]),
        0,
        1
    )

    diff = count_diff * (1/3) + max_face_diff * (1/6) + min_face_diff * (1/6) + ratio_diff * (1/3)

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
) -> CompanyFixedDetail:
    return {
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


def create_trial(trial):
    pass
