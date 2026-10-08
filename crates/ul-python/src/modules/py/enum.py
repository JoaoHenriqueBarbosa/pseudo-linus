import sys
import builtins as bltns
from functools import partial
from types import MappingProxyType, DynamicClassAttribute


__all__ = [
        'EnumType', 'EnumMeta', 'EnumDict',
        'Enum', 'IntEnum', 'StrEnum', 'Flag', 'IntFlag', 'ReprEnum',
        'auto', 'unique', 'property', 'verify', 'member', 'nonmember',
        'FlagBoundary', 'STRICT', 'CONFORM', 'EJECT', 'KEEP',
        'global_flag_repr', 'global_enum_repr', 'global_str', 'global_enum',
        'EnumCheck', 'CONTINUOUS', 'NAMED_FLAGS', 'UNIQUE',
        'pickle_by_global_name', 'pickle_by_enum_name',
        ]


class nonmember(object):
    """
    Protects item from becoming an Enum member during class creation.
    """
    def __init__(self, value):
        self.value = value

class member(object):
    """
    Forces item to become an Enum member during class creation.
    """
    def __init__(self, value):
        self.value = value


def _is_dunder(name):
    """
    Returns True if a __dunder__ name, False otherwise.
    """
    return (
            len(name) > 4 and
            name[:2] == name[-2:] == '__' and
            name[2] != '_' and
            name[-3] != '_'
            )

def _is_sunder(name):
    """
    Returns True if a _sunder_ name, False otherwise.
    """
    return (
            len(name) > 2 and
            name[0] == name[-1] == '_' and
            name[1] != '_' and
            name[-2] != '_'
            )

def _is_internal_class(cls_name, obj):
    # do not use `re` as `re` imports `enum`
    if not isinstance(obj, type):
        return False
    qualname = getattr(obj, '__qualname__', '')
    s_pattern = cls_name + '.' + getattr(obj, '__name__', '')
    e_pattern = '.' + s_pattern
    return qualname == s_pattern or qualname.endswith(e_pattern)

def _is_private(cls_name, name):
    # do not use `re` as `re` imports `enum`
    pattern = '_%s__' % (cls_name, )
    pat_len = len(pattern)
    if (
            len(name) > pat_len
            and name.startswith(pattern)
            and (name[-1] != '_' or name[-2] != '_')
        ):
        return True
    else:
        return False

def _is_single_bit(num):
    """
    True if only one bit set in num (should be an int)
    """
    if num == 0:
        return False
    num &= num - 1
    return num == 0

def _make_class_unpicklable(obj):
    """
    Make the given obj un-picklable.

    obj should be either a dictionary, or an Enum
    """
    def _break_on_call_reduce(self, proto):
        raise TypeError('%r cannot be pickled' % self)
    if isinstance(obj, dict):
        obj['__reduce_ex__'] = _break_on_call_reduce
        obj['__module__'] = '<unknown>'
    else:
        setattr(obj, '__reduce_ex__', _break_on_call_reduce)
        setattr(obj, '__module__', '<unknown>')

def _iter_bits_lsb(num):
    # num must be a positive integer
    original = num
    if isinstance(num, Enum):
        num = num.value
    if num < 0:
        raise ValueError('%r is not a positive integer' % original)
    while num:
        b = num & (~num + 1)
        yield b
        num ^= b

def show_flag_values(value):
    return list(_iter_bits_lsb(value))

def bin(num, max_bits=None):
    """
    Like built-in bin(), except negative values are represented in
    twos-compliment, and the leading bit always indicates sign
    (0=positive, 1=negative).

    >>> bin(10)
    '0b0 1010'
    >>> bin(~10)   # ~10 is -11
    '0b1 0101'
    """

    ceiling = 2 ** (num).bit_length()
    if num >= 0:
        s = bltns.bin(num + ceiling).replace('1', '0', 1)
    else:
        s = bltns.bin(~num ^ (ceiling - 1) + ceiling)
    sign = s[:3]
    digits = s[3:]
    if max_bits is not None:
        if len(digits) < max_bits:
            digits = (sign[-1] * max_bits + digits)[-max_bits:]
    return "%s %s" % (sign, digits)

def _dedent(text):
    """
    Like textwrap.dedent.  Rewritten because we cannot import textwrap.
    """
    lines = text.split('\n')
    for i, ch in enumerate(lines[0]):
        if ch != ' ':
            break
    for j, l in enumerate(lines):
        lines[j] = l[i:]
    return '\n'.join(lines)

class _not_given:
    def __repr__(self):
        return('<not given>')
_not_given = _not_given()

class _auto_null:
    def __repr__(self):
        return '_auto_null'
_auto_null = _auto_null()

class auto:
    """
    Instances are replaced with an appropriate value in Enum class suites.
    """
    def __init__(self, value=_auto_null):
        self.value = value

    def __repr__(self):
        return "auto(%r)" % self.value


def _is_descriptor(value):
    kind = type(value).__name__
    return kind in ('function', 'property', 'classmethod', 'staticmethod', 'method', 'builtin_function_or_method')


class EnumType(type):
    def __new__(mcs, name, bases, ns, **kwds):
        member_type = object
        first_enum = None
        for base in bases:
            if isinstance(base, EnumMeta):
                if base._member_names_:
                    raise TypeError('<enum %r> cannot extend %r' % (name, base))
                if first_enum is None:
                    first_enum = base
            elif base in (int, str, float, tuple, bytes) and member_type is object:
                member_type = base
        if member_type is object and first_enum is not None:
            member_type = first_enum._member_type_
        generate = None
        for base in bases:
            if hasattr(base, '_generate_next_value_'):
                generate = base._generate_next_value_
                break
        if '_generate_next_value_' in ns:
            generate = ns['_generate_next_value_']
        members = []
        clean = {}
        for key, value in ns.items():
            if _is_dunder(key) or _is_sunder(key) or _is_descriptor(value) or isinstance(value, type):
                clean[key] = value
            else:
                members.append((key, value))
        cls = super().__new__(mcs, name, bases, clean)
        cls._member_names_ = []
        cls._member_map_ = {}
        cls._value2member_map_ = {}
        cls._member_type_ = member_type
        if first_enum is not None:
            cls._generate_next_value_ = clean.get('_generate_next_value_', generate)
        last_values = []
        for key, value in members:
            if isinstance(value, auto):
                if value.value is not _auto_null:
                    value = value.value
                elif generate is None:
                    raise TypeError('auto() needs _generate_next_value_')
                else:
                    value = generate(key, 1, len(last_values), last_values[:])
            args = value if isinstance(value, tuple) and member_type is not tuple else (value,)
            if '__new__' in clean:
                # `__new__` do corpo do Enum (ex.: `int.__new__(cls, value)` + atributos): cria o membro.
                member = clean['__new__'](cls, *args)
                if not hasattr(member, '_value_'):
                    member._value_ = args[0] if len(args) == 1 else value
                value = member._value_
                args = (value,)
            elif member_type is object:
                member = object.__new__(cls)
                if '__init__' in clean or hasattr(cls, '__init__') and cls.__init__ is not object.__init__:
                    pass
            else:
                member = member_type.__new__(cls, *args)
            member._name_ = key
            member.__objclass__ = cls
            member._value_ = value if member_type is object else (args[0] if len(args) == 1 else value)
            last_values.append(member._value_)
            existing = None
            try:
                existing = cls._value2member_map_.get(member._value_)
            except TypeError:
                for other in cls._member_map_.values():
                    if other._value_ == member._value_:
                        existing = other
                        break
            if existing is not None:
                setattr(cls, key, existing)
                cls._member_map_[key] = existing
                continue
            setattr(cls, key, member)
            cls._member_names_.append(key)
            cls._member_map_[key] = member
            try:
                cls._value2member_map_[member._value_] = member
            except TypeError:
                pass
        return cls

    def __call__(cls, value, names=None, *args, start=1, **kwds):
        if names is not None:
            return cls._create_(value, names, start=start)
        if type(value) is cls:
            return value
        try:
            return cls._value2member_map_[value]
        except (KeyError, TypeError):
            for member in cls._member_map_.values():
                if member._value_ == value:
                    return member
        result = cls._missing_(value)
        if result is None:
            raise ValueError('%r is not a valid %s' % (value, cls.__name__))
        return result

    def _create_(cls, class_name, names, start=1):
        bases = (cls,)
        if isinstance(names, str):
            names = names.replace(',', ' ').split()
        ns = {}
        if isinstance(names, dict):
            ns.update(names)
        else:
            for index, item in enumerate(names):
                if isinstance(item, str):
                    ns[item] = index + start
                else:
                    ns[item[0]] = item[1]
        return EnumMeta.__new__(EnumMeta, class_name, bases, ns)

    def _convert_(cls, name, module, filter, source=None, *, boundary=None, as_global=False):
        """Enum a partir das constantes globais de `module` (ou de `source`) que passam em `filter`."""
        import sys
        module_globals = sys.modules[module].__dict__
        caller = sys._getframe(1).f_globals
        if caller.get('__name__') == module:
            module_globals = caller
        source = source.__dict__ if source else module_globals
        members = [(n, v) for n, v in source.items() if filter(n)]
        try:
            members.sort(key=lambda t: (t[1], t[0]))
        except TypeError:
            members.sort(key=lambda t: t[0])
        ns = {n: v for n, v in members}
        ns['__module__'] = module
        new = EnumMeta.__new__(EnumMeta, name, (cls,), ns)
        if as_global:
            global_enum(new)
        else:
            sys.modules[new.__module__].__dict__.update(new.__members__)
        module_globals[name] = new
        return new

    def __iter__(cls):
        return iter([cls._member_map_[name] for name in cls._member_names_])

    def __reversed__(cls):
        return iter([cls._member_map_[name] for name in reversed(cls._member_names_)])

    def __len__(cls):
        return len(cls._member_names_)

    def __contains__(cls, value):
        if isinstance(value, cls):
            return True
        return value in cls._value2member_map_

    def __getitem__(cls, name):
        return cls._member_map_[name]

    def __repr__(cls):
        return "<%s %r>" % ('flag' if issubclass(cls, Flag) else 'enum', cls.__name__)

    @property
    def __members__(cls):
        return dict(cls._member_map_)


EnumMeta = EnumType


class Enum(metaclass=EnumType):
    @property
    def name(self):
        return self._name_

    @property
    def value(self):
        return self._value_

    @classmethod
    def _missing_(cls, value):
        return None

    @staticmethod
    def _generate_next_value_(name, start, count, last_values):
        for last in reversed(last_values):
            try:
                return last + 1
            except TypeError:
                pass
        return start

    def __repr__(self):
        return '<%s.%s: %r>' % (self.__class__.__name__, self._name_, self._value_)

    def __str__(self):
        return '%s.%s' % (self.__class__.__name__, self._name_)

    def __format__(self, spec):
        return format(str(self), spec)

    def __reduce_ex__(self, proto):
        return self.__class__, (self._value_,)


class ReprEnum(Enum):
    pass


class IntEnum(int, ReprEnum):
    def __str__(self):
        return str(self._value_)

    def __format__(self, spec):
        return format(self._value_, spec)


class StrEnum(str, ReprEnum):
    @staticmethod
    def _generate_next_value_(name, start, count, last_values):
        return name.lower()

    def __str__(self):
        return self._value_

    def __format__(self, spec):
        return format(self._value_, spec)


def pickle_by_global_name(self, proto):
    # should not be used with Flag-type enums
    return self.name
_reduce_ex_by_global_name = pickle_by_global_name

def pickle_by_enum_name(self, proto):
    # should not be used with Flag-type enums
    return getattr, (self.__class__, self._name_)

class FlagBoundary(StrEnum):
    """
    control how out of range values are handled
    "strict" -> error is raised             [default for Flag]
    "conform" -> extra bits are discarded
    "eject" -> lose flag status
    "keep" -> keep flag status and all bits [default for IntFlag]
    """
    STRICT = auto()
    CONFORM = auto()
    EJECT = auto()
    KEEP = auto()
STRICT, CONFORM, EJECT, KEEP = FlagBoundary


def _high_bit(value):
    return value.bit_length() - 1


class Flag(Enum):
    @staticmethod
    def _generate_next_value_(name, start, count, last_values):
        if not count:
            return start if start is not None else 1
        high = max(last_values)
        return 2 ** (_high_bit(high) + 1)

    _boundary_keep_ = False
    _boundary_ = STRICT

    @classmethod
    def _missing_(cls, value):
        if not isinstance(value, int):
            raise ValueError('%r is not a valid %s' % (value, cls.__name__))
        known = 0
        for member in cls._member_map_.values():
            known |= member._value_
        if value & ~known and not cls._boundary_keep_:
            raise ValueError('%r is not a valid %s' % (value, cls.__name__))
        member_type = cls._member_type_
        member = object.__new__(cls) if member_type is object else member_type.__new__(cls, value)
        member._name_ = None
        member._value_ = value
        names = member._members_in()
        if names and not value & ~known:
            member._name_ = '|'.join(names)
        cls._value2member_map_[value] = member
        return member

    def __iter__(self):
        """Os membros de um bit só contidos no valor, em ordem crescente de valor."""
        singles = [m for m in self.__class__._member_map_.values()
                   if m._value_ and m._value_ & (m._value_ - 1) == 0]
        singles.sort(key=lambda m: m._value_)
        seen = set()
        for m in singles:
            if m._value_ & self._value_ == m._value_ and m._value_ not in seen:
                seen.add(m._value_)
                yield m

    def __len__(self):
        return sum(1 for _ in self)

    def _known_bits(self):
        known = 0
        for member in self.__class__._member_map_.values():
            known |= member._value_
        return known

    def _members_in(self):
        names = []
        known = 0
        for name in self.__class__._member_names_:
            member = self.__class__._member_map_[name]
            known |= member._value_
            if member._value_ and member._value_ & self._value_ == member._value_:
                names.append(name)
        extra = self._value_ & ~known
        if extra:
            names.append(str(extra))
        return names

    def __repr__(self):
        cls_name = self.__class__.__name__
        if self._name_ is not None:
            return '<%s.%s: %r>' % (cls_name, self._name_, self._value_)
        names = self._members_in()
        if not names or (len(names) == 1 and self._value_ & ~self._known_bits() == self._value_):
            return '<%s: %r>' % (cls_name, self._value_)
        return '<%s.%s: %r>' % (cls_name, '|'.join(names), self._value_)

    def __str__(self):
        cls_name = self.__class__.__name__
        if self._name_ is not None:
            return '%s.%s' % (cls_name, self._name_)
        names = self._members_in()
        if not names:
            return '%s(%r)' % (cls_name, self._value_)
        return '%s.%s' % (cls_name, '|'.join(names))

    def __contains__(self, other):
        if not isinstance(other, self.__class__):
            raise TypeError("unsupported operand type(s) for 'in': '%s' and '%s'"
                            % (type(other).__qualname__, self.__class__.__qualname__))
        return other._value_ & self._value_ == other._value_

    def __bool__(self):
        return bool(self._value_)

    def __or__(self, other):
        if not isinstance(other, self.__class__):
            return NotImplemented
        return self.__class__(self._value_ | other._value_)

    def __and__(self, other):
        if not isinstance(other, self.__class__):
            return NotImplemented
        return self.__class__(self._value_ & other._value_)

    def __xor__(self, other):
        if not isinstance(other, self.__class__):
            return NotImplemented
        return self.__class__(self._value_ ^ other._value_)

    def __invert__(self):
        known = 0
        for member in self.__class__._member_map_.values():
            known |= member._value_
        return self.__class__(known & ~self._value_)


class IntFlag(int, ReprEnum, Flag):
    _boundary_keep_ = True
    _boundary_ = KEEP
    _generate_next_value_ = staticmethod(Flag._generate_next_value_)

    def __str__(self):
        return str(self._value_)

    def __format__(self, spec):
        return format(self._value_, spec)

    def __or__(self, other):
        value = other._value_ if isinstance(other, self.__class__) else other
        return self.__class__(self._value_ | value)

    def __and__(self, other):
        value = other._value_ if isinstance(other, self.__class__) else other
        return self.__class__(self._value_ & value)

    def __xor__(self, other):
        value = other._value_ if isinstance(other, self.__class__) else other
        return self.__class__(self._value_ ^ value)

    __ror__ = __or__
    __rand__ = __and__
    __rxor__ = __xor__


def _dataclass_repr(self):
    dcf = self.__dataclass_fields__
    return ', '.join(
            '%s=%r' % (k, getattr(self, k))
            for k in dcf.keys()
            if dcf[k].repr
            )

def global_enum_repr(self):
    """
    use module.enum_name instead of class.enum_name

    the module is the last module in case of a multi-module name
    """
    module = self.__class__.__module__.split('.')[-1]
    return '%s.%s' % (module, self._name_)

def global_flag_repr(self):
    """
    use module.flag_name instead of class.flag_name

    the module is the last module in case of a multi-module name
    """
    module = self.__class__.__module__.split('.')[-1]
    cls_name = self.__class__.__name__
    if self._name_ is None:
        return "%s.%s(%r)" % (module, cls_name, self._value_)
    if _is_single_bit(self._value_):
        return '%s.%s' % (module, self._name_)
    if self._boundary_ is not FlagBoundary.KEEP:
        return '|'.join(['%s.%s' % (module, name) for name in self.name.split('|')])
    else:
        name = []
        for n in self._name_.split('|'):
            if n[0].isdigit():
                name.append(n)
            else:
                name.append('%s.%s' % (module, n))
        return '|'.join(name)

def global_str(self):
    """
    use enum_name instead of class.enum_name
    """
    if self._name_ is None:
        cls_name = self.__class__.__name__
        return "%s(%r)" % (cls_name, self._value_)
    else:
        return self._name_

def global_enum(cls, update_str=False):
    """
    decorator that makes the repr() of an enum member reference its module
    instead of its class; also exports all members to the enum's module's
    global namespace
    """
    if issubclass(cls, Flag):
        cls.__repr__ = global_flag_repr
    else:
        cls.__repr__ = global_enum_repr
    if not issubclass(cls, ReprEnum) or update_str:
        cls.__str__ = global_str
    sys.modules[cls.__module__].__dict__.update(cls.__members__)
    return cls


def _simple_enum(etype=Enum, *, boundary=None, use_args=None):
    """Recria a classe decorada como um Enum de `etype`, com os atributos dela como membros."""
    def convert(cls):
        ns = {}
        for key, value in cls.__dict__.items():
            if key in ('__dict__', '__weakref__'):
                continue
            ns[key] = value
        return type(etype)(cls.__name__, (etype,), ns)
    return convert


def unique(enumeration):
    duplicates = []
    for name, member in enumeration._member_map_.items():
        if name != member._name_:
            duplicates.append((name, member._name_))
    if duplicates:
        alias_details = ', '.join('%s -> %s' % (alias, name) for alias, name in duplicates)
        raise ValueError('duplicate values found in %r: %s' % (enumeration, alias_details))
    return enumeration


class property(DynamicClassAttribute):
    """
    This is a descriptor, used to define attributes that act differently
    when accessed through an enum member and through an enum class.
    Instance access is the same as property(), but access to an attribute
    through the enum class will instead look in the class' _member_map_ for
    a corresponding enum member.
    """

    member = None
    _attr_type = None
    _cls_type = None

    def __get__(self, instance, ownerclass=None):
        if instance is None:
            if self.member is not None:
                return self.member
            else:
                raise AttributeError(
                        '%r has no attribute %r' % (ownerclass, self.name)
                        )
        if self.fget is not None:
            # use previous enum.property
            return self.fget(instance)
        elif self._attr_type == 'attr':
            # look up previous attibute
            return getattr(self._cls_type, self.name)
        elif self._attr_type == 'desc':
            # use previous descriptor
            return getattr(instance._value_, self.name)
        # look for a member by this name.
        try:
            return ownerclass._member_map_[self.name]
        except KeyError:
            raise AttributeError(
                    '%r has no attribute %r' % (ownerclass, self.name)
                    ) from None

    def __set__(self, instance, value):
        if self.fset is not None:
            return self.fset(instance, value)
        raise AttributeError(
                "<enum %r> cannot set attribute %r" % (self.clsname, self.name)
                )

    def __delete__(self, instance):
        if self.fdel is not None:
            return self.fdel(instance)
        raise AttributeError(
                "<enum %r> cannot delete attribute %r" % (self.clsname, self.name)
                )

    def __set_name__(self, ownerclass, name):
        self.name = name
        self.clsname = ownerclass.__name__


class _proto_member:
    """
    intermediate step for enum members between class execution and final creation
    """

    def __init__(self, value):
        self.value = value

    def __set_name__(self, enum_class, member_name):
        """
        convert each quasi-member into an instance of the new enum class
        """
        # first step: remove ourself from enum_class
        delattr(enum_class, member_name)
        # second step: create member based on enum_class
        value = self.value
        if not isinstance(value, tuple):
            args = (value, )
        else:
            args = value
        if enum_class._member_type_ is tuple:   # special case for tuple enums
            args = (args, )     # wrap it one more time
        if not enum_class._use_args_:
            enum_member = enum_class._new_member_(enum_class)
        else:
            enum_member = enum_class._new_member_(enum_class, *args)
        if not hasattr(enum_member, '_value_'):
            if enum_class._member_type_ is object:
                enum_member._value_ = value
            else:
                try:
                    enum_member._value_ = enum_class._member_type_(*args)
                except Exception as exc:
                    new_exc = TypeError(
                            '_value_ not set in __new__, unable to create it'
                            )
                    new_exc.__cause__ = exc
                    raise new_exc
        value = enum_member._value_
        enum_member._name_ = member_name
        enum_member.__objclass__ = enum_class
        enum_member.__init__(*args)
        enum_member._sort_order_ = len(enum_class._member_names_)

        if Flag is not None and issubclass(enum_class, Flag):
            if isinstance(value, int):
                enum_class._flag_mask_ |= value
                if _is_single_bit(value):
                    enum_class._singles_mask_ |= value
            enum_class._all_bits_ = 2 ** ((enum_class._flag_mask_).bit_length()) - 1

        # If another member with the same value was already defined, the
        # new member becomes an alias to the existing one.
        try:
            try:
                # try to do a fast lookup to avoid the quadratic loop
                enum_member = enum_class._value2member_map_[value]
            except TypeError:
                for name, canonical_member in enum_class._member_map_.items():
                    if canonical_member._value_ == value:
                        enum_member = canonical_member
                        break
                else:
                    raise KeyError
        except KeyError:
            # this could still be an alias if the value is multi-bit and the
            # class is a flag class
            if (
                    Flag is None
                    or not issubclass(enum_class, Flag)
                ):
                # no other instances found, record this member in _member_names_
                enum_class._member_names_.append(member_name)
            elif (
                    Flag is not None
                    and issubclass(enum_class, Flag)
                    and isinstance(value, int)
                    and _is_single_bit(value)
                ):
                # no other instances found, record this member in _member_names_
                enum_class._member_names_.append(member_name)

        enum_class._add_member_(member_name, enum_member)
        try:
            # This may fail if value is not hashable. We can't add the value
            # to the map, and by-value lookups for this value will be
            # linear.
            enum_class._value2member_map_.setdefault(value, enum_member)
            if value not in enum_class._hashable_values_:
                enum_class._hashable_values_.append(value)
        except TypeError:
            # keep track of the value in a list so containment checks are quick
            enum_class._unhashable_values_.append(value)
            enum_class._unhashable_values_map_.setdefault(member_name, []).append(value)


class EnumDict(dict):
    """
    Track enum member order and ensure member names are not reused.

    EnumType will use the names found in self._member_names as the
    enumeration member names.
    """
    def __init__(self, cls_name=None):
        super().__init__()
        self._member_names = {} # use a dict -- faster look-up than a list, and keeps insertion order since 3.7
        self._last_values = []
        self._ignore = []
        self._auto_called = False
        self._cls_name = cls_name

    def __setitem__(self, key, value):
        """
        Changes anything not dundered or not a descriptor.

        If an enum member name is used twice, an error is raised; duplicate
        values are not checked for.

        Single underscore (sunder) names are reserved.
        """
        if self._cls_name is not None and _is_private(self._cls_name, key):
            # do nothing, name will be a normal attribute
            pass
        elif _is_sunder(key):
            if key not in (
                    '_order_',
                    '_generate_next_value_', '_numeric_repr_', '_missing_', '_ignore_',
                    '_iter_member_', '_iter_member_by_value_', '_iter_member_by_def_',
                    '_add_alias_', '_add_value_alias_',
                    # While not in use internally, those are common for pretty
                    # printing and thus excluded from Enum's reservation of
                    # _sunder_ names
                    ) and not key.startswith('_repr_'):
                raise ValueError(
                        '_sunder_ names, such as %r, are reserved for future Enum use'
                        % (key, )
                        )
            if key == '_generate_next_value_':
                # check if members already defined as auto()
                if self._auto_called:
                    raise TypeError("_generate_next_value_ must be defined before members")
                _gnv = value.__func__ if isinstance(value, staticmethod) else value
                setattr(self, '_generate_next_value', _gnv)
            elif key == '_ignore_':
                if isinstance(value, str):
                    value = value.replace(',',' ').split()
                else:
                    value = list(value)
                self._ignore = value
                already = set(value) & set(self._member_names)
                if already:
                    raise ValueError(
                            '_ignore_ cannot specify already set names: %r'
                            % (already, )
                            )
        elif _is_dunder(key):
            if key == '__order__':
                key = '_order_'
        elif key in self._member_names:
            # descriptor overwriting an enum?
            raise TypeError('%r already defined as %r' % (key, self[key]))
        elif key in self._ignore:
            pass
        elif isinstance(value, nonmember):
            # unwrap value here; it won't be processed by the below `else`
            value = value.value
        elif isinstance(value, partial):
            import warnings
            warnings.warn('functools.partial will be a method descriptor '
                          'in future Python versions; wrap it in '
                          'enum.member() if you want to preserve the '
                          'old behavior', FutureWarning, stacklevel=2)
        elif _is_descriptor(value):
            pass
        elif self._cls_name is not None and _is_internal_class(self._cls_name, value):
            # do nothing, name will be a normal attribute
            pass
        else:
            if key in self:
                # enum overwriting a descriptor?
                raise TypeError('%r already defined as %r' % (key, self[key]))
            elif isinstance(value, member):
                # unwrap value here -- it will become a member
                value = value.value
            non_auto_store = True
            single = False
            if isinstance(value, auto):
                single = True
                value = (value, )
            if isinstance(value, tuple) and any(isinstance(v, auto) for v in value):
                # insist on an actual tuple, no subclasses, in keeping with only supporting
                # top-level auto() usage (not contained in any other data structure)
                auto_valued = []
                t = type(value)
                for v in value:
                    if isinstance(v, auto):
                        non_auto_store = False
                        if v.value == _auto_null:
                            v.value = self._generate_next_value(
                                    key, 1, len(self._member_names), self._last_values[:],
                                    )
                            self._auto_called = True
                        v = v.value
                        self._last_values.append(v)
                    auto_valued.append(v)
                if single:
                    value = auto_valued[0]
                else:
                    try:
                        # accepts iterable as multiple arguments?
                        value = t(auto_valued)
                    except TypeError:
                        # then pass them in singly
                        value = t(*auto_valued)
            self._member_names[key] = None
            if non_auto_store:
                self._last_values.append(value)
        super().__setitem__(key, value)

    @property
    def member_names(self):
        return list(self._member_names)

    def update(self, members, **more_members):
        try:
            for name in members.keys():
                self[name] = members[name]
        except AttributeError:
            for name, value in members:
                self[name] = value
        for name, value in more_members.items():
            self[name] = value

_EnumDict = EnumDict        # keep private name for backwards compatibility


class EnumCheck(StrEnum):
    """
    various conditions to check an enumeration for
    """
    CONTINUOUS = "no skipped integer values"
    NAMED_FLAGS = "multi-flag aliases may not contain unnamed flags"
    UNIQUE = "one name per value"
CONTINUOUS, NAMED_FLAGS, UNIQUE = EnumCheck


class verify:
    """
    Check an enumeration for various constraints. (see EnumCheck)
    """
    def __init__(self, *checks):
        self.checks = checks
    def __call__(self, enumeration):
        checks = self.checks
        cls_name = enumeration.__name__
        if Flag is not None and issubclass(enumeration, Flag):
            enum_type = 'flag'
        elif issubclass(enumeration, Enum):
            enum_type = 'enum'
        else:
            raise TypeError("the 'verify' decorator only works with Enum and Flag")
        for check in checks:
            if check is UNIQUE:
                # check for duplicate names
                duplicates = []
                for name, member in enumeration.__members__.items():
                    if name != member.name:
                        duplicates.append((name, member.name))
                if duplicates:
                    alias_details = ', '.join(
                            ["%s -> %s" % (alias, name) for (alias, name) in duplicates])
                    raise ValueError('aliases found in %r: %s' %
                            (enumeration, alias_details))
            elif check is CONTINUOUS:
                values = set(e.value for e in enumeration)
                if len(values) < 2:
                    continue
                low, high = min(values), max(values)
                missing = []
                if enum_type == 'flag':
                    # check for powers of two
                    for i in range(_high_bit(low)+1, _high_bit(high)):
                        if 2**i not in values:
                            missing.append(2**i)
                elif enum_type == 'enum':
                    # check for powers of one
                    for i in range(low+1, high):
                        if i not in values:
                            missing.append(i)
                else:
                    raise Exception('verify: unknown type %r' % enum_type)
                if missing:
                    raise ValueError(('invalid %s %r: missing values %s' % (
                            enum_type, cls_name, ', '.join((str(m) for m in missing)))
                            )[:256])
                            # limit max length to protect against DOS attacks
            elif check is NAMED_FLAGS:
                # examine each alias and check for unnamed flags
                member_names = enumeration._member_names_
                member_values = [m.value for m in enumeration]
                missing_names = []
                missing_value = 0
                for name, alias in enumeration._member_map_.items():
                    if name in member_names:
                        # not an alias
                        continue
                    if alias.value < 0:
                        # negative numbers are not checked
                        continue
                    values = list(_iter_bits_lsb(alias.value))
                    missed = [v for v in values if v not in member_values]
                    if missed:
                        missing_names.append(name)
                        for val in missed:
                            missing_value |= val
                if missing_names:
                    if len(missing_names) == 1:
                        alias = 'alias %s is missing' % missing_names[0]
                    else:
                        alias = 'aliases %s and %s are missing' % (
                                ', '.join(missing_names[:-1]), missing_names[-1]
                                )
                    if _is_single_bit(missing_value):
                        value = 'value 0x%x' % missing_value
                    else:
                        value = 'combined values of 0x%x' % missing_value
                    raise ValueError(
                            'invalid Flag %r: %s %s [use enum.show_flag_values(value) for details]'
                            % (cls_name, alias, value)
                            )
        return enumeration

def _test_simple_enum(checked_enum, simple_enum):
    """
    A function that can be used to test an enum created with :func:`_simple_enum`
    against the version created by subclassing :class:`Enum`::

        >>> from enum import Enum, _simple_enum, _test_simple_enum
        >>> @_simple_enum(Enum)
        ... class Color:
        ...     RED = auto()
        ...     GREEN = auto()
        ...     BLUE = auto()
        >>> class CheckedColor(Enum):
        ...     RED = auto()
        ...     GREEN = auto()
        ...     BLUE = auto()
        >>> _test_simple_enum(CheckedColor, Color)

    If differences are found, a :exc:`TypeError` is raised.
    """
    failed = []
    if checked_enum.__dict__ != simple_enum.__dict__:
        checked_dict = checked_enum.__dict__
        checked_keys = list(checked_dict.keys())
        simple_dict = simple_enum.__dict__
        simple_keys = list(simple_dict.keys())
        member_names = set(
                list(checked_enum._member_map_.keys())
                + list(simple_enum._member_map_.keys())
                )
        for key in set(checked_keys + simple_keys):
            if key in ('__module__', '_member_map_', '_value2member_map_', '__doc__',
                       '__static_attributes__', '__firstlineno__'):
                # keys known to be different, or very long
                continue
            elif key in member_names:
                # members are checked below
                continue
            elif key not in simple_keys:
                failed.append("missing key: %r" % (key, ))
            elif key not in checked_keys:
                failed.append("extra key:   %r" % (key, ))
            else:
                checked_value = checked_dict[key]
                simple_value = simple_dict[key]
                if callable(checked_value) or isinstance(checked_value, bltns.property):
                    continue
                if key == '__doc__':
                    # remove all spaces/tabs
                    compressed_checked_value = checked_value.replace(' ','').replace('\t','')
                    compressed_simple_value = simple_value.replace(' ','').replace('\t','')
                    if compressed_checked_value != compressed_simple_value:
                        failed.append("%r:\n         %s\n         %s" % (
                                key,
                                "checked -> %r" % (checked_value, ),
                                "simple  -> %r" % (simple_value, ),
                                ))
                elif checked_value != simple_value:
                    failed.append("%r:\n         %s\n         %s" % (
                            key,
                            "checked -> %r" % (checked_value, ),
                            "simple  -> %r" % (simple_value, ),
                            ))
        failed.sort()
        for name in member_names:
            failed_member = []
            if name not in simple_keys:
                failed.append('missing member from simple enum: %r' % name)
            elif name not in checked_keys:
                failed.append('extra member in simple enum: %r' % name)
            else:
                checked_member_dict = checked_enum[name].__dict__
                checked_member_keys = list(checked_member_dict.keys())
                simple_member_dict = simple_enum[name].__dict__
                simple_member_keys = list(simple_member_dict.keys())
                for key in set(checked_member_keys + simple_member_keys):
                    if key in ('__module__', '__objclass__', '_inverted_'):
                        # keys known to be different or absent
                        continue
                    elif key not in simple_member_keys:
                        failed_member.append("missing key %r not in the simple enum member %r" % (key, name))
                    elif key not in checked_member_keys:
                        failed_member.append("extra key %r in simple enum member %r" % (key, name))
                    else:
                        checked_value = checked_member_dict[key]
                        simple_value = simple_member_dict[key]
                        if checked_value != simple_value:
                            failed_member.append("%r:\n         %s\n         %s" % (
                                    key,
                                    "checked member -> %r" % (checked_value, ),
                                    "simple member  -> %r" % (simple_value, ),
                                    ))
            if failed_member:
                failed.append('%r member mismatch:\n      %s' % (
                        name, '\n      '.join(failed_member),
                        ))
        for method in (
                '__str__', '__repr__', '__reduce_ex__', '__format__',
                '__getnewargs_ex__', '__getnewargs__', '__reduce_ex__', '__reduce__'
            ):
            if method in simple_keys and method in checked_keys:
                # cannot compare functions, and it exists in both, so we're good
                continue
            elif method not in simple_keys and method not in checked_keys:
                # method is inherited -- check it out
                checked_method = getattr(checked_enum, method, None)
                simple_method = getattr(simple_enum, method, None)
                if hasattr(checked_method, '__func__'):
                    checked_method = checked_method.__func__
                    simple_method = simple_method.__func__
                if checked_method != simple_method:
                    failed.append("%r:  %-30s %s" % (
                            method,
                            "checked -> %r" % (checked_method, ),
                            "simple -> %r" % (simple_method, ),
                            ))
            else:
                # if the method existed in only one of the enums, it will have been caught
                # in the first checks above
                pass
    if failed:
        raise TypeError('enum mismatch:\n   %s' % '\n   '.join(failed))

def _old_convert_(etype, name, module, filter, source=None, *, boundary=None):
    """
    Create a new Enum subclass that replaces a collection of global constants
    """
    # convert all constants from source (or module) that pass filter() to
    # a new Enum called name, and export the enum and its members back to
    # module;
    # also, replace the __reduce_ex__ method so unpickling works in
    # previous Python versions
    module_globals = sys.modules[module].__dict__
    if source:
        source = source.__dict__
    else:
        source = module_globals
    # _value2member_map_ is populated in the same order every time
    # for a consistent reverse mapping of number to name when there
    # are multiple names for the same number.
    members = [
            (name, value)
            for name, value in source.items()
            if filter(name)]
    try:
        # sort by value
        members.sort(key=lambda t: (t[1], t[0]))
    except TypeError:
        # unless some values aren't comparable, in which case sort by name
        members.sort(key=lambda t: t[0])
    cls = etype(name, members, module=module, boundary=boundary or KEEP)
    return cls

_stdlib_enums = IntEnum, StrEnum, IntFlag
