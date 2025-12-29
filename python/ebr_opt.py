from typing import List, TypedDict

import optuna

import mon2y

MAX_REV = 50

MAX_BOND_COUNT = 50
MIN_BOND_FACE_STEP = 0
MAX_BOND_FACE_STEP = 10
MIN_BOND_COUPON_STEP = 0
MAX_BOND_COUPON_STEP = 10

def suggest_revenue(feature: str, trial: optuna.Trial) -> List[int]:
    return [trial.suggest_int(f"feature_rev_rnd_{i}", 0, 50) for i in range(0, 6)]

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

