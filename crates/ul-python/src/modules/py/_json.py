"""json speedups
"""

# O `_json` do CPython é C (`Modules/_json.c`). As classes ficam aqui só para ter tipo, atributos
# e `dir()` dos tipos em C; o trabalho é do `_json_native`, porte do C, importado dentro de cada
# método para não deixar nome auxiliar no módulo (o `dir(_json)` do Debian só lista a API pública).

from _json_native import scanstring, encode_basestring_ascii, encode_basestring

# Os nomes dos `__slots__`, lidos no fim do módulo (o VM esconde o `__slots__` de um shim de tipo em C).
_SCANNER_SLOTS = ('object_hook', 'object_pairs_hook', 'parse_constant', 'parse_float', 'parse_int', 'strict')
_ENCODER_SLOTS = ('default', 'encoder', 'indent', 'item_separator', 'key_separator', 'markers', 'skipkeys', 'sort_keys')


class Scanner:
    """JSON scanner object"""

    __slots__ = _SCANNER_SLOTS

    def __new__(cls, *args, **kwargs):
        import _json_native
        ctx = _json_native.scanner_args(*args, **kwargs)
        self = object.__new__(cls)
        put = object.__setattr__
        put(self, 'strict', bool(ctx.strict))
        put(self, 'object_hook', ctx.object_hook)
        put(self, 'object_pairs_hook', ctx.object_pairs_hook)
        put(self, 'parse_float', ctx.parse_float)
        put(self, 'parse_int', ctx.parse_int)
        put(self, 'parse_constant', ctx.parse_constant)
        return self

    def __call__(self, *args, **kwargs):
        import _json_native
        return _json_native.scan_once(self, *args, **kwargs)

    def __setattr__(self, name, value):
        if name in ('object_hook', 'object_pairs_hook', 'parse_constant', 'parse_float', 'parse_int', 'strict'):
            raise AttributeError('readonly attribute')
        raise AttributeError(f"'_json.Scanner' object has no attribute '{name}' and no __dict__ for setting new attributes")

    def __delattr__(self, name):
        Scanner.__setattr__(self, name, None)


class Encoder:
    """Encoder(markers, default, encoder, indent, key_separator, item_separator, sort_keys, skipkeys, allow_nan)"""

    __slots__ = _ENCODER_SLOTS

    def __new__(cls, *args, **kwargs):
        import _json_native
        self = object.__new__(cls)
        markers, default, encoder, indent, key_sep, item_sep, sort_keys, skipkeys, _ = _json_native.encoder_args(self, *args, **kwargs)
        put = object.__setattr__
        put(self, 'markers', markers)
        put(self, 'default', default)
        put(self, 'encoder', encoder)
        put(self, 'indent', indent)
        put(self, 'key_separator', key_sep)
        put(self, 'item_separator', item_sep)
        put(self, 'sort_keys', sort_keys)
        put(self, 'skipkeys', skipkeys)
        return self

    def __call__(self, *args, **kwargs):
        import _json_native
        return _json_native.encode(self, *args, **kwargs)

    def __setattr__(self, name, value):
        if name in ('default', 'encoder', 'indent', 'item_separator', 'key_separator', 'markers', 'skipkeys', 'sort_keys'):
            raise AttributeError('readonly attribute')
        raise AttributeError(f"'_json.Encoder' object has no attribute '{name}' and no __dict__ for setting new attributes")

    def __delattr__(self, name):
        Encoder.__setattr__(self, name, None)


class member_descriptor:
    # Os campos dos tipos em C aparecem na classe como `<attribute 'x' of '_json.Y' objects>`; o
    # valor de cada instância fica nela mesma, que tem precedência sobre um descritor sem `__set__`.
    def __init__(self, name, owner):
        self.__name__ = name
        self.__objclass__ = owner

    def __get__(self, obj, owner=None):
        if obj is None:
            return self
        raise AttributeError(self.__name__)

    def __repr__(self):
        return f"<attribute '{self.__name__}' of '_json.{self.__objclass__.__name__}' objects>"


member_descriptor.__module__ = 'builtins'
for _cls, _names in ((Scanner, _SCANNER_SLOTS), (Encoder, _ENCODER_SLOTS)):
    for _name in _names:
        setattr(_cls, _name, member_descriptor(_name, _cls))
del Scanner.__slots__, Encoder.__slots__, _cls, _names, _name, member_descriptor, _SCANNER_SLOTS, _ENCODER_SLOTS
make_scanner = Scanner
make_encoder = Encoder
del Scanner, Encoder
