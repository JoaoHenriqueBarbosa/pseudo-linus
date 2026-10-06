"""enum do sandbox (Python embutido)."""


class auto:
    """Valor a ser escolhido por `_generate_next_value_`."""

    value = None

    def __repr__(self):
        return 'auto()'


def _is_dunder(name):
    return len(name) > 4 and name.startswith('__') and name.endswith('__')


def _is_sunder(name):
    return len(name) > 2 and name.startswith('_') and name.endswith('_') and not name.startswith('__')


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
                if generate is None:
                    raise TypeError('auto() needs _generate_next_value_')
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


class IntFlag(int, Flag):
    _boundary_keep_ = True

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


def global_enum_repr(self):
    return '%s.%s' % (self.__class__.__module__, self._name_)


def global_enum(cls, update_str=False):
    """Marca o Enum como exportado no módulo: repr `modulo.MEMBRO`.

    Quem define o enum exporta os membros (o sandbox não tem `globals()` vivo para o decorador).
    """
    cls.__repr__ = global_enum_repr
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
