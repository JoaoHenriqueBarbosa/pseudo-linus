#!/bin/sh
# Gera testbench/golden/chromium/<página>.dom rodando `chromium --headless --dump-dom` do oráculo (Debian 13) sobre
# cada página de testbench/corpus/headless/. O stderr do chromium (ruído de dbus do contêiner) é descartado.
# Uso: testbench/oracle/gen-chromium.sh [página.html ...]   (sem argumento, todas)
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
corpus=$root/testbench/corpus/headless
golden=$root/testbench/golden/chromium
mkdir -p "$golden"
if [ "$#" -eq 0 ]; then set -- $(cd "$corpus" && ls *.html); fi
for page in "$@"; do
  docker run --rm --network none -v "$corpus":/pg:ro pl-oracle-chromium \
    chromium --headless --no-sandbox --disable-gpu --dump-dom "file:///pg/$page" 2>/dev/null \
    >"$golden/${page%.html}.dom"
  echo "golden: ${page%.html}.dom"
done
