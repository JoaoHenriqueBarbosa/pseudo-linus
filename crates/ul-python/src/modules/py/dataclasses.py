"""dataclasses do sandbox (Python embutido)."""

import copy
import typing

__all__ = [
    'dataclass', 'field', 'Field', 'FrozenInstanceError', 'InitVar', 'KW_ONLY', 'MISSING',
    'fields', 'asdict', 'astuple', 'make_dataclass', 'replace', 'is_dataclass',
]


class FrozenInstanceError(AttributeError):
    pass


class _MISSING_TYPE:
    def __repr__(self):
        return 'MISSING'


MISSING = _MISSING_TYPE()


class _KW_ONLY_TYPE:
    pass


KW_ONLY = _KW_ONLY_TYPE()


class InitVar:
    def __init__(self, type):
        self.type = type

    def __class_getitem__(cls, type):
        return InitVar(type)

    def __repr__(self):
        return 'dataclasses.InitVar[%s]' % getattr(self.type, '__name__', repr(self.type))


class _FieldKind:
    def __init__(self, name):
        self.name = name

    def __repr__(self):
        return self.name


_FIELD = _FieldKind('_FIELD')
_FIELD_CLASSVAR = _FieldKind('_FIELD_CLASSVAR')
_FIELD_INITVAR = _FieldKind('_FIELD_INITVAR')


class Field:
    def __init__(self, default, default_factory, init, repr, hash, compare, metadata, kw_only):
        self.name = None
        self.type = None
        self.default = default
        self.default_factory = default_factory
        self.init = init
        self.repr = repr
        self.hash = hash
        self.compare = compare
        self.metadata = metadata if metadata is not None else {}
        self.kw_only = kw_only
        self._field_type = None

    def __repr__(self):
        return ('Field(name=%r,type=%r,default=%r,default_factory=%r,init=%r,repr=%r,hash=%r,'
                'compare=%r,metadata=%r,kw_only=%r,_field_type=%r)'
                % (self.name, self.type, self.default, self.default_factory, self.init, self.repr,
                   self.hash, self.compare, self.metadata, self.kw_only, self._field_type))


def field(*, default=MISSING, default_factory=MISSING, init=True, repr=True, hash=None, compare=True,
          metadata=None, kw_only=MISSING):
    if default is not MISSING and default_factory is not MISSING:
        raise ValueError('cannot specify both default and default_factory')
    return Field(default, default_factory, init, repr, hash, compare, metadata, kw_only)


def _is_classvar(annotation):
    if annotation is typing.ClassVar:
        return True
    if isinstance(annotation, typing._GenericAlias) and annotation.__origin__ is typing.ClassVar:
        return True
    if isinstance(annotation, str):
        return annotation.startswith('ClassVar') or annotation.startswith('typing.ClassVar')
    return False


def _is_initvar(annotation):
    if annotation is InitVar or isinstance(annotation, InitVar):
        return True
    if isinstance(annotation, str):
        return annotation.startswith('InitVar') or annotation.startswith('dataclasses.InitVar')
    return False


def _plural(n, word):
    return '%d %s%s' % (n, word, '' if n == 1 else 's')


def _join_names(names):
    quoted = ["'%s'" % n for n in names]
    if len(quoted) == 1:
        return quoted[0]
    if len(quoted) == 2:
        return quoted[0] + ' and ' + quoted[1]
    return ', '.join(quoted[:-1]) + ', and ' + quoted[-1]


def _make_init(cls, all_fields, frozen):
    init_fields = [f for f in all_fields if f.init]
    positional = [f for f in init_fields if not f.kw_only]
    keyword_only = [f for f in init_fields if f.kw_only]
    qualname = cls.__qualname__
    has_post_init = hasattr(cls, '__post_init__')

    def __init__(self, *args, **kwargs):
        if len(args) > len(positional):
            required = len([f for f in positional if f.default is MISSING and f.default_factory is MISSING])
            top = len(positional) + 1
            if required + 1 == top:
                expected = '%d positional argument%s' % (top, '' if top == 1 else 's')
            else:
                expected = 'from %d to %d positional arguments' % (required + 1, top)
            raise TypeError('%s.__init__() takes %s but %d were given' % (qualname, expected, len(args) + 1))
        values = {}
        for f, value in zip(positional, args):
            values[f.name] = value
        for name, value in kwargs.items():
            if name in values:
                raise TypeError("%s.__init__() got multiple values for argument '%s'" % (qualname, name))
            if not any(f.name == name for f in init_fields):
                raise TypeError("%s.__init__() got an unexpected keyword argument '%s'" % (qualname, name))
            values[name] = value
        missing = [f.name for f in positional if f.name not in values
                   and f.default is MISSING and f.default_factory is MISSING]
        if missing:
            raise TypeError('%s.__init__() missing %s required positional argument%s: %s'
                            % (qualname, len(missing), '' if len(missing) == 1 else 's', _join_names(missing)))
        missing = [f.name for f in keyword_only if f.name not in values
                   and f.default is MISSING and f.default_factory is MISSING]
        if missing:
            raise TypeError('%s.__init__() missing %s required keyword-only argument%s: %s'
                            % (qualname, len(missing), '' if len(missing) == 1 else 's', _join_names(missing)))
        post = []
        for f in all_fields:
            if f.init:
                if f.name in values:
                    value = values[f.name]
                elif f.default_factory is not MISSING:
                    value = f.default_factory()
                else:
                    value = f.default
            else:
                if f.default_factory is not MISSING:
                    value = f.default_factory()
                elif f.default is not MISSING:
                    value = f.default
                else:
                    continue
            if f._field_type is _FIELD_INITVAR:
                post.append(value)
            elif frozen:
                object.__setattr__(self, f.name, value)
            else:
                setattr(self, f.name, value)
        if has_post_init:
            self.__post_init__(*post)

    return __init__


def _process_class(cls, init, repr, eq, order, unsafe_hash, frozen, match_args, kw_only, slots):
    fields_map = {}
    for base in reversed(cls.__mro__[1:]):
        base_fields = base.__dict__.get('__dataclass_fields__')
        if base_fields:
            for f in base_fields.values():
                fields_map[f.name] = f
    annotations = cls.__dict__.get('__annotations__', {})
    own = []
    kw_only_marker = kw_only
    for name, annotation in annotations.items():
        if annotation is KW_ONLY or (isinstance(annotation, type) and annotation is _KW_ONLY_TYPE):
            kw_only_marker = True
            continue
        default = cls.__dict__.get(name, MISSING)
        if isinstance(default, Field):
            f = default
        else:
            if isinstance(default, (list, dict, set)):
                raise ValueError('mutable default %s for field %s is not allowed: use default_factory'
                                 % (type(default), name))
            f = field(default=default)
        f.name = name
        f.type = annotation
        if _is_classvar(annotation):
            f._field_type = _FIELD_CLASSVAR
        elif _is_initvar(annotation):
            f._field_type = _FIELD_INITVAR
        else:
            f._field_type = _FIELD
        if f.kw_only is MISSING:
            f.kw_only = kw_only_marker
        if f._field_type is _FIELD_CLASSVAR:
            if f.default is not MISSING:
                setattr(cls, name, f.default)
            continue
        if f.default is MISSING:
            if name in cls.__dict__:
                delattr(cls, name)
        else:
            setattr(cls, name, f.default)
        fields_map[name] = f
        own.append(f)
    ordered = list(fields_map.values())
    seen_default = None
    for f in ordered:
        if f._field_type is _FIELD_CLASSVAR or not f.init or f.kw_only:
            continue
        if f.default is not MISSING or f.default_factory is not MISSING:
            seen_default = f.name
        elif seen_default is not None:
            raise TypeError('non-default argument %r follows default argument %r' % (f.name, seen_default))
    cls.__dataclass_fields__ = fields_map
    cls.__dataclass_params__ = (init, repr, eq, order, unsafe_hash, frozen)
    real = [f for f in ordered if f._field_type is not _FIELD_CLASSVAR]
    init_fields = [f for f in real if f.init]
    if init and '__init__' not in cls.__dict__:
        cls.__init__ = _make_init(cls, real, frozen)
    stored = [f for f in real if f._field_type is _FIELD]
    if repr and '__repr__' not in cls.__dict__:
        repr_fields = [f for f in stored if f.repr]

        def __repr__(self):
            return '%s(%s)' % (self.__class__.__qualname__,
                               ', '.join('%s=%r' % (f.name, getattr(self, f.name)) for f in repr_fields))

        cls.__repr__ = __repr__
    cmp_fields = [f for f in stored if f.compare]
    if eq and '__eq__' not in cls.__dict__:
        def __eq__(self, other):
            if other.__class__ is self.__class__:
                return (tuple(getattr(self, f.name) for f in cmp_fields)
                        == tuple(getattr(other, f.name) for f in cmp_fields))
            return NotImplemented

        cls.__eq__ = __eq__
    if order:
        def _cmp(op):
            def method(self, other):
                if other.__class__ is self.__class__:
                    return op(tuple(getattr(self, f.name) for f in cmp_fields),
                              tuple(getattr(other, f.name) for f in cmp_fields))
                return NotImplemented
            return method

        import operator
        cls.__lt__ = _cmp(operator.lt)
        cls.__le__ = _cmp(operator.le)
        cls.__gt__ = _cmp(operator.gt)
        cls.__ge__ = _cmp(operator.ge)
    has_explicit_hash = '__hash__' in cls.__dict__ and cls.__dict__['__hash__'] is not None
    if unsafe_hash or (eq and frozen):
        if not has_explicit_hash:
            hash_fields = [f for f in stored if (f.compare if f.hash is None else f.hash)]

            def __hash__(self):
                return hash(tuple(getattr(self, f.name) for f in hash_fields))

            cls.__hash__ = __hash__
    elif eq and not frozen and '__hash__' not in cls.__dict__:
        cls.__hash__ = None
    if frozen:
        names = [f.name for f in real]

        def __setattr__(self, name, value):
            if type(self) is cls or name in names:
                raise FrozenInstanceError("cannot assign to field %r" % name)
            object.__setattr__(self, name, value)

        def __delattr__(self, name):
            if type(self) is cls or name in names:
                raise FrozenInstanceError("cannot delete field %r" % name)
            object.__delattr__(self, name)

        cls.__setattr__ = __setattr__
        cls.__delattr__ = __delattr__
    if match_args:
        cls.__match_args__ = tuple(f.name for f in init_fields if not f.kw_only)
    return cls


def dataclass(cls=None, /, *, init=True, repr=True, eq=True, order=False, unsafe_hash=False, frozen=False,
              match_args=True, kw_only=False, slots=False, weakref_slot=False):
    def wrap(cls):
        return _process_class(cls, init, repr, eq, order, unsafe_hash, frozen, match_args, kw_only, slots)

    if cls is None:
        return wrap
    return wrap(cls)


def fields(class_or_instance):
    try:
        fields_map = class_or_instance.__dataclass_fields__
    except AttributeError:
        raise TypeError('must be called with a dataclass type or instance') from None
    return tuple(f for f in fields_map.values() if f._field_type is _FIELD)


def is_dataclass(obj):
    cls = obj if isinstance(obj, type) else type(obj)
    return hasattr(cls, '__dataclass_fields__')


def _is_dataclass_instance(obj):
    return hasattr(type(obj), '__dataclass_fields__')


def asdict(obj, *, dict_factory=dict):
    if not _is_dataclass_instance(obj):
        raise TypeError('asdict() should be called on dataclass instances')
    return _asdict_inner(obj, dict_factory)


def _asdict_inner(obj, dict_factory):
    if _is_dataclass_instance(obj):
        return dict_factory([(f.name, _asdict_inner(getattr(obj, f.name), dict_factory)) for f in fields(obj)])
    if isinstance(obj, tuple) and hasattr(obj, '_fields'):
        return type(obj)(*[_asdict_inner(v, dict_factory) for v in obj])
    if isinstance(obj, (list, tuple)):
        return type(obj)(_asdict_inner(v, dict_factory) for v in obj)
    if isinstance(obj, dict):
        return type(obj)((_asdict_inner(k, dict_factory), _asdict_inner(v, dict_factory)) for k, v in obj.items())
    return copy.deepcopy(obj)


def astuple(obj, *, tuple_factory=tuple):
    if not _is_dataclass_instance(obj):
        raise TypeError('astuple() should be called on dataclass instances')
    return _astuple_inner(obj, tuple_factory)


def _astuple_inner(obj, tuple_factory):
    if _is_dataclass_instance(obj):
        return tuple_factory([_astuple_inner(getattr(obj, f.name), tuple_factory) for f in fields(obj)])
    if isinstance(obj, tuple) and hasattr(obj, '_fields'):
        return type(obj)(*[_astuple_inner(v, tuple_factory) for v in obj])
    if isinstance(obj, (list, tuple)):
        return type(obj)(_astuple_inner(v, tuple_factory) for v in obj)
    if isinstance(obj, dict):
        return type(obj)((_astuple_inner(k, tuple_factory), _astuple_inner(v, tuple_factory)) for k, v in obj.items())
    return copy.deepcopy(obj)


def replace(obj, /, **changes):
    if not _is_dataclass_instance(obj):
        raise TypeError('replace() should be called on dataclass instances')
    for f in obj.__dataclass_fields__.values():
        if f._field_type is _FIELD_CLASSVAR:
            continue
        if not f.init:
            if f.name in changes:
                raise ValueError('field %s is declared with init=False, it cannot be specified with replace()'
                                 % f.name)
            continue
        if f.name not in changes:
            if f._field_type is _FIELD_INITVAR and f.default is MISSING:
                raise ValueError("InitVar %r must be specified with replace()" % (f.name,))
            changes[f.name] = getattr(obj, f.name)
    return obj.__class__(**changes)


def make_dataclass(cls_name, fields, *, bases=(), namespace=None, init=True, repr=True, eq=True, order=False,
                   unsafe_hash=False, frozen=False, match_args=True, kw_only=False, slots=False):
    ns = dict(namespace) if namespace else {}
    annotations = {}
    for item in fields:
        if isinstance(item, str):
            annotations[item] = typing.Any
        elif len(item) == 2:
            annotations[item[0]] = item[1]
        else:
            annotations[item[0]] = item[1]
            ns[item[0]] = item[2]
    ns['__annotations__'] = annotations
    cls = type(cls_name, bases, ns)
    return dataclass(cls, init=init, repr=repr, eq=eq, order=order, unsafe_hash=unsafe_hash, frozen=frozen,
                     match_args=match_args, kw_only=kw_only, slots=slots)
