#!/usr/bin/env python3
"""Corredor do test262 sobre o zjsc.

Para cada teste monta o programa (harness `assert.js` e `sta.js`, `doneprintHandle.js` nos assíncronos, os `includes` do
metadado e o texto do teste), nos modos que as flags pedem (sloppy e strict por padrão, só strict com `onlyStrict`, só sloppy com
`noStrict` e `raw`, módulo com `module`), roda cada um em um processo do `test262_worker` com tempo limite e memória limitada, e
confere o veredicto: sem exceção nos positivos, a exceção e a fase certas nos `negative`, `Test262:AsyncTestComplete` nos
assíncronos. Um teste passa quando passa em todos os seus modos.

Uso (dentro do container de build, com o worker já compilado):
  test262-run.py --test262 DIR --worker BIN --out resultados.jsonl [--dirs built-ins,language,intl402,annexB]
                 [--jobs 16] [--timeout 20] [--limit N] [--filter SUBSTRING]
  test262-run.py --summary resultados.jsonl --report relatorio.md --commit HASH

Os resultados (`--out`) são gerados em massa e não se versionam; o relatório e este corredor sim.
"""
import argparse
import collections
import concurrent.futures
import json
import os
import re
import resource
import subprocess
import sys
import tempfile

MARKER = "__PARSED__"
ASYNC_DONE = "Test262:AsyncTestComplete"


def parse_meta(text):
    start = text.find("/*---")
    end = text.find("---*/")
    meta = {"includes": [], "flags": [], "features": [], "negative": None}
    if start < 0 or end < 0:
        return meta
    lines = text[start + 5:end].split("\n")
    key = None
    for raw in lines:
        if not raw.strip():
            continue
        indent = len(raw) - len(raw.lstrip())
        line = raw.strip()
        if indent == 0 and ":" in line:
            name, _, value = line.partition(":")
            key = name.strip()
            value = value.strip()
            if key in ("includes", "flags", "features"):
                if value.startswith("["):
                    meta[key] = [item.strip() for item in value.strip("[]").split(",") if item.strip()]
                elif value:
                    meta[key] = [value]
            elif key == "negative":
                meta["negative"] = {}
            continue
        if key in ("includes", "flags", "features") and line.startswith("- "):
            meta[key].append(line[2:].strip())
        elif key == "negative" and ":" in line:
            name, _, value = line.partition(":")
            meta["negative"][name.strip()] = value.strip()
    return meta


def read(path):
    with open(path, encoding="utf-8", errors="surrogatepass") as handle:
        return handle.read()


class Harness:
    def __init__(self, root):
        self.root = os.path.join(root, "harness")
        self.cache = {}

    def get(self, name):
        if name not in self.cache:
            self.cache[name] = read(os.path.join(self.root, name))
        return self.cache[name]


def build(harness, meta, test_text, mode):
    flags = meta["flags"]
    raw = "raw" in flags
    pieces = []
    if mode == "strict":
        pieces.append('"use strict";')
    if mode == "module":
        pieces.append("globalThis.print = L;")
    if not raw:
        pieces.append("print('%s');" % MARKER)
        names = ["assert.js", "sta.js"]
        if "async" in flags:
            names.append("doneprintHandle.js")
        for include in meta["includes"]:
            if include not in names:
                names.append(include)
        for name in names:
            pieces.append(harness.get(name))
    pieces.append(test_text)
    return "\n".join(pieces)


def modes_for(meta):
    flags = meta["flags"]
    if "module" in flags:
        return ["module"]
    if "onlyStrict" in flags:
        return ["strict"]
    if "noStrict" in flags or "raw" in flags:
        return ["sloppy"]
    return ["sloppy", "strict"]


def run_worker(worker, mode, source, fixtures, timeout, memory):
    with tempfile.NamedTemporaryFile("w", suffix=".js", delete=False, encoding="utf-8", errors="surrogatepass") as handle:
        handle.write(source)
        path = handle.name

    def limits():
        resource.setrlimit(resource.RLIMIT_AS, (memory, memory))

    try:
        kind = "module" if mode == "module" else "script"
        result = subprocess.run(
            [worker, kind, path] + (fixtures if mode == "module" else []),
            capture_output=True,
            timeout=timeout,
            preexec_fn=limits,
        )
    except subprocess.TimeoutExpired:
        return {"status": "timeout"}
    finally:
        os.unlink(path)
    out = result.stdout.decode("utf-8", errors="replace")
    if result.returncode != 0 or not out.startswith("THROWN "):
        tail = result.stderr.decode("utf-8", errors="replace").strip().splitlines()
        reason = tail[-1][:160] if tail else ""
        return {"status": "crash", "code": result.returncode, "detail": reason}
    head, _, output = out.partition("--OUTPUT--\n")
    thrown = head[len("THROWN "):].strip()
    return {"status": "ran", "thrown": None if thrown == "-" else thrown, "output": output}


def verdict(meta, ran):
    """(passou, causa) para um modo."""
    if ran["status"] == "timeout":
        return False, "timeout"
    if ran["status"] == "crash":
        return False, "crash (código %s) %s" % (ran.get("code"), ran.get("detail", ""))
    thrown, output = ran["thrown"], ran["output"]
    negative = meta["negative"]
    if negative:
        wanted = negative.get("type", "")
        phase = negative.get("phase", "runtime")
        if thrown is None:
            return False, "negativo: esperava %s (%s), não lançou" % (wanted, phase)
        got = thrown.split(":", 1)[0]
        if got != wanted:
            return False, "negativo: esperava %s, veio %s" % (wanted, thrown)
        parsed = MARKER in output.split("\n")
        if phase == "runtime" and "raw" not in meta["flags"] and not parsed:
            return False, "negativo: esperava erro em execução (%s) e veio na análise: %s" % (wanted, thrown)
        if phase != "runtime" and parsed:
            return False, "negativo: esperava erro de %s (%s) e o programa executou" % (phase, wanted)
        return True, ""
    if thrown is not None:
        return False, thrown
    if "async" in meta["flags"]:
        lines = output.split("\n")
        if any(line.startswith("Test262:AsyncTestFailure") for line in lines):
            return False, next(line for line in lines if line.startswith("Test262:AsyncTestFailure"))
        if ASYNC_DONE not in lines:
            return False, "assíncrono: não terminou (sem Test262:AsyncTestComplete)"
    return True, ""


def run_test(args, harness, path, relative):
    text = read(path)
    meta = parse_meta(text)
    fixtures = []
    directory = os.path.dirname(path)
    if "module" in meta["flags"]:
        fixtures = [os.path.join(directory, name) for name in os.listdir(directory) if name.endswith("_FIXTURE.js")]
    causes = []
    for mode in modes_for(meta):
        source = build(harness, meta, text, mode)
        try:
            ran = run_worker(args.worker, mode, source, fixtures, args.timeout, args.memory)
        except OSError as error:
            ran = {"status": "crash", "code": -1, "detail": str(error)}
        ok, cause = verdict(meta, ran)
        if not ok:
            causes.append("[%s] %s" % (mode, cause))
    return {"path": relative, "pass": not causes, "causes": causes, "features": meta["features"]}


def collect(root, dirs, needle):
    found = []
    for top in dirs:
        base = os.path.join(root, "test", top)
        for current, _, files in os.walk(base):
            for name in sorted(files):
                if not name.endswith(".js") or name.endswith("_FIXTURE.js"):
                    continue
                path = os.path.join(current, name)
                relative = os.path.relpath(path, os.path.join(root, "test"))
                if needle and needle not in relative:
                    continue
                found.append((path, relative))
    found.sort(key=lambda item: item[1])
    return found


def normalize(cause):
    cause = re.sub(r"^\[(sloppy|strict|module)\] ", "", cause)
    cause = re.sub(r"'[^']*'|\"[^\"]*\"|«[^»]*»", "'X'", cause)
    cause = re.sub(r"\b\d+(\.\d+)?\b", "N", cause)
    return cause[:110]


def summarize(results_path, report_path, commit):
    rows = [json.loads(line) for line in open(results_path, encoding="utf-8")]
    total = len(rows)
    passed = sum(1 for row in rows if row["pass"])
    by_top = collections.OrderedDict()
    by_second = collections.defaultdict(lambda: [0, 0])
    for row in rows:
        parts = row["path"].split("/")
        top = parts[0]
        entry = by_top.setdefault(top, [0, 0])
        entry[0] += 1
        entry[1] += row["pass"]
        second = "/".join(parts[:2])
        by_second[second][0] += 1
        by_second[second][1] += row["pass"]
    causes = collections.Counter()
    example = {}
    for row in rows:
        if row["pass"]:
            continue
        key = normalize(row["causes"][0])
        causes[key] += 1
        example.setdefault(key, row["path"])
    lines = ["# Linha de base do test262 sobre o zjsc", ""]
    lines.append("test262 no commit `%s`. Medido com `scripts/test262-run.py` e o `examples/test262_worker.rs`." % commit)
    lines.append("")
    lines.append("Total: %d testes, passaram %d (%.1f%%), falharam %d." % (total, passed, 100.0 * passed / max(total, 1), total - passed))
    lines.append("")
    lines.append("## Por diretório de topo")
    lines.append("")
    lines.append("| diretório | testes | passou | % |")
    lines.append("|---|---|---|---|")
    for top, (count, ok) in by_top.items():
        lines.append("| %s | %d | %d | %.1f |" % (top, count, ok, 100.0 * ok / max(count, 1)))
    lines.append("")
    lines.append("## Maiores subdiretórios com falha")
    lines.append("")
    lines.append("| diretório | testes | falharam | % que passa |")
    lines.append("|---|---|---|---|")
    worst = sorted(by_second.items(), key=lambda item: item[1][0] - item[1][1], reverse=True)[:25]
    for name, (count, ok) in worst:
        lines.append("| %s | %d | %d | %.1f |" % (name, count, count - ok, 100.0 * ok / max(count, 1)))
    lines.append("")
    lines.append("## As 10 maiores causas de falha")
    lines.append("")
    lines.append("Agrupadas pela primeira causa de cada teste, com números e textos entre aspas normalizados.")
    lines.append("")
    lines.append("| # | testes | causa | exemplo |")
    lines.append("|---|---|---|---|")
    for index, (cause, count) in enumerate(causes.most_common(10), 1):
        lines.append("| %d | %d | `%s` | %s |" % (index, count, cause.replace("|", "/"), example[cause]))
    with open(report_path, "w", encoding="utf-8") as handle:
        handle.write("\n".join(lines) + "\n")
    print("\n".join(lines))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--test262")
    parser.add_argument("--worker")
    parser.add_argument("--out")
    parser.add_argument("--dirs", default="built-ins,language,intl402,annexB")
    parser.add_argument("--jobs", type=int, default=16)
    parser.add_argument("--timeout", type=float, default=20)
    parser.add_argument("--memory", type=int, default=3 * 1024 ** 3)
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--filter", default="")
    parser.add_argument("--summary")
    parser.add_argument("--report")
    parser.add_argument("--commit", default="")
    args = parser.parse_args()
    if args.summary:
        summarize(args.summary, args.report, args.commit)
        return
    harness = Harness(args.test262)
    tests = collect(args.test262, args.dirs.split(","), args.filter)
    if args.limit:
        tests = tests[: args.limit]
    done = 0
    with open(args.out, "w", encoding="utf-8") as sink, concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        futures = [pool.submit(run_test, args, harness, path, relative) for path, relative in tests]
        for future in concurrent.futures.as_completed(futures):
            sink.write(json.dumps(future.result(), ensure_ascii=False) + "\n")
            done += 1
            if done % 500 == 0:
                sink.flush()
                print("%d/%d" % (done, len(tests)), flush=True)
    print("fim: %d testes" % len(tests))


if __name__ == "__main__":
    main()
