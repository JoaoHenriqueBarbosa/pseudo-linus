"""`_tokenize.TokenizerIter`: tokenizador por regex no formato que o `tokenize.py` do 3.13 espera.

Diferença conhecida do CPython: f-strings saem como um único token STRING (sem FSTRING_START/MIDDLE/END).
"""
import re
import token as _t

_NAME = r'\w+'
_HEX = r'0[xX](?:_?[0-9a-fA-F])+'
_BIN = r'0[bB](?:_?[01])+'
_OCT = r'0[oO](?:_?[0-7])+'
_DEC = r'(?:0(?:_?0)*|[1-9](?:_?[0-9])*)'
_INT = '(?:' + _HEX + '|' + _BIN + '|' + _OCT + '|' + _DEC + ')'
_EXP = r'[eE][-+]?[0-9](?:_?[0-9])*'
_PTFLOAT = r'[0-9](?:_?[0-9])*\.(?:[0-9](?:_?[0-9])*)?|\.[0-9](?:_?[0-9])*'
_FLOAT = '(?:(?:' + _PTFLOAT + ')(?:' + _EXP + ')?|[0-9](?:_?[0-9])*' + _EXP + ')'
_IMAG = '(?:[0-9](?:_?[0-9])*[jJ]|' + _FLOAT + '[jJ])'
_NUMBER = '(?:' + _IMAG + '|' + _FLOAT + '|' + _INT + ')'
_PREFIX = r'(?:[rRbBuUfF]|[rR][bBfF]|[bBfF][rR])?'
_OPS = sorted(
    ['**=', '//=', '>>=', '<<=', '...', '!=', '%=', '&=', '**', '*=', '+=', '-=', '->', '//', '/=', ':=',
     '<<', '<=', '==', '>=', '>>', '@=', '^=', '|=', '~', '%', '&', '(', ')', '*', '+', ',', '-', '.', '/',
     ':', ';', '<', '=', '>', '@', '[', ']', '^', '{', '|', '}', '!'],
    key=len, reverse=True)
_OP_RE = '|'.join(re.escape(o) for o in _OPS)
_TOKEN = re.compile(
    '(?P<ws>[ \\f\\t]*)(?:(?P<comment>#[^\\r\\n]*)|(?P<number>' + _NUMBER + ')|(?P<string>' + _PREFIX +
    '(?:\'\'\'|"""|\'|"))|(?P<name>' + _NAME + ')|(?P<op>' + _OP_RE + ')|(?P<nl>\\r?\\n)|(?P<cont>\\\\\\r?\\n))')
_END = {"'": re.compile(r"[^'\\]*(?:\\.[^'\\]*)*'", re.S), '"': re.compile(r'[^"\\]*(?:\\.[^"\\]*)*"', re.S),
        "'''": re.compile(r"[^'\\]*(?:(?:\\.|'(?!''))[^'\\]*)*'''", re.S),
        '"""': re.compile(r'[^"\\]*(?:(?:\\.|"(?!""))[^"\\]*)*"""', re.S)}


class TokenizerIter:
    def __init__(self, readline, *, extra_tokens=False, encoding=None):
        self._readline = readline
        self._extra = extra_tokens
        self._encoding = encoding
        self._gen = self._run()

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._gen)

    def _read(self):
        try:
            line = self._readline()
        except StopIteration:
            return ''
        if isinstance(line, bytes):
            line = line.decode(self._encoding or 'utf-8')
        return line

    def _error(self, msg, lnum, col, line):
        raise SyntaxError(msg, ('<string>', lnum, col + 1, line))

    def _run(self):
        lnum = 0
        parenlev = 0
        indents = [0]
        continued = False
        last_line = ''
        line = ''
        while True:
            last_line = line
            line = self._read()
            lnum += 1
            pos, maxpos = 0, len(line)
            if not line:
                break
            if not continued and parenlev == 0:
                col = 0
                while pos < maxpos and line[pos] in ' \t\f':
                    col = (col // 8 + 1) * 8 if line[pos] == '\t' else (0 if line[pos] == '\f' else col + 1)
                    pos += 1
                if pos == maxpos:
                    break
                if line[pos] in '#\r\n':
                    if line[pos] == '#':
                        text = line[pos:].rstrip('\r\n')
                        yield (_t.COMMENT, text, (lnum, pos), (lnum, pos + len(text)), line)
                        pos += len(text)
                    if self._extra:
                        yield (_t.NL, line[pos:], (lnum, pos), (lnum, len(line)), line)
                    continue
                if col > indents[-1]:
                    indents.append(col)
                    yield (_t.INDENT, line[:pos], (lnum, 0), (lnum, pos), line)
                while col < indents[-1]:
                    if col not in indents:
                        raise IndentationError('unindent does not match any outer indentation level',
                                               ('<string>', lnum, pos, line))
                    indents = indents[:-1]
                    yield (_t.DEDENT, '', (lnum, pos), (lnum, pos), line)
            else:
                continued = False
            while pos < maxpos:
                m = _TOKEN.match(line, pos)
                if m is None:
                    if line[pos:].strip(' \t\f\r\n') == '':
                        break
                    self._error('invalid syntax', lnum, pos, line)
                kind = m.lastgroup
                start = m.start(kind)
                end = m.end(kind)
                pos = m.end()
                if kind == 'comment':
                    yield (_t.COMMENT, m.group('comment'), (lnum, start), (lnum, end), line)
                elif kind == 'number':
                    yield (_t.NUMBER, m.group('number'), (lnum, start), (lnum, end), line)
                elif kind == 'name':
                    yield (_t.NAME, m.group('name'), (lnum, start), (lnum, end), line)
                elif kind == 'op':
                    s = m.group('op')
                    if s in '([{':
                        parenlev += 1
                    elif s in ')]}':
                        parenlev -= 1
                    yield (_t.OP, s, (lnum, start), (lnum, end), line)
                elif kind == 'nl':
                    typ = _t.NL if parenlev > 0 else _t.NEWLINE
                    if typ == _t.NL and not self._extra:
                        continue
                    yield (typ, m.group('nl'), (lnum, start), (lnum, end), line)
                elif kind == 'cont':
                    continued = True
                    pos = maxpos
                elif kind == 'string':
                    quote = m.group('string')
                    q = quote.lstrip('rRbBuUfF')
                    sl, sc = lnum, start
                    body = line[pos:]
                    em = _END[q].match(body)
                    text = line[start:pos]
                    if em:
                        text += em.group()
                        pos += em.end()
                        yield (_t.STRING, text, (sl, sc), (lnum, pos), line)
                    elif len(q) == 3 or body.endswith('\\\n') or body.endswith('\\\r\n'):
                        text = line[start:]
                        pos = maxpos
                        full = [line]
                        while True:
                            nxt = self._read()
                            if not nxt:
                                self._error('unterminated triple-quoted string literal (detected at line %d)' % lnum
                                            if len(q) == 3 else 'unterminated string literal', sl, sc, line)
                            lnum += 1
                            full.append(nxt)
                            em = _END[q].match(nxt)
                            if em:
                                text += nxt[:em.end()]
                                pos = em.end()
                                maxpos = len(nxt)
                                yield (_t.STRING, text, (sl, sc), (lnum, pos), ''.join(full))
                                line = nxt
                                break
                            text += nxt
                    else:
                        self._error('unterminated string literal (detected at line %d)' % lnum, lnum, start, line)
        if last_line and last_line[-1:] not in '\r\n' and not last_line.strip().startswith('#'):
            yield (_t.NEWLINE, '', (lnum - 1, len(last_line)), (lnum - 1, len(last_line) + 1), '')
        for _ in indents[1:]:
            yield (_t.DEDENT, '', (lnum, 0), (lnum, 0), '')
        yield (_t.ENDMARKER, '', (lnum, 0), (lnum, 0), '')
