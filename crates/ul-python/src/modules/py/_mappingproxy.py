"""`mappingproxy`: a visão somente leitura de um mapeamento (o `__dict__` dos tipos embutidos)."""


class mappingproxy:
    __module__ = 'builtins'
    __slots__ = ('_mapping',)

    def __init__(self, mapping):
        if not hasattr(mapping, 'keys') or not hasattr(mapping, '__getitem__') or isinstance(mapping, (list, tuple)):
            raise TypeError('mappingproxy() argument must be a mapping, not %s' % type(mapping).__name__)
        self._mapping = mapping

    def __getitem__(self, key):
        return self._mapping[key]

    def __iter__(self):
        return iter(self._mapping)

    def __len__(self):
        return len(self._mapping)

    def __contains__(self, key):
        return key in self._mapping

    def __eq__(self, other):
        if isinstance(other, mappingproxy):
            other = other._mapping
        return self._mapping == other

    def __ne__(self, other):
        return not self == other

    __hash__ = None

    # `mappingproxy[str, int]`: o `types.GenericAlias`, que o `types` ainda não pode importar daqui.
    __class_getitem__ = classmethod(type(list[int]))

    def __or__(self, other):
        if isinstance(other, mappingproxy):
            other = other._mapping
        return self._mapping | other

    def __ror__(self, other):
        return other | self._mapping

    def __reversed__(self):
        return reversed(list(self._mapping))

    def get(self, key, default=None):
        return self._mapping.get(key, default)

    def keys(self):
        return self._mapping.keys()

    def values(self):
        return self._mapping.values()

    def items(self):
        return self._mapping.items()

    def copy(self):
        return self._mapping.copy()

    def __repr__(self):
        return 'mappingproxy(%r)' % (self._mapping,)

    def __str__(self):
        return str(self._mapping)
