"""Extrai do CPython as docstrings que o fonte em Python do disco não dá, para `runtime.tsv`.

Roda no oráculo da bancada (o Debian 13 em Docker), com os nomes dos módulos embutidos do
interpretador (os de `src/modules/pysrc.rs`) como argumentos:

    docker run --rm -i -v "$PWD/crates/ul-python/data/cpython-docs:/g:ro" pseudo-linus-oracle:dev \
        python3 -I /g/extract.py sys itertools ... > crates/ul-python/data/cpython-docs/runtime.tsv

Cada linha é `módulo<TAB>nome qualificado<TAB>docstring em JSON` (`null` quando não há); o nome
vazio é o próprio módulo. Entram as funções e classes de topo e os membros das classes (fora os
métodos especiais). Para módulos
escritos em C entra tudo; para módulos com `.py`, só o que o objeto em tempo de execução expõe
diferente do fonte (o `bisect.py` tem docstrings, mas `bisect.bisect` vem do `_bisect` em C).
"""

import ast
import importlib
import inspect
import json
import sys

MEMBER_TYPES = ('getset_descriptor', 'member_descriptor')


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
            out.append('%s\t%s\t%s' % (name, qual, json.dumps(doc)))

        emit('', module)
        for key in sorted(vars(module)):
            value = vars(module)[key]
            if inspect.isclass(value):
                if value.__module__ not in (name, name.rsplit('.', 1)[-1], module.__name__):
                    continue
                emit(key, value)
                for attr in sorted(vars(value)):
                    # Os métodos especiais têm as docstrings genéricas dos slots ("Return self+value."),
                    # que são do interpretador e não do módulo.
                    if attr.startswith('__') and attr.endswith('__'):
                        continue
                    member = vars(value)[attr]
                    if (callable(member) or isinstance(member, (property, staticmethod, classmethod))
                            or type(member).__name__ in MEMBER_TYPES):
                        emit(key + '.' + attr, member)
            elif callable(value) and type(value).__name__ in ('builtin_function_or_method', 'function'):
                emit(key, value)
    print('\n'.join(out))


if __name__ == '__main__':
    main(sys.argv[1:])
