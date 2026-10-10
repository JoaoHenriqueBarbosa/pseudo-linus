#!/bin/sh
# Regera o golden do formato de pilha do V8 (tests/golden/v8_stack/expected.json) no Chromium do oráculo: abre o
# probe.html, que roda o probe.js por <script src>, e grava a lista que a sonda deixou no <pre id="o">.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)/tests/golden/v8_stack
docker run --rm --memory 4g -v "$here":/p pl-oracle-chromium \
    chromium --headless --no-sandbox --disable-gpu --dump-dom file:///p/probe.html 2>/dev/null |
    python3 -I -c '
import html, json, re, sys
page = sys.stdin.read()
text = re.search(r"<pre id=\"o\">(.*?)</pre>", page, re.S).group(1)
cases = json.loads(html.unescape(text))
json.dump(cases, sys.stdout, ensure_ascii=False, indent=1)
sys.stdout.write("\n")
' >"$here/expected.json"
