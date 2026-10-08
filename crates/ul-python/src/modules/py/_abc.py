"""`_abc`: o núcleo em C do `abc` do CPython (`Modules/_abc.c`), escrito em Python.

O `abc.py` do Debian importa daqui `get_cache_token`, `_abc_init`, `_abc_register`, `_abc_instancecheck`,
`_abc_subclasscheck`, `_get_dump`, `_reset_registry` e `_reset_caches`, e monta a `ABCMeta` sobre eles. O
estado de cada classe abstrata fica num `_abc_data` em `cls._abc_impl`: o registro de subclasses virtuais, o
cache positivo e o cache negativo (conjuntos de classes) e a versão do cache negativo.

No C esses conjuntos guardam referências fracas, para o ABC não manter vivas as classes que consultou. Aqui eles
guardam as classes: a consulta é um `in` direto, sem criar uma referência fraca por chamada, e o `_get_dump`
monta as referências fracas na hora, que é o único lugar onde o programa as enxerga.
"""

# `abc_invalidation_counter` do C: cada `register()` o incrementa e invalida os caches negativos.
_invalidation_counter = 0

# `COLLECTION_FLAGS`: `Py_TPFLAGS_SEQUENCE | Py_TPFLAGS_MAPPING`.
_COLLECTION_FLAGS = (1 << 5) | (1 << 6)


class _abc_data:
    """Internal state held by ABC machinery."""

    __slots__ = ('registry', 'cache', 'negative_cache', 'negative_cache_version')


def _new_data():
    data = _abc_data()
    data.registry = set()
    data.cache = set()
    data.negative_cache = set()
    data.negative_cache_version = _invalidation_counter
    return data


def _get_impl(self):
    impl = self._abc_impl
    if type(impl) is not _abc_data:
        raise TypeError("_abc_impl is set to a wrong type")
    return impl


def _is_abstract(value):
    return bool(getattr(value, '__isabstractmethod__', False))


def _compute_abstract_methods(self):
    abstracts = set()
    # Etapa 1: os métodos abstratos declarados na própria classe.
    ns = self.__dict__
    for key in list(ns.keys()):
        if _is_abstract(ns[key]):
            abstracts.add(key)
    # Etapa 2: os abstratos herdados que a classe ainda não implementou.
    for base in self.__bases__:
        base_abstracts = getattr(base, '__abstractmethods__', None)
        if base_abstracts is None:
            continue
        for key in base_abstracts:
            if _is_abstract(getattr(self, key, None)):
                abstracts.add(key)
    self.__abstractmethods__ = frozenset(abstracts)


def _abc_init(self, /):
    _compute_abstract_methods(self)
    self._abc_impl = _new_data()
    # `__abc_tpflags__` pede os sinalizadores de coleção (sequência ou mapeamento) do `match`; o `match` daqui
    # decide por `isinstance`, então só a validação e a remoção da chave do corpo da classe valem.
    ns = self.__dict__
    if '__abc_tpflags__' in ns:
        flags = ns['__abc_tpflags__']
        if type(flags) is int and flags & _COLLECTION_FLAGS == _COLLECTION_FLAGS:
            raise TypeError("__abc_tpflags__ cannot be both Py_TPFLAGS_SEQUENCE and Py_TPFLAGS_MAPPING")
        delattr(self, '__abc_tpflags__')


def _abc_register(self, subclass, /):
    global _invalidation_counter
    if not isinstance(subclass, type):
        raise TypeError("Can only register classes")
    if issubclass(subclass, self):
        # Já é subclasse.
        return subclass
    # O teste de ciclo vem depois do "já é subclasse", para o caso comum não pagar por ele.
    if issubclass(self, subclass):
        raise RuntimeError("Refusing to create an inheritance cycle")
    _get_impl(self).registry.add(subclass)
    # Invalida os caches negativos.
    _invalidation_counter += 1
    return subclass


def _abc_instancecheck(self, instance, /):
    impl = _get_impl(self)
    subclass = instance.__class__
    # Consulta o cache positivo direto, sem passar por `__subclasscheck__`.
    if subclass in impl.cache:
        return True
    subtype = type(instance)
    if subtype is subclass:
        if impl.negative_cache_version == _invalidation_counter and subclass in impl.negative_cache:
            return False
        return self.__subclasscheck__(subclass)
    result = self.__subclasscheck__(subclass)
    if result:
        return result
    return self.__subclasscheck__(subtype)


def _abc_subclasscheck(self, subclass, /):
    if not isinstance(subclass, type):
        raise TypeError("issubclass() arg 1 must be a class")
    impl = _get_impl(self)
    # 1. Cache positivo.
    if subclass in impl.cache:
        return True
    # 2. Cache negativo; pode ter de ser invalidado.
    if impl.negative_cache_version < _invalidation_counter:
        impl.negative_cache.clear()
        impl.negative_cache_version = _invalidation_counter
    elif subclass in impl.negative_cache:
        return False
    # 3. O gancho `__subclasshook__`.
    ok = self.__subclasshook__(subclass)
    if ok is True:
        impl.cache.add(subclass)
        return True
    if ok is False:
        impl.negative_cache.add(subclass)
        return False
    if ok is not NotImplemented:
        raise RuntimeError("__subclasshook__ must return either False, True, or NotImplemented")
    # 4. Subclasse direta (a classe está no MRO).
    for base in subclass.__mro__:
        if base is self:
            impl.cache.add(subclass)
            return True
    # 5. Subclasse de uma classe registrada (recursivo).
    if subclass in impl.registry:
        return True
    # Cópia local, para o registro poder mudar durante as chamadas recursivas.
    for registered in list(impl.registry):
        if issubclass(subclass, registered):
            impl.cache.add(subclass)
            return True
    # 6. Subclasse de uma subclasse (recursivo).
    subclasses = self.__subclasses__()
    if not isinstance(subclasses, list):
        raise TypeError("__subclasses__() must return a list")
    for scls in subclasses:
        if issubclass(subclass, scls):
            impl.cache.add(subclass)
            return True
    # Nada serviu: atualiza o cache negativo.
    impl.negative_cache.add(subclass)
    return False


def _get_dump(self, /):
    from _weakref import ref
    impl = _get_impl(self)
    return ({ref(c) for c in impl.registry}, {ref(c) for c in impl.cache}, {ref(c) for c in impl.negative_cache},
            impl.negative_cache_version)


def _reset_registry(self, /):
    _get_impl(self).registry.clear()


def _reset_caches(self, /):
    impl = _get_impl(self)
    impl.cache.clear()
    impl.negative_cache.clear()


def get_cache_token():
    return _invalidation_counter
