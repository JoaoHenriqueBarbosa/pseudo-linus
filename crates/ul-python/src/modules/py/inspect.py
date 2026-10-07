"""inspect enxuto: predicados de tipo, `signature` e `getdoc` sobre os objetos da VM."""

import types
import sys

__all__ = ['isfunction', 'ismethod', 'isclass', 'ismodule', 'iscoroutine', 'iscoroutinefunction',
           'isgenerator', 'isgeneratorfunction', 'isasyncgen', 'isasyncgenfunction', 'isawaitable',
           'isbuiltin', 'isroutine', 'callable', 'getdoc', 'signature', 'Signature', 'Parameter',
           'getmembers', 'currentframe', 'unwrap', 'cleandoc', 'isabstract', 'formatannotation', 'BoundArguments']

callable = callable

CO_OPTIMIZED = 1
CO_NEWLOCALS = 2
CO_VARARGS = 4
CO_VARKEYWORDS = 8
CO_NESTED = 16
CO_GENERATOR = 32
CO_NOFREE = 64
CO_COROUTINE = 128
CO_ITERABLE_COROUTINE = 256
CO_ASYNC_GENERATOR = 512


def isfunction(obj):
    return type(obj) is type(isfunction)


def ismethod(obj):
    return type(obj).__name__ == 'method'


def isclass(obj):
    return isinstance(obj, type)


def ismodule(obj):
    return isinstance(obj, types.ModuleType)


def isbuiltin(obj):
    return type(obj).__name__ == 'builtin_function_or_method'


def isroutine(obj):
    return isbuiltin(obj) or isfunction(obj) or ismethod(obj)


def _unwrap_method(obj):
    return getattr(obj, '__func__', obj)


def iscoroutinefunction(obj):
    obj = _unwrap_method(obj)
    while hasattr(obj, '__wrapped__'):
        obj = obj.__wrapped__
    return bool(getattr(obj, '_is_coroutine_function', False)) or _code_flag(obj, 'coroutine')


def isasyncgenfunction(obj):
    return _code_flag(_unwrap_method(obj), 'asyncgen')


def isgeneratorfunction(obj):
    return _code_flag(_unwrap_method(obj), 'generator')


def _code_flag(obj, kind):
    code = getattr(obj, '__code__', None)
    if code is None:
        return False
    return kind in getattr(code, 'co_kinds', ())


def iscoroutine(obj):
    return type(obj).__name__ == 'coroutine'


def isgenerator(obj):
    return type(obj).__name__ == 'generator'


def isasyncgen(obj):
    return type(obj).__name__ == 'async_generator'


def isawaitable(obj):
    return iscoroutine(obj) or hasattr(obj, '__await__')


def iscode(obj):
    return type(obj).__name__ == 'code'


def isframe(obj):
    return type(obj).__name__ == 'frame'


def istraceback(obj):
    return type(obj).__name__ == 'traceback'


def ismethoddescriptor(obj):
    if isclass(obj) or ismethod(obj) or isfunction(obj):
        return False
    tp = type(obj)
    return hasattr(tp, '__get__') and not hasattr(tp, '__set__')


def ismethodwrapper(obj):
    return type(obj).__name__ == 'method-wrapper'


def getmodulename(path):
    """Return the module name for a given file, or None."""
    import os
    import importlib.machinery
    fname = os.path.basename(path)
    # Check for paths that look like an actual module file
    suffixes = [(-len(suffix), suffix)
                    for suffix in importlib.machinery.all_suffixes()]
    suffixes.sort() # try longest suffixes first, in case they overlap
    for neglen, suffix in suffixes:
        if fname.endswith(suffix):
            return fname[:neglen]
    return None


def getmodule(obj, _filename=None):
    if ismodule(obj):
        return obj
    name = getattr(obj, '__module__', None)
    if name:
        return sys.modules.get(name)
    return None


def getfile(obj):
    if ismodule(obj):
        f = getattr(obj, '__file__', None)
        if f:
            return f
        raise TypeError('{!r} is a built-in module'.format(obj))
    if isclass(obj):
        mod = sys.modules.get(getattr(obj, '__module__', None))
        f = getattr(mod, '__file__', None)
        if f:
            return f
        raise OSError('source code not available')
    if ismethod(obj):
        obj = obj.__func__
    if isfunction(obj):
        return obj.__code__.co_filename
    if istraceback(obj):
        obj = obj.tb_frame
    if isframe(obj):
        obj = obj.f_code
    if iscode(obj):
        return obj.co_filename
    raise TypeError('module, class, method, function, traceback, frame, or code object was expected, got %s'
                    % type(obj).__name__)


def getsourcefile(obj):
    filename = getfile(obj)
    if filename.endswith('.py'):
        return filename
    return None


def unwrap(func, *, stop=None):
    while hasattr(func, '__wrapped__'):
        if stop is not None and stop(func):
            break
        func = func.__wrapped__
    return func


def cleandoc(doc):
    """Tira a indentação comum das linhas de um docstring (da segunda em diante)."""
    lines = doc.expandtabs().split('\n')
    margin = sys.maxsize
    for line in lines[1:]:
        content = len(line.lstrip(' '))
        if content:
            indent = len(line) - content
            margin = min(margin, indent)
    if lines:
        lines[0] = lines[0].lstrip(' ')
    if margin < sys.maxsize:
        for i in range(1, len(lines)):
            lines[i] = lines[i][margin:]
    while lines and not lines[-1]:
        lines.pop()
    while lines and not lines[0]:
        lines.pop(0)
    return '\n'.join(lines)


def _finddoc(obj):
    if isinstance(obj, type):
        for base in obj.__mro__[1:]:
            doc = base.__dict__.get('__doc__') if base is not object else None
            if isinstance(doc, str):
                return doc
    return None


def getdoc(obj):
    doc = getattr(obj, '__doc__', None)
    if not isinstance(doc, str):
        doc = _finddoc(obj)
    if not isinstance(doc, str):
        return None
    return cleandoc(doc)


def isabstract(obj):
    if not isinstance(obj, type):
        return False
    return bool(getattr(obj, '__abstractmethods__', None))


def getmembers(obj, predicate=None):
    out = []
    for name in dir(obj):
        try:
            value = getattr(obj, name)
        except AttributeError:
            continue
        if predicate is None or predicate(value):
            out.append((name, value))
    return sorted(out, key=lambda kv: kv[0])


def currentframe():
    return sys._getframe(1)


class _Empty:
    def __repr__(self):
        return '<class \'inspect._empty\'>'


_empty = _Empty()


class Parameter:
    POSITIONAL_ONLY = 0
    POSITIONAL_OR_KEYWORD = 1
    VAR_POSITIONAL = 2
    KEYWORD_ONLY = 3
    VAR_KEYWORD = 4
    empty = _empty

    def __init__(self, name, kind, *, default=_empty, annotation=_empty):
        self.name = name
        self.kind = kind
        self.default = default
        self.annotation = annotation

    def replace(self, *, name=_empty, kind=_empty, default=_empty, annotation=_empty):
        return Parameter(self.name if name is _empty else name, self.kind if kind is _empty else kind,
                         default=self.default if default is _empty else default,
                         annotation=self.annotation if annotation is _empty else annotation)

    def __str__(self):
        text = self.name
        if self.annotation is not _empty:
            text += ': ' + formatannotation(self.annotation)
        if self.default is not _empty:
            text += (' = ' if self.annotation is not _empty else '=') + repr(self.default)
        if self.kind == 2:
            text = '*' + text
        elif self.kind == 4:
            text = '**' + text
        return text

    def __repr__(self):
        return '<Parameter "%s">' % self

    def __eq__(self, other):
        return (isinstance(other, Parameter) and self.name == other.name and self.kind == other.kind
                and self.default == other.default and self.annotation == other.annotation)

    def __hash__(self):
        return hash((self.name, self.kind))


def formatannotation(annotation, base_module=None):
    if getattr(annotation, '__module__', None) == 'typing':
        return repr(annotation).replace('typing.', '')
    if isinstance(annotation, type):
        if annotation.__module__ in ('builtins', base_module):
            return annotation.__qualname__
        return annotation.__module__ + '.' + annotation.__qualname__
    return repr(annotation)


class Signature:
    empty = _empty

    def __init__(self, parameters=None, *, return_annotation=_empty):
        self.parameters = {p.name: p for p in (parameters or [])}
        self.return_annotation = return_annotation

    def format(self, *, max_width=None):
        return str(self)

    def __str__(self):
        parts = []
        params = list(self.parameters.values())
        star_done = False
        for i, p in enumerate(params):
            if p.kind == 3 and not star_done and not any(q.kind == 2 for q in params):
                parts.append('*')
                star_done = True
            parts.append(str(p))
            if p.kind == 0 and (i + 1 == len(params) or params[i + 1].kind != 0):
                parts.append('/')
        text = '(' + ', '.join(parts) + ')'
        if self.return_annotation is not _empty:
            text += ' -> ' + formatannotation(self.return_annotation)
        return text

    def replace(self, *, parameters=_empty, return_annotation=_empty):
        return Signature(list(self.parameters.values()) if parameters is _empty else parameters,
                         return_annotation=self.return_annotation if return_annotation is _empty
                         else return_annotation)

    def bind(self, *args, **kwargs):
        return self._bind(args, kwargs, False)

    def bind_partial(self, *args, **kwargs):
        return self._bind(args, kwargs, True)

    def _bind(self, args, kwargs, partial):
        params = list(self.parameters.values())
        arguments = {}
        args = list(args)
        kwargs = dict(kwargs)
        var_kw = None
        for p in params:
            if p.kind == 2:
                arguments[p.name] = tuple(args)
                args = []
            elif p.kind == 4:
                var_kw = p
            elif p.kind in (0, 1) and args:
                arguments[p.name] = args.pop(0)
            elif p.kind in (1, 3) and p.name in kwargs:
                arguments[p.name] = kwargs.pop(p.name)
            elif p.default is _empty and not partial:
                raise TypeError('missing a required argument: %r' % p.name)
        if args:
            raise TypeError('too many positional arguments')
        if kwargs:
            if var_kw is None:
                raise TypeError('got an unexpected keyword argument %r' % next(iter(kwargs)))
            arguments[var_kw.name] = kwargs
        return BoundArguments(self, arguments)

    def __repr__(self):
        return '<Signature %s>' % self

    def __eq__(self, other):
        return (isinstance(other, Signature) and list(self.parameters.values()) == list(other.parameters.values())
                and self.return_annotation == other.return_annotation)

    def __hash__(self):
        return hash(tuple(self.parameters))


class BoundArguments:
    def __init__(self, signature, arguments):
        self.signature = signature
        self.arguments = arguments

    @property
    def args(self):
        out = []
        for p in self.signature.parameters.values():
            if p.kind in (3, 4) or p.name not in self.arguments:
                break
            value = self.arguments[p.name]
            if p.kind == 2:
                out.extend(value)
            else:
                out.append(value)
        return tuple(out)

    @property
    def kwargs(self):
        out = {}
        seen_kw = False
        for p in self.signature.parameters.values():
            if p.kind == 4 and p.name in self.arguments:
                out.update(self.arguments[p.name])
            elif p.kind == 3 or seen_kw:
                if p.name in self.arguments:
                    out[p.name] = self.arguments[p.name]
            elif p.name not in self.arguments:
                seen_kw = True
        return out

    def apply_defaults(self):
        for p in self.signature.parameters.values():
            if p.name not in self.arguments and p.default is not _empty:
                self.arguments[p.name] = p.default
            elif p.name not in self.arguments and p.kind == 2:
                self.arguments[p.name] = ()
            elif p.name not in self.arguments and p.kind == 4:
                self.arguments[p.name] = {}

    def __repr__(self):
        return '<BoundArguments (%s)>' % ', '.join('%s=%r' % kv for kv in self.arguments.items())


def _class_signature(cls):
    for klass in cls.__mro__[:-1]:
        for attr in ('__new__', '__init__'):
            if attr in klass.__dict__:
                sig = signature(klass.__dict__[attr])
                params = list(sig.parameters.values())[1:]
                return Signature(params, return_annotation=sig.return_annotation)
    return Signature([])


def signature(obj, *, follow_wrapped=True):
    if follow_wrapped:
        obj = unwrap(obj)
    explicit = getattr(obj, '__signature__', None)
    if isinstance(explicit, Signature):
        return explicit
    if isinstance(obj, type):
        return _class_signature(obj)
    if ismethod(obj):
        sig = signature(obj.__func__, follow_wrapped=follow_wrapped)
        return Signature(list(sig.parameters.values())[1:], return_annotation=sig.return_annotation)
    obj = _unwrap_method(obj)
    code = getattr(obj, '__code__', None)
    if code is None:
        call = getattr(type(obj), '__call__', None)
        if call is not None and hasattr(call, '__code__'):
            sig = signature(call)
            return Signature(list(sig.parameters.values())[1:], return_annotation=sig.return_annotation)
        raise ValueError('no signature found for %r' % (obj,))
    annotations = getattr(obj, '__annotations__', None) or {}
    names = list(code.co_varnames[:code.co_argcount])
    defaults = tuple(getattr(obj, '__defaults__', None) or ())
    kwdefaults = getattr(obj, '__kwdefaults__', None) or {}
    posonly = getattr(code, 'co_posonlyargcount', 0)
    params = []
    first_default = len(names) - len(defaults)
    for i, name in enumerate(names):
        kind = 0 if i < posonly else 1
        default = defaults[i - first_default] if i >= first_default else _empty
        params.append(Parameter(name, kind, default=default, annotation=annotations.get(name, _empty)))
    rest = list(code.co_varnames[code.co_argcount:])
    kwonly = rest[:code.co_kwonlyargcount]
    rest = rest[code.co_kwonlyargcount:]
    if code.co_flags_varargs:
        vname = rest.pop(0)
        params.append(Parameter(vname, 2, annotation=annotations.get(vname, _empty)))
    for name in kwonly:
        params.append(Parameter(name, 3, default=kwdefaults.get(name, _empty),
                                annotation=annotations.get(name, _empty)))
    if code.co_flags_varkw and rest:
        params.append(Parameter(rest[0], 4, annotation=annotations.get(rest[0], _empty)))
    return Signature(params, return_annotation=annotations.get('return', _empty))


def isgetsetdescriptor(obj):
    return type(obj).__name__ == 'getset_descriptor'


def ismemberdescriptor(obj):
    return type(obj).__name__ == 'member_descriptor'


def isdatadescriptor(obj):
    if isclass(obj) or ismethod(obj) or isfunction(obj):
        return False
    tp = type(obj)
    return hasattr(tp, '__set__') or hasattr(tp, '__delete__')


def getmro(cls):
    return cls.__mro__


def getabsfile(obj, _filename=None):
    import os
    return os.path.normcase(os.path.abspath(getsourcefile(obj) or getfile(obj)))


def getattr_static(obj, attr, default=_empty):
    """Sem executar descritores: procura no `__dict__` da instância e no das classes do MRO."""
    instance_dict = getattr(obj, '__dict__', None) if not isclass(obj) else None
    klass = obj if isclass(obj) else type(obj)
    if instance_dict is not None and attr in instance_dict:
        return instance_dict[attr]
    for base in getmro(klass):
        d = getattr(base, '__dict__', {})
        if attr in d:
            return d[attr]
    if default is not _empty:
        return default
    raise AttributeError(attr)


def getcomments(obj):
    return None


def getclasstree(classes, unique=False):
    children = {}
    roots = []
    for c in classes:
        if c.__bases__:
            for parent in c.__bases__:
                children.setdefault(parent, [])
                if c not in children[parent]:
                    children[parent].append(c)
                if unique and parent in classes:
                    break
        elif c not in roots:
            roots.append(c)
    for parent in children:
        if parent not in classes and parent not in roots:
            roots.append(parent)

    def walk(parent):
        out = []
        for c in sorted(children.get(parent, []), key=lambda k: (k.__module__, k.__name__)) if False else children.get(parent, []):
            out.append((c, c.__bases__))
            sub = walk(c)
            if sub:
                out.append(sub)
        return out

    result = []
    for r in roots:
        result.append((r, r.__bases__))
        sub = walk(r)
        if sub:
            result.append(sub)
    return result


def classify_class_attrs(cls):
    """(name, kind, defining class, objeto) de cada atributo de `cls`."""
    from collections import namedtuple
    Attribute = namedtuple('Attribute', 'name kind defining_class object')
    mro = getmro(cls)
    result = []
    for name in dir(cls):
        homecls = None
        obj = None
        for base in mro:
            d = getattr(base, '__dict__', {})
            if name in d:
                homecls = base
                obj = d[name]
                break
        if homecls is None:
            try:
                obj = getattr(cls, name)
            except AttributeError:
                continue
            homecls = cls
        if isinstance(obj, staticmethod):
            kind = 'static method'
        elif isinstance(obj, classmethod):
            kind = 'class method'
        elif isinstance(obj, property):
            kind = 'readonly property' if obj.fset is None else 'data descriptor'
        elif isfunction(obj) or ismethoddescriptor(obj) or isbuiltin(obj):
            kind = 'method'
        elif isdatadescriptor(obj):
            kind = 'data descriptor'
        else:
            kind = 'data'
        result.append(Attribute(name, kind, homecls, obj))
    # Classes comuns ganham `__dict__` e `__weakref__` no primeiro ancestral de usuário.
    names = {a.name for a in result}
    if '__slots__' not in getattr(cls, '__dict__', {}):
        root = None
        for base in mro:
            if base is not object and getattr(base, '__bases__', ()) in ((object,), ()):
                root = base
        if root is not None:
            for dname, text in (('__dict__', 'dictionary for instance variables'),
                                ('__weakref__', 'list of weak references to the object')):
                if dname not in names:
                    result.append(Attribute(dname, 'data descriptor', root, _DocHolder(text)))
            result.sort(key=lambda a: a.name)
    return result


class _DocHolder:
    def __init__(self, doc):
        self.__doc__ = doc


def stack(context=1):
    frames = []
    f = sys._getframe(1)
    while f is not None:
        frames.append((f, f.f_code.co_filename, f.f_lineno, f.f_code.co_name, None, None))
        f = f.f_back
    return frames
