import re, sys, itertools
from sim import L, parse

def line_for(text, W, gap, flag, mode, kwi, P):
    S = text.replace('\n', ' ').strip()
    t = len(flag)
    ref = "in.txt:1"
    if mode == 'none': M = gap; Wp = W; R = 0
    elif mode == 'R': M = 0; Wp = W; R = 0
    else:
        R = len(ref); M = R + gap; Wp = W - (R + gap)
    h = Wp // 2
    Bm = max(0, h - gap - 2 * t)
    Km = max(0, h - 2 * t)
    ms = list(re.finditer(r'[A-Za-z0-9_]+', S))
    m = ms[kwi]
    ks, ke = m.start(), m.end(); kw = m.group()
    bt = L(S[:ks]); at = L(S[ke:])
    Am = max(0, Km - len(kw))
    before = bt.win_end((0, bt.n()), Bm)
    after = at.win_start((0, at.n()), Am)
    blen = before[1] - before[0]
    alen = after[1] - after[0]
    uL = Bm - blen
    max_tail = max(0, Bm - blen - gap - 1)
    ts = at.trim((after[1], at.n()))[0]
    tail = at.win_start((ts, at.n()), max_tail)
    if tail[1] - tail[0] > 2 and at.sp(tail[1] - 2) and not at.sp(tail[1] - 1):
        tail = at.trim((tail[0], tail[1] - 1))
    K = len(kw) + alen
    uR = max(0, Km - K - gap)
    btxt = bt.t[before[0]:before[1]]
    ell = max(len(w) for w in re.findall(r'[A-Za-z]+', S))
    cap2 = ell + (h - blen) + P['b'] + P['c'] * t
    cap2 = max(cap2, 0)
    full = bt.trim((0, before[0]))
    fl = full[1] - full[0]
    if fl <= min(uR, cap2):
        head = full
    else:
        head = bt.win_end((0, before[0]), max(0, min(uR, cap2 - P['e'] * t)))
    f = dict(before=bt.t[before[0]:before[1]], after=at.t[after[0]:after[1]],
             tail=at.t[tail[0]:tail[1]], head=bt.t[head[0]:head[1]])
    if after[1] != at.n():
        if tail[0] == tail[1]: f['after'] += flag
        elif tail[1] != at.n(): f['tail'] += flag
    if before[0] != 0:
        if head[0] == head[1]: f['before'] = flag + f['before']
        elif head[0] != 0: f['head'] = flag + f['head']
    E = M + h - gap; Kc = M + h
    buf = [' '] * 400
    def put(col, s):
        for i, c in enumerate(s): buf[col + i] = c
    put(M, f['tail'])
    bf = f['before']
    if bf == flag and flag != '' and before[0] != 0 and before[0] == before[1]:
        put(E + 1 - len(bf), bf); Kc += 1
    else:
        put(E - len(bf), bf)
    put(Kc, kw + f['after'])
    hd = f['head']
    if hd: put(M + Wp - len(hd), hd)
    s = ''.join(buf).rstrip()
    if mode == 'A': s = ref + ':' + s[len(ref) + 1:]
    if mode == 'R': s = s.ljust(W + gap) + ref
    return s

def run(text, W, gap, flag, mode, P):
    n = len(list(re.finditer(r'[A-Za-z0-9_]+', text)))
    return [line_for(text, W, gap, flag, mode, i, P) for i in range(n)]

def load(paths, modes=('none', 'R')):
    items = []
    for path in paths:
        for hdr, lines in parse(path).items():
            m = re.match(r'@@ w=(\d+) g=(\d+) f=\[(.*)\] t=\[(.*)\] opt=(\w+)', hdr)
            W, g, f, t, mode = int(m[1]), int(m[2]), m[3], m[4], m[5]
            if mode in modes: items.append((W, g, f, t, mode, lines))
    return items

if __name__ == '__main__':
    items = load(['ptxdata.txt', 'ptxdata2.txt'])
    best = []
    for a, b, c, d in itertools.product((1, 2, 3), range(0, 12), range(-2, 3), range(-2, 3)):
        P = dict(a=a, b=b, c=c, d=d)
        ok = sum(sorted(run(t, W, g, f, mode, P)) == sorted(lines) for W, g, f, t, mode, lines in items)
        best.append((ok, P))
    best.sort(key=lambda x: -x[0])
    print(len(items))
    for ok, P in best[:6]: print(ok, P)
