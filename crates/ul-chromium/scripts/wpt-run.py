#!/usr/bin/env python3
"""Corredor do web-platform-tests sobre o `ul-chromium`.

Descobre os testes do recorte (`.html` com testharness.js, `.any.js` na variante window e `.window.js`), exclui o que o
navegador headless não serve (listando a causa), roda cada arquivo em um processo do `wpt_worker` com tempo limite e
memória limitada e guarda o resultado POR SUBTESTE (PASS, FAIL, TIMEOUT, NOTRUN, PRECONDITION_FAILED) em JSONL.

Uso (dentro do container de build, com o worker já compilado):
  wpt-run.py --wpt DIR --worker BIN --out resultados.jsonl [--dirs dom/nodes,url,...] [--jobs 12] [--timeout 120]
             [--limit N] [--filter SUBSTRING]
  wpt-run.py --summary resultados.jsonl

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

DEFAULT_DIRS = [
    "dom/nodes", "dom/events", "dom/collections", "dom/traversal", "dom/ranges", "html/dom", "html/semantics/forms",
    "html/syntax", "css/cssom", "css/selectors", "fetch/api", "url", "encoding", "console", "XMLHttpRequest",
    "html/webappapis/timers", "webstorage",
]
SUBTEST_STATUS = {0: "PASS", 1: "FAIL", 2: "TIMEOUT", 3: "NOTRUN", 4: "PRECONDITION_FAILED"}
HARNESS_STATUS = {0: "OK", 1: "ERROR", 2: "TIMEOUT", 3: "PRECONDITION_FAILED"}
SKIP_DIRS = {"resources", "support", "tools", "common", "assets", "crashtests", "reference", "references", "helpers"}

# O que o navegador headless de um agente não serve, olhado no texto do teste: (expressão, causa).
SOURCE_EXCLUSIONS = [
    (re.compile(r"\.sub\.|[?&]pipe=|get-host-info|\.py[\"'?]|/xhr/resources/|/fetch/api/resources/"),
     "exige o wptserve (substituição .sub, pipes, handlers python)"),
    (re.compile(r"testdriver(-vendor|-actions)?\.js"), "exige o WebDriver (testdriver)"),
    (re.compile(r"\bnew\s+(WebSocket|SharedWorker|Worker|EventSource)\b|navigator\.serviceWorker|\bws://|\bwss://"),
     "exige Worker, WebSocket ou Service Worker"),
    (re.compile(r"<link[^>]+rel=[\"']?(match|mismatch)\b"), "reftest (compara renderização)"),
]


def meta_lines(text):
    return [line[len("// META:"):].strip() for line in text.splitlines() if line.startswith("// META:")]


def window_globals(metas):
    selected = {"window", "dedicatedworker"}
    for meta in metas:
        if not meta.startswith("global="):
            continue
        for token in meta[len("global="):].split(","):
            token = token.strip()
            if token == "!default":
                selected = set()
            elif token.startswith("!"):
                selected.discard(token[1:])
            elif token:
                selected.add(token)
    return selected


def wrapper_html(url_path, metas):
    """A página que o wptserve gera para um `.any.js` ou `.window.js` na variante window."""
    scripts = []
    title = os.path.basename(url_path)
    base = os.path.dirname(url_path)
    for meta in metas:
        if meta.startswith("script="):
            src = meta[len("script="):].strip()
            scripts.append(src if src.startswith("/") else base + "/" + src)
        elif meta.startswith("title="):
            title = meta[len("title="):].strip()
    source = url_path.rsplit(".html", 1)[0] + ".js"
    tags = "\n".join('<script src="%s"></script>' % src for src in scripts)
    return (
        "<!doctype html>\n<meta charset=utf-8>\n<title>%s</title>\n"
        "<script>self.GLOBAL = { isWindow: function() { return true; }, isWorker: function() { return false; }, "
        "isShadowRealm: function() { return false; } };</script>\n"
        '<script src="/resources/testharness.js"></script>\n<script src="/resources/testharnessreport.js"></script>\n'
        '<div id="log"></div>\n%s\n<script src="%s"></script>\n' % (title, tags, source)
    )


def discover(wpt, directories):
    """[(url, arquivo, html gerado ou None)] e [(url, causa)] dos excluídos."""
    tests, excluded = [], []
    for directory in directories:
        top = os.path.join(wpt, directory)
        for root, dirs, files in os.walk(top):
            dirs[:] = sorted(name for name in dirs if name not in SKIP_DIRS)
            for name in sorted(files):
                path = os.path.join(root, name)
                url = "/" + os.path.relpath(path, wpt).replace(os.sep, "/")
                kind = None
                if name.endswith((".html", ".htm", ".xhtml")) and not re.search(r"-ref\.|-notref\.|-manual\.|\.worker\.", name):
                    kind = "html"
                elif name.endswith(".any.js"):
                    kind = "any"
                elif name.endswith(".window.js"):
                    kind = "window"
                elif name.endswith(".worker.js"):
                    excluded.append((url, "exige Worker, WebSocket ou Service Worker"))
                    continue
                else:
                    continue
                text = open(path, encoding="utf-8", errors="replace").read()
                if kind == "html":
                    if "testharness.js" not in text:
                        if re.search(r"rel=[\"']?(match|mismatch)\b", text):
                            excluded.append((url, "reftest (compara renderização)"))
                        continue
                    cause = next((why for pattern, why in SOURCE_EXCLUSIONS if pattern.search(text)), None)
                    if cause:
                        excluded.append((url, cause))
                        continue
                    tests.append((url, path, None))
                    continue
                metas = meta_lines(text)
                if kind == "any" and "window" not in window_globals(metas):
                    excluded.append((url, "exige Worker, WebSocket ou Service Worker"))
                    continue
                cause = next((why for pattern, why in SOURCE_EXCLUSIONS if pattern.search(text + "\n".join(metas))), None)
                if cause:
                    excluded.append((url, cause))
                    continue
                page_url = url[: -len(".js")] + ".html"
                tests.append((page_url, path, wrapper_html(page_url, metas)))
    return tests, excluded


def run_one(worker, wpt, url, html, timeout, memory):
    handle = None
    args = [worker, wpt, url]
    if html is not None:
        handle = tempfile.NamedTemporaryFile("w", suffix=".html", delete=False, encoding="utf-8")
        handle.write(html)
        handle.close()
        args.append(handle.name)

    def limits():
        resource.setrlimit(resource.RLIMIT_AS, (memory, memory))

    try:
        result = subprocess.run(args, capture_output=True, timeout=timeout, preexec_fn=limits)
        lines = result.stdout.decode("utf-8", "replace").strip().splitlines()
        try:
            data = json.loads(lines[-1])
        except (IndexError, ValueError):
            tail = result.stderr.decode("utf-8", "replace").strip()[-300:]
            code = result.returncode
            data = {"harness": {"status": "CRASH", "message": "código %s: %s" % (code, tail)}, "tests": []}
    except subprocess.TimeoutExpired:
        data = {"harness": {"status": "WALLTIMEOUT", "message": "estourou %ds de relógio" % timeout}, "tests": []}
    finally:
        if handle:
            os.unlink(handle.name)
    harness = data["harness"]
    if isinstance(harness["status"], int):
        harness["status"] = HARNESS_STATUS.get(harness["status"], str(harness["status"]))
    for test in data["tests"]:
        test["status"] = SUBTEST_STATUS.get(test["status"], str(test["status"]))
    return {"path": url, **data}


def collect(args):
    directories = args.dirs.split(",") if args.dirs else DEFAULT_DIRS
    tests, excluded = discover(args.wpt, directories)
    if args.filter:
        tests = [t for t in tests if args.filter in t[0]]
    if args.limit:
        tests = tests[: args.limit]
    memory = 6 << 30
    with open(args.out, "w", encoding="utf-8") as out:
        for url, cause in excluded:
            out.write(json.dumps({"path": url, "excluded": cause}, ensure_ascii=False) + "\n")
        done = 0
        with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
            futures = [pool.submit(run_one, args.worker, args.wpt, url, html, args.timeout, memory) for url, _, html in tests]
            for future in concurrent.futures.as_completed(futures):
                out.write(json.dumps(future.result(), ensure_ascii=False) + "\n")
                out.flush()
                done += 1
                if done % 200 == 0:
                    print("%d/%d" % (done, len(tests)), flush=True)
    print("fim: %d arquivos rodados, %d excluídos" % (len(tests), len(excluded)))


def normalize(message):
    message = (message or "").split("\n")[0]
    message = re.sub(r"\"[^\"]*\"|'[^']*'", "'X'", message)
    message = re.sub(r"\d+(\.\d+)?", "N", message)
    return message[:110]


def summarize(path):
    rows = [json.loads(line) for line in open(path, encoding="utf-8")]
    excluded = [r for r in rows if "excluded" in r]
    ran = [r for r in rows if "excluded" not in r]
    by_dir = collections.defaultdict(lambda: collections.Counter())
    causes = collections.Counter()
    examples = {}
    for row in ran:
        parts = row["path"].strip("/").split("/")
        top = "/".join(parts[:2]) if parts[0] in ("dom", "html", "css") else parts[0]
        if row["path"].startswith("/html/semantics/forms"):
            top = "html/semantics/forms"
        counter = by_dir[top]
        counter["files"] += 1
        tests = row["tests"]
        if not tests:
            # Arquivo sem subteste reportado conta como uma unidade reprovada (travou, caiu ou não carregou).
            counter["total"] += 1
            counter["unreported"] += 1
            key = "arquivo sem resultado: %s %s" % (row["harness"]["status"], normalize(row["harness"].get("message")))
            causes[key] += 1
            examples.setdefault(key, row["path"])
            continue
        for test in tests:
            counter["total"] += 1
            counter[test["status"]] += 1
            if test["status"] != "PASS":
                key = "%s: %s" % (test["status"], normalize(test.get("message")))
                causes[key] += 1
                examples.setdefault(key, "%s :: %s" % (row["path"], test["name"][:60]))
    lines = ["| Diretório | Arquivos | Subtestes | Passou | % | Sem resultado |", "|---|---|---|---|---|---|"]
    total = passed = files = 0
    for name in sorted(by_dir):
        c = by_dir[name]
        pct = 100.0 * c["PASS"] / c["total"] if c["total"] else 0.0
        lines.append("| %s | %d | %d | %d | %.1f | %d |" % (name, c["files"], c["total"], c["PASS"], pct, c["unreported"]))
        total += c["total"]
        passed += c["PASS"]
        files += c["files"]
    lines.append("| **total** | %d | %d | %d | %.1f | |" % (files, total, passed, 100.0 * passed / max(total, 1)))
    print("\n".join(lines))
    print("\nExcluídos (%d):" % len(excluded))
    for cause, count in collections.Counter(r["excluded"] for r in excluded).most_common():
        print("- %s: %d" % (cause, count))
    print("\nMaiores causas:")
    for cause, count in causes.most_common(25):
        print("%6d  %s   (ex.: %s)" % (count, cause, examples[cause]))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--wpt")
    parser.add_argument("--worker")
    parser.add_argument("--out")
    parser.add_argument("--dirs")
    parser.add_argument("--jobs", type=int, default=12)
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--limit", type=int)
    parser.add_argument("--filter")
    parser.add_argument("--summary")
    args = parser.parse_args()
    if args.summary:
        summarize(args.summary)
    else:
        collect(args)


if __name__ == "__main__":
    sys.exit(main())
