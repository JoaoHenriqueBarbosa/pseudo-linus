#!/usr/bin/env python3
"""Extrai dos arquivos de sessão do oráculo os moldes do estado de página (PageState em mojo) das duas formas de
entrada de navegação que o `--dump-dom` grava: a do `about:blank` com uma navegação depois e a da página (URL de 34
caracteres). A sequência e os dois GUIDs de cada estado variam por execução e saem zerados; o endereço da página tem de
ser o de 34 caracteres (`http://127.0.0.1:8000/ss-none.html`), que o `sessions.rs` troca pelo endereço de verdade.

Uso: session-state-template.py <Session_N do oráculo> <diretório de saída>
(o Session_N sai de `measure-navigation.sh` com `ss-none.html` em modo dump)."""
import struct
import sys

SEQ_AT = 0x6C
GUID_AT = (0x1D4, 0x234)


def commands(path):
    data = open(path, "rb").read()
    pos = 8
    while pos + 2 <= len(data):
        (size,) = struct.unpack_from("<H", data, pos)
        yield data[pos + 2], data[pos + 3 : pos + 2 + size]
        pos += 2 + size


def state_of(payload):
    pos = 12
    (n,) = struct.unpack_from("<i", payload, pos)
    pos += 4 + ((n + 3) & ~3)
    (t,) = struct.unpack_from("<i", payload, pos)
    pos += 4 + ((t * 2 + 3) & ~3)
    (s,) = struct.unpack_from("<i", payload, pos)
    return bytearray(payload[pos + 4 : pos + 4 + s])


def zeroed(state):
    state[SEQ_AT : SEQ_AT + 16] = bytes(16)
    for at in GUID_AT:
        state[at : at + 72] = bytes(72)
    return bytes(state)


navigations = [payload for command, payload in commands(sys.argv[1]) if command == 6]
assert len(navigations) == 2, "esperava as duas entradas (about:blank e a página)"
for name, payload in (("session-state-blank.bin", navigations[0]), ("session-state-page.bin", navigations[1])):
    blob = zeroed(state_of(payload))
    assert len(blob) == 636, len(blob)
    open(f"{sys.argv[2]}/{name}", "wb").write(blob)
