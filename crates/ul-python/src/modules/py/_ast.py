"""_ast: as classes de nó da árvore sintática (`Parser/Python.asdl` do 3.13) e a ponte com o parser nativo.

Cada nó tem `_fields`, `_attributes` e `__match_args__`. Campo opcional (`?`) vale `None` na classe e
campo de lista (`*`) nasce `[]`. O parser nativo (`_ast_native.parse`) devolve a árvore como tuplas
`(nome, ((campo, valor), ...), posição)` que `_build` transforma em instâncias.
"""

import _ast_native

PyCF_ONLY_AST = 1024
PyCF_TYPE_COMMENTS = 4096
PyCF_ALLOW_TOP_LEVEL_AWAIT = 8192
PyCF_OPTIMIZED_AST = 33792

_POS = 'lineno col_offset end_lineno end_col_offset'

# nome|base|campos|atributos (P = lineno col_offset end_lineno end_col_offset); `*` lista, `?` opcional.
_SPEC = """
mod|AST||
Module|mod|body* type_ignores*|
Interactive|mod|body*|
Expression|mod|body|
FunctionType|mod|argtypes* returns|
stmt|AST||P
FunctionDef|stmt|name args body* decorator_list* returns? type_comment? type_params*|P
AsyncFunctionDef|stmt|name args body* decorator_list* returns? type_comment? type_params*|P
ClassDef|stmt|name bases* keywords* body* decorator_list* type_params*|P
Return|stmt|value?|P
Delete|stmt|targets*|P
Assign|stmt|targets* value type_comment?|P
TypeAlias|stmt|name type_params* value|P
AugAssign|stmt|target op value|P
AnnAssign|stmt|target annotation value? simple|P
For|stmt|target iter body* orelse* type_comment?|P
AsyncFor|stmt|target iter body* orelse* type_comment?|P
While|stmt|test body* orelse*|P
If|stmt|test body* orelse*|P
With|stmt|items* body* type_comment?|P
AsyncWith|stmt|items* body* type_comment?|P
Match|stmt|subject cases*|P
Raise|stmt|exc? cause?|P
Try|stmt|body* handlers* orelse* finalbody*|P
TryStar|stmt|body* handlers* orelse* finalbody*|P
Assert|stmt|test msg?|P
Import|stmt|names*|P
ImportFrom|stmt|module? names* level?|P
Global|stmt|names*|P
Nonlocal|stmt|names*|P
Expr|stmt|value|P
Pass|stmt||P
Break|stmt||P
Continue|stmt||P
expr|AST||P
BoolOp|expr|op values*|P
NamedExpr|expr|target value|P
BinOp|expr|left op right|P
UnaryOp|expr|op operand|P
Lambda|expr|args body|P
IfExp|expr|test body orelse|P
Dict|expr|keys* values*|P
Set|expr|elts*|P
ListComp|expr|elt generators*|P
SetComp|expr|elt generators*|P
DictComp|expr|key value generators*|P
GeneratorExp|expr|elt generators*|P
Await|expr|value|P
Yield|expr|value?|P
YieldFrom|expr|value|P
Compare|expr|left ops* comparators*|P
Call|expr|func args* keywords*|P
FormattedValue|expr|value conversion format_spec?|P
JoinedStr|expr|values*|P
Constant|expr|value kind?|P
Attribute|expr|value attr ctx|P
Subscript|expr|value slice ctx|P
Starred|expr|value ctx|P
Name|expr|id ctx|P
List|expr|elts* ctx|P
Tuple|expr|elts* ctx|P
Slice|expr|lower? upper? step?|P
expr_context|AST||
Load|expr_context||
Store|expr_context||
Del|expr_context||
boolop|AST||
And|boolop||
Or|boolop||
operator|AST||
Add|operator||
Sub|operator||
Mult|operator||
MatMult|operator||
Div|operator||
Mod|operator||
Pow|operator||
LShift|operator||
RShift|operator||
BitOr|operator||
BitXor|operator||
BitAnd|operator||
FloorDiv|operator||
unaryop|AST||
Invert|unaryop||
Not|unaryop||
UAdd|unaryop||
USub|unaryop||
cmpop|AST||
Eq|cmpop||
NotEq|cmpop||
Lt|cmpop||
LtE|cmpop||
Gt|cmpop||
GtE|cmpop||
Is|cmpop||
IsNot|cmpop||
In|cmpop||
NotIn|cmpop||
comprehension|AST|target iter ifs* is_async|
excepthandler|AST||P
ExceptHandler|excepthandler|type? name? body*|P
arguments|AST|posonlyargs* args* vararg? kwonlyargs* kw_defaults* kwarg? defaults*|
arg|AST|arg annotation? type_comment?|P
keyword|AST|arg? value|P
alias|AST|name asname?|P
withitem|AST|context_expr optional_vars?|
match_case|AST|pattern guard? body*|
pattern|AST||P
MatchValue|pattern|value|P
MatchSingleton|pattern|value|P
MatchSequence|pattern|patterns*|P
MatchMapping|pattern|keys* patterns* rest?|P
MatchClass|pattern|cls patterns* kwd_attrs* kwd_patterns*|P
MatchStar|pattern|name?|P
MatchAs|pattern|pattern? name?|P
MatchOr|pattern|patterns*|P
type_ignore|AST||
TypeIgnore|type_ignore|lineno tag|
type_param|AST||P
TypeVar|type_param|name bound? default_value?|P
ParamSpec|type_param|name default_value?|P
TypeVarTuple|type_param|name default_value?|P
"""


class AST:
    _fields = ()
    _attributes = ()
    __match_args__ = ()

    def __init__(self, *args, **kwargs):
        fields = self._fields
        if len(args) > len(fields):
            raise TypeError('%s constructor takes at most %d positional argument%s'
                            % (type(self).__name__, len(fields), '' if len(fields) == 1 else 's'))
        for name, value in zip(fields, args):
            if name in kwargs:
                raise TypeError("%s got multiple values for argument '%s'" % (type(self).__name__, name))
            setattr(self, name, value)
        for name, value in kwargs.items():
            setattr(self, name, value)
        defaults = type(self)._defaults
        for name in fields[len(args):]:
            if name not in kwargs:
                kind = defaults.get(name)
                if kind == '*':
                    setattr(self, name, [])
                elif name == 'ctx':
                    setattr(self, name, Load())

    def __reduce__(self):
        return type(self), (), self.__dict__


class _ListOf:
    """Tipo de campo de lista (`list[...]`): só o `__origin__` interessa a `ast.dump`."""
    __origin__ = list


def _make(name, base, fields, attrs):
    names = tuple(f.rstrip('*?') for f in fields)
    ns = {
        '_fields': names,
        '_attributes': attrs,
        '__match_args__': names,
        '_defaults': {f.rstrip('*?'): f[-1] for f in fields if f[-1] in '*?'},
        '_field_types': {f.rstrip('*?'): (_ListOf if f[-1] == '*' else object) for f in fields},
        '__module__': 'ast',
    }
    for f in fields:
        if f[-1] == '?':
            ns[f[:-1]] = None
    return type(name, (base,), ns)


import sys
_this = sys.modules[__name__]


def _define():
    classes = {'AST': AST}
    AST._defaults = {}
    AST.__module__ = 'ast'
    for line in _SPEC.strip().split('\n'):
        name, base, fields, attrs = line.split('|')
        attrs = tuple(_POS.split()) if attrs == 'P' else ()
        cls = _make(name, classes[base], fields.split(), attrs)
        classes[name] = cls
        setattr(_this, name, cls)
    return classes


_classes = _define()


def _build(t):
    if isinstance(t, list):
        return [_build(x) for x in t]
    if not isinstance(t, tuple):
        return t
    name, fields, pos = t
    if name == '__complex__':
        return complex(fields[0][1], fields[1][1])
    cls = _classes[name]
    node = cls.__new__(cls)
    for key, value in fields:
        setattr(node, key, _build(value))
    if pos is not None:
        node.lineno, node.col_offset, node.end_lineno, node.end_col_offset = pos
    return node


def _parse(source, filename='<unknown>', mode='exec'):
    if isinstance(source, (bytes, bytearray)):
        source = source.decode('utf-8')
    return _build(_ast_native.parse(source, filename, mode))
