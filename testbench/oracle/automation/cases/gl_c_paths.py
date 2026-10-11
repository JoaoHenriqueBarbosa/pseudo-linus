"""O C que cada caminho empilha até um método de usuário, lido pelo `_stack_saved`: cada caso troca de greenlet uma
vez, de dentro de um método mágico ou de um callback alcançado por uma nativa, por um slot ou por uma instrução, e a
cena imprime a pilha copiada menos o corpo do greenlet."""

import functools
import itertools
import operator
import sys

import greenlet


def build(HIT):
    class U:
        """Cada método mágico chama HIT uma vez por instância."""

        def __init__(self, v=1):
            self.v = v
            self.done = False

        def _hit(self):
            if not self.done:
                self.done = True
                HIT()

        def __str__(self): self._hit(); return 's'
        def __repr__(self): self._hit(); return 'r'
        def __format__(self, spec): self._hit(); return 'f'
        def __hash__(self): self._hit(); return 1
        def __eq__(self, o): self._hit(); return True
        def __ne__(self, o): self._hit(); return False
        def __lt__(self, o): self._hit(); return False
        def __gt__(self, o): self._hit(); return False
        def __le__(self, o): self._hit(); return True
        def __ge__(self, o): self._hit(); return True
        def __index__(self): self._hit(); return 3
        def __int__(self): self._hit(); return 3
        def __float__(self): self._hit(); return 3.0
        def __round__(self, n=None): self._hit(); return 3
        def __abs__(self): self._hit(); return 3
        def __len__(self): self._hit(); return 1
        def __bool__(self): self._hit(); return True
        def __add__(self, o): self._hit(); return 1
        def __radd__(self, o): self._hit(); return 1
        def __iadd__(self, o): self._hit(); return self
        def __sub__(self, o): self._hit(); return 1
        def __mul__(self, o): self._hit(); return 1
        def __mod__(self, o): self._hit(); return 1
        def __divmod__(self, o): self._hit(); return (1, 1)
        def __getitem__(self, k): self._hit(); return 1
        def __setitem__(self, k, v): self._hit()
        def __delitem__(self, k): self._hit()
        def __contains__(self, k): self._hit(); return True
        def __iter__(self): self._hit(); return iter([1])
        def __call__(self, *a): self._hit(); return 1
        def __getattr__(self, n): self._hit(); return 1
        def __enter__(self): self._hit(); return self
        def __exit__(self, *a): return False
        def __neg__(self): self._hit(); return 1
        def __invert__(self): self._hit(); return 1

    class Plain:
        """Só o __str__: o object.__format__ e o %s passam por ele."""
        def __str__(self): HIT(); return 'p'

    class Prop:
        @property
        def p(self): HIT(); return 1

    class Init:
        def __init__(self): HIT()

    class Nx:
        def __init__(self): self.n = 0
        def __iter__(self): return self
        def __next__(self):
            self.n += 1
            if self.n > 1:
                raise StopIteration
            HIT()
            return 1

    class Old:
        def __getitem__(self, i):
            if i > 0:
                raise IndexError
            HIT()
            return 1

    def gen():
        HIT()
        yield 1

    def outer():
        yield from gen()

    def forgen():
        for _ in gen():
            pass

    def with_():
        with U():
            pass

    def dict_store_direct():
        d = {}
        d[U()] = 1

    def setit():
        u = U()
        u[0] = 1

    def delit():
        u = U()
        del u[0]

    def iadd():
        u = U()
        u += 1

    def truth():
        return 1 if U() else 0

    def not_():
        return not U()

    def while_():
        u = U()
        while u:
            break

    def assert_():
        assert U()

    def key(x):
        HIT()
        return x

    once = [False]

    def key1(x):
        if not once[0]:
            once[0] = True
            HIT()
        return x

    def reset():
        once[0] = False

    cases = [
        # operadores e slots
        ('add', lambda: U() + 1), ('radd', lambda: 1 + U()), ('sub', lambda: U() - 1), ('mul', lambda: U() * 2),
        ('iadd', iadd), ('neg', lambda: -U()), ('invert', lambda: ~U()),
        ('eq', lambda: U() == 1), ('ne', lambda: U() != 1), ('lt', lambda: U() < 1), ('ge', lambda: U() >= 1),
        ('in_list', lambda: U() in [1]), ('in_tuple', lambda: U() in (1,)), ('contains', lambda: 1 in U()),
        ('list_count', lambda: [U()].count(1)), ('list_index', lambda: [U()].index(1)),
        ('list_remove', lambda: [U()].remove(1)), ('list_eq', lambda: [U()] == [1]), ('tuple_eq', lambda: (U(),) == (1,)),
        ('list_lt', lambda: [U()] < [2]),
        ('getitem', lambda: U()[0]), ('setitem', setit), ('delitem', delit),
        ('dict_getitem', lambda: {1: 1}.get(U())), ('dict_sub', lambda: {1: 1}[U()]),
        ('dict_store', lambda: {}.__setitem__(U(), 1)), ('dict_store_op', lambda: operator.setitem({}, U(), 1)),
        ('dict_store_direct', dict_store_direct),
        ('dict_in', lambda: U() in {1: 1}), ('dict_pop', lambda: {1: 1}.pop(U(), 0)),
        ('dict_setdefault', lambda: {}.setdefault(U(), 1)), ('dict_literal', lambda: {U(): 1}),
        ('dict_build', lambda: dict([(U(), 1)])), ('dict_del', lambda: {1: 1}.__delitem__(U())),
        ('set_add', lambda: set().add(U())), ('set_in', lambda: U() in {1}), ('set_literal', lambda: {U()}),
        ('set_build', lambda: set([U()])), ('frozenset_build', lambda: frozenset([U()])),
        ('set_discard', lambda: {1}.discard(U())), ('set_remove', lambda: {1}.remove(U())),
        ('set_union', lambda: {1}.union([U()])), ('set_issubset', lambda: {U()}.issubset({1})),
        ('hash', lambda: hash(U())),
        ('truth', truth), ('not', not_), ('while', while_), ('assert', assert_), ('bool', lambda: bool(U())),
        ('len', lambda: len(U())), ('abs', lambda: abs(U())), ('round', lambda: round(U())),
        ('int', lambda: int(U())), ('float', lambda: float(U())), ('index_list', lambda: [1, 2, 3, 4][U()]),
        ('slice', lambda: 'abcdef'[U():]), ('hex', lambda: hex(U())), ('bin', lambda: bin(U())),
        ('oct', lambda: oct(U())), ('range', lambda: range(U())), ('divmod', lambda: divmod(U(), 2)),
        ('str', lambda: str(U())), ('repr', lambda: repr(U())), ('ascii', lambda: ascii(U())),
        ('format', lambda: format(U())), ('format_plain', lambda: format(Plain())),
        ('fstr', lambda: f'{U()}'), ('fstr_plain', lambda: f'{Plain()}'), ('fstr_spec', lambda: f'{U():>3}'),
        ('fstr_repr', lambda: f'{U()!r}'), ('pct_s', lambda: '%s' % Plain()), ('pct_r', lambda: '%r' % U()),
        ('pct_d', lambda: '%d' % U()), ('str_format', lambda: '{}'.format(U())),
        ('str_format_plain', lambda: '{}'.format(Plain())), ('format_map', lambda: '{a}'.format_map(U())),
        ('print', lambda: print(Plain(), end='')),
        ('getattr', lambda: getattr(U(), 'x')), ('hasattr', lambda: hasattr(U(), 'x')), ('attr', lambda: U().x),
        ('prop', lambda: Prop().p), ('init', lambda: Init()), ('call', lambda: U()()), ('with', with_),
        ('iter_for', lambda: [x for x in U()]), ('iter_list', lambda: list(U())), ('iter_tuple', lambda: tuple(U())),
        ('next_for', lambda: [x for x in Nx()]), ('next_list', lambda: list(Nx())), ('next_builtin', lambda: next(Nx())),
        ('next_sum', lambda: sum(Nx())), ('next_set', lambda: set(Nx())), ('old_iter', lambda: list(Old())),
        ('sorted_items', lambda: sorted([U(), U()])), ('min_items', lambda: min([U(), U()])),
        ('max_items', lambda: max([U(), U()])), ('sum_items', lambda: sum([U()])),
        ('sorted_key', lambda: (reset(), sorted([2, 1], key=key1))), ('min_key', lambda: (reset(), min([2, 1], key=key1))),
        ('max_key', lambda: (reset(), max([2, 1], key=key1))), ('list_sort_key', lambda: (reset(), [2, 1].sort(key=key1))),
        ('map', lambda: list(map(lambda x: key(x), [1]))), ('map_for', lambda: [y for y in map(key, [1])]),
        ('filter', lambda: list(filter(key, [1]))), ('filter_for', lambda: [y for y in filter(key, [1])]),
        ('reduce', lambda: functools.reduce(lambda a, b: key(a), [1, 2])),
        ('starmap', lambda: list(itertools.starmap(lambda a: key(a), [(1,)]))),
        ('gen_next', lambda: next(gen())), ('gen_for', forgen), ('gen_list', lambda: list(gen())),
        ('gen_tuple', lambda: tuple(gen())), ('gen_sum', lambda: sum(gen())), ('gen_set', lambda: set(gen())),
        ('gen_sorted', lambda: sorted(gen())), ('gen_join', lambda: ''.join(str(x) for x in gen())),
        ('gen_min', lambda: min(gen())), ('gen_any', lambda: any(gen())), ('gen_all', lambda: all(gen())),
        ('gen_dict', lambda: dict((x, x) for x in gen())), ('gen_extend', lambda: [].extend(gen())),
        ('gen_star', lambda: [*gen()]), ('gen_enumerate', lambda: list(enumerate(gen()))),
        ('gen_zip', lambda: list(zip(gen()))), ('gen_map', lambda: list(map(str, gen()))),
        ('yf_list', lambda: list(outer())), ('yf_next', lambda: next(outer())), ('yf_for', lambda: [x for x in outer()]),
        ('exec', lambda: exec('HIT()', {'HIT': HIT})), ('eval', lambda: eval('HIT()', {'HIT': HIT})),
    ]
    return cases


main = greenlet.getcurrent()
for tag, f in build(lambda: main.switch()):
    g = greenlet.greenlet(f)
    try:
        g.switch()
        cost = g._stack_saved - 1512 if not g.dead else None
        while not g.dead:
            g.switch()
    except Exception as e:
        cost = 'err %s' % type(e).__name__
    print(tag, cost)
    sys.stdout.flush()
