import mon2y
import pytest

def test_c4_hyperrewards_structure():
    results = mon2y.explore(mon2y.Games.C4, iterations=10, thread_count=1, time_limit_secs=None, exploration_constant=None)
    assert len(results) > 0
    
    for result in results:
        assert "turns" in result
        assert isinstance(result["turns"], int)
        assert "rwalk" in result
        assert isinstance(result["rwalk"], int)
        assert "first_player_won" in result
        assert isinstance(result["first_player_won"], bool)

def test_c4_hyperrewards_meta():
    meta = mon2y.get_hyperreward_meta(mon2y.Games.C4)
    assert "turns" in meta
    assert meta["turns"] == "u32"
    assert "rwalk" in meta
    assert meta["rwalk"] == "u32"
    assert "first_player_won" in meta
    assert meta["first_player_won"] == "bool"