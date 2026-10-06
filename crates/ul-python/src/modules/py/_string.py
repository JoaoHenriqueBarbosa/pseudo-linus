"""_string: o analisador de formato que `string.Formatter` usa (`str.format` já vive na VM)."""


def formatter_parser(s):
    if not isinstance(s, str):
        raise TypeError("expected str, got %s" % type(s).__name__)
    out = []
    i = 0
    n = len(s)
    lit = []
    while i < n:
        c = s[i]
        if c == '{':
            if i + 1 < n and s[i + 1] == '{':
                out.append((''.join(lit) + '{', None, None, None))
                lit = []
                i += 2
                continue
            if i + 1 >= n:
                raise ValueError("Single '{' encountered in format string")
            depth = 1
            j = i + 1
            while j < n and depth:
                if s[j] == '{':
                    depth += 1
                elif s[j] == '}':
                    depth -= 1
                j += 1
            if depth:
                raise ValueError("expected '}' before end of string")
            body = s[i + 1:j - 1]
            k = 0
            bracket = False
            while k < len(body):
                ch = body[k]
                if ch == '[':
                    bracket = True
                elif ch == ']':
                    bracket = False
                elif not bracket and ch in '!:':
                    break
                k += 1
            name = body[:k]
            conversion = None
            spec = ''
            rest = body[k:]
            if rest.startswith('!'):
                if len(rest) < 2:
                    raise ValueError("end of string while looking for conversion specifier")
                conversion = rest[1]
                rest = rest[2:]
                if rest and not rest.startswith(':'):
                    raise ValueError("expected ':' after conversion specifier")
            if rest.startswith(':'):
                spec = rest[1:]
            out.append((''.join(lit), name, spec, conversion))
            lit = []
            i = j
        elif c == '}':
            if i + 1 < n and s[i + 1] == '}':
                out.append((''.join(lit) + '}', None, None, None))
                lit = []
                i += 2
                continue
            raise ValueError("Single '}' encountered in format string")
        else:
            lit.append(c)
            i += 1
    if lit:
        out.append((''.join(lit), None, None, None))
    return iter(out)


def formatter_field_name_split(name):
    if not isinstance(name, str):
        raise TypeError("expected str, got %s" % type(name).__name__)
    i = 0
    n = len(name)
    while i < n and name[i] not in '.[':
        i += 1
    first = name[:i]
    if first.isdigit():
        first = int(first)
    rest = []
    while i < n:
        if name[i] == '.':
            j = i + 1
            while j < n and name[j] not in '.[':
                j += 1
            if j == i + 1:
                raise ValueError("Empty attribute in format string")
            rest.append((True, name[i + 1:j]))
            i = j
        elif name[i] == '[':
            j = name.find(']', i)
            if j < 0:
                raise ValueError("Missing ']' in format string")
            key = name[i + 1:j]
            rest.append((False, int(key) if key.isdigit() else key))
            i = j + 1
        else:
            raise ValueError("Only '.' or '[' may follow ']' in format field specifier")
    return first, iter(rest)
