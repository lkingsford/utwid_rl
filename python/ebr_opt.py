from typing import List, TypedDict

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

def suggest_revenue(feature: str, trial: optuna.Trial) -> List[int]:
    # Revenue goes for two rounds each time
    return [trial.suggest_int(f"{feature}_rev_rnd_{i}", 0, MAX_REVENUE) for i in range(0, 3) for _ in (0, 1)]

Terrain = TypedDict("Terrain", {"build_cost": int, "revenue": List[int]})

def suggest_terrain(terrain: str, trial: optuna.Trial) -> Terrain:
    return {
        "build_cost": trial.suggest_int(f"{terrain}_build_cost", MIN_BUILD_COST, MAX_BUILD_COST),
        "revenue": suggest_revenue(terrain, trial)
    }

Feature = TypedDict("Feature", {"feature_type": str, "location_name": str, "revenue": List[int], "additional_cost": int})

def suggest_modified_feature(feature: Feature, trial: optuna.Trial) -> Feature:
    return {
        "feature_type": feature["feature_type"],
        "location_name": feature["location_name"],
        "revenue": suggest_revenue(feature["location_name"], trial),
        "additional_cost": trial.suggest_int(f"{feature}_additional_cost", MIN_ADDITIONAL_COST, MAX_ADDITIONAL_COST)
    }

Bond = TypedDict("Bond", {"face_value": int, "coupon": int})

def suggest_bonds(trial: optuna.Trial) -> List[Bond]:
    bond_count = trial.suggest_int("bond_count", 1, 50)
    
    bonds: List[Bond] = []
    face = 0
    coupon = 0
    for i in range(bond_count):
        face = trial.suggest_int(f"bond_face_{i}", face + MIN_BOND_FACE_STEP, face + MAX_BOND_FACE_STEP)
        coupon = trial.suggest_int(f"bond_coupon_{i}", coupon + MIN_BOND_COUPON_STEP, coupon + MAX_BOND_COUPON_STEP)
        bonds.append(
            {
                "face_value": face,
                "coupon": coupon
            }
        )

    return bonds

CompanyFixedDetail = TypedDict("CompanyFixedDetail", {
    "starting": List[int],
    "private": bool,
    "stock_available": int,
    "track_available": int,
    "initial_treasury": int,
    "initial_interest": int})



