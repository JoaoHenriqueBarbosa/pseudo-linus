"""range() aceita objeto com __index__ (start, stop, step), como o PyNumber_Index: propaga a exceção dele e recusa o
__index__ que devolve não-int."""


class Index:
    def __init__(self, value):
        self.value = value

    def __index__(self):
        return self.value


class BadIndex:
    def __index__(self):
        raise ValueError('bad index')


def show(name, call):
    try:
        print(name, call())
    except Exception as exc:
        print(name, type(exc).__name__, exc)


show('range_stop', lambda: list(range(Index(3))))
show('range_all', lambda: list(range(Index(1), Index(7), Index(2))))
show('range_repr', lambda: range(Index(1), Index(4)))
show('range_bad', lambda: range(BadIndex()))
show('range_str', lambda: range(Index('x')))
show('range_float', lambda: range(1.5))
show('range_big', lambda: len(range(Index(2 ** 40))))
show('range_bool', lambda: list(range(Index(True))))
show('range_zero_step', lambda: range(1, 2, Index(0)))
