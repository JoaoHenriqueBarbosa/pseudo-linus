import re, sys, collections
from sim import L, parse

def parts(text, W, gap, flag, mode, kwi):
    S = text.replace('\n', ' ').strip()
    t = len(flag)
    ref = "in.txt:1"
    if mode == 'none': M = gap; Wp = W
    elif mode == 'R': M = 0; Wp = W
    else:
        R = len(ref); M = R + gap; Wp = W - (R + gap)
    h = Wp // 2
    Bm = max(0, h - gap - 2 * t); Km = max(0, h - 2 * t)
    ms = list(re.finditer(r'[A-Za-z0-9_]+', S))
    m = ms[kwi]; ks, ke = m.start(), m.end(); kw = m.group()
    bt = L(S[:ks]); at = L(S[ke:])
    before = bt.win_end((0, bt.n()), Bm)
    after = at.win_start((0, at.n()), max(0, Km - len(kw)))
    return dict(S=S, t=t, M=M, Wp=Wp, h=h, Bm=Bm, Km=Km, kw=kw, bt=bt, at=at, before=before, after=after, gap=gap, flag=flag, mode=mode, W=W)

def render(p, head_cap, tail_cap):
    bt, at, before, after = p['bt'], p['at'], p['before'], p['after']
    t, gap, flag, M, Wp, h, Km, Bm, kw = p['t'], p['gap'], p['flag'], p['M'], p['Wp'], p['h'], p['Km'], p['Bm'], p['kw']
    blen = before[1] - before[0]
    ts = at.trim((after[1], at.n()))[0]
    tail = at.win_start((ts, at.n()), tail_cap)
    if tail[1] - tail[0] > 2 and at.sp(tail[1] - 2) and not at.sp(tail[1] - 1):
        tail = at.trim((tail[0], tail[1] - 1))
    head = bt.win_end((0, before[0]), head_cap)
    f = dict(before=bt.t[before[0]:before[1]], after=at.t[after[0]:after[1]], tail=at.t[tail[0]:tail[1]], head=bt.t[head[0]:head[1]])
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
    if p['mode'] == 'A': s = "in.txt:1" + ':' + s[9:]
    if p['mode'] == 'R': s = s.ljust(p['W'] + gap) + "in.txt:1"
    return s, f, head

if __name__ == '__main__':
    rows = []
    for path in sys.argv[1:]:
        for hdr, lines in parse(path).items():
            m = re.match(r'@@ w=(\d+) g=(\d+) f=\[(.*)\] t=\[(.*)\] opt=(\w+)', hdr)
            W, g, f, t, mode = int(m[1]), int(m[2]), m[3], m[4], m[5]
            if mode not in ('none', 'R'): continue
            n = len(list(re.finditer(r'[A-Za-z0-9_]+', t)))
            orac = set(lines)
            for i in range(n):
                p = parts(t, W, g, f, mode, i)
                blen = p['before'][1] - p['before'][0]
                btxt = p['bt'].t[p['before'][0]:p['before'][1]]
                ell = len(btxt.split(' ')[-1]) if btxt else 0
                uL = p['Bm'] - blen
                K = len(p['kw']) + p['after'][1] - p['after'][0]
                uR = max(0, p['Km'] - K - g)
                tcap = max(0, p['Bm'] - blen - g - 1)
                valid = []
                for hc in range(0, 70):
                    s, fl, head = render(p, hc, tcap)
                    if s in orac: valid.append(hc)
                if valid:
                    # head 'flagged' para o menor cap válido
                    s, fl, head = render(p, valid[0], tcap)
                    flagged = p['bt'].t[head[0]:head[1]] != '' and head[0] != 0 and f != ''
                    rows.append(dict(W=W, g=g, t=len(f), ell=ell, uL=uL, uR=uR, lo=min(valid), hi=max(valid), blen=blen, K=K, headlen=head[1]-head[0], flagged=(head[0] != 0), hh=len(f)))
    import pickle
    pickle.dump(rows, open('rows3.pkl', 'wb'))
    print(len(rows))
