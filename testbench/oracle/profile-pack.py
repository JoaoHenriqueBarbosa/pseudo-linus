#!/usr/bin/env python3
"""Gera o molde do perfil do Chromium (crates/ul-chromium/profile/template.pack) a partir de execuções do oráculo.

Uso: profile-pack.py <saída.pack> <cópiaA> <cópiaB> <cópiaC> [...]
     profile-pack.py --exit-against <árvore viva.jsonl> <saída.pack> <cópiaA> <cópiaB> <cópiaC> [...]
Cada <cópia> é uma saída do `measure-profile.sh copy` (live/, profile/, config/ e os *-tree.jsonl). Com três ou mais
execuções, o que é igual em todas vira byte fixo; o que varia vira marcador, que o hospedeiro preenche no primeiro
uso (relógio, aleatório). A primeira cópia dá os bytes fixos.
Com --exit-against, as cópias só têm a saída (`measure-profile.sh dump-copy`): o molde leva só a camada 1, com a árvore
inteira da saída e a remoção do que a árvore viva dada (a do molde principal) tem e a saída não.

Formato (depois de zlib): "ULPP1\\n", u32 quantidade, e por entrada:
  u8 camada (0 = ao iniciar, 1 = ao sair), u8 raiz (0 = --user-data-dir, 1 = ~/.config/chromium),
  u8 tipo (0 dir, 1 arquivo, 2 symlink, 3 remoção: o que existe vivo e a saída apaga, com a subárvore; o caminho que
  termina em "*" remove todo nome do diretório com aquele prefixo), u16 modo, u16 tamanho do caminho, caminho, u32 tamanho do conteúdo,
  conteúdo, u8 quantidade de retalhos binários, e por retalho: u32 deslocamento, u32 tamanho, u8 tipo (0 aleatório).
Caminho e conteúdo de texto podem ter marcadores: U+E000 + "tipo:argumento" + U+E000 (UTF-8: EE 80 80).
Tipos de marcador: wk:<delta µs> (tempo WebKit = início + delta), wkf:<delta> (o mesmo, como double),
unixs:<delta s>, unixf:<delta µs> (segundos Unix inteiro e fracionário), ts:<delta µs> (carimbo do LOG do leveldb, em
milissegundos), userdir (o --user-data-dir),
uuid, hex:<n> (hex maiúsculo), rb64:<n>:<pad> (n caracteres base64 aleatórios; o último é ajustado para o
preenchimento `pad`), rint:<lo>:<hi> (inteiro aleatório). O texto depois de "#" no marcador é só um índice e se ignora.
"""
import json
import os
import re
import struct
import sys
import zlib

MARK = ""
WEBKIT_EPOCH_US = 11644473600 * 1_000_000  # microssegundos de 1601 a 1970

SKIP = [
    re.compile(p)
    for p in (
        r"^Singleton(Cookie|Lock|Socket)$",
        r"(^|/)\.org\.chromium\.Chromium\.",
        r"^Default/Network Persistent State$",
        # As entradas do cache de disco são da página carregada, não do molde.
        r"^Default/Cache/Cache_Data/[0-9a-f]{16}_[0-9]$",
    )
]
UUID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
LOG_TS = re.compile(rb"^(\d{4})/(\d\d)/(\d\d)-(\d\d):(\d\d):(\d\d)\.(\d{3}) ", re.M)
B64_CHARS = re.compile(r"^[A-Za-z0-9+/]+={0,2}$")


def load_tree(path):
    out = {}
    for line in open(path):
        entry = json.loads(line)
        out[entry["path"]] = entry
    return out


class Copy:
    def __init__(self, root):
        self.root = root
        live = os.path.join(root, "live-tree.jsonl")
        self.live = load_tree(live) if os.path.exists(live) else {}
        self.exit = load_tree(os.path.join(root, "profile-tree.jsonl"))
        self.config = {k: v for k, v in load_tree(os.path.join(root, "config-tree.jsonl")).items()}
        self.anchor = None

    def read(self, layer_dir, rel):
        with open(os.path.join(self.root, layer_dir, rel), "rb") as handle:
            return handle.read()


def epoch_us_from_webkit(value):
    return value - WEBKIT_EPOCH_US


def parse_mode(text):
    return int(text, 8)


def skipped(rel):
    return any(p.search(rel) for p in SKIP)


# --- JSON ---------------------------------------------------------------------------------------------------------

def flatten(value, path=()):
    if isinstance(value, dict):
        for key, item in value.items():
            yield from flatten(item, path + (key,))
    elif isinstance(value, list) and value and not all(isinstance(i, (int, float, str)) for i in value):
        for index, item in enumerate(value):
            yield from flatten(item, path + (index,))
    elif isinstance(value, list):
        for index, item in enumerate(value):
            yield path + (index,), item
    else:
        yield path, value


def set_path(root, path, value):
    for key in path[:-1]:
        root = root[key]
    root[path[-1]] = value


def get_path(root, path):
    for key in path:
        root = root[key]
    return root


def is_wk(value):
    return isinstance(value, str) and re.fullmatch(r"13\d{15}", value) is not None


def tokenize_json(blobs, anchors, kind_name):
    """Devolve (texto com marcadores, lista de (marcador, valor concreto da cópia A))."""
    docs = [json.loads(b.decode("utf-8")) for b in blobs]
    flats = [dict(flatten(d)) for d in docs]
    base = docs[0]
    replaced = []
    for path in sorted(flats[0], key=lambda p: tuple(str(x) for x in p)):
        values = [f.get(path) for f in flats]
        if len({json.dumps(v) for v in values}) == 1:
            continue
        a = values[0]
        deltas = [None] * len(values)
        token, bare = None, False
        if all(is_wk(v) for v in values):
            ds = [int(v) - anchors[i] for i, v in enumerate(values)]
            token = f"wk:{ds[0]}"
        elif all(isinstance(v, float) and v > 1.3e16 for v in values):
            ds = [int(v) - anchors[i] for i, v in enumerate(values)]
            token, bare = f"wkf:{ds[0]}", True
        elif all(isinstance(v, str) and re.fullmatch(r"\d{10}", v) for v in values) and abs(int(values[0]) - epoch_us_from_webkit(anchors[0]) // 1_000_000) < 600:
            ds = [int(v) - epoch_us_from_webkit(anchors[i]) // 1_000_000 for i, v in enumerate(values)]
            token = f"unixs:{ds[0]}"
        elif all(isinstance(v, float) and 1.7e9 < v < 1.9e9 for v in values):
            ds = [round(v * 1e6) - epoch_us_from_webkit(anchors[i]) for i, v in enumerate(values)]
            token, bare = f"unixf:{ds[0]}", True
        elif all(isinstance(v, str) and UUID.match(v) for v in values):
            token = "uuid"
        elif all(isinstance(v, str) and re.fullmatch(r"[0-9A-F]{64}", v) for v in values):
            token = "hex:64"
        elif all(isinstance(v, str) and re.fullmatch(r"\d{10,}", v) for v in values) and "next_check_time" in str(path):
            ds = [int(v) - anchors[i] for i, v in enumerate(values)]
            token = f"wk:{ds[0]}"
        elif all(isinstance(v, str) and B64_CHARS.match(v) for v in values) and len({len(v) for v in values}) == 1:
            common = os.path.commonprefix(values)
            pad = min(len(v) - len(v.rstrip("=")) for v in values)
            token = f"rb64:{len(a) - len(common) - pad}:{pad}"
            a_token_value = a[len(common):len(a) - pad]
            set_path(base, path, common + MARK + token + f"#{len(replaced)}" + MARK + "=" * pad)
            replaced.append((MARK + token + f"#{len(replaced)}" + MARK, a_token_value, False))
            continue
        elif all(isinstance(v, int) and not isinstance(v, bool) for v in values):
            token, bare = f"rint:0:{max(10 ** len(str(max(values))), 8000)}", True
        elif all(isinstance(v, str) and re.fullmatch(r"\d+", v) for v in values):
            token = f"rint:0:{10 ** max(len(v) for v in values)}"
        else:
            raise SystemExit(f"{kind_name}: campo variável sem regra: {path} {values!r}")
        marker = MARK + token + f"#{len(replaced)}" + MARK
        set_path(base, path, marker)
        replaced.append((marker, a, bare))
    text = json.dumps(base, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return text, replaced, docs[0]


def finish_json(text, replaced, original_bytes):
    """Tira as aspas dos marcadores numéricos e confere que, com os valores de A, o texto volta aos bytes de A."""
    check = text
    for marker, concrete, bare in replaced:
        if bare:
            text = text.replace('"' + marker + '"', marker)
            check = check.replace('"' + marker + '"', json.dumps(concrete))
        else:
            check = check.replace(marker, concrete if isinstance(concrete, str) else json.dumps(concrete))
    if check.encode("utf-8") != original_bytes:
        for index, (x, y) in enumerate(zip(check.encode("utf-8"), original_bytes)):
            if x != y:
                raise SystemExit(f"a serialização JSON não reproduz o original no byte {index}: {check[max(0, index-40):index+40]!r}")
        raise SystemExit("a serialização JSON difere em tamanho do original")
    return text.encode("utf-8")


# --- LOG do leveldb -------------------------------------------------------------------------------------------------

def tokenize_log(data, anchor_epoch_us):
    import calendar

    def repl(match):
        year, month, day, hour, minute, second, milli = (int(x) for x in match.groups())
        stamp = calendar.timegm((year, month, day, hour, minute, second)) * 1_000_000 + milli * 1000
        return (MARK + f"ts:{stamp - anchor_epoch_us}" + MARK).encode() + b" "

    return LOG_TS.sub(repl, data).replace(b"/tmp/profile", (MARK + "userdir" + MARK).encode())


# --- montagem -------------------------------------------------------------------------------------------------------

def main():
    exit_against = None
    args = sys.argv[1:]
    if args[0] == "--exit-against":
        exit_against, args = load_tree(args[1]), args[2:]
    out_path, roots = args[0], args[1:]
    copies = [Copy(r) for r in roots]
    if len(copies) < 3:
        raise SystemExit("precisa de pelo menos três cópias para separar o fixo do variável")
    entries = []
    report = []

    def anchor_of(copy, layer_dir):
        prefs = json.loads(copy.read("profile", "Default/Preferences"))
        return int(prefs["profile"]["creation_time"])

    for copy in copies:
        copy.anchor = anchor_of(copy, "profile")
    anchors = [c.anchor for c in copies]
    local_anchor_key = ("breadcrumbs", "enabled_time")

    def add(layer, root, kind, mode, path, data=b"", patches=()):
        entries.append((layer, root, kind, mode, path, data, list(patches)))

    a = copies[0]
    # camada 0: a árvore viva; camada 1: o que sai diferente ou novo ao fim
    layers = ((0, "live", "live"), (1, "exit", "profile")) if exit_against is None else ((1, "exit", "profile"),)
    for layer, tree_name, layer_dir in layers:
        tree = getattr(a, tree_name)
        for rel in sorted(tree):
            entry = tree[rel]
            if skipped(rel):
                continue
            live_entry = a.live.get(rel)
            if layer == 1:
                same = live_entry is not None and entry["type"] == live_entry["type"] and (entry["type"] != "f" or a.read("live", rel) == a.read("profile", rel))
                if same:
                    continue
            mode = parse_mode(entry["mode"])
            if entry["type"] == "d":
                if layer == 0 or (exit_against is not None and rel not in exit_against):
                    add(layer, 0, 0, mode, rel)
                continue
            if entry["type"] == "l":
                continue
            if entry["type"] != "f":
                continue
            blobs = [c.read(layer_dir, rel) for c in copies if rel in getattr(c, tree_name)]
            data = blobs[0]
            patches = []
            final_path = rel
            for prefix in ("Default/Sessions/Session_", "Default/Sessions/Tabs_"):
                if rel.startswith(prefix):
                    delta = int(rel.rsplit("_", 1)[1]) - a.anchor
                    final_path = prefix + MARK + f"wk:{delta}" + MARK
            if len(blobs) == len(copies) and any(b != blobs[0] for b in blobs[1:]):
                if rel in ("Local State", "Default/Preferences"):
                    these = anchors
                    if rel == "Local State":
                        these = []
                        for c in copies:
                            doc = json.loads(c.read(layer_dir, rel))
                            these.append(min(int(v) for _, v in flatten(doc) if is_wk(v)))
                    text, replaced, _ = tokenize_json(blobs, these, rel)
                    data = finish_json(text, replaced, blobs[0])
                    report.append(f"json  {rel}: {len(replaced)} campos")
                elif rel.endswith("/LOG") or rel == "LOG":
                    data = tokenize_log(blobs[0], epoch_us_from_webkit(a.anchor))
                    for other in blobs[1:]:
                        pass
                    report.append(f"log   {rel}")
                elif os.path.basename(rel) == "Variations":
                    report.append(f"fixo  {rel}: varia só no tamanho, fica o de A")
                else:
                    sizes = {len(b) for b in blobs}
                    if len(sizes) != 1:
                        report.append(f"AVISO {rel}: tamanhos {sorted(sizes)}, fica o de A")
                    else:
                        diff = sorted({i for b in blobs[1:] for i, (x, y) in enumerate(zip(blobs[0], b)) if x != y})
                        runs = []
                        for i in diff:
                            if runs and i == runs[-1][1]:
                                runs[-1][1] += 1
                            else:
                                runs.append([i, i + 1])
                        patches = [(s, e - s, 0) for s, e in runs]
                        report.append(f"bin   {rel}: {len(runs)} retalhos aleatórios ({sum(e - s for s, e in runs)} bytes; checksum não refeito)")
            add(layer, 0, 1, mode, final_path, data, patches)
    # O que existe com o navegador vivo e a saída apaga (o -wal dos SQLite no --dump-dom).
    removed = set()
    for rel in sorted(set(exit_against if exit_against is not None else a.live) - set(a.exit)):
        if skipped(rel) or any(rel.startswith(parent + "/") for parent in removed):
            continue
        # O nome do Session_ do começo vem do relógio: a saída apaga todos os do começo.
        name = "Default/Sessions/Session_*" if re.fullmatch(r"Default/Sessions/Session_\d+", rel) else rel
        removed.add(rel)
        add(1, 0, 3, 0, name)
    # blob_storage/<uuid>: o nome é aleatório por execução
    for index, (layer, root, kind, mode, path, data, patches) in enumerate(entries):
        match = re.fullmatch(r"Default/blob_storage/([0-9a-f-]{36})", path)
        if match:
            entries[index] = (layer, root, kind, mode, "Default/blob_storage/" + MARK + "uuid" + MARK, data, patches)
    # ~/.config/chromium: a base do crashpad (no molde principal)
    for rel in sorted(a.config if exit_against is None else ()):
        entry = a.config[rel]
        mode = parse_mode(entry["mode"])
        if entry["type"] == "d":
            add(0, 1, 0, mode, rel)
        elif entry["type"] == "f":
            blobs = [c.read("config", rel) for c in copies]
            diff = sorted({i for b in blobs[1:] for i, (x, y) in enumerate(zip(blobs[0], b)) if x != y})
            runs = []
            for i in diff:
                if runs and i == runs[-1][1]:
                    runs[-1][1] += 1
                else:
                    runs.append([i, i + 1])
            add(0, 1, 1, mode, rel, blobs[0], [(s, e - s, 0) for s, e in runs])
            report.append(f"cfg   {rel}: {len(runs)} retalhos aleatórios")
    body = bytearray(b"ULPP1\n" + struct.pack("<I", len(entries)))
    for layer, root, kind, mode, path, data, patches in entries:
        pb = path.encode("utf-8")
        body += struct.pack("<BBBHH", layer, root, kind, mode, len(pb)) + pb + struct.pack("<I", len(data)) + data
        body += struct.pack("<B", len(patches))
        for offset, length, ptype in patches:
            body += struct.pack("<IIB", offset, length, ptype)
    with open(out_path, "wb") as handle:
        handle.write(zlib.compress(bytes(body), 9))
    print("\n".join(report))
    print(f"# {len(entries)} entradas, {len(body)} bytes brutos, {os.path.getsize(out_path)} comprimidos")


if __name__ == "__main__":
    main()
