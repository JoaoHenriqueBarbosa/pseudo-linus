"""abc do sandbox: classes base abstratas.

`abstractmethod` marca a função com `__isabstractmethod__`; a VM recusa instanciar uma classe
derivada de `ABC` que ainda tenha métodos abstratos sem implementação.
"""


def abstractmethod(funcobj):
    funcobj.__isabstractmethod__ = True
    return funcobj


def abstractclassmethod(f):
    f.__isabstractmethod__ = True
    return classmethod(f)


def abstractstaticmethod(f):
    f.__isabstractmethod__ = True
    return staticmethod(f)


def abstractproperty(f):
    f.__isabstractmethod__ = True
    return property(f)


class ABCMeta(type):
    """Metaclasse das classes abstratas: `register`, `isinstance` e `issubclass` virtuais."""

    def __new__(mcs, name, bases, ns, **kwargs):
        cls = super().__new__(mcs, name, bases, ns, **kwargs)
        cls._abc_registry_ = []
        cls._abc_subclasses_ = []
        abstracts = {n for n, v in ns.items() if getattr(v, '__isabstractmethod__', False)}
        for base in bases:
            for n in getattr(base, '__abstractmethods__', ()):
                if getattr(getattr(cls, n, None), '__isabstractmethod__', False):
                    abstracts.add(n)
        cls.__abstractmethods__ = frozenset(abstracts)
        for base in bases:
            subs = getattr(base, '_abc_subclasses_', None)
            if subs is not None:
                subs.append(cls)
        return cls

    def register(cls, subclass):
        cls._abc_registry_.append(subclass)
        return subclass

    def __instancecheck__(cls, instance):
        return cls.__subclasscheck__(type(instance))

    def __subclasscheck__(cls, subclass):
        if not isinstance(subclass, type):
            raise TypeError('issubclass() arg 1 must be a class')
        if subclass is cls or cls in getattr(subclass, '__mro__', ()):
            return True
        for registered in cls._abc_registry_:
            if subclass is registered or issubclass(subclass, registered):
                return True
        hook = cls.__subclasshook__(subclass)
        if hook is not NotImplemented:
            return hook
        for scls in cls._abc_subclasses_:
            if issubclass(subclass, scls):
                return True
        return False

    def __subclasshook__(cls, subclass):
        return NotImplemented


class ABC(metaclass=ABCMeta):
    """Helper class that provides a standard way to create an ABC using
    inheritance.
    """
    __abstract_base__ = True


def get_cache_token():
    return 0
