from typing import List, TypedDict, NamedTuple, TypeVar, Generic

import optuna

import mon2y

MAX_REVENUE = 50

MIN_BUILD_COST = 0
MAX_BUILD_COST = 20

MIN_ADDITIONAL_COST = 0
MAX_ADDITIONAL_COST = 20

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
    difference: float


def suggest_revenue(feature: str, trial: optuna.Trial) -> List[int]:
    # Revenue goes for two rounds each time
    return [
        trial.suggest_int(f"{feature}_rev_rnd_{i}", 0, MAX_REVENUE)
        for i in range(0, 3)
        for _ in (0, 1)
    ]


Terrain = TypedDict("Terrain", {"build_cost": int, "revenue": List[int]})


def suggest_modified_terrain(
    terrain: Terrain, terrain_type: str, trial: optuna.Trial
) -> Terrain:
    return {
        "build_cost": trial.suggest_int(
            f"{terrain_type}_build_cost", MIN_BUILD_COST, MAX_BUILD_COST
        ),
        "revenue": suggest_revenue(terrain_type, trial),
    }


Feature = TypedDict(
    "Feature",
    {
        "feature_type": str,
        "location_name": str,
        "revenue": List[int],
        "additional_cost": int,
    },
)


def suggest_modified_feature(feature: Feature, trial: optuna.Trial) -> Feature:
    return {
        "feature_type": feature["feature_type"],
        "location_name": feature["location_name"],
        "revenue": suggest_revenue(feature["location_name"], trial),
        "additional_cost": trial.suggest_int(
            f"{feature}_additional_cost", MIN_ADDITIONAL_COST, MAX_ADDITIONAL_COST
        ),
    }


Bond = TypedDict("Bond", {"face_value": int, "coupon": int})


def suggest_bonds(trial: optuna.Trial) -> List[Bond]:
    bond_count = trial.suggest_int("bond_count", 1, 50)

    bonds: List[Bond] = []
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
        bonds.append({"face_value": face, "coupon": coupon})

    return bonds


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
