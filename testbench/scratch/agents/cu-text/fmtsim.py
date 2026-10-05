import sys, itertools
INF = 1 << 60

def wrap(words, first_indent, other_indent, width, goal, P, last_line_length=0, spaces=None):
    # words: list of (text, space_after)  (space após a palavra; a última tem 0)
    n = len(words)
    length = [len(w[0]) for w in words]
    space = [w[1] for w in words]
    period = [w[2] if len(w) > 2 else False for w in words]
    final = [w[3] if len(w) > 3 else False for w in words]
    punct = [w[4] if len(w) > 4 else False for w in words]
    paren = [w[5] if len(w) > 5 else False for w in words]
    best_cost = [0] * (n + 1)
    next_break = [n] * (n + 1)
    line_length = [0] * (n + 1)
    sentinel = width
    SQR = lambda x: x * x
    SHORT = lambda x: (P['short'] * SQR(x)) * (P.get('over', 1) if x < 0 else 1)
    RAGGED = lambda x: (P['short'] * SQR(x)) // P['rdiv']
    WIDOW = lambda x: P['widow'] // (x + 2)
    ORPHAN = lambda x: P['orphan'] // (x + 2)
    def line_cost(nxt, ln):
        if nxt == n:
            return P.get('lastk', 0) * SQR(max(0, goal - ln) if P.get('lastmax', True) else goal - ln) // P.get('lastdiv', 1)
        c = SHORT(goal - ln)
        if next_break[nxt] != n:
            c += RAGGED(ln - line_length[nxt])
        elif P.get('rl'):
            c += (P['short'] * SQR(ln - line_length[nxt])) // P['rl']
        return c
    def base_cost(this):
        cost = P['base']
        if this > 0:
            if period[this - 1]:
                if final[this - 1]: cost -= P['sentence']
                else: cost += P['nobreak']
            elif punct[this - 1]:
                cost -= P['punct']
            elif this > 1 and final[this - 2]:
                cost += WIDOW(length[this - 1])
        if paren[this]:
            cost -= P['paren']
        elif punct[this] and False: pass
        om = P.get('omode', 'a')
        if om == 'a':
            if final[this] and this + 1 < n:
                cost += ORPHAN(length[this + 1])
        elif om == 'b':
            if final[this] and next_break[this] == this + 1:
                cost += ORPHAN(length[this])
        elif om == 'c':
            if final[this] and this + 1 < n:
                cost += ORPHAN(length[this])
        elif om == 'd':
            if this + 1 < n and final[this + 1]:
                cost += ORPHAN(length[this + 1])
        if this == n - 1:
            cost += P.get('lone', 0) // (length[this] + P.get('lone_off', 2))
        if P.get('lastw') and next_break[this] == n:
            cost += P['lastw'] // (line_length[this] + P.get('lastoff', 2))
        return cost
    best_cost[n] = 0
    for start in range(n - 1, -1, -1):
        best = INF
        ln = first_indent if start == 0 else other_indent
        w = start
        ln += length[w]
        while True:
            w += 1
            wcost = line_cost(w, ln) + best_cost[w]
            if start == 0 and last_line_length > 0:
                wcost += RAGGED(ln - last_line_length)
            if wcost < best:
                best = wcost; next_break[start] = w; line_length[start] = ln
            if w == n: break
            ln += space[w - 1] + length[w]
            if not ln < width: break
        best_cost[start] = best + base_cost(start)
    # reconstrói
    lines = []
    i = 0
    while i < n:
        j = next_break[i]
        line = ''
        for k in range(i, j):
            line += words[k][0]
            if k < j - 1: line += ' ' * space[k]
        lines.append(line)
        i = j
    return lines

def mk2(text):
    # texto com 1 espaço após palavra normal e 2 após ponto final
    import re
    toks = re.split(r' +', text.strip())
    out = []
    n = len(toks)
    for k, w in enumerate(toks):
        period = w[-1] in '.?!'
        last = k == n - 1
        final = period  # sempre seguido de 2 espaços ou fim
        punct = (not period) and w[-1] in ',;:'
        out.append((w, 0 if last else (2 if final else 1), period, final or last, punct, False))
    return out

def mk(text):
    ws = text.split()
    return [(w, 1, False, False, False, False) for w in ws[:-1]] + [(ws[-1], 0, False, True, False, False)]

if __name__ == '__main__':
    tests = [
      ("the quick brown fox jumps over the lazy dog and keeps running", 0, 0, 20, ["the quick brown","fox jumps over","the lazy dog and","keeps running"]),
      ("recuado um pouco e com bastante texto pra quebrar em mais de uma linha", 4, 4, 30, ["recuado um pouco e","com bastante texto","pra quebrar em mais de","uma linha"]),
    ]
    P = dict(short=1, ragged=0.5, widow=200, orphan=150, base=0, sentence=0, nobreak=0, punct=0, paren=0)
    for goalf in (lambda w: w*93//100, lambda w: w*187//200):
        for text, fi, oi, width, exp in tests:
            goal = goalf(width)
            out = wrap(mk(text), fi, oi, width, goal, P)
            print(goal, out == exp, out)
