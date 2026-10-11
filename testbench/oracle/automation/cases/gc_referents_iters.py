"""gc.get_referents de reversed(), dos iteradores de dict, set, bytes e range, e dos objetos do itertools (cada um com o
tipo na frente e os campos do `tp_traverse`)."""

import gc
import itertools
import operator


def names(*objs):
    return [type(r).__name__ for r in gc.get_referents(*objs)]


d = {'a': 1, 'b': 2}
lst = [1, 2, 3]
tup = (1, 2, 3)
s = {1, 2}

print('reversed_list', names(reversed(lst)))
rl = reversed(lst)
next(rl)
print('reversed_list_started', names(rl))
print('reversed_tuple', names(reversed(tup)))
print('reversed_str', names(reversed('abc')))
print('reversed_range', names(reversed(range(3))))
print('reversed_dict', names(reversed(d)))
print('dict_items_iter', names(iter(d.items())))
print('dict_keys_iter', names(iter(d.keys())))
print('dict_values_iter', names(iter(d.values())))
print('dict_iter', names(iter(d)))
print('dict_reversed_items', names(reversed(d.items())))
print('dict_view_items', names(d.items()))
print('dict_view_keys', names(d.keys()))
print('set_iter', names(iter(s)))
print('bytes_iter', names(iter(b'ab')))
print('bytearray_iter', names(iter(bytearray(b'ab'))))
print('range_iter', names(iter(range(3))))
print('chain', names(itertools.chain([1], [2])))
print('chain_from', names(itertools.chain.from_iterable([[1], [2]])))
print('count', names(itertools.count(1)))
print('repeat', names(itertools.repeat(1, 3)))
print('cycle', names(itertools.cycle([1, 2])))
print('islice', names(itertools.islice([1, 2, 3], 2)))
print('takewhile', names(itertools.takewhile(bool, [1, 0])))
print('dropwhile', names(itertools.dropwhile(bool, [1, 0])))
print('filterfalse', names(itertools.filterfalse(bool, [1, 0])))
print('starmap', names(itertools.starmap(operator.add, [(1, 2)])))
print('accumulate', names(itertools.accumulate([1, 2])))
print('zip_longest', names(itertools.zip_longest([1], [2])))
print('product', names(itertools.product([1], [2])))
print('permutations', names(itertools.permutations([1, 2])))
print('combinations', names(itertools.combinations([1, 2], 1)))
print('cwr', names(itertools.combinations_with_replacement([1, 2], 1)))
print('groupby', names(itertools.groupby([1, 1, 2])))
print('pairwise', names(itertools.pairwise([1, 2, 3])))
print('batched', names(itertools.batched([1, 2, 3], 2)))
t1, t2 = itertools.tee([1, 2, 3])
print('tee', names(t1))
print('compress', names(itertools.compress([1, 2], [1, 0])))
cg = itertools.chain([1], [2])
next(cg)
print('chain_started', names(cg))
cy = itertools.cycle([1, 2])
next(cy)
print('cycle_started', names(cy))

for label, it in [
    ('list_iter', iter(lst)),
    ('items_iter', iter(d.items())),
    ('keys_iter', iter(d.keys())),
    ('reversed_list', reversed(lst)),
    ('reversed_items', reversed(d.items())),
    ('reversed_values', reversed(d.values())),
    ('set_iter', iter(s)),
    ('reversed_tuple', reversed(tup)),
    ('reversed_str', reversed('abc')),
    ('reversed_dict', reversed(d)),
    ('bytes_iter', iter(b'ab')),
]:
    list(it)
    print(label + '_done', names(it))

print('count_step', names(itertools.count(1, 2)))
print('count_float', names(itertools.count(1.5)))
print('count_big', names(itertools.count(2 ** 70)))
print('count_big_step', names(itertools.count(2 ** 70, 3)))
print('count_started', names(next(iter([itertools.count(5)])) if False else itertools.count(5)))
c = itertools.count(5)
next(c)
print('count_next', names(c))
print('repeat_forever', names(itertools.repeat('x')))
a = itertools.accumulate([1, 2, 3])
next(a)
print('accumulate_started', names(a))
print('accumulate_func', names(itertools.accumulate([1, 2], operator.add, initial=0)))
pw = itertools.pairwise([1, 2, 3])
next(pw)
print('pairwise_started', names(pw))
g = itertools.groupby([1, 1, 2])
next(g)
print('groupby_started', names(g))
print('groupby_key', names(itertools.groupby([1], key=abs)))
isl = itertools.islice([1, 2, 3], 1)
list(isl)
print('islice_done', names(isl))
pr = itertools.product([1, 2], [3])
next(pr)
print('product_started', names(pr))
pm = itertools.permutations([1, 2])
next(pm)
print('permutations_started', names(pm))
cb = itertools.combinations([1, 2, 3], 2)
next(cb)
print('combinations_started', names(cb))
cr = itertools.combinations_with_replacement([1, 2], 2)
next(cr)
print('cwr_started', names(cr))
zl = itertools.zip_longest([1], [2, 3], fillvalue=0)
next(zl)
next(zl)
print('zip_longest_started', names(zl))
tw = itertools.takewhile(bool, [1, 0])
list(tw)
print('takewhile_done', names(tw))
dw = itertools.dropwhile(bool, [1, 0, 2])
next(dw)
print('dropwhile_started', names(dw))
print('filterfalse_none', names(itertools.filterfalse(None, [1, 0])))
print('batched_strict', names(itertools.batched([1, 2, 3], 2, strict=True)))
print('compress_started', names(itertools.compress([1, 2], [1, 0])))
print('tee_data', names(gc.get_referents(t1)[1]))
print('tee_n3', [names(x) for x in itertools.tee([1, 2], 3)])
print('product_repeat', names(itertools.product([1], repeat=2)))
print('product_empty', names(itertools.product()))
print('chain_empty', names(itertools.chain()))
print('chain_done', names(itertools.chain([1])) if False else None)
ch = itertools.chain([1])
list(ch)
print('chain_done', names(ch))
cy = itertools.cycle([1])
next(cy)
next(cy)
print('cycle_two', names(cy))
