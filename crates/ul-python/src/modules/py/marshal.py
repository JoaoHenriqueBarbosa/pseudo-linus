"""marshal do sandbox (Python embutido): serialização binária no formato do CPython 3.13 (`Python/marshal.c`).

Cobre None, bool, Ellipsis, StopIteration, int, float, complex, str, bytes, tuple, list, dict, set, frozenset e
objetos `code` (TYPE_CODE), com as marcas de referência (`FLAG_REF`) e `TYPE_REF`.

`FLAG_REF` no CPython (`w_ref`): um objeto só entra na tabela de referências quando a contagem de referências dele é
maior que 1; objeto de contagem 1 sai sem marca e, repetido, seria escrito por inteiro. Esta implementação não tem
contagem de referências, então a aproxima de forma determinística, do jeito que ela se comporta num código recém
compilado:

* o objeto raiz leva a marca (quem chama o segura, a contagem é pelo menos 2; é o que dá o `\\xe3` de todo .pyc);
* objeto que aparece mais de uma vez no grafo leva a marca na primeira vez e vira `TYPE_REF` nas seguintes. A
  igualdade é por valor para int, float, complex, str e bytes (o compilador funde as constantes iguais), por valor
  também para tuple e frozenset dentro de `co_consts` (`merge_consts`), e por identidade para o resto (os campos
  estruturais de um código, como `co_names` e `co_localsplusnames`, são tuplas distintas ainda que iguais);
* objeto imortal ou internado leva a marca mesmo aparecendo uma vez: int de -5 a 256, tuple vazia, bytes de zero ou
  um byte e str internada. Str internada é a que só tem `[0-9A-Za-z_]` (`all_name_chars` do compilador, que interna
  as constantes assim e todos os nomes) e a vazia;
* os demais (bytes de `co_code`, tuplas de `co_consts` únicas, código aninhado, str não internada, float, int
  grande) saem sem marca quando aparecem uma vez.

Limite conhecido: dois objetos distintos e iguais que o CPython não fundiria (duas str dinâmicas iguais, fora de um
código) aqui viram um só; e a str dinâmica identificadora que o CPython não internou sai internada.

Na leitura aceita tudo o que o CPython escreve (as marcas, `TYPE_REF`, todas as formas de str)."""

version = 4


# O `marshal` do Debian é C embutido no executável: o módulo só tem a API pública. Os auxiliares
# vivem dentro de `_build`, que devolve os quatro nomes públicos e some no fim.
def _build():
    import struct
    import sys

    FLAG_REF = 0x80
    MAX_DEPTH = 2000
    # Funções definidas aqui viram `builtin_function_or_method` (módulo de C), sem `__code__`.
    CODE_TYPE = type(compile('0', '', 'eval'))

    # co_flags e tipos de variável de `localsplus` (Include/cpython/code.h).
    CO_VARARGS = 0x04
    CO_VARKEYWORDS = 0x08
    FAST_LOCAL = 0x20
    FAST_CELL = 0x40
    FAST_FREE = 0x80

    def is_name_str(s):
        """`all_name_chars` do compilador: o que o CPython interna ao criar o código."""
        return s.isascii() and all(c.isalnum() or c == '_' for c in s)

    def scalar_key(obj):
        t = type(obj)
        if t is str:
            return ('s', obj)
        if t is int:
            return ('i', obj)
        if t is float:
            return ('f', struct.pack('<d', obj))
        if t is complex:
            return ('c', struct.pack('<dd', obj.real, obj.imag))
        if t is bytes:
            return ('b', obj)
        return None

    def key_of(obj, merged):
        k = scalar_key(obj)
        if k is not None:
            return k
        if merged and type(obj) is tuple:
            return ('t', tuple(key_of(x, True) for x in obj))
        if merged and type(obj) is frozenset:
            return ('z', frozenset(key_of(x, True) for x in obj))
        return ('id', id(obj))

    def immortal(obj):
        t = type(obj)
        if t is int:
            return -5 <= obj <= 256
        if t is str:
            return is_name_str(obj)
        if t is tuple:
            return not obj
        if t is bytes:
            return len(obj) <= 1
        return False

    def local_kinds(code):
        """`co_localspluskinds` a partir de `co_varnames`, `co_cellvars` e `co_freevars` (o objeto da VM não
        guarda os bytes). Fica de fora o bit `CO_FAST_HIDDEN` das variáveis de compreensão inline, que o objeto
        não expõe."""
        cells = set(code.co_cellvars)
        kinds = bytearray()
        for name in code.co_varnames:
            kinds.append(FAST_LOCAL | (FAST_CELL if name in cells else 0))
        for name in code.co_cellvars:
            if name not in code.co_varnames:
                kinds.append(FAST_CELL)
        kinds.extend([FAST_FREE] * len(code.co_freevars))
        return bytes(kinds)

    def local_names(code):
        names = list(code.co_varnames)
        names.extend(n for n in code.co_cellvars if n not in code.co_varnames)
        names.extend(code.co_freevars)
        return tuple(names)

    class Writer:
        def __init__(self, ver, allow_code):
            self.version = ver
            self.allow_code = allow_code
            self.out = []
            self.counting = True
            self.counts = {}
            self.flagged = set()
            self.refs = {}
            self.keep = []
            self.fields = {}

        def w_long(self, n):
            self.out.append(struct.pack('<i', n))

        def w_type(self, code, flag):
            self.out.append(bytes([ord(code) | (FLAG_REF if flag else 0)]))

        def code_fields(self, code):
            """Campos estruturais do código, lidos uma vez só para as duas passadas verem os mesmos objetos."""
            got = self.fields.get(id(code))
            if got is None:
                got = {
                    'code': code.co_code,
                    'consts': code.co_consts,
                    'names': code.co_names,
                    'localsplusnames': local_names(code),
                    'localspluskinds': local_kinds(code),
                    'linetable': code.co_linetable,
                    'exceptiontable': code.co_exceptiontable,
                }
                self.keep.append(code)
                self.fields[id(code)] = got
            return got

        def ref(self, obj, merged, root):
            """`w_ref`: devolve (já escreveu um TYPE_REF, marcar FLAG_REF)."""
            if self.version < 3:
                return False, False
            key = key_of(obj, merged)
            if self.counting:
                seen = self.counts.get(key, 0)
                self.counts[key] = seen + 1
                self.keep.append(obj)
                if seen == 0 and (root or immortal(obj)):
                    self.counts[key] = 2
                return seen > 0, False
            index = self.refs.get(key)
            if index is not None:
                self.out.append(b'r')
                self.w_long(index)
                return True, False
            if self.counts[key] > 1:
                self.refs[key] = len(self.refs)
                return False, True
            return False, False

        def w_object(self, obj, depth, merged=False):
            if depth > MAX_DEPTH:
                raise ValueError('object too deeply nested to marshal')
            if obj is None:
                return self.single(b'N')
            if obj is True:
                return self.single(b'T')
            if obj is False:
                return self.single(b'F')
            if obj is StopIteration:
                return self.single(b'S')
            if obj is Ellipsis:
                return self.single(b'.')
            done, flag = self.ref(obj, merged, depth == 0)
            if done:
                return
            self.complex_object(obj, flag, depth, merged)

        def single(self, tag):
            if not self.counting:
                self.out.append(tag)

        def w_bytes(self, tag, flag, data, short=False):
            if self.counting:
                return
            self.w_type(tag, flag)
            if short:
                self.out.append(bytes([len(data)]))
            else:
                self.w_long(len(data))
            self.out.append(data)

        def complex_object(self, obj, flag, depth, merged):
            t = type(obj)
            counting = self.counting
            if isinstance(obj, int):
                if counting:
                    return
                if -0x80000000 <= obj <= 0x7fffffff:
                    self.w_type('i', flag)
                    self.w_long(obj)
                else:
                    self.w_type('l', flag)
                    digits = []
                    n = abs(obj)
                    while n:
                        digits.append(n & 0x7fff)
                        n >>= 15
                    self.w_long(-len(digits) if obj < 0 else len(digits))
                    for d in digits:
                        self.out.append(struct.pack('<H', d))
            elif isinstance(obj, float):
                if counting:
                    return
                if self.version > 1:
                    self.w_type('g', flag)
                    self.out.append(struct.pack('<d', obj))
                else:
                    text = repr(float(obj)).encode()
                    self.w_type('f', flag)
                    self.out.append(bytes([len(text)]) + text)
            elif isinstance(obj, complex):
                if counting:
                    return
                if self.version > 1:
                    self.w_type('y', flag)
                    self.out.append(struct.pack('<dd', obj.real, obj.imag))
                else:
                    self.w_type('x', flag)
                    for part in (obj.real, obj.imag):
                        text = repr(float(part)).encode()
                        self.out.append(bytes([len(text)]) + text)
            elif t is str or isinstance(obj, str):
                self.w_str(obj, flag)
            elif isinstance(obj, (bytes, bytearray, memoryview)):
                self.w_bytes('s', flag, bytes(obj))
            elif isinstance(obj, tuple):
                if not counting:
                    if self.version >= 4 and len(obj) < 256:
                        self.w_type(')', flag)
                        self.out.append(bytes([len(obj)]))
                    else:
                        self.w_type('(', flag)
                        self.w_long(len(obj))
                for item in obj:
                    self.w_object(item, depth + 1, merged)
            elif isinstance(obj, list):
                if not counting:
                    self.w_type('[', flag)
                    self.w_long(len(obj))
                for item in obj:
                    self.w_object(item, depth + 1)
            elif isinstance(obj, dict):
                if not counting:
                    self.w_type('{', flag)
                for k, v in obj.items():
                    self.w_object(k, depth + 1)
                    self.w_object(v, depth + 1)
                if not counting:
                    self.out.append(b'0')
            elif isinstance(obj, (set, frozenset)):
                if not counting:
                    self.w_type('>' if isinstance(obj, frozenset) else '<', flag)
                    self.w_long(len(obj))
                for item in obj:
                    self.w_object(item, depth + 1, merged)
            elif t is CODE_TYPE:
                if not self.allow_code:
                    raise ValueError('unmarshallable object')
                self.w_code(obj, flag, depth)
            else:
                raise ValueError('unmarshallable object')

        def w_str(self, obj, flag):
            if self.counting:
                return
            data = obj.encode('utf-8', 'surrogatepass')
            interned = is_name_str(obj)
            if self.version >= 4 and obj.isascii():
                if len(data) < 256:
                    self.w_bytes('Z' if interned else 'z', flag, data, True)
                else:
                    self.w_bytes('A' if interned else 'a', flag, data)
            else:
                self.w_bytes('t' if interned and self.version >= 3 else 'u', flag, data)

        def w_code(self, code, flag, depth):
            f = self.code_fields(code)
            counting = self.counting
            if not counting:
                self.w_type('c', flag)
                self.w_long(code.co_argcount)
                self.w_long(code.co_posonlyargcount)
                self.w_long(code.co_kwonlyargcount)
                self.w_long(code.co_stacksize)
                self.w_long(code.co_flags)
            self.w_object(f['code'], depth + 1)
            self.w_object(f['consts'], depth + 1, True)
            self.w_object(f['names'], depth + 1)
            self.w_object(f['localsplusnames'], depth + 1)
            self.w_object(f['localspluskinds'], depth + 1)
            self.w_object(code.co_filename, depth + 1)
            self.w_object(code.co_name, depth + 1)
            self.w_object(code.co_qualname, depth + 1)
            if not counting:
                self.w_long(code.co_firstlineno)
            self.w_object(f['linetable'], depth + 1)
            self.w_object(f['exceptiontable'], depth + 1)

    def dumps(value, version=version, /, *, allow_code=True):
        w = Writer(version, allow_code)
        w.w_object(value, 0)
        w.counting = False
        w.w_object(value, 0)
        return b''.join(w.out)

    def dump(value, file, version=version, /, *, allow_code=True):
        file.write(dumps(value, version, allow_code=allow_code))

    class Reader:
        def __init__(self, data, allow_code):
            self.data = data
            self.pos = 0
            self.refs = []
            self.allow_code = allow_code

        def take(self, n):
            end = self.pos + n
            if n < 0 or end > len(self.data):
                raise EOFError('marshal data too short')
            chunk = self.data[self.pos:end]
            self.pos = end
            return chunk

        def long(self):
            return struct.unpack('<i', self.take(4))[0]

        def text(self, n, interned=False):
            raw = self.take(n)
            value = raw.decode('utf-8', 'surrogatepass')
            return sys.intern(value) if interned else value

        def code(self, slot):
            if not self.allow_code:
                raise ValueError('unmarshalling code objects is disallowed')
            argcount = self.long()
            posonly = self.long()
            kwonly = self.long()
            stacksize = self.long()
            flags = self.long()
            body = self.obj()
            consts = self.obj()
            names = self.obj()
            localsplus = self.obj()
            kinds = self.obj()
            filename = self.obj()
            name = self.obj()
            qualname = self.obj()
            firstlineno = self.long()
            linetable = self.obj()
            exceptiontable = self.obj()
            if len(localsplus) != len(kinds):
                raise ValueError('bad marshal data (code: localsplusnames and localspluskinds differ)')
            varnames = tuple(n for n, k in zip(localsplus, kinds) if k & FAST_LOCAL)
            cellvars = tuple(n for n, k in zip(localsplus, kinds) if k & FAST_CELL)
            freevars = tuple(n for n, k in zip(localsplus, kinds) if k & FAST_FREE)
            try:
                value = CODE_TYPE(argcount, posonly, kwonly, len(varnames), stacksize, flags, body, consts, names,
                                  varnames, filename, name, qualname, firstlineno, linetable, exceptiontable,
                                  freevars, cellvars)
            except TypeError:
                raise ValueError('bad marshal data (code objects cannot be built by this runtime)') from None
            if slot is not None:
                self.refs[slot] = value
            return value

        def obj(self):
            code = self.take(1)[0]
            flag = code & FLAG_REF
            t = chr(code & ~FLAG_REF)
            slot = None
            if flag:
                slot = len(self.refs)
                self.refs.append(None)

            def done(value):
                if slot is not None:
                    self.refs[slot] = value
                return value

            if t == 'N':
                return done(None)
            if t == 'T':
                return done(True)
            if t == 'F':
                return done(False)
            if t == 'S':
                return done(StopIteration)
            if t == '.':
                return done(Ellipsis)
            if t == 'i':
                return done(self.long())
            if t == 'l':
                n = self.long()
                count = abs(n)
                value = 0
                for i in range(count):
                    value |= struct.unpack('<H', self.take(2))[0] << (15 * i)
                return done(-value if n < 0 else value)
            if t == 'g':
                return done(struct.unpack('<d', self.take(8))[0])
            if t == 'f':
                return done(float(self.take(self.take(1)[0]).decode()))
            if t == 'y':
                re, im = struct.unpack('<dd', self.take(16))
                return done(complex(re, im))
            if t == 'x':
                re = float(self.take(self.take(1)[0]).decode())
                im = float(self.take(self.take(1)[0]).decode())
                return done(complex(re, im))
            if t in 'uat':
                return done(self.text(self.long(), t == 't'))
            if t in 'zZ':
                return done(self.text(self.take(1)[0], t == 'Z'))
            if t == 'A':
                return done(self.text(self.long(), True))
            if t == 's':
                return done(bytes(self.take(self.long())))
            if t == ')':
                n = self.take(1)[0]
                return done(tuple(self.obj() for _ in range(n)))
            if t == '(':
                n = self.long()
                if n < 0:
                    raise ValueError('bad marshal data (tuple size out of range)')
                return done(tuple(self.obj() for _ in range(n)))
            if t == '[':
                result = []
                done(result)
                for _ in range(self.long()):
                    result.append(self.obj())
                return result
            if t == '{':
                result = {}
                done(result)
                while True:
                    if self.data[self.pos:self.pos + 1] == b'0':
                        self.pos += 1
                        return result
                    key = self.obj()
                    result[key] = self.obj()
            if t == '<':
                result = set()
                done(result)
                for _ in range(self.long()):
                    result.add(self.obj())
                return result
            if t == '>':
                n = self.long()
                return done(frozenset(self.obj() for _ in range(n)))
            if t == 'c':
                return self.code(slot)
            if t == 'r':
                index = self.long()
                if not 0 <= index < len(self.refs):
                    raise ValueError('bad marshal data (invalid reference)')
                return self.refs[index]
            raise ValueError('bad marshal data (unknown type code)')

    def loads(data, /, *, allow_code=True):
        return Reader(bytes(data), allow_code).obj()

    def load(file, /, *, allow_code=True):
        data = file.read()
        reader = Reader(bytes(data), allow_code)
        value = reader.obj()
        rest = len(data) - reader.pos
        if rest and hasattr(file, 'seek'):
            file.seek(-rest, 1)
        return value

    public = {'dumps': dumps, 'dump': dump, 'loads': loads, 'load': load}
    for name, f in public.items():
        f.__qualname__ = name
    return public


globals().update(_build())
del _build
