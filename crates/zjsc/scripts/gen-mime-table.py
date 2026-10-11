#!/usr/bin/env python3
"""Gera src/runtime/mime_table.rs a partir de src/http_types/MimeType.rs do bun.

Uso: scripts/gen-mime-table.py [FONTE_DO_BUN] (padrão: $BUN_SRC, ou `.bun-src` ao lado do repositório)

Lê a tabela EXTENSIONS (extensão => literal) e as sobrescritas de `Compact::to_mime_type`
(os literais que o bun troca por constantes canônicas, com `;charset=utf-8`).
"""
import os
import re
import sys
from pathlib import Path

# O fonte do bun fica fora do repositório (público): `$BUN_SRC`, ou `.bun-src` na pasta que contém o repositório.
DEFAULT_BUN_SRC = Path(os.environ.get("BUN_SRC", Path(__file__).resolve().parents[4] / ".bun-src"))
root = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_BUN_SRC
source = (root / "src/http_types/MimeType.rs").read_text()

start = source.index("pub(crate) static EXTENSIONS")
end = source.index("const IMAGES_HEADERS")
table = {}
for ext, mime in re.findall(r'b"([^"]+)" => t!\("([^"]+)"\)', source[start:end]):
    table.setdefault(ext, mime)  # duplicata: vale a primeira, como no bun

# `Compact::to_mime_type`: literal da tabela => constante canônica.
canonical = {
    "application/webassembly": "application/wasm",
    "application/javascript": "text/javascript;charset=utf-8",
    "application/json": "application/json;charset=utf-8",
    "application/x-www-form-urlencoded": "application/x-www-form-urlencoded;charset=UTF-8",
    "image/vnd.microsoft.icon": "image/vnd.microsoft.icon",
    "text/css": "text/css;charset=utf-8",
    "text/html": "text/html;charset=utf-8",
    "text/javascript": "text/javascript;charset=utf-8",
    "text/jsx": "text/javascript;charset=utf-8",
    "text/plain": "text/plain;charset=utf-8",
}

lines = [
    "//! Gerado por scripts/gen-mime-table.py a partir de src/http_types/MimeType.rs do bun. Não editar à mão.",
    "",
    "/// Extensão (minúscula, sem ponto), ordenada, para o tipo que o bun devolve (já com as constantes canônicas).",
    "pub(crate) static EXTENSIONS: &[(&[u8], &[u8])] = &[",
]
for ext in sorted(table):
    mime = canonical.get(table[ext], table[ext])
    lines.append(f'    (b"{ext}", b"{mime}"),')
lines += [
    "];",
    "",
    "/// O tipo da extensão (sem ponto, qualquer caixa), ou `None` se o bun não a conhece.",
    "pub(crate) fn by_extension(extension: &[u8]) -> Option<&'static [u8]> {",
    "    let lower = extension.to_ascii_lowercase();",
    "    EXTENSIONS.binary_search_by(|(key, _)| (*key).cmp(lower.as_slice())).ok().map(|index| EXTENSIONS[index].1)",
    "}",
    "",
]
out = Path(__file__).resolve().parent.parent / "src/runtime/mime_table.rs"
out.write_text("\n".join(lines))
print(f"{len(table)} extensões -> {out}")
