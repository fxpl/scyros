import math


# Parameters of every kind
def scale(self, a, b: float, c=1.0, d: float = 2.0, *args, e, **kwargs):
    # sqrt in a comment is not counted
    return float(a) * b


def ratio(a, /, b, *, c):
    """sqrt in a docstring is a string"""
    return math.sqrt(a / b)


def loops(values):
    total = 0.0
    for v in values:
        total += v
    while total > 100:
        total = total / 2
    if total < 0:
        total = -total
    result = total if total > 1 else float(1)
    match result:
        case 0:
            return 0.0
        case _:
            return math.sqrt(result)


half = lambda x, y=2: float(x) / y
