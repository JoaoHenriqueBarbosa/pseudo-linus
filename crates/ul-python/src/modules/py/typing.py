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


class _SpecialForm:
    def __init__(self, name):
        self._name = name
        self.__name__ = name

    def __repr__(self):
        return 'typing.' + self._name

    def __getitem__(self, params):
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


class TypeVar:
    def __init__(self, name, *constraints, bound=None, covariant=False, contravariant=False, default=None,
                 infer_variance=False):
        self.__name__ = name
        self.__constraints__ = constraints
        self.__bound__ = bound
        self.__covariant__ = covariant
        self.__contravariant__ = contravariant
        self.__infer_variance__ = infer_variance
        self.__default__ = default

    def has_default(self):
        return self.__default__ is not None

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
    pass


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


class IO:
    pass


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


class Pattern:
    pass


class Match:
    pass
