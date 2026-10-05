#!/usr/bin/env python3
"""Explora o iconv da glibc do oráculo via ctypes (só leitura; não grava dados do repositório).

Roda dentro do contêiner: python3 explore.py  (lê `iconv -l`, classifica cada nome).
"""
import ctypes, ctypes.util, errno, sys, subprocess, json, collections

libc = ctypes.CDLL(ctypes.util.find_library("c"), use_errno=True)
libc.iconv_open.restype = ctypes.c_void_p
libc.iconv_open.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
libc.iconv.restype = ctypes.c_size_t
libc.iconv.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_char_p), ctypes.POINTER(ctypes.c_size_t),
                       ctypes.POINTER(ctypes.c_char_p), ctypes.POINTER(ctypes.c_size_t)]
libc.iconv_close.argtypes = [ctypes.c_void_p]
INVALID = ctypes.c_void_p(-1).value


class Conv:
    def __init__(self, to, frm):
        self.cd = libc.iconv_open(to.encode(), frm.encode())
        if self.cd == INVALID or self.cd is None:
            raise OSError(ctypes.get_errno(), "iconv_open")

    def run(self, data, flush=True):
        """Devolve (saída, errno, consumido). errno 0 se tudo certo."""
        libc.iconv(self.cd, None, None, None, None)  # reset
        inbuf = ctypes.create_string_buffer(data, len(data))
        inp = ctypes.c_char_p(ctypes.addressof(inbuf))
        inleft = ctypes.c_size_t(len(data))
        outbuf = ctypes.create_string_buffer(max(64, len(data) * 8))
        outp = ctypes.c_char_p(ctypes.addressof(outbuf))
        outleft = ctypes.c_size_t(len(outbuf))
        rc = libc.iconv(self.cd, ctypes.byref(inp), ctypes.byref(inleft), ctypes.byref(outp), ctypes.byref(outleft))
        err = 0
        if rc == ctypes.c_size_t(-1).value:
            err = ctypes.get_errno()
        elif flush:
            rc2 = libc.iconv(self.cd, None, None, ctypes.byref(outp), ctypes.byref(outleft))
            if rc2 == ctypes.c_size_t(-1).value:
                err = ctypes.get_errno()
        produced = len(outbuf) - outleft.value
        consumed = len(data) - inleft.value
        return outbuf.raw[:produced], err, consumed

    def close(self):
        libc.iconv_close(self.cd)


def names():
    out = subprocess.run(["iconv", "-l"], capture_output=True, text=True).stdout
    return [l for l in out.split("\n") if l]


if __name__ == "__main__":
    groups = collections.defaultdict(list)
    probes = ["A", "é", "\U0001f600", "€", "\u0000", "﻿"]
    for n in names():
        try:
            dec = Conv("UTF-32LE", n)
        except OSError:
            groups["NOOPEN_FROM"].append(n)
            continue
        # SBCS?
        sbcs = True
        any_ok = False
        for b in range(256):
            out, err, cons = dec.run(bytes([b]))
            if err == errno.EINVAL:
                sbcs = False
                break
            if err == 0 and len(out) != 4:
                sbcs = False
                break
            if err == 0:
                any_ok = True
        dec.close()
        if sbcs and any_ok:
            groups["SBCS"].append(n)
            continue
        try:
            enc = Conv(n, "UTF-32LE")
        except OSError:
            groups["NOOPEN_TO"].append(n)
            continue
        sig = []
        for p in probes:
            out, err, cons = enc.run(p.encode("utf-32-le"))
            sig.append((out.hex(), err))
        enc.close()
        groups[json.dumps(sig)].append(n)
    for k, v in groups.items():
        print(k[:150], len(v), " ".join(v[:12]))
