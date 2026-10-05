#!/usr/bin/env python3
"""Gera `caps_table.rs` a partir do `include/Caps` do ncurses 6.5-20250216.

Uso: gen_caps.py <ncurses-6.5-20250216/include/Caps> > caps_table.rs

Reproduz o que o ncurses gera em tempo de build com MKnames.awk, MKcodes.awk, MKtermsort.sh e
MKparametrized.sh: as tabelas de nomes (terminfo, variável C e código termcap) das capacidades
predefinidas, as ordens de classificação do infocmp (ordem de `sort` no locale C, sobre as linhas
"nome<TAB>índice") e os vetores `parametrized` e `*_from_termcap`.
"""
import sys

KINDS = {"bool": "Kind::Bool", "num": "Kind::Num", "str": "Kind::Str"}


def read_rows(path):
    rows = []
    with open(path, encoding="latin-1") as fh:
        for raw in fh:
            if raw.startswith("#"):
                continue
            line = raw.rstrip("\n")
            # o MKtermsort.sh junta sequências de tabs; o awk separa por brancos
            fields = line.split()
            if len(fields) < 7:
                continue
            rows.append((fields, line))
    return rows


def main():
    rows = read_rows(sys.argv[1])
    tables = {"bool": [], "num": [], "str": []}
    for fields, line in rows:
        var, info, kind, tc = fields[0], fields[1], fields[2], fields[3]
        if kind not in tables:
            continue
        from_tc = fields[6][0] == "Y"
        param = 0
        if kind == "str":
            if var.startswith("acs_") or var.startswith("label_format"):
                param = -1
            elif any(c == "#" and i + 1 < len(line) and line[i + 1].isdigit() for i, c in enumerate(line)):
                param = 1
        tables[kind].append((var, info, tc, from_tc, param))

    out = []
    out.append("// GERADO por gen_caps.py a partir de include/Caps do ncurses 6.5-20250216. Não edite à mão.")
    out.append("")
    out.append("use super::{Cap, Kind};")
    out.append("")
    for kind, name in (("bool", "BOOLS"), ("num", "NUMS"), ("str", "STRS")):
        out.append(f"pub static {name}: [Cap; {len(tables[kind])}] = [")
        for var, info, tc, from_tc, param in tables[kind]:
            out.append(
                f'    Cap {{ var: "{var}", info: "{info}", tc: "{tc}", kind: {KINDS[kind]}, '
                f'from_tc: {"true" if from_tc else "false"}, param: {param} }},'
            )
        out.append("];")
        out.append("")
    for kind, name in (("bool", "BOOL"), ("num", "NUM"), ("str", "STR")):
        for label, pick in (("TERMINFO", 1), ("VARIABLE", 0), ("TERMCAP", 2)):
            lines = [f"{row[pick]}\t{i}" for i, row in enumerate(tables[kind])]
            lines.sort()
            idx = [int(l.split("\t")[1]) for l in lines]
            out.append(f"pub static {name}_{label}_SORT: [u16; {len(idx)}] = [")
            for k in range(0, len(idx), 16):
                out.append("    " + ", ".join(str(v) for v in idx[k : k + 16]) + ",")
            out.append("];")
            out.append("")
    sys.stdout.write("\n".join(out))


main()
