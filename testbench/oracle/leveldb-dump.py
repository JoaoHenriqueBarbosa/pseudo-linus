#!/usr/bin/env python3
"""Decodifica os `.log` de um diretório do leveldb (os lotes de escrita) para texto legível.

Ferramenta do oráculo (Python só aqui, por regra do projeto): lê o formato de log do leveldb (blocos de 32 KiB,
cabeçalho de 7 bytes com CRC, tipo 1 cheio, 2 primeiro, 3 meio, 4 último) e o `WriteBatch` (sequência, contagem, e
cada operação `put`/`delete` com chave e valor). As chaves do Local Storage do Chromium aparecem com o prefixo
`_<origem>\\0` e o byte de formato (0 = UTF-16, 1 = Latin1) separados.

Uso: leveldb-dump.py <arquivo.log> [...]
"""
import struct
import sys


def records(data):
    out, pending, pos = [], b"", 0
    while pos < len(data):
        left = 32768 - (pos % 32768)
        if left < 7:
            pos += left
            continue
        if pos + 7 > len(data):
            break
        _crc, length, kind = struct.unpack_from("<IHB", data, pos)
        payload = data[pos + 7 : pos + 7 + length]
        pos += 7 + length
        if kind == 1:
            out.append(payload)
        elif kind == 2:
            pending = payload
        elif kind == 3:
            pending += payload
        elif kind == 4:
            out.append(pending + payload)
            pending = b""
    return out


def varint(buf, pos):
    result, shift = 0, 0
    while True:
        byte = buf[pos]
        pos += 1
        result |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return result, pos
        shift += 7


def show(raw):
    text = bytes(raw)
    return repr(text if len(text) <= 60 else text[:60] + b"...(%d bytes)" % len(text))


def batch(record):
    sequence, count = struct.unpack_from("<QI", record, 0)
    pos, ops = 12, []
    for _ in range(count):
        kind = record[pos]
        pos += 1
        length, pos = varint(record, pos)
        key = record[pos : pos + length]
        pos += length
        if kind == 1:
            length, pos = varint(record, pos)
            value = record[pos : pos + length]
            pos += length
            ops.append(("put", key, value))
        else:
            ops.append(("delete", key, None))
    return sequence, ops


for name in sys.argv[1:]:
    print("==", name)
    for record in records(open(name, "rb").read()):
        sequence, ops = batch(record)
        print("batch seq=%d ops=%d bytes=%d" % (sequence, len(ops), len(record)))
        for kind, key, value in ops:
            line = "  %-6s %s" % (kind, show(key))
            if value is not None:
                line += " => " + show(value)
            print(line)
