#!/usr/bin/env python3
"""Gera src/wtf/url_character_class_table.rs (a `characterClassTable` de URLParser.cpp 65 a 321 e o
enum `URLCharacterClass` de URLParser.cpp 53) lendo o próprio C++ em upstream/WTF/wtf/URLParser.cpp.
Uso: scripts/gen-url-character-class-table.py.
"""
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOURCE = os.path.join(ROOT, "upstream", "WTF", "wtf", "URLParser.cpp")
TARGET = os.path.join(ROOT, "src", "wtf", "url_character_class_table.rs")


def snake(name):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).upper()


def main():
    with open(SOURCE, encoding="utf-8") as f:
        text = f.read()

    enum_body = re.search(r"enum URLCharacterClass \{(.*?)\};", text, re.S).group(1)
    classes = {}
    for name, value in re.findall(r"(\w+)\s*=\s*(0x[0-9A-Fa-f]+)", enum_body):
        classes[name] = int(value, 16)

    table_body = re.search(r"characterClassTable \{(.*?)\n\};", text, re.S).group(1)
    entries = []
    for line in table_body.splitlines():
        line = line.strip()
        if not line:
            continue
        expression, _, comment = line.partition("// ")
        expression = expression.strip().rstrip(",").strip()
        value = 0
        if expression != "0":
            for part in expression.split("|"):
                value |= classes[part.strip()]
        entries.append((value, expression, comment.strip()))
    assert len(entries) == 256, len(entries)

    lines = ["//! Gerado por `scripts/gen-url-character-class-table.py` a partir de",
             "//! `upstream/WTF/wtf/URLParser.cpp` (`URLCharacterClass`, linha 53, e `characterClassTable`,",
             "//! linha 65). Não editar à mão.", "",
             "/// `enum URLCharacterClass` (URLParser.cpp 53)."]
    for name, value in classes.items():
        lines.append(f"pub const {snake(name)}: u8 = 0x{value:X};")
    lines += ["", "/// `characterClassTable` (URLParser.cpp 65), indexada pelo ponto de código (0 a 255).",
              "pub static CHARACTER_CLASS_TABLE: [u8; 256] = ["]
    for value, expression, comment in entries:
        lines.append(f"    0x{value:02X}, // {comment}: {expression}")
    lines += ["];", ""]
    with open(TARGET, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))


main()
