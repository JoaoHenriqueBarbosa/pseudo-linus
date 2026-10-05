#!/usr/bin/env python3
"""Gera as tabelas dos charsets de um byte do iconv da glibc do oráculo (codegen, roda no contêiner).

Saídas em /out:
  sbcs.bin        256 x u16 (little endian) por tabela única: ponto de código que o byte decodifica, 0xFFFF se inválido
  sbcs_enc.txt    exceções do sentido inverso: "tabela cp byte" (byte -1 = não codificável) em relação ao inverso da tabela
  sbcs_names.txt  "NOME tabela" para cada nome do `iconv -l` que é de um byte
"""
import ctypes, ctypes.util, errno, subprocess, struct, sys, collections

libc = ctypes.CDLL(ctypes.util.find_library("c"), use_errno=True)
libc.iconv_open.restype = ctypes.c_void_p
libc.iconv_open.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
libc.iconv.restype = ctypes.c_size_t
libc.iconv.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_char_p), ctypes.POINTER(ctypes.c_size_t),
                       ctypes.POINTER(ctypes.c_char_p), ctypes.POINTER(ctypes.c_size_t)]
libc.iconv_close.argtypes = [ctypes.c_void_p]
INVALID = ctypes.c_void_p(-1).value
SIZE_T_MAX = ctypes.c_size_t(-1).value


class Fast:
    """Uma conversão de entrada pequena reaproveitando os buffers."""

    def __init__(self, to, frm):
        cd = libc.iconv_open(to.encode(), frm.encode())
        if cd is None or cd == INVALID:
            raise OSError("iconv_open")
        self.cd = cd
        self.inbuf = ctypes.create_string_buffer(16)
        self.outbuf = ctypes.create_string_buffer(64)
        self.inp = ctypes.c_char_p()
        self.outp = ctypes.c_char_p()
        self.inleft = ctypes.c_size_t()
        self.outleft = ctypes.c_size_t()
        self.in_addr = ctypes.addressof(self.inbuf)
        self.out_addr = ctypes.addressof(self.outbuf)

    def run(self, data):
        libc.iconv(self.cd, None, None, None, None)
        n = len(data)
        ctypes.memmove(self.in_addr, data, n)
        self.inp.value = None
        self.inp = ctypes.c_char_p(self.in_addr)
        self.outp = ctypes.c_char_p(self.out_addr)
        self.inleft.value = n
        self.outleft.value = 64
        rc = libc.iconv(self.cd, ctypes.byref(self.inp), ctypes.byref(self.inleft), ctypes.byref(self.outp), ctypes.byref(self.outleft))
        err = 0
        if rc == SIZE_T_MAX:
            err = ctypes.get_errno()
        else:
            rc2 = libc.iconv(self.cd, None, None, ctypes.byref(self.outp), ctypes.byref(self.outleft))
            if rc2 == SIZE_T_MAX:
                err = ctypes.get_errno()
        produced = 64 - self.outleft.value
        return bytes(self.outbuf.raw[:produced]), err, n - self.inleft.value

    def close(self):
        libc.iconv_close(self.cd)


def names():
    out = subprocess.run(["iconv", "-l"], capture_output=True, text=True).stdout
    return [l for l in out.split("\n") if l]


def decode_table(name):
    try:
        dec = Fast("UTF-32LE", name)
    except OSError:
        return None
    table = []
    any_ok = False
    for b in range(256):
        out, err, cons = dec.run(bytes([b]))
        if err == errno.EINVAL:
            dec.close()
            return None
        if err == 0:
            if len(out) != 4:
                dec.close()
                return None
            cp = struct.unpack("<I", out)[0]
            if cp > 0xFFFE:
                dec.close()
                return None
            table.append(cp)
            any_ok = True
        else:
            table.append(0xFFFF)
    dec.close()
    return tuple(table) if any_ok else None


def encode_exceptions(name, table):
    inv = {}
    for b, cp in enumerate(table):
        if cp != 0xFFFF and cp not in inv:
            inv[cp] = b
    enc = Fast(name, "UTF-32LE")
    exc = []
    for cp in range(0x10000):
        if 0xD800 <= cp <= 0xDFFF:
            continue
        out, err, cons = enc.run(struct.pack("<I", cp))
        got = out.hex() if err == 0 else "-"
        want = ("%02x" % inv[cp]) if cp in inv else "-"
        if got != want:
            exc.append((cp, got))
    enc.close()
    return tuple(exc)


def main():
    uniq = {}
    tables = []
    exceptions = []
    name_table = []
    for n in names():
        t = decode_table(n)
        if t is None:
            continue
        # a exceção depende do nome só pela tabela; o primeiro nome de cada tabela é o representante
        if t not in uniq:
            uniq[t] = []
        uniq[t].append(n)
    for t, ns in uniq.items():
        rep = ns[0]
        exc = encode_exceptions(rep, t)
        key = (t, exc)
        idx = None
        for i, (tt, ee) in enumerate(tables):
            if tt == t and ee == exc:
                idx = i
                break
        if idx is None:
            tables.append((t, exc))
            idx = len(tables) - 1
        for n in ns:
            name_table.append((n, idx))
        # confere que todos os nomes da mesma tabela codificam igual (amostra)
        for n in ns[1:]:
            e2 = encode_exceptions(n, t) if len(ns) < 6 else exc
            if e2 != exc:
                sys.stderr.write("aviso: %s difere de %s no sentido inverso\n" % (n, rep))
                tables.append((t, e2))
                name_table[-1] = (n, len(tables) - 1)
    with open("/out/sbcs.bin", "wb") as f:
        for t, exc in tables:
            f.write(b"".join(struct.pack("<H", cp) for cp in t))
    with open("/out/sbcs_enc.txt", "w") as f:
        for i, (t, exc) in enumerate(tables):
            for cp, b in exc:
                f.write("%d %d %s\n" % (i, cp, b))
    with open("/out/sbcs_names.txt", "w") as f:
        for n, i in name_table:
            f.write("%s %d\n" % (n, i))
    sys.stderr.write("tabelas: %d, nomes: %d, exceções: %d\n" % (len(tables), len(name_table), sum(len(e) for _, e in tables)))


main()
