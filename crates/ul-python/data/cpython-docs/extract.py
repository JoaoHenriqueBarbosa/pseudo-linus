"""Extrai do CPython as docstrings que o fonte em Python do disco não dá, para `runtime.tsv`.

Roda no oráculo da bancada (o Debian 13 em Docker), com os nomes dos módulos como argumentos (sem
argumentos valem os de `modules.txt`: os módulos embutidos do interpretador, os de
`src/modules/pysrc.rs`, mais os de C que as funções nativas pedem):

    docker run --rm -i -v "$PWD/crates/ul-python/data/cpython-docs:/g:ro" pseudo-linus-oracle:dev \
        python3 -I /g/extract.py > crates/ul-python/data/cpython-docs/runtime.tsv

Cada linha é `módulo<TAB>nome qualificado<TAB>docstring em JSON` (`null` quando não há); o nome
vazio é o próprio módulo. Entram as funções e classes de topo e os membros das classes (fora os
métodos especiais, que só os tipos de `builtins` têm: `str.upper`, `int.__add__`, `len`).
Para módulos escritos em C entra tudo; para módulos com `.py`, só o que o objeto em tempo de execução expõe
diferente do fonte (o `bisect.py` tem docstrings, mas `bisect.bisect` vem do `_bisect` em C).
"""

import ast
import importlib
import inspect
import json
import os
import sys
import types

MEMBER_TYPES = ('getset_descriptor', 'member_descriptor')

# Tipos embutidos que `builtins` não publica com o próprio nome.
EXTRA_TYPES = (types.FunctionType, types.MethodType, types.ModuleType, type(None), type(NotImplemented),
               type(Ellipsis), types.SimpleNamespace, types.MappingProxyType)


def cleaned(raw):
    """A docstring como o compilador do CPython 3.13 a guarda (com a indentação limpa)."""
    if raw is None:
        return None
    ns = {}
    tree = ast.fix_missing_locations(ast.Module(body=[ast.Expr(ast.Constant(raw))], type_ignores=[]))
    exec(compile(tree, '<doc>', 'exec'), ns)
    return ns['__doc__']


def source_docs(path):
    """As docstrings do fonte, pelo nome qualificado (como `cpydocs.rs` as coleta)."""
    with open(path, encoding='utf-8') as f:
        tree = ast.parse(f.read())
    docs = {'': cleaned(ast.get_docstring(tree, clean=False))}

    def walk(body, prefix):
        for node in body:
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
                qual = prefix + '.' + node.name if prefix else node.name
                docs.setdefault(qual, cleaned(ast.get_docstring(node, clean=False)))
                inner = qual + '.<locals>' if not isinstance(node, ast.ClassDef) else qual
                walk(node.body, inner)
            else:
                for field in ('body', 'orelse', 'finalbody'):
                    walk(getattr(node, field, []) or [], prefix)
                for handler in getattr(node, 'handlers', []) or []:
                    walk(handler.body, prefix)

    walk(tree.body, '')
    return docs


def computes_module_doc(path):
    """O fonte atribui `__doc__` no nível do módulo (`__doc__ += ...`), dentro ou fora de um `if`."""
    with open(path, encoding='utf-8') as f:
        tree = ast.parse(f.read())

    def assigns(node):
        targets = [node.target] if isinstance(node, ast.AugAssign) else getattr(node, 'targets', [])
        return any(isinstance(t, ast.Name) and t.id == '__doc__' for t in targets)

    def scan(body):
        for node in body:
            if isinstance(node, (ast.Assign, ast.AugAssign)) and assigns(node):
                return True
            if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
                inner = [getattr(node, f, None) or [] for f in ('body', 'orelse', 'finalbody')]
                inner += [h.body for h in getattr(node, 'handlers', []) or []]
                if any(scan(b) for b in inner):
                    return True
        return False

    return scan(tree.body)


def main(names):
    out = []
    for name in names:
        try:
            module = importlib.import_module(name)
        except ImportError:
            print('ausente:', name, file=sys.stderr)
            continue
        path = getattr(module, '__file__', None)
        known = source_docs(path) if path and path.endswith('.py') else None

        def emit(qual, obj):
            doc = getattr(obj, '__doc__', None)
            doc = doc if isinstance(doc, str) else None
            if known is not None and qual in known and known[qual] == doc:
                return
            # O módulo que monta o próprio `__doc__` ao ser importado (o `pdb` anexa a ajuda dos comandos) o
            # recalcula quando roda aqui: gravar o resultado faria o anexo repetir.
            if qual == '' and known is not None and computes_module_doc(path):
                return
            out.append('%s\t%s\t%s' % (name, qual, json.dumps(doc)))

        def emit_class(key, value):
            emit(key, value)
            for attr in sorted(vars(value)):
                # Os métodos especiais têm as docstrings genéricas dos slots ("Return self+value."), que
                # são do interpretador e não do módulo; só os tipos de `builtins` as têm na tabela
                # (`int.__add__`, `list.__getitem__`).
                if attr.startswith('__') and attr.endswith('__') and name != 'builtins':
                    continue
                member = vars(value)[attr]
                if (callable(member) or isinstance(member, (property, staticmethod, classmethod))
                        or type(member).__name__ in MEMBER_TYPES):
                    emit(key + '.' + attr, member)

        emit('', module)
        for key in sorted(vars(module)):
            value = vars(module)[key]
            if inspect.isclass(value):
                # Classe de outro módulo é reexportação e fica de fora, salvo a de um módulo em C
                # embutido no interpretador que este módulo publica como sua (o `os.uname_result`
                # e o `os.DirEntry` são do `posix`).
                if (value.__module__ not in (name, name.rsplit('.', 1)[-1], module.__name__)
                        and (value.__module__ not in sys.builtin_module_names or value.__module__ == 'builtins')):
                    continue
                emit_class(key, value)
            elif callable(value) and type(value).__name__ in ('builtin_function_or_method', 'function'):
                emit(key, value)
        if name == 'builtins':
            # Os tipos embutidos que não são nomes de `builtins` (`function`, `NoneType`...), pelo `__name__`.
            for extra in EXTRA_TYPES:
                emit_class(extra.__name__, extra)
    print('\n'.join(out))


if __name__ == '__main__':
    args = sys.argv[1:]
    if not args:
        with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'modules.txt'), encoding='utf-8') as f:
            args = f.read().split()
    main(args)
