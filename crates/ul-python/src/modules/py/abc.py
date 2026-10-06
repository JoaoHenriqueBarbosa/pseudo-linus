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


class ABC:
    """Classe base para classes abstratas (`class Foo(ABC)`)."""
    __abstract_base__ = True


class ABCMeta:
    pass


def get_cache_token():
    return 0
