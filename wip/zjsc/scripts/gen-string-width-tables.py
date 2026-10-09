#!/usr/bin/env python3
"""Gera src/runtime/string_width_tables.rs a partir de stringWidthTables.h do bun.

Transformação mecânica dos três estágios da tabela de classificação (Unicode 17).
Uso: gen-string-width-tables.py [stringWidthTables.h] [saída.rs]
O fonte do bun fica fora do repositório: `$BUN_SRC`, ou `.bun-src` na pasta que contém o repositório.
"""
import os
import re
import sys
from pathlib import Path

DEFAULT_BUN_SRC = Path(os.environ.get("BUN_SRC", Path(__file__).resolve().parents[4] / ".bun-src"))
HEADER = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_BUN_SRC / "src/jsc/bindings/stringWidthTables.h"
OUT = Path(sys.argv[2] if len(sys.argv) > 2 else Path(__file__).resolve().parent.parent / "src/runtime/string_width_tables.rs")

text = HEADER.read_text()


def table(name: str, ctype: str, rust_type: str, size: int) -> str:
    match = re.search(rf"{name}\[(\d+)\]\s*=\s*\{{(.*?)\}};", text, re.S)
    assert match, name
    assert int(match.group(1)) == size, (name, match.group(1))
    values = [int(v) for v in re.findall(r"\d+", match.group(2))]
    assert len(values) == size, (name, len(values))
    lines = []
    for start in range(0, size, 24):
        lines.append("    " + ", ".join(str(v) for v in values[start:start + 24]) + ",")
    body = "\n".join(lines)
    ident = re.sub(r"(?<!^)(?=[A-Z0-9])", "_", name.removeprefix("k")).upper()
    return f"pub(crate) static {ident}: [{rust_type}; {size}] = [\n{body}\n];\n"


out = [
    "//! Tabelas de três estágios da largura visível (`stringWidthTables.h` do bun, Unicode 17.0.0).",
    "//!",
    "//! Gerado por `scripts/gen-string-width-tables.py`; não edite à mão.",
    "//!",
    "//! `GRAPHEME_BREAK_STAGE1[cp >> 8] + (cp & 0xFF)` indexa o estágio 2, que indexa o estágio 3.",
    "//! Cada byte do estágio 3 empacota: bits 0 a 4 a classe de quebra de grafema, bits 5 e 6 a classe de",
    "//! largura (0 zero, 1 estreito, 2 largo, 3 ambíguo do East Asian Width) e o bit 7 a propriedade Emoji.",
    "",
    table("kGraphemeBreakStage1", "uint16_t", "u16", 8192),
    table("kGraphemeBreakStage2", "uint8_t", "u8", 32768),
    table("kGraphemeBreakStage3", "uint8_t", "u8", 33),
]
OUT.write_text("\n".join(out))
print(f"escrito {OUT}")
