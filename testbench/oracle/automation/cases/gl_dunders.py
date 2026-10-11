"""O switch de greenlet dentro de um dunder de usuário chamado por código nativo (o %s e o %r, str.format, format() e
f-string, str() e repr(), o __eq__ da busca de chave em dict, set e frozenset, `in`, __contains__, __len__, __bool__,
__getitem__ e __setitem__ pelo operador, __hash__ numa chave de dict, __add__, __lt__ na comparação, __call__, __iter__,
__getattr__, __del__ explícito, __index__, __int__, __float__, bytes(), abs, round, divmod): no CPython a pilha de C é
copiada junto, e a troca funciona em qualquer profundidade."""
import greenlet

main = greenlet.getcurrent()


def drive(body):
    child = greenlet.greenlet(body)
    seen = []
    try:
        value = child.switch()
        while not child.dead:
            seen.append(value)
            value = child.switch()
        return seen, value
    except Exception as exc:
        return type(exc).__name__, str(exc)


def k(tag):
    main.switch(tag)


class D:
    def __init__(self, v=1):
        self.v = v

    def __str__(self):
        k('str')
        return 'S'

    def __repr__(self):
        k('repr')
        return 'R'

    def __format__(self, spec):
        k('format')
        return 'F' + spec

    def __eq__(self, other):
        k('eq')
        return isinstance(other, D) and self.v == other.v

    def __hash__(self):
        k('hash')
        return hash(self.v)

    def __lt__(self, other):
        k('lt')
        return self.v < other.v

    def __contains__(self, item):
        k('contains')
        return True

    def __len__(self):
        k('len')
        return 3

    def __bool__(self):
        k('bool')
        return True

    def __getitem__(self, i):
        k('getitem')
        return i

    def __setitem__(self, i, v):
        k('setitem')

    def __delitem__(self, i):
        k('delitem')

    def __add__(self, other):
        k('add')
        return 7

    def __radd__(self, other):
        k('radd')
        return 8

    def __call__(self, *args):
        k('call')
        return 9

    def __index__(self):
        k('index')
        return 2

    def __int__(self):
        k('int')
        return 4

    def __float__(self):
        k('float')
        return 1.5

    def __bytes__(self):
        k('bytes')
        return b'B'

    def __abs__(self):
        k('abs')
        return 5

    def __round__(self, n=None):
        k('round')
        return 6

    def __divmod__(self, other):
        k('divmod')
        return (1, 2)

    def __neg__(self):
        k('neg')
        return -1

    def __getattr__(self, name):
        if name.startswith('dyn'):
            k('getattr')
            return 10
        raise AttributeError(name)


def case(name, fn):
    print(name, drive(fn))


d = D()
case('percent_s', lambda: '%s' % d)
case('percent_r', lambda: '%r' % d)
case('percent_tuple', lambda: '%s-%r' % (d, d))
case('percent_dict', lambda: '%(a)s' % {'a': d})
case('format_method', lambda: '{}'.format(d))
case('format_spec', lambda: '{:x}'.format(d))
case('format_builtin', lambda: format(d, 'y'))
case('fstring', lambda: f'{d}')
case('fstring_spec', lambda: f'{d:z}')
case('fstring_repr', lambda: f'{d!r}')
case('str_call', lambda: str(d))
case('repr_call', lambda: repr(d))
case('print_str', lambda: print(d, file=open('/dev/null', 'w')))
case('dict_get_eq', lambda: {D(1): 'a'}[D(1)])
case('dict_in', lambda: D(1) in {D(1): 'a'})
case('dict_get_method', lambda: {D(1): 'a'}.get(D(1)))
case('dict_setitem', lambda: {D(1): 'a'}.__setitem__(D(1), 'b'))
case('dict_pop', lambda: {D(1): 'a'}.pop(D(1)))
case('set_in', lambda: D(1) in {D(1)})
case('set_add', lambda: {D(1)}.add(D(1)))
case('set_discard', lambda: {D(1)}.discard(D(1)))
case('set_literal', lambda: {D(1), D(1)})
case('frozenset_in', lambda: D(1) in frozenset([D(1)]))
case('dict_from_list', lambda: dict([(D(1), 1), (D(1), 2)]))
case('set_from_list', lambda: set([D(1), D(1)]))
case('set_op', lambda: {D(1)} | {D(1)})
case('set_eq', lambda: {D(1)} == {D(1)})
case('dict_eq', lambda: {1: D(1)} == {1: D(1)})
case('list_eq', lambda: [D(1)] == [D(1)])
case('tuple_eq', lambda: (D(1),) == (D(1),))
case('tuple_lt', lambda: (D(1),) < (D(2),))
case('list_in', lambda: D(1) in [D(1)])
case('in_operator', lambda: 1 in d)
case('len_call', lambda: len(d))
case('bool_call', lambda: bool(d))
case('if_bool', lambda: 1 if d else 2)
case('not_bool', lambda: not d)
case('getitem_op', lambda: d[1])
case('setitem_op', lambda: d.__setitem__(1, 2))
case('setitem_stmt', lambda: exec('d[1] = 2', {'d': d}))
case('delitem_stmt', lambda: exec('del d[1]', {'d': d}))
case('add_op', lambda: d + 1)
case('radd_op', lambda: 1 + d)
case('sum_call', lambda: sum([d, d]))
case('lt_op', lambda: D(1) < D(2))
case('call_op', lambda: d(1))
case('neg_op', lambda: -d)
case('abs_call', lambda: abs(d))
case('round_call', lambda: round(d))
case('divmod_call', lambda: divmod(d, 2))
case('int_call', lambda: int(d))
case('float_call', lambda: float(d))
case('bytes_call', lambda: bytes(d))
case('index_list', lambda: [10, 20, 30][d])
case('index_range', lambda: list(range(d)))
case('index_hex', lambda: hex(d))
case('index_bin', lambda: bin(d))
case('index_slice', lambda: 'abcdef'[d:])
case('getattr_dyn', lambda: d.dyn1)
case('getattr_builtin', lambda: getattr(d, 'dyn2'))
case('hasattr_builtin', lambda: hasattr(d, 'dyn3'))
case('max_call', lambda: max(D(1), D(2)))
case('min_list', lambda: min([D(1), D(2)]))
case('sorted_call', lambda: sorted([D(2), D(1)]))
case('list_index', lambda: [D(1)].index(D(1)))
case('list_count', lambda: [D(1)].count(D(1)))
case('list_remove', lambda: [D(1)].remove(D(1)))
case('str_join_str', lambda: ','.join([str(d)]))
case('dict_key_hash_literal', lambda: {D(5): 1})
case('counter_like', lambda: __import__('collections').Counter([D(1), D(1)]))
