from statistics import fmean
from typing import List, TypedDict, NamedTuple, TypeVar, Generic, Tuple

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

MIN_INITIAL_CASH = 1
MAX_INITIAL_CASH = 60

# Fixing player count, initially
PLAYER_COUNT = 3

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
        calc_norm_diff(original_cash, suggested_cash, MIN_INITIAL_CASH, MAX_INITIAL_CASH),
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
    hyperparams: EbrHyperparams = mon2y.default_hyperparams[mon2y.Games.EBR]

    diffs = []

    terrain_diff_sum = 0
    for terrain_type, terrain in hyperparams["terrain_attributes"].items():
        suggested = suggest_modified_terrain(terrain, terrain_type, trial)
        terrain_diff_sum += suggested.difference
        hyperparams["terrain_attributes"][terrain_type] = suggested.suggestion
    diffs.append((terrain_diff_sum / len(hyperparams["terrain_attributes"]), 1))

    feature_diff_sum = 0
    # A feature is a tuple of (coords, feature_details)
    for i, (coords, feature) in enumerate(hyperparams["features"]):
        suggested = suggest_modified_feature(feature, trial)
        feature_diff_sum += suggested.difference
        hyperparams["features"][i] = (coords, suggested.suggestion)
    diffs.append((feature_diff_sum / len(hyperparams["features"]), 1))

    company_diff_sum = 0
    for company_id, details in hyperparams["company_fixed_details"].items():
        suggested = suggest_modified_company_fixed_detail(company_id, details, trial)
        company_diff_sum += suggested.difference
        hyperparams["company_fixed_details"][company_id] = suggested.suggestion
    diffs.append((company_diff_sum / len(hyperparams["company_fixed_details"])), 1)

    suggested_bonds = suggest_bonds(hyperparams["bonds"], trial)
    hyperparams["bonds"] = suggested_bonds.suggestion
    diffs.append((suggested_bonds.difference, 1))

    initial_cash = suggest_initial_cash(PLAYER_COUNT, hyperparams["initial_cash"], trial)
    hyperparams["initial_cash"] = initial_cash.suggestion
    diffs.append((initial_cash.difference, 1))

    return ModifiedSuggestion(hyperparams, fmean(diffs))
