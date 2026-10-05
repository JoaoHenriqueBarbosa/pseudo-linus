import re, sys, collections
from sim import *

def one(text, W, gap, flag, mode, P, kwi, hm_override=None, tm_override=None):
    # replica de run() que devolve (linha, features) para a kwi-ésima palavra
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
    before_text = L(S[:ks]); after_text = L(S[ke:])
    Am = max(0, Km - len(kw))
    before = before_text.win_end((0, before_text.n()), Bm)
    after = after_text.win_start((0, after_text.n()), Am)
    blen = before[1] - before[0]
    alen = after[1] - after[0]
    max_tail = max(0, Bm - blen - gap - 1) if tm_override is None else tm_override
    ts = after_text.trim((after[1], after_text.n()))[0]
    tail = after_text.win_start((ts, after_text.n()), max_tail)
    if tail[1] - tail[0] > 2 and after_text.sp(tail[1] - 2) and not after_text.sp(tail[1] - 1):
        tail = after_text.trim((tail[0], tail[1] - 1))
    max_head = max(0, Km - (len(kw) + alen) - gap) if hm_override is None else hm_override
    full = before_text.trim((0, before[0]))
    if full[1]-full[0] <= max_head:
        head = full
    else:
        head = before_text.win_end((0, before[0]), max(0, max_head - t))
    f = dict(before=before_text.t[before[0]:before[1]], after=after_text.t[after[0]:after[1]],
             tail=after_text.t[tail[0]:tail[1]], head=before_text.t[head[0]:head[1]])
    if after[1] != after_text.n():
        if tail[0] == tail[1]: f['after'] += flag
        elif tail[1] != after_text.n(): f['tail'] += flag
    if before[0] != 0:
        if head[0] == head[1]: f['before'] = flag + f['before']
        elif head[0] != 0: f['head'] = flag + f['head']
    E = M + h - gap; K = M + h
    buf = [' '] * 400
    def put(col, s):
        for i, c in enumerate(s): buf[col + i] = c
    put(M, f['tail'])
    bf = f['before']
    if bf == flag and flag != '' and before[0] != 0 and before[0] == before[1]:
        put(E + 1 - len(bf), bf); K += 1
    else:
        put(E - len(bf), bf)
    put(K, kw + f['after'])
    hd = f['head']
    if hd: put(M + Wp - len(hd), hd)
    s = ''.join(buf).rstrip()
    if mode == 'A': s = ref + ':' + s[len(ref) + 1:]
    if mode == 'R': s = s.ljust(W + gap) + ref
    feat = dict(W=W, g=gap, t=t, h=h, Km=Km, Bm=Bm, klen=len(kw), alen=alen, blen=blen, M=M, Wp=Wp,
                tail_len=len(f['tail']), head_len=len(f['head']), hasafter_trunc=after[1] != after_text.n(),
                kw=kw)
    return s, feat

if __name__ == '__main__':
    blocks = parse('ptxdata.txt')
    rows = []
    for hdr, lines in blocks.items():
        m = re.match(r'@@ w=(\d+) g=(\d+) f=\[(.*)\] t=\[(.*)\] opt=(\w+)', hdr)
        W, g, f, t, mode = int(m[1]), int(m[2]), m[3], m[4], m[5]
        if mode != 'none' and mode != 'R': continue
        S = t
        n = len(list(re.finditer(r'[A-Za-z0-9_]+', S)))
        orac = set(lines)
        for i in range(n):
            valid = []
            for hm in range(0, 61):
                s, ft = one(t, W, g, f, mode, None, i, hm_override=hm)
                if s in orac: valid.append(hm)
            s0, ft0 = one(t, W, g, f, mode, None, i)
            if valid:
                rows.append((ft0, min(valid), max(valid)))
            else:
                rows.append((ft0, None, None))
    import pickle
    pickle.dump(rows, open('headrows.pkl', 'wb'))
    good = [r for r in rows if r[1] is not None]
    print(len(rows), len(good))
