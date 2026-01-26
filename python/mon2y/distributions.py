from typing import NamedTuple, List, Any, Optional, Union


class IntDistribution(NamedTuple):
    """
    A discrete uniform distribution on a given integer range.

    Attributes:
        low: The lower bound of the range.
        high: The upper bound of the range.
        step: The step of the range.
        log: If True, the distribution is sampled in the log domain.
    """

    low: int
    high: int
    step: int = 1
    log: bool = False


class FloatDistribution(NamedTuple):
    """
    A uniform distribution on a given floating-point range.

    Attributes:
        low: The lower bound of the range.
        high: The upper bound of the range.
        step: The step of the range. If None, the range is continuous.
        log: If True, the distribution is sampled in the log domain.
    """

    low: float
    high: float
    step: Optional[float] = None
    log: bool = False


class CategoricalDistribution(NamedTuple):
    """
    A categorical distribution.

    Attributes:
        choices: The list of possible values.
    """

    choices: List[Any]


type Distribution = Union[IntDistribution, FloatDistribution, CategoricalDistribution]
