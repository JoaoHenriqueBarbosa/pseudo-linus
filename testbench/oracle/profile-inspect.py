#!/usr/bin/env python3
"""Inspeciona o molde do perfil (template.pack): lista as entradas e procura bytes.

Uso: profile-inspect.py <template.pack> [padrão-regex-em-bytes]
Sem padrão, lista camada, raiz, tipo, modo, tamanho e caminho. Com padrão, lista só as entradas que o contêm e as
posições (a primeira dezena), com contexto.
"""
import re
import struct
import sys
import zlib


def entries(pack):
    data = zlib.decompress(open(pack, "rb").read())
    assert data[:6] == b"ULPP1\n"
    (count,) = struct.unpack_from("<I", data, 6)
    pos = 10
    for _ in range(count):
        layer, root, kind, mode, path_len = struct.unpack_from("<BBBHH", data, pos)
        pos += 7
        path = data[pos:pos + path_len].decode("utf-8")
        pos += path_len
        (size,) = struct.unpack_from("<I", data, pos)
        pos += 4
        content = data[pos:pos + size]
        pos += size
        (patches,) = struct.unpack_from("<B", data, pos)
        pos += 1 + patches * 9
        yield layer, root, kind, mode, path, content, patches


def main():
    pack = sys.argv[1]
    pattern = re.compile(sys.argv[2].encode()) if len(sys.argv) > 2 else None
    for layer, root, kind, mode, path, content, patches in entries(pack):
        if pattern is None:
            print(layer, root, kind, oct(mode), len(content), patches, path)
            continue
        found = list(pattern.finditer(content))
        if found:
            print(f"{path}: {len(found)}")
            for match in found[:10]:
                print("   ", match.start(), content[max(0, match.start() - 30):match.end() + 30])


if __name__ == "__main__":
    main()
