import mon2y
import pytest

def test_c4_hyperparams_meta():
    meta = mon2y.get_hyperparam_meta(mon2y.Games.C4)
    assert "board_width" in meta
    bw_meta = meta["board_width"]
    assert bw_meta["default"]["type"] == "uint"
    assert bw_meta["default"]["value"] == 7
    assert bw_meta["range"] is None
    
    assert "board_height" in meta
    bh_meta = meta["board_height"]
    assert bh_meta["default"]["type"] == "uint"
    assert bh_meta["default"]["value"] == 6

def test_c4_explore_with_hyperparams():
    hyperparams = {"board_width": 8, "board_height": 7}
    # This test just checks that the call doesn't crash.
    mon2y.explore(
        mon2y.Games.C4,
        iterations=10,
        thread_count=1,
        hyperparams=hyperparams
    )
