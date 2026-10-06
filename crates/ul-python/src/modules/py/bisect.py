"""bisect: busca binária em listas ordenadas."""


def bisect_right(a, x, lo=0, hi=None, *, key=None):
    if lo < 0:
        raise ValueError('lo must be non-negative')
    if hi is None:
        hi = len(a)
    while lo < hi:
        mid = (lo + hi) // 2
        v = a[mid] if key is None else key(a[mid])
        if x < v:
            hi = mid
        else:
            lo = mid + 1
    return lo


def bisect_left(a, x, lo=0, hi=None, *, key=None):
    if lo < 0:
        raise ValueError('lo must be non-negative')
    if hi is None:
        hi = len(a)
    while lo < hi:
        mid = (lo + hi) // 2
        v = a[mid] if key is None else key(a[mid])
        if v < x:
            lo = mid + 1
        else:
            hi = mid
    return lo


def insort_right(a, x, lo=0, hi=None, *, key=None):
    k = x if key is None else key(x)
    lo = bisect_right(a, k, lo, hi, key=key)
    a.insert(lo, x)


def insort_left(a, x, lo=0, hi=None, *, key=None):
    k = x if key is None else key(x)
    lo = bisect_left(a, k, lo, hi, key=key)
    a.insert(lo, x)


bisect = bisect_right
insort = insort_right
