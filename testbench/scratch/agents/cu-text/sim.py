import re, sys, itertools

class L:
    def __init__(s, text): s.t = text
    def n(s): return len(s.t)
    def sp(s, i): return s.t[i].isspace()
    def trim(s, r):
        a, b = r
        while a < b and s.sp(a): a += 1
        while r[0] < b and s.sp(b - 1): b -= 1
        return (min(a, b), b)
    def align_start(s, r):
        a, b = r
        if a == b or a == 0 or s.sp(a) or s.sp(a - 1): return r
        while a < b and not s.sp(a): a += 1
        return (a, b)
    def align_end(s, r):
        a, b = r
        if a == b or b == s.n() or s.sp(b - 1) or s.sp(b): return r
        while a < b and not s.sp(b - 1): b -= 1
        return (a, b)
    def win_end(s, r, width):
        end = s.trim(r)[1]
        start = max(end - width, 0)
        return s.trim(s.align_start((start, end)))
    def win_start(s, r, width):
        start = r[0]
        end = min(r[1], start + width)
        end = s.align_end((start, end))[1]
        return (start, s.trim((start, end))[1])

def run(text, W, gap, flag, mode, P):
    S = text.replace('\n', ' ').strip()
    t = len(flag)
    R = 0
    refs = []
    # refs: file in.txt line 1
    ref = "in.txt:1"
    if mode == 'none': M = gap; Wp = W
    elif mode == 'R': M = 0; Wp = W
    else:
        R = len(ref); M = R + gap; Wp = W - (R + gap)
    h = Wp // 2
    Bm = max(0, h - gap - 2 * t + P['db'])
    Km = max(0, h - 2 * t + P['dk'])
    out = []
    for m in re.finditer(r'[A-Za-z0-9_]+', S):
        ks, ke = m.start(), m.end()
        kw = m.group()
        before_text = L(S[:ks]); after_text = L(S[ke:])
        Am = max(0, Km - len(kw) + P['da'])
        before = before_text.win_end((0, before_text.n()), Bm)
        after = after_text.win_start((0, after_text.n()), Am)
        max_tail = max(0, Bm - (before[1] - before[0]) - gap + P['dt'])
        ts = after_text.trim((after[1], after_text.n()))[0]
        tail = after_text.win_start((ts, after_text.n()), max_tail)
        if tail[1] - tail[0] > 2 and after_text.sp(tail[1] - 2) and not after_text.sp(tail[1] - 1):
            tail = after_text.trim((tail[0], tail[1] - 1))
        max_head = max(0, Km - (len(kw) + after[1] - after[0]) - gap + P['dh'])
        head = before_text.win_end((0, before[0]), max_head)
        f = dict(before=before_text.t[before[0]:before[1]], after=after_text.t[after[0]:after[1]],
                 tail=after_text.t[tail[0]:tail[1]], head=before_text.t[head[0]:head[1]])
        if after[1] != after_text.n():
            if tail[0] == tail[1]: f['after'] += flag
            elif tail[1] != after_text.n(): f['tail'] += flag
        if before[0] != 0:
            if head[0] == head[1]: f['before'] = flag + f['before']
            elif head[0] != 0: f['head'] = flag + f['head']
        # layout
        E = M + h - gap
        left = [' '] * 0
        line = ''
        tl = f['tail']; bf = f['before']
        K = M + h
        buf = [' '] * max(200, W + 100)
        def put(col, s):
            for i, c in enumerate(s): buf[col + i] = c
        put(M, tl)
        if P.get('hang') and bf == flag and flag != '' and before[0] != 0 and before[0] == before[1]:
            put(E + 1 - len(bf), bf)
            K += 1
        else:
            put(E - len(bf), bf)
        put(K, kw + f['after'])
        hd = f['head']
        if hd: put(M + Wp - len(hd), hd)
        s = ''.join(buf).rstrip()
        if mode == 'A' and False: pass
        if mode == 'A':
            s = ref + ':' + s[len(ref) + 1:]
        if mode in ('R',):
            s = s.ljust(W + gap) + ref
        out.append(s)
    return out

def parse(path):
    blocks = {}
    cur = None
    for l in open(path).read().split('\n'):
        if l.startswith('@@ '):
            cur = l; blocks[cur] = []
        elif cur is not None and l != '':
            blocks[cur].append(l)
    return blocks

if __name__ == '__main__':
    blocks = parse('/home/john/projects/pseudo-linus/testbench/scratch/agents/cu-text/ptxdata.txt')
    best = None
    P = dict(db=0, dk=0, da=0, dt=0, dh=0)
    ok = 0; bad = 0; badlist = []
    for hdr, lines in blocks.items():
        m = re.match(r'@@ w=(\d+) g=(\d+) f=\[(.*)\] t=\[(.*)\] opt=(\w+)', hdr)
        W, g, f, t, mode = int(m[1]), int(m[2]), m[3], m[4], m[5]
        sim = run(t, W, g, f, mode, P)
        if sorted(sim) == sorted(lines): ok += 1
        else:
            bad += 1; badlist.append((hdr, sim, lines))
    print('ok', ok, 'bad', bad)
    for hdr, sim, lines in badlist[:int(sys.argv[1]) if len(sys.argv) > 1 else 3]:
        print(hdr)
        for a in sorted(set(sim) - set(lines)): print('  SIM|' + a)
        for a in sorted(set(lines) - set(sim)): print('  ORA|' + a)
