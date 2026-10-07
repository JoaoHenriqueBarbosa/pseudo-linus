"""Acha funções de repasse: corpo de uma linha só, que chama outra função com os próprios parâmetros.

Uso: python3 scripts/dry-forwarders.py <raiz>... ; imprime `arquivo:linha nome -> alvo`.

Pula `impl Trait for` (a assinatura é imposta pela trait), o módulo de testes de cada arquivo e o
código de terceiros (`vendor/`, `staging/`). Repasse que só renomeia vira reexportação
(`pub use x::y as z;`), e repasse que só muda o caminho vira chamada direta.
"""
import os
import re
import sys

FN = re.compile(
    r'^(\s*)(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:unsafe\s+)?fn\s+(\w+)\s*(<[^>]*>)?\s*\((.*?)\)'
    r'\s*(?:->\s*([^{]*?))?\s*(?:where[^{]*)?\{\s*$'
)
IMPL = re.compile(r'^\s*impl\b')
IMPL_TRAIT = re.compile(r'^\s*impl\b.*\bfor\b')
CALL = re.compile(r'^(?:return\s+)?(?:Ok\()?([\w:.]+?)(?:::<[^>]*>)?\((.*)\)\)?;?$')
SKIP_DIRS = {'target', 'wip', '.git', 'tests', 'node_modules', 'vendor', 'staging', 'benches'}


def split_top(s, opening, closing):
    """Divide `s` nas vírgulas de nível zero."""
    out, depth, cur = [], 0, ''
    for ch in s:
        if ch in opening:
            depth += 1
        elif ch in closing:
            depth -= 1
        if ch == ',' and depth == 0:
            out.append(cur)
            cur = ''
        else:
            cur += ch
    if cur.strip():
        out.append(cur)
    return out


def params(sig):
    out = []
    for p in (x.strip() for x in split_top(sig, '<([', '>)]')):
        if p.endswith('self'):
            out.append('self')
            continue
        m = re.match(r'(?:mut\s+)?(\w+)\s*:', p)
        if m:
            out.append(m.group(1))
    return out


def plain_arg(a, ps):
    a = re.sub(r'^&(mut\s+)?', '', a.strip())
    a = re.sub(r'\.(clone|as_ref|as_slice|as_str|into|to_vec|to_owned)\(\)$', '', a)
    return a.lstrip('*') in ps


def scan(path):
    lines = open(path, encoding='utf-8', errors='replace').read().split('\n')
    trait_impl = None
    for i, line in enumerate(lines):
        if re.match(r'^\s*#\[cfg\(test\)\]', line):
            return
        if IMPL.match(line):
            trait_impl = (bool(IMPL_TRAIT.match(line)), len(line) - len(line.lstrip()))
        m = FN.match(line)
        if not m or i + 2 >= len(lines):
            continue
        indent = m.group(1)
        if trait_impl and trait_impl[0] and len(indent) > trait_impl[1]:
            continue
        if lines[i + 2] != indent + '}':
            continue
        body = lines[i + 1].strip()
        c = CALL.match(body)
        if body.startswith('//') or not c:
            continue
        ps = params(m.group(4))
        args = split_top(c.group(2), '([{<', ')]}>')
        if all(plain_arg(a, ps) for a in args) and len(args) >= len([p for p in ps if p != 'self']):
            yield i + 1, m.group(2), c.group(1)


def main(roots):
    for root in roots:
        for dp, dns, fns in os.walk(root):
            dns[:] = sorted(d for d in dns if d not in SKIP_DIRS)
            for f in sorted(fns):
                if f.endswith('.rs'):
                    p = os.path.join(dp, f)
                    for ln, name, target in scan(p):
                        print(f'{p}:{ln} {name} -> {target}')


main(sys.argv[1:] or ['crates'])
