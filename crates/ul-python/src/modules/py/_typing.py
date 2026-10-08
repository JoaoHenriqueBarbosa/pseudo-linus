"""Primitive types and utility functions for typing."""

# O `_typing` do CPython (`Modules/_typingmodule.c` e `Objects/typevarobject.c`) escrito em Python. O `typing.py`
# do Debian importa daqui `TypeVar`, `ParamSpec`, `TypeVarTuple`, `ParamSpecArgs`, `ParamSpecKwargs`,
# `TypeAliasType`, `Generic`, `NoDefault` e `_idfunc`, e os dá como se fossem dele (`__module__ == 'typing'`).
#
# Como no C, o que depende do `typing.py` (a substituição de parâmetros, a união `T | int`, o `__class_getitem__`
# de `Generic`) chama de volta a função de mesmo nome do `typing` na hora do uso, nunca na importação.

import _sys
import sys


class _Lazy:
    """Valor calculado na primeira leitura (limite, restrições e padrão de `def f[T: int = str]`).

    Só o escopo sintético dos parâmetros de tipo (PEP 695) o cria; no C são as funções avaliadoras que o
    compilador entrega ao `TypeVar`.
    """

    __slots__ = ('thunk',)

    def __init__(self, thunk):
        self.thunk = thunk


def _resolve(owner, attr):
    """O valor guardado em `owner.attr`, avaliando e guardando o resultado se ainda é um `_Lazy`."""
    value = getattr(owner, attr)
    if type(value) is _Lazy:
        value = value.thunk()
        setattr(owner, attr, value)
    return value


def _call_typing(name, *args, **kwargs):
    """Chama `typing.<name>(*args, **kwargs)`: o `call_typing_func_object` do C."""
    import typing
    return getattr(typing, name)(*args, **kwargs)


def _caller_module():
    """O `__name__` das globais de quem chamou o construtor (o `caller()` do C), ou `None`.

    Os quadros deste módulo são de código C para o `sys._getframe`: o quadro 0 já é o do chamador.
    """
    try:
        return sys._getframe(0).f_globals.get('__name__')
    except ValueError:
        return None


def _type_check(arg, msg):
    # O `None` vira `type(None)` sem passar pelo `typing`, que pode estar ainda sendo importado.
    if arg is None:
        return type(None)
    return _call_typing('_type_check', arg, msg)


def _idfunc(x, /):
    return x


# No C é uma função embutida: `NewType.__call__ = _idfunc` não liga o `self` ao ser guardada na classe.
_idfunc = _sys._builtin(_idfunc)


def _unsubclassable(cls):
    """Os tipos do C sem `Py_TPFLAGS_BASETYPE`: criar uma subclasse é um `TypeError`."""
    tp_name = cls.__name__ if cls.__module__ == 'builtins' else f'typing.{cls.__name__}'
    message = f"type '{tp_name}' is not an acceptable base type"

    def __init_subclass__(subclass, /, *args, **kwargs):
        raise TypeError(message)

    cls.__init_subclass__ = classmethod(__init_subclass__)
    return cls


def _new_param(cls, name):
    """Um `cls` novo com `__name__` e `__module__` (o do chamador do construtor), a base de todo parâmetro
    de tipo."""
    if not isinstance(name, str):
        raise TypeError(f"{cls.__name__}() argument 'name' must be str, not {type(name).__name__}")
    self = object.__new__(cls)
    self.__name__ = name
    self.__module__ = _caller_module()
    return self


def _intrinsic_param(cls, *args, **kwargs):
    """O parâmetro que a sintaxe `def f[T]()` cria (o `_Py_make_typevar` do C): sem `__module__` próprio, a
    leitura cai no da classe, `typing`."""
    self = cls(*args, **kwargs)
    del self.__module__
    return self


def _new_variable(cls, name, bound, covariant, contravariant, infer_variance, default):
    """O que `TypeVar` e `ParamSpec` têm em comum: variância, limite e padrão."""
    self = _new_param(cls, name)
    if covariant and contravariant:
        raise ValueError("Bivariant type variables are not supported.")
    if infer_variance and (covariant or contravariant):
        raise ValueError("Variance cannot be specified with infer_variance.")
    if bound is not None and type(bound) is not _Lazy:
        bound = _type_check(bound, "Bound must be a type.")
    self._bound = bound
    self.__covariant__ = bool(covariant)
    self.__contravariant__ = bool(contravariant)
    self.__infer_variance__ = bool(infer_variance)
    self._default = default
    return self


def _bound_of(self):
    return _resolve(self, '_bound')


def _default_of(self):
    return _resolve(self, '_default')


def _has_default(self):
    return self.__default__ is not NoDefault


def _variance_repr(self):
    if self.__infer_variance__:
        return self.__name__
    if self.__covariant__:
        prefix = '+'
    elif self.__contravariant__:
        prefix = '-'
    else:
        prefix = '~'
    return prefix + self.__name__


def _name_repr(self):
    return self.__name__


def _reduce_name(self):
    # Um parâmetro de tipo se serializa pelo nome (`pickle` o acha no módulo dele).
    return self.__name__


def _no_subclass_entries(self, bases):
    raise TypeError(f"Cannot subclass an instance of {type(self).__name__}")


def _union(self, other):
    return _call_typing('_make_union', self, other)


def _reverse_union(self, other):
    return _call_typing('_make_union', other, self)


@_unsubclassable
class NoDefaultType:
    """The type of the NoDefault singleton."""

    # O `tp_name` do C é só `NoDefaultType`, sem módulo: o tipo é de `builtins`.
    __module__ = 'builtins'

    def __new__(cls, *args, **kwargs):
        if args or kwargs:
            raise TypeError("NoDefaultType takes no arguments")
        return NoDefault

    def __repr__(self):
        return 'typing.NoDefault'

    def __reduce__(self):
        return 'NoDefault'


NoDefault = object.__new__(NoDefaultType)


@_unsubclassable
class TypeVar:
    __module__ = 'typing'

    def __new__(cls, name, *constraints, bound=None, covariant=False, contravariant=False,
                infer_variance=False, default=NoDefault):
        self = _new_variable(cls, name, bound, covariant, contravariant, infer_variance, default)
        if len(constraints) == 1 and type(constraints[0]) is _Lazy:
            # `def f[T: (int, str)]`: o avaliador devolve a tupla inteira.
            self._constraints = constraints[0]
        elif len(constraints) == 1:
            raise TypeError("A single constraint is not allowed")
        elif constraints:
            if bound is not None:
                raise TypeError("Constraints cannot be combined with bound=...")
            self._constraints = constraints
        else:
            self._constraints = ()
        return self

    __bound__ = property(_bound_of)
    __default__ = property(_default_of)
    has_default = _has_default
    __repr__ = _variance_repr
    __reduce__ = _reduce_name
    __mro_entries__ = _no_subclass_entries
    __or__ = _union
    __ror__ = _reverse_union

    @property
    def __constraints__(self):
        return _resolve(self, '_constraints')

    def __typing_subst__(self, arg):
        return _call_typing('_typevar_subst', self, arg)

    def __typing_prepare_subst__(self, alias, args):
        params = alias.__parameters__
        index = params.index(self)
        if index < len(args):
            # Já há um valor para esta variável.
            return args
        if index == len(args):
            default = self.__default__
            if default is not NoDefault:
                return args + (default,)
        raise TypeError(f"Too few arguments for {alias}; actual {len(args)}, expected at least {index + 1}")


@_unsubclassable
class TypeVarTuple:
    __module__ = 'typing'

    def __new__(cls, name, *, default=NoDefault):
        self = _new_param(cls, name)
        self._default = default
        return self

    __default__ = property(_default_of)
    has_default = _has_default
    __repr__ = _name_repr
    __reduce__ = _reduce_name
    __mro_entries__ = _no_subclass_entries

    def __iter__(self):
        import typing
        return iter((typing.Unpack[self],))

    def __typing_subst__(self, arg):
        raise TypeError("Substitution of bare TypeVarTuple is not supported")

    def __typing_prepare_subst__(self, alias, args):
        return _call_typing('_typevartuple_prepare_subst', self, alias, args)


def _proxy_new(cls, origin):
    self = object.__new__(cls)
    self.__origin__ = origin
    return self


def _proxy_repr(self, suffix):
    origin = self.__origin__
    if type(origin) is ParamSpec:
        return f"{origin.__name__}.{suffix}"
    return f"{origin!r}.{suffix}"


def _proxy_eq(self, other):
    if type(other) is not type(self):
        return NotImplemented
    return self.__origin__ == other.__origin__


@_unsubclassable
class ParamSpecArgs:
    __module__ = 'typing'

    def __new__(cls, origin, /):
        return _proxy_new(cls, origin)

    def __repr__(self):
        return _proxy_repr(self, 'args')

    __eq__ = _proxy_eq
    __mro_entries__ = _no_subclass_entries


@_unsubclassable
class ParamSpecKwargs:
    __module__ = 'typing'

    def __new__(cls, origin, /):
        return _proxy_new(cls, origin)

    def __repr__(self):
        return _proxy_repr(self, 'kwargs')

    __eq__ = _proxy_eq
    __mro_entries__ = _no_subclass_entries


@_unsubclassable
class ParamSpec:
    __module__ = 'typing'

    def __new__(cls, name, *, bound=None, covariant=False, contravariant=False, infer_variance=False,
                default=NoDefault):
        return _new_variable(cls, name, bound, covariant, contravariant, infer_variance, default)

    __bound__ = property(_bound_of)
    __default__ = property(_default_of)
    has_default = _has_default
    __repr__ = _variance_repr
    __reduce__ = _reduce_name
    __mro_entries__ = _no_subclass_entries
    __or__ = _union
    __ror__ = _reverse_union

    @property
    def args(self):
        return ParamSpecArgs(self)

    @property
    def kwargs(self):
        return ParamSpecKwargs(self)

    def __typing_subst__(self, arg):
        return _call_typing('_paramspec_subst', self, arg)

    def __typing_prepare_subst__(self, alias, args):
        return _call_typing('_paramspec_prepare_subst', self, alias, args)


@_unsubclassable
class TypeAliasType:
    __module__ = 'typing'

    def __new__(cls, name, value, *, type_params=()):
        self = _new_param(cls, name)
        if not isinstance(type_params, tuple):
            raise TypeError("type_params must be a tuple")
        for param in type_params:
            if type(param) not in (TypeVar, ParamSpec, TypeVarTuple):
                raise TypeError(f"Expected a type param, got {param!r}")
        self._type_params = type_params
        # O `type X = valor` entrega o valor numa função sem argumentos com o nome do alias, calculada só na
        # primeira leitura de `__value__`; qualquer outro `value` é o próprio valor.
        code = getattr(value, '__code__', None)
        if code is not None and code.co_name == name and code.co_argcount == 0:
            self._evaluate = value
            self._value = None
        else:
            self._evaluate = None
            self._value = value
        return self

    __repr__ = _name_repr
    __reduce__ = _reduce_name
    __mro_entries__ = _no_subclass_entries

    @property
    def __value__(self):
        if self._evaluate is not None:
            self._value = self._evaluate()
            self._evaluate = None
        return self._value

    @property
    def __type_params__(self):
        return self._type_params

    @property
    def __parameters__(self):
        params = []
        for param in self._type_params:
            if type(param) is TypeVarTuple:
                params.extend(param)
            else:
                params.append(param)
        return tuple(params)

    def __getitem__(self, parameters):
        if not self._type_params:
            raise TypeError("Only generic type aliases are subscriptable")
        from types import GenericAlias
        return GenericAlias(self, parameters)


class Generic:
    __module__ = 'typing'
    __slots__ = ()

    def __class_getitem__(cls, args):
        return _call_typing('_generic_class_getitem', cls, args)

    def __init_subclass__(cls, *args, **kwargs):
        return _call_typing('_generic_init_subclass', cls, *args, **kwargs)
