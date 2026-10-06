"""`_csv`: dialetos, registro e a fachada de `reader`/`writer` sobre o motor nativo `_csvimpl`."""

import _csvimpl

__version__ = "1.0"
__doc__ = "CSV parsing and writing."

QUOTE_MINIMAL = _csvimpl.QUOTE_MINIMAL
QUOTE_ALL = _csvimpl.QUOTE_ALL
QUOTE_NONNUMERIC = _csvimpl.QUOTE_NONNUMERIC
QUOTE_NONE = _csvimpl.QUOTE_NONE
QUOTE_STRINGS = _csvimpl.QUOTE_STRINGS
QUOTE_NOTNULL = _csvimpl.QUOTE_NOTNULL
Error = _csvimpl.Error

_FIELDS = ('delimiter', 'doublequote', 'escapechar', 'lineterminator', 'quotechar',
           'quoting', 'skipinitialspace', 'strict')
_DEFAULTS = {
    'delimiter': ',', 'doublequote': True, 'escapechar': None, 'lineterminator': '\r\n',
    'quotechar': '"', 'quoting': QUOTE_MINIMAL, 'skipinitialspace': False, 'strict': False,
}
_field_limit = 131072


class Dialect:
    """Descrição validada de um dialeto (`_csv.Dialect`)."""

    _valid = False
    delimiter = ','
    doublequote = True
    escapechar = None
    lineterminator = '\r\n'
    quotechar = '"'
    quoting = QUOTE_MINIMAL
    skipinitialspace = False
    strict = False

    def __init__(self, dialect=None, **kwargs):
        values = dict(_DEFAULTS)
        if dialect is not None:
            if isinstance(dialect, str):
                dialect = get_dialect(dialect)
            for name in _FIELDS:
                if hasattr(dialect, name):
                    values[name] = getattr(dialect, name)
        for name, value in kwargs.items():
            if name not in _FIELDS:
                raise TypeError("'%s' is an invalid keyword argument for Dialect()" % name)
            values[name] = value
        self._check(values)
        for name in _FIELDS:
            object.__setattr__(self, name, values[name])

    @staticmethod
    def _check(v):
        d = v['delimiter']
        if not isinstance(d, str):
            raise TypeError('"delimiter" must be a unicode character, not %s' % type(d).__name__)
        if len(d) != 1:
            raise TypeError('"delimiter" must be a unicode character, not a string of length %d' % len(d))
        for key in ('quotechar', 'escapechar'):
            c = v[key]
            if c is not None and not isinstance(c, str):
                raise TypeError('"%s" must be string or None, not %s' % (key, type(c).__name__))
            if c is not None and len(c) != 1:
                raise TypeError('"%s" must be a unicode character or None, not a string of length %d' % (key, len(c)))
        if not isinstance(v['lineterminator'], str):
            raise TypeError('"lineterminator" must be a string')
        if not isinstance(v['quoting'], int) or not 0 <= v['quoting'] <= 5:
            raise TypeError('bad "quoting" value')
        if v['quotechar'] is None and v['quoting'] != QUOTE_NONE:
            raise TypeError('quotechar must be set if quoting enabled')


_dialects = {}


def register_dialect(name, dialect=None, **kwargs):
    if not isinstance(name, str):
        raise TypeError('dialect name must be a string')
    _dialects[name] = Dialect(dialect, **kwargs)


def unregister_dialect(name):
    if name not in _dialects:
        raise Error('unknown dialect')
    del _dialects[name]


def get_dialect(name):
    try:
        return _dialects[name]
    except (KeyError, TypeError):
        raise Error('unknown dialect') from None


def list_dialects():
    return list(_dialects)


def field_size_limit(*args, **kwargs):
    global _field_limit
    if len(args) + len(kwargs) > 1:
        raise TypeError('field_size_limit expected at most 1 argument, got %d' % (len(args) + len(kwargs)))
    old = _field_limit
    if args or kwargs:
        new = args[0] if args else kwargs['new_limit']
        if not isinstance(new, int):
            raise TypeError("limit must be an integer")
        _field_limit = new
    return old


def _options(dialect, kwargs):
    base = Dialect(dialect if dialect is not None else 'excel', **kwargs)
    return {name: getattr(base, name) for name in _FIELDS}


def reader(csvfile, dialect='excel', **kwargs):
    r = _csvimpl.reader(csvfile, **_options(dialect, kwargs))
    return r


def writer(csvfile, dialect='excel', **kwargs):
    if not hasattr(csvfile, 'write'):
        raise TypeError('argument 1 must have a "write" method')
    return _csvimpl.writer(csvfile, **_options(dialect, kwargs))
