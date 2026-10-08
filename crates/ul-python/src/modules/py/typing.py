"""typing do sandbox (Python embutido): anotações de tipo em tempo de execução."""

import collections
import collections.abc

TYPE_CHECKING = False


def _type_repr(obj):
    if isinstance(obj, type):
        if obj.__module__ == 'builtins':
            return obj.__qualname__
        return '%s.%s' % (obj.__module__, obj.__qualname__)
    if obj is ...:
        return '...'
    if obj is None:
        return 'None'
    if isinstance(obj, list):
        return '[%s]' % ', '.join(_type_repr(a) for a in obj)
    return repr(obj)


class _Final:
    """Base das classes internas do typing que não aceitam subclasse fora dele (`_root=True`)."""

    __slots__ = ('__weakref__',)

    def __init_subclass__(cls, /, *args, **kwds):
        if '_root' not in kwds:
            raise TypeError("Cannot subclass special typing classes")


class _SpecialForm(_Final, _root=True):
    def __init__(self, getitem):
        # Como no CPython, a forma especial embrulha a função que trata `[...]`; as formas
        # embutidas deste módulo passam só o nome e usam o tratamento de `_builtin_getitem`.
        if isinstance(getitem, str):
            self._getitem = None
            self._name = getitem
        else:
            self._getitem = getitem
            self._name = getitem.__name__
            self.__doc__ = getitem.__doc__
        self.__name__ = self._name

    def __getattr__(self, item):
        if item in {'__name__', '__qualname__'}:
            return self._name
        raise AttributeError(item)

    def __mro_entries__(self, bases):
        raise TypeError(f"Cannot subclass {self!r}")

    def __repr__(self):
        return 'typing.' + self._name

    def __reduce__(self):
        return self._name

    def __instancecheck__(self, obj):
        raise TypeError(f"{self} cannot be used with isinstance()")

    def __subclasscheck__(self, cls):
        raise TypeError(f"{self} cannot be used with issubclass()")

    def __getitem__(self, params):
        if self._getitem is not None:
            return self._getitem(self, params)
        return self._builtin_getitem(params)

    def _builtin_getitem(self, params):
        if self._name == 'Optional':
            return _GenericAlias(Union, (params, type(None)), name='Optional', display=(params,))
        if self._name == 'Union':
            if not isinstance(params, tuple):
                params = (params,)
            flat = []
            for p in params:
                parts = p.__args__ if isinstance(p, _GenericAlias) and p.__origin__ is Union else (p,)
                for part in parts:
                    if part is None:
                        part = type(None)
                    if part not in flat:
                        flat.append(part)
            if len(flat) == 1:
                return flat[0]
            none_type = type(None)
            if len(flat) == 2 and none_type in flat:
                other = flat[0] if flat[1] is none_type else flat[1]
                return _GenericAlias(Union, tuple(flat), name='Optional', display=(other,))
            return _GenericAlias(Union, tuple(flat), name='Union')
        if not isinstance(params, tuple):
            params = (params,)
        if self._name == 'Annotated':
            if len(params) < 2:
                raise TypeError('Annotated[...] should be used with at least two arguments (a type and an annotation).')
            alias = _GenericAlias(params[0], (params[0],), name='Annotated',
                                  display=params)
            alias.__metadata__ = tuple(params[1:])
            return alias
        return _GenericAlias(self, params, name=self._name)

    def __or__(self, other):
        return Union[self, other]

    def __ror__(self, other):
        return Union[other, self]

    def __call__(self, *args, **kwargs):
        raise TypeError("Cannot instantiate %r" % (self,))


class _GenericAlias:
    def __init__(self, origin, args, name=None, display=None):
        self.__origin__ = origin
        self.__args__ = tuple(args)
        self._name = name
        self._display = display if display is not None else self.__args__

    def __repr__(self):
        origin = self.__origin__
        if self._name in ('Optional',):
            return 'typing.Optional[%s]' % _type_repr(self._display[0])
        if self._name == 'Callable':
            args, result = self.__args__[0], self.__args__[1]
            if args is ...:
                return 'typing.Callable[..., %s]' % _type_repr(result)
            return 'typing.Callable[[%s], %s]' % (', '.join(_type_repr(a) for a in args), _type_repr(result))
        params = ', '.join(_type_repr(a) for a in self._display)
        if self._name is not None:
            return 'typing.%s[%s]' % (self._name, params)
        return '%s[%s]' % (_type_repr(origin), params)

    def __getitem__(self, params):
        if not isinstance(params, tuple):
            params = (params,)
        return _GenericAlias(self.__origin__, params, name=self._name)

    def __mro_entries__(self, bases):
        return (self.__origin__,)

    def __or__(self, other):
        return Union[self, other]

    def __ror__(self, other):
        return Union[other, self]

    def __eq__(self, other):
        if not isinstance(other, _GenericAlias):
            return NotImplemented
        return self.__origin__ == other.__origin__ and self.__args__ == other.__args__

    def __hash__(self):
        return hash((id(self.__origin__), len(self.__args__)))

    def __call__(self, *args, **kwargs):
        origin = self.__origin__
        if isinstance(origin, _SpecialForm):
            raise TypeError('Cannot instantiate %r' % (self,))
        return origin(*args, **kwargs)


class _BuiltinAlias:
    """`typing.List` e companhia: um apelido de um tipo que aceita `[...]`."""

    def __init__(self, origin, name, nparams=1):
        self._origin = origin
        self._name = name
        self.__origin__ = origin

    def __repr__(self):
        return 'typing.' + self._name

    def __getitem__(self, params):
        if not isinstance(params, tuple):
            params = (params,)
        return _GenericAlias(self._origin, params, name=self._name)

    def __call__(self, *args, **kwargs):
        return self._origin(*args, **kwargs)

    def __or__(self, other):
        return Union[self, other]

    def __ror__(self, other):
        return Union[other, self]

    def __instancecheck__(self, obj):
        return self.__subclasscheck__(type(obj))

    def __subclasscheck__(self, cls):
        if isinstance(cls, _BuiltinAlias):
            return issubclass(cls.__origin__, self.__origin__)
        if not isinstance(cls, _GenericAlias):
            return issubclass(cls, self.__origin__)
        raise TypeError("Subscripted generics cannot be used with class and instance checks")


Any = _SpecialForm('Any')
NoReturn = _SpecialForm('NoReturn')
Never = _SpecialForm('Never')
ClassVar = _SpecialForm('ClassVar')
Final = _SpecialForm('Final')
Optional = _SpecialForm('Optional')
Union = _SpecialForm('Union')
Literal = _SpecialForm('Literal')
Annotated = _SpecialForm('Annotated')
TypeAlias = _SpecialForm('TypeAlias')
Self = _SpecialForm('Self')
LiteralString = _SpecialForm('LiteralString')
Concatenate = _SpecialForm('Concatenate')
TypeGuard = _SpecialForm('TypeGuard')
Required = _SpecialForm('Required')
NotRequired = _SpecialForm('NotRequired')
Unpack = _SpecialForm('Unpack')

List = _BuiltinAlias(list, 'List')
Dict = _BuiltinAlias(dict, 'Dict')
Set = _BuiltinAlias(set, 'Set')
FrozenSet = _BuiltinAlias(frozenset, 'FrozenSet')
Tuple = _BuiltinAlias(tuple, 'Tuple')
Type = _BuiltinAlias(type, 'Type')
Deque = _BuiltinAlias(collections.deque, 'Deque')
DefaultDict = _BuiltinAlias(collections.defaultdict, 'DefaultDict')
OrderedDict = _BuiltinAlias(collections.OrderedDict, 'OrderedDict')
Counter = _BuiltinAlias(collections.Counter, 'Counter')
ChainMap = _BuiltinAlias(collections.ChainMap, 'ChainMap')

Callable = _BuiltinAlias(collections.abc.Callable, 'Callable')
Iterable = _BuiltinAlias(collections.abc.Iterable, 'Iterable')
Iterator = _BuiltinAlias(collections.abc.Iterator, 'Iterator')
Generator = _BuiltinAlias(collections.abc.Generator, 'Generator')
Sequence = _BuiltinAlias(collections.abc.Sequence, 'Sequence')
MutableSequence = _BuiltinAlias(collections.abc.MutableSequence, 'MutableSequence')
Mapping = _BuiltinAlias(collections.abc.Mapping, 'Mapping')
MutableMapping = _BuiltinAlias(collections.abc.MutableMapping, 'MutableMapping')
AbstractSet = _BuiltinAlias(collections.abc.Set, 'AbstractSet')
MutableSet = _BuiltinAlias(collections.abc.MutableSet, 'MutableSet')
Collection = _BuiltinAlias(collections.abc.Collection, 'Collection')
Container = _BuiltinAlias(collections.abc.Container, 'Container')
Reversible = _BuiltinAlias(collections.abc.Reversible, 'Reversible')
Awaitable = _BuiltinAlias(collections.abc.Awaitable, 'Awaitable')
Coroutine = _BuiltinAlias(collections.abc.Coroutine, 'Coroutine')
AsyncIterator = _BuiltinAlias(collections.abc.AsyncIterator, 'AsyncIterator')
AsyncIterable = _BuiltinAlias(collections.abc.AsyncIterable, 'AsyncIterable')
Hashable = collections.abc.Hashable
Sized = collections.abc.Sized
ByteString = collections.abc.ByteString

Text = str
AnyStr = None


class _Lazy:
    """Valor calculado na primeira leitura (limite, restrições e padrão de `def f[T: int = str]`)."""

    __slots__ = ('thunk',)

    def __init__(self, thunk):
        self.thunk = thunk


def _resolve_lazy(owner, attr):
    value = getattr(owner, attr)
    if type(value) is _Lazy:
        value = value.thunk()
        setattr(owner, attr, value)
    return value


class TypeVar:
    def __init__(self, name, *constraints, bound=None, covariant=False, contravariant=False, default=None,
                 infer_variance=False):
        self.__name__ = name
        # Um único `_Lazy` guarda a tupla de restrições inteira, calculada depois.
        self._constraints = constraints[0] if len(constraints) == 1 and type(constraints[0]) is _Lazy else constraints
        self._bound = bound
        self.__covariant__ = covariant
        self.__contravariant__ = contravariant
        self.__infer_variance__ = infer_variance
        self._default = default

    @property
    def __constraints__(self):
        return _resolve_lazy(self, '_constraints')

    @property
    def __bound__(self):
        return _resolve_lazy(self, '_bound')

    @property
    def __default__(self):
        return _resolve_lazy(self, '_default')

    def has_default(self):
        return self._default is not None

    def __repr__(self):
        if self.__infer_variance__:
            return self.__name__
        if self.__covariant__:
            prefix = '+'
        elif self.__contravariant__:
            prefix = '-'
        else:
            prefix = '~'
        return prefix + self.__name__

    def __or__(self, other):
        return Union[self, other]

    def __ror__(self, other):
        return Union[other, self]


class ParamSpec(TypeVar):
    @property
    def args(self):
        return ParamSpecArgs(self)

    @property
    def kwargs(self):
        return ParamSpecKwargs(self)


class TypeVarTuple(TypeVar):
    pass


class TypeAliasType:
    """`type X[T] = ...`: o valor é calculado na primeira leitura de `__value__`."""

    def __init__(self, name, value, *, type_params=()):
        self.__name__ = name
        self.__qualname__ = name
        self.__type_params__ = tuple(type_params)
        self.__module__ = _sys_modules_name()
        self._thunk = value
        self._computed = False
        self._value = None

    @property
    def __value__(self):
        if not self._computed:
            self._value = self._thunk()
            self._computed = True
        return self._value

    def __getitem__(self, params):
        if not isinstance(params, tuple):
            params = (params,)
        return _GenericAlias(self, params)

    def __or__(self, other):
        return Union[self, other]

    def __ror__(self, other):
        return Union[other, self]

    def __repr__(self):
        return self.__name__


def _sys_modules_name():
    import sys
    try:
        return sys._getframe(2).f_globals.get('__name__', '__main__')
    except Exception:
        return '__main__'


AnyStr = TypeVar('AnyStr', str, bytes)


class Generic:
    def __class_getitem__(cls, params):
        if not isinstance(params, tuple):
            params = (params,)
        return _GenericAlias(cls, params)

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)


_PROTOCOL_SKIP = frozenset({
    '__module__', '__doc__', '__dict__', '__annotations__', '_is_protocol', '_is_runtime_protocol',
    '__abstractmethods__', '__parameters__', '__orig_bases__', '__orig_class__', '__init__', '__weakref__',
    '__qualname__', '__slots__', '__subclasshook__', '__class_getitem__', '__init_subclass__', '__protocol_attrs__',
    '__non_callable_proto_members__', '__match_args__', '__static_attributes__', '__firstlineno__', '__new__',
    '__abc_impl__', '_abc_impl', '__type_params__', '__hash__', '__qualname__',
})


def _protocol_attrs(cls):
    attrs = set()
    for base in cls.__mro__[:-1]:
        if base.__name__ in ('Protocol', 'Generic') and base.__module__ == __name__:
            continue
        if not base.__dict__.get('_is_protocol', False):
            continue
        for name in list(base.__dict__) + list(base.__dict__.get('__annotations__', {})):
            if name not in _PROTOCOL_SKIP and not name.startswith('_abc_'):
                attrs.add(name)
    return attrs


class _ProtocolMeta(type):
    def __instancecheck__(cls, instance):
        if not cls.__dict__.get('_is_protocol', False):
            return cls in type(instance).__mro__
        if not cls.__dict__.get('_is_runtime_protocol', False):
            raise TypeError('Instance and class checks can only be used with @runtime_checkable protocols')
        if cls in type(instance).__mro__:
            return True
        for attr in _protocol_attrs(cls):
            try:
                value = getattr(instance, attr)
            except AttributeError:
                return False
            if value is None and callable(getattr(cls, attr, None)):
                return False
        return True

    def __subclasscheck__(cls, other):
        if not cls.__dict__.get('_is_protocol', False):
            return isinstance(other, type) and cls in other.__mro__
        if not cls.__dict__.get('_is_runtime_protocol', False):
            raise TypeError('Instance and class checks can only be used with @runtime_checkable protocols')
        if not isinstance(other, type):
            raise TypeError('issubclass() arg 1 must be a class')
        if cls in other.__mro__:
            return True
        for attr in _protocol_attrs(cls):
            if not any(attr in base.__dict__ or attr in base.__dict__.get('__annotations__', {})
                       for base in other.__mro__):
                return False
        return True


class Protocol(Generic, metaclass=_ProtocolMeta):
    _is_protocol = True

    def __init_subclass__(cls, *args, **kwargs):
        super().__init_subclass__(*args, **kwargs)
        cls._is_protocol = any(b is Protocol for b in cls.__bases__)


def runtime_checkable(cls):
    if not cls.__dict__.get('_is_protocol', False):
        raise TypeError('@runtime_checkable can be only applied to protocol classes, got %r' % cls)
    cls._is_runtime_protocol = True
    return cls


_overload_registry = {}


def _overload_key(func):
    func = getattr(func, '__func__', func)
    return (getattr(func, '__module__', None), getattr(func, '__qualname__', None))


def overload(func):
    """Guarda a variante para `get_overloads`; chamar a variante levanta como no CPython."""
    _overload_registry.setdefault(_overload_key(func), []).append(func)

    def _overload_dummy(*args, **kwds):
        raise NotImplementedError(
            "You should not call an overloaded function. "
            "A series of @typing.overload-decorated functions "
            "outside a stub module should always be followed "
            "by an implementation that is not @typing.overload-decorated.")
    return _overload_dummy


def get_overloads(func):
    return list(_overload_registry.get(_overload_key(func), []))


def clear_overloads():
    _overload_registry.clear()


def final(f):
    return f


def no_type_check(arg):
    return arg


def cast(typ, val):
    return val


def assert_type(val, typ):
    return val


def assert_never(arg):
    raise AssertionError('Expected code to be unreachable')


def reveal_type(obj):
    return obj


def override(method):
    return method


def dataclass_transform(**kwargs):
    return lambda cls_or_fn: cls_or_fn


def NewType(name, tp):
    def new_type(x):
        return x
    new_type.__name__ = name
    new_type.__supertype__ = tp
    return new_type


def get_origin(tp):
    if isinstance(tp, (_GenericAlias, _BuiltinAlias)):
        return tp.__origin__
    import types
    if isinstance(tp, types.UnionType):
        return types.UnionType
    return getattr(tp, '__origin__', None)


def get_args(tp):
    if isinstance(tp, _GenericAlias):
        if tp._name == 'Callable' and isinstance(tp.__args__[0], (list, tuple)):
            return (list(tp.__args__[0]), tp.__args__[1])
        return tp.__args__
    return getattr(tp, '__args__', ())


def _eval_hint(value, globalns, localns):
    if isinstance(value, str):
        value = eval(value, globalns, localns)
    if value is None:
        return type(None)
    return value


def get_type_hints(obj, globalns=None, localns=None, include_extras=False):
    import sys
    hints = {}
    if isinstance(obj, type):
        for base in reversed(obj.__mro__):
            ann = base.__dict__.get('__annotations__', {})
            if globalns is None:
                module = sys.modules.get(base.__module__)
                base_globals = dict(getattr(module, '__dict__', {}))
            else:
                base_globals = globalns
            base_locals = dict(vars(base)) if localns is None else localns
            for name, value in ann.items():
                hints[name] = _eval_hint(value, base_globals, base_locals)
        return hints
    wrapped = obj
    while hasattr(wrapped, '__wrapped__'):
        wrapped = wrapped.__wrapped__
    if globalns is None:
        globalns = getattr(wrapped, '__globals__', None)
        if globalns is None:
            module = sys.modules.get(getattr(obj, '__module__', None))
            globalns = getattr(module, '__dict__', {})
    names = dict(localns) if localns else {}
    for param in getattr(obj, '__type_params__', ()):
        names.setdefault(param.__name__, param)
    for name, value in getattr(obj, '__annotations__', {}).items():
        hints[name] = _eval_hint(value, globalns, names)
    return hints


def is_typeddict(tp):
    return isinstance(tp, _TypedDictMeta)


class _NamedTupleMeta(type):
    def __new__(mcs, name, bases, ns):
        if name == 'NamedTuple' and not bases:
            return super().__new__(mcs, name, bases, ns)
        annotations = ns.get('__annotations__', {})
        fields = list(annotations)
        defaults = [ns[f] for f in fields if f in ns]
        nt = collections.namedtuple(name, fields, defaults=defaults)
        for key, value in ns.items():
            if key not in fields and key not in ('__annotations__', '__module__', '__qualname__'):
                setattr(nt, key, value)
        nt.__annotations__ = annotations
        return nt


class NamedTuple(metaclass=_NamedTupleMeta):
    def __new__(cls, typename, fields=None, **kwargs):
        if fields is None:
            fields = list(kwargs.items())
        return collections.namedtuple(typename, [name for name, _ in fields])


class _TypedDictMeta(type):
    def __new__(mcs, name, bases, ns, total=True):
        cls = super().__new__(mcs, name, bases, ns)
        annotations = {}
        required = set()
        optional = set()
        for base in bases:
            annotations.update(getattr(base, '__annotations__', {}))
            required |= set(getattr(base, '__required_keys__', ()))
            optional |= set(getattr(base, '__optional_keys__', ()))
        own = ns.get('__annotations__', {})
        annotations.update(own)
        (required if total else optional).update(own)
        cls.__annotations__ = annotations
        cls.__total__ = total
        cls.__required_keys__ = frozenset(required)
        cls.__optional_keys__ = frozenset(optional)
        return cls

    def __call__(cls, *args, **kwargs):
        return dict(*args, **kwargs)


class TypedDict(metaclass=_TypedDictMeta):
    pass


class IO(Generic):
    __slots__ = ()


class TextIO(IO):
    pass


class BinaryIO(IO):
    pass


class SupportsInt(Protocol):
    pass


class SupportsFloat(Protocol):
    pass


class SupportsIndex(Protocol):
    pass


class SupportsAbs(Protocol):
    pass


import re as _re
Pattern = _BuiltinAlias(_re.Pattern, 'Pattern')
Match = _BuiltinAlias(_re.Match, 'Match')


class ForwardRef:
    """Referência adiantada: a anotação em texto, avaliada sob demanda."""

    __slots__ = ('__forward_arg__', '__forward_code__',
                 '__forward_evaluated__', '__forward_value__',
                 '__forward_is_argument__', '__forward_is_class__',
                 '__forward_module__')

    def __init__(self, arg, is_argument=True, module=None, *, is_class=False):
        if not isinstance(arg, str):
            raise TypeError(f"Forward reference must be a string -- got {arg!r}")
        arg_to_compile = f'({arg},)[0]' if arg.startswith('*') else arg
        try:
            code = compile(arg_to_compile, '<string>', 'eval')
        except SyntaxError:
            raise SyntaxError(f"Forward reference must be an expression -- got {arg!r}")
        self.__forward_arg__ = arg
        self.__forward_code__ = code
        self.__forward_evaluated__ = False
        self.__forward_value__ = None
        self.__forward_is_argument__ = is_argument
        self.__forward_is_class__ = is_class
        self.__forward_module__ = module

    def __eq__(self, other):
        if not isinstance(other, ForwardRef):
            return NotImplemented
        return (self.__forward_arg__ == other.__forward_arg__
                and self.__forward_module__ == other.__forward_module__)

    def __hash__(self):
        return hash((self.__forward_arg__, self.__forward_module__))

    def __or__(self, other):
        return Union[self, other]

    def __ror__(self, other):
        return Union[other, self]

    def __repr__(self):
        module = '' if self.__forward_module__ is None else f', module={self.__forward_module__!r}'
        return f'ForwardRef({self.__forward_arg__!r}{module})'


# Internos que bibliotecas como o `typing_extensions` leem do módulo (CPython 3.13).
import functools as _functools

ReadOnly = _SpecialForm('ReadOnly')
TypeIs = _SpecialForm('TypeIs')
NoDefault = _SpecialForm('NoDefault')
_ASSERT_NEVER_REPR_MAX_LENGTH = 100
_caches = {}
_cleanups = []
EXCLUDED_ATTRIBUTES = frozenset({
    '__parameters__', '__orig_bases__', '__orig_class__', '_is_protocol', '_is_runtime_protocol',
    '__protocol_attrs__', '__non_callable_proto_members__', '__type_params__', '__abstractmethods__',
    '__annotations__', '__dict__', '__doc__', '__init__', '__module__', '__new__', '__slots__',
    '__subclasshook__', '__weakref__', '__class_getitem__', '__match_args__', '__static_attributes__',
    '__firstlineno__', '_MutableMapping__marker',
})
_SpecialGenericAlias = _BuiltinAlias
_AnnotatedAlias = _GenericAlias


class _ConcatenateGenericAlias(_GenericAlias):
    pass


class _Sentinel:
    __slots__ = ()

    def __repr__(self):
        return '<sentinel>'


_sentinel = _Sentinel()


def _overload_dummy(*args, **kwds):
    """Helper for @overload to raise when called."""
    raise NotImplementedError(
        "You should not call an overloaded function. "
        "A series of @overload-decorated functions "
        "outside a stub module should always be followed "
        "by an implementation that is not @overload-ed.")


def _type_convert(arg, module=None, *, allow_special_forms=False):
    if arg is None:
        return type(None)
    if isinstance(arg, str):
        return ForwardRef(arg, module=module, is_class=allow_special_forms)
    return arg


def _type_check(arg, msg, is_argument=True, module=None, *, allow_special_forms=False):
    invalid_generic_forms = (Generic, Protocol)
    if not allow_special_forms:
        invalid_generic_forms += (ClassVar,)
        if is_argument:
            invalid_generic_forms += (Final,)
    arg = _type_convert(arg, module=module, allow_special_forms=allow_special_forms)
    if isinstance(arg, _GenericAlias) and arg.__origin__ in invalid_generic_forms:
        raise TypeError(f"{arg} is not valid as type argument")
    if arg in (Any, LiteralString, NoReturn, Never, Self, TypeAlias):
        return arg
    if allow_special_forms and arg in (ClassVar, Final):
        return arg
    if isinstance(arg, _SpecialForm) or arg in (Generic, Protocol):
        raise TypeError(f"Plain {arg} is not valid as type argument")
    if type(arg) is tuple:
        raise TypeError(f"{msg} Got {arg!r:.100}.")
    return arg


def _is_param_expr(arg):
    return arg is ... or isinstance(arg, (tuple, list, ParamSpec, _ConcatenateGenericAlias))


def _tp_cache(func=None, /, *, typed=False):
    def decorator(func):
        cache = _functools.lru_cache(typed=typed)(func)
        _caches[func] = cache
        _cleanups.append(cache.cache_clear)
        del cache

        @_functools.wraps(func)
        def inner(*args, **kwds):
            try:
                return _caches[func](*args, **kwds)
            except TypeError:
                pass
            return func(*args, **kwds)
        return inner

    if func is not None:
        return decorator(func)
    return decorator


def _eval_type(t, globalns, localns, type_params=_sentinel, *, recursive_guard=frozenset()):
    if isinstance(t, ForwardRef):
        return eval(t.__forward_code__, globalns, localns)
    if isinstance(t, str):
        return eval(t, globalns, localns)
    return t


def _get_defaults(func):
    code = func.__code__
    pos_count = code.co_argcount
    arg_names = code.co_varnames[:pos_count]
    defaults = func.__defaults__ or ()
    kwdefaults = func.__kwdefaults__
    res = dict(kwdefaults) if kwdefaults else {}
    pos_offset = pos_count - len(defaults)
    for name, value in zip(arg_names[pos_offset:], defaults):
        res[name] = value
    return res


def is_protocol(tp, /):
    return isinstance(tp, type) and getattr(tp, '_is_protocol', False) and tp != Protocol


def get_protocol_members(tp, /):
    if not is_protocol(tp):
        raise TypeError(f'{tp!r} is not a Protocol')
    return frozenset(_protocol_attrs(tp))


class ParamSpecArgs:
    def __init__(self, origin):
        self.__origin__ = origin

    def __repr__(self):
        return f"{self.__origin__.__name__}.args"


class ParamSpecKwargs:
    def __init__(self, origin):
        self.__origin__ = origin

    def __repr__(self):
        return f"{self.__origin__.__name__}.kwargs"


class SupportsBytes(Protocol):
    pass


class SupportsComplex(Protocol):
    pass


class SupportsRound(Protocol):
    pass


ItemsView = _BuiltinAlias(collections.abc.ItemsView, 'ItemsView')
KeysView = _BuiltinAlias(collections.abc.KeysView, 'KeysView')
ValuesView = _BuiltinAlias(collections.abc.ValuesView, 'ValuesView')
MappingView = _BuiltinAlias(collections.abc.MappingView, 'MappingView')


def no_type_check_decorator(decorator):
    return decorator


import contextlib as _contextlib
AsyncGenerator = _BuiltinAlias(collections.abc.AsyncGenerator, 'AsyncGenerator')
ContextManager = _BuiltinAlias(_contextlib.AbstractContextManager, 'ContextManager')
AsyncContextManager = _BuiltinAlias(_contextlib.AbstractAsyncContextManager, 'AsyncContextManager')
