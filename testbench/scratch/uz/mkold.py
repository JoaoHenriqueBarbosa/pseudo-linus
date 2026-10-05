"""Gera zips com os métodos antigos do PKZIP 1.x (1 = shrink, 6 = implode) pra sondar o unzip.

Os codificadores seguem o que o unzip 6.0 decodifica (unshrink.c e explode.c); a prova de que a
fixture é válida é o unzip do oráculo extrair com o CRC certo.

Uso: python3 mkold.py <diretório de saída>
"""

import heapq
import os
import random
import struct
import sys
import zlib


class BitWriter:
    def __init__(self):
        self.out = bytearray()
        self.acc = 0
        self.n = 0

    def put(self, value, nbits):
        """Bits do valor, do menos significativo pro mais (como o NEEDBITS lê)."""
        for i in range(nbits):
            self.acc |= ((value >> i) & 1) << self.n
            self.n += 1
            if self.n == 8:
                self.out.append(self.acc)
                self.acc = 0
                self.n = 0

    def finish(self):
        if self.n:
            self.out.append(self.acc)
            self.acc = 0
            self.n = 0
        return bytes(self.out)


# ---- shrink -------------------------------------------------------------------------------------

HSIZE = 8192
BOGUS = 256
FREE = HSIZE


def shrink(data, clear_every=None, lagged=True):
    """LZW do shrink. `lagged`: o dicionário cresce junto com o do decodificador (dá pra fazer a
    limpeza parcial em sincronia); senão é o LZW clássico, que gera os códigos KwKwK."""
    bw = BitWriter()
    codesize = 9
    parent = [BOGUS] * 256 + [FREE] * (HSIZE - 256)
    value = list(range(256)) + [0] * (HSIZE - 256)
    table = {bytes([i]): i for i in range(256)}
    lastfree = BOGUS

    def emit(code):
        nonlocal codesize
        while code >= (1 << codesize):
            bw.put(BOGUS, codesize)
            bw.put(1, codesize)
            codesize += 1
        bw.put(code, codesize)

    def string_of(code):
        s = []
        while code != BOGUS:
            s.append(value[code])
            code = parent[code]
        return bytes(reversed(s))

    def add(prev_code, ch):
        nonlocal lastfree
        code = lastfree + 1
        while code < HSIZE and parent[code] != FREE:
            code += 1
        lastfree = code
        if code >= HSIZE:
            return False
        value[code] = ch
        parent[code] = prev_code
        table.setdefault(string_of(code), code)
        return True

    def partial_clear():
        nonlocal lastfree
        has_child = set()
        for c in range(BOGUS + 1, lastfree + 1):
            p = parent[c]
            if p != FREE and p > BOGUS:
                has_child.add(p)
        for c in range(BOGUS + 1, lastfree + 1):
            if c not in has_child and parent[c] != FREE:
                s = string_of(c)
                if table.get(s) == c:
                    del table[s]
                parent[c] = FREE
        lastfree = BOGUS
        bw.put(BOGUS, codesize)
        bw.put(2, codesize)

    i = 0
    prev = None
    full = False
    emitted = 0
    pending_clear = False
    while i < len(data):
        j = i + 1
        # Com limpeza pendente, um literal: o anterior vira um código que a limpeza não solta.
        while not pending_clear and j < len(data) and data[i:j + 1] in table:
            j += 1
        code = table[data[i:j]]
        if lagged:
            emit(code)
            if prev is not None and not full:
                full = not add(prev, data[i])
            prev = code
        else:
            emit(code)
            if j < len(data) and not full:
                full = not add(code, data[j])
        emitted += 1
        # A limpeza solta folhas; se o código anterior for uma delas, o decodificador penduraria a
        # próxima entrada num código livre. Espera um anterior literal (nunca é solto).
        if clear_every and lagged and emitted % clear_every == 0:
            pending_clear = True
        # O decodificador falha se não achar código livre: limpa antes de lotar.
        if lagged and sum(1 for c in range(lastfree + 1, HSIZE) if parent[c] == FREE) < 64:
            pending_clear = True
        if lagged and pending_clear and prev is not None and prev < 256:
            partial_clear()
            pending_clear = False
            full = False
        i = j
    return bw.finish()


# ---- implode ------------------------------------------------------------------------------------

def huffman_lengths(freqs, maxbits=16):
    heap = [(f, i, (i,)) for i, f in enumerate(freqs)]
    heapq.heapify(heap)
    depth = [0] * len(freqs)
    tie = len(freqs)
    while len(heap) > 1:
        f1, _, s1 = heapq.heappop(heap)
        f2, _, s2 = heapq.heappop(heap)
        for s in s1 + s2:
            depth[s] += 1
        heapq.heappush(heap, (f1 + f2, tie, s1 + s2))
        tie += 1
    assert max(depth) <= maxbits, max(depth)
    return depth


def canonical(lengths):
    """Códigos canônicos (por comprimento, depois por símbolo), como o huft_build monta."""
    maxlen = max(lengths)
    count = [0] * (maxlen + 1)
    for l in lengths:
        count[l] += 1
    code = 0
    nxt = [0] * (maxlen + 2)
    for b in range(1, maxlen + 1):
        code = (code + count[b - 1]) << 1 if b > 1 else 0
        nxt[b] = code
    codes = [0] * len(lengths)
    for s, l in enumerate(lengths):
        codes[s] = nxt[l]
        nxt[l] += 1
    return codes


def put_code(bw, code, length):
    """O explode lê com os bits invertidos: o código canônico, do bit alto pro baixo, complementado."""
    for i in range(length - 1, -1, -1):
        bw.put(1 - ((code >> i) & 1), 1)


def tree_bytes(lengths):
    pairs = []
    i = 0
    while i < len(lengths):
        j = i
        while j < len(lengths) and lengths[j] == lengths[i] and j - i < 16:
            j += 1
        pairs.append(((j - i - 1) << 4) | (lengths[i] - 1))
        i = j
    return bytes([len(pairs) - 1]) + bytes(pairs)


def lz77(data, window, minlen, maxlen):
    toks = []
    i = 0
    heads = {}
    while i < len(data):
        best = (0, 0)
        key = data[i:i + minlen]
        for p in reversed(heads.get(key, [])[-64:]):
            if i - p > window:
                break
            l = 0
            while l < maxlen and i + l < len(data) and data[p + l] == data[i + l]:
                l += 1
            if l > best[0]:
                best = (l, i - p)
        if best[0] >= minlen:
            toks.append(("m", best[0], best[1]))
            for k in range(i, i + best[0]):
                heads.setdefault(data[k:k + minlen], []).append(k)
            i += best[0]
        else:
            toks.append(("l", data[i]))
            heads.setdefault(key, []).append(i)
            i += 1
    return toks


def implode(data, lit_tree, big_window):
    window = 8192 if big_window else 4096
    bdl = 7 if big_window else 6
    minlen = 3 if lit_tree else 2
    toks = lz77(data, window, minlen, minlen + 63 + 255)
    lit_f = [1] * 256
    len_f = [1] * 64
    dist_f = [1] * 64
    for t in toks:
        if t[0] == "l":
            lit_f[t[1]] += 1
        else:
            len_f[min(t[1] - minlen, 63)] += 1
            dist_f[(t[2] - 1) >> bdl] += 1
    head = b""
    lit_len = huffman_lengths(lit_f) if lit_tree else None
    if lit_tree:
        head += tree_bytes(lit_len)
    len_len = huffman_lengths(len_f)
    dist_len = huffman_lengths(dist_f)
    head += tree_bytes(len_len) + tree_bytes(dist_len)
    lit_c = canonical(lit_len) if lit_tree else None
    len_c = canonical(len_len)
    dist_c = canonical(dist_len)
    bw = BitWriter()
    for t in toks:
        if t[0] == "l":
            bw.put(1, 1)
            if lit_tree:
                put_code(bw, lit_c[t[1]], lit_len[t[1]])
            else:
                bw.put(t[1], 8)
        else:
            _, length, dist = t
            bw.put(0, 1)
            bw.put((dist - 1) & ((1 << bdl) - 1), bdl)
            hi = (dist - 1) >> bdl
            put_code(bw, dist_c[hi], dist_len[hi])
            sym = min(length - minlen, 63)
            put_code(bw, len_c[sym], len_len[sym])
            if sym == 63:
                bw.put(length - minlen - 63, 8)
    return head + bw.finish()


# ---- contêiner ----------------------------------------------------------------------------------

def make_zip(path, members):
    """members: (nome, método, gpf, dados, comprimido)."""
    out = bytearray()
    central = bytearray()
    for name, method, gpf, data, comp in members:
        nb = name.encode()
        crc = zlib.crc32(data) & 0xFFFFFFFF
        off = len(out)
        dostime, dosdate = 0x6000, 0x5A21
        out += struct.pack("<IHHHHHIIIHH", 0x04034B50, 10, gpf, method, dostime, dosdate, crc, len(comp), len(data), len(nb), 0)
        out += nb + comp
        central += struct.pack("<IHHHHHHIIIHHHHHII", 0x02014B50, 0x031E, 10, gpf, method, dostime, dosdate, crc,
                               len(comp), len(data), len(nb), 0, 0, 0, 0, 0o100644 << 16, off)
        central += nb
    cd_off = len(out)
    out += central
    out += struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, len(members), len(members), len(central), cd_off, 0)
    with open(path, "wb") as f:
        f.write(out)


def sample_text(n, seed):
    rnd = random.Random(seed)
    words = ("o rato roeu a roupa do rei de roma enquanto a rainha ria e o reino inteiro "
             "dormia sob a chuva fina de outono lorem ipsum dolor sit amet").split()
    out = []
    size = 0
    while size < n:
        line = " ".join(rnd.choice(words) for _ in range(rnd.randint(3, 14))) + "\n"
        out.append(line)
        size += len(line)
    return "".join(out).encode()[:n]


def sample_binary(n, seed):
    rnd = random.Random(seed)
    out = bytearray()
    while len(out) < n:
        if rnd.random() < 0.3 and len(out) > 300:
            p = rnd.randrange(max(0, len(out) - 4000), len(out) - 1)
            out += out[p:p + rnd.randint(2, 300)]
        else:
            out += bytes(rnd.randrange(256) for _ in range(rnd.randint(1, 20)))
    return bytes(out[:n])


def raw_deflate(data):
    c = zlib.compressobj(9, zlib.DEFLATED, -15)
    return c.compress(data) + c.flush()


def main():
    dest = sys.argv[1]
    os.makedirs(dest, exist_ok=True)
    text = sample_text(150_000, 1)
    small = sample_text(3000, 2)
    binary = sample_binary(90_000, 3)
    runs = b"a" * 5000 + b"ab" * 3000 + b"abc" * 2000

    make_zip(f"{dest}/imp_lit8k.zip", [("texto.txt", 6, 6, text, implode(text, True, True))])
    make_zip(f"{dest}/imp_lit4k.zip", [("pequeno.txt", 6, 4, small, implode(small, True, False))])
    make_zip(f"{dest}/imp_nolit4k.zip", [("bin.dat", 6, 0, binary, implode(binary, False, False))])
    make_zip(f"{dest}/imp_nolit8k.zip", [("runs.dat", 6, 2, runs, implode(runs, False, True))])
    make_zip(f"{dest}/shr.zip", [("texto.txt", 1, 0, text, shrink(text))])
    make_zip(f"{dest}/shr_clear.zip", [("texto.txt", 1, 0, text, shrink(text, clear_every=700))])
    make_zip(f"{dest}/shr_kwk.zip", [("runs.dat", 1, 0, runs, shrink(runs, lagged=False))])
    make_zip(f"{dest}/old_mix.zip", [
        ("a_shr.txt", 1, 0, small, shrink(small)),
        ("b_imp.dat", 6, 2, binary, implode(binary, False, True)),
        ("c_def.txt", 8, 0, text, raw_deflate(text)),
        ("d_shr.dat", 1, 0, runs, shrink(runs, lagged=False)),
    ])

    # Variantes corrompidas.
    imp = implode(small, True, False)
    make_zip(f"{dest}/bad_imp_pad.zip", [("pad.txt", 6, 4, small, imp + b"\x55\xaa\x00")])
    make_zip(f"{dest}/bad_imp_trunc.zip", [("trunc.txt", 6, 4, small, imp[:-12])])
    make_zip(f"{dest}/bad_imp_tree.zip", [("tree.txt", 6, 4, small, b"\xf0" + imp[1:])])
    flip = bytearray(imp)
    flip[len(flip) // 2] ^= 0x5A
    make_zip(f"{dest}/bad_imp_flip.zip", [("flip.txt", 6, 4, small, bytes(flip))])
    shr = shrink(small)
    make_zip(f"{dest}/bad_shr_trunc.zip", [("trunc.txt", 1, 0, small, shr[: len(shr) // 2])])
    flip = bytearray(shr)
    flip[len(flip) // 3] ^= 0xA5
    make_zip(f"{dest}/bad_shr_flip.zip", [("flip.txt", 1, 0, small, bytes(flip))])
    bw = BitWriter()
    bw.put(ord("A"), 9)
    size = 9
    for _ in range(5):
        bw.put(256, size)
        bw.put(1, size)
        size += 1
    bw.put(ord("B"), 13)
    make_zip(f"{dest}/bad_shr_bits.zip", [("bits.txt", 1, 0, b"AB", bw.finish())])
    make_zip(f"{dest}/bad_mix.zip", [
        ("a_flip.txt", 1, 0, small, bytes(flip)),
        ("b_ok.txt", 6, 4, small, imp),
        ("c_pad.txt", 6, 4, small, imp + b"\x01\x02"),
    ])


if __name__ == "__main__":
    main()
