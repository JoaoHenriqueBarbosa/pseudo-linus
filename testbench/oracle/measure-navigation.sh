#!/bin/sh
# Mede o que uma navegação grava no perfil do chromium do oráculo (History, Sessions, UKM, Preferences por site, cache
# de disco): serve testbench/corpus/headless-storage/ em http://127.0.0.1:8000, carrega a página (ou about:blank) com
# --user-data-dir=/tmp/profile e copia o perfil inteiro, depois da saída, para $OUT/profile, mais a árvore em
# $OUT/profile-tree.jsonl.
# MODO: `dump` (--dump-dom, o padrão) ou `term` (--remote-debugging-port e SIGTERM depois de ESPERA segundos).
# Uso: OUT=<dir do host> [MODO=dump|term] [ESPERA=8] testbench/oracle/measure-navigation.sh página.html|about:blank
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
: "${OUT:?OUT: diretório do host que recebe a cópia}"
page=${1:?página.html ou about:blank}
mode=${MODO:-dump}
wait_s=${ESPERA:-8}
mkdir -p "$OUT"
docker run --rm --memory 4g -v "$root/testbench/corpus/headless-storage:/pg:ro" -v "$root/testbench/oracle/profile-tree.py:/usr/local/bin/profile-tree.py:ro" -v "$OUT:/out" -e PAGE="$page" -e MODE="$mode" -e WAIT="$wait_s" pl-oracle-chromium sh -c '
cd /pg && python3 -m http.server 8000 --bind 127.0.0.1 >/dev/null 2>&1 &
sleep 1
if [ "$PAGE" = about:blank ]; then URL=about:blank; else URL="http://127.0.0.1:8000/$PAGE"; fi
if [ "$MODE" = dump ]; then
  chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --virtual-time-budget=20000 --dump-dom "$URL" >/out/dom.txt 2>/dev/null
else
  chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --remote-debugging-port=9222 --remote-allow-origins=* "$URL" >/dev/null 2>&1 &
  browser=$!
  sleep "$WAIT"
  kill -TERM "$browser"
  i=0
  while kill -0 "$browser" 2>/dev/null && [ "$i" -lt 60 ]; do sleep 0.5; i=$((i + 1)); done
fi
cp -a /tmp/profile /out/profile
profile-tree.py /tmp/profile > /out/profile-tree.jsonl
chmod -R a+rX /out'
