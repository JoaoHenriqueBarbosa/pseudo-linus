#!/bin/sh
# Mede o que o chromium do oráculo grava no disco quando o MESMO --user-data-dir é usado em execuções seguidas:
# serve testbench/corpus/headless-storage/ em http://127.0.0.1:8000, carrega cada página da lista (uma por execução do
# navegador) e copia o perfil depois de cada execução para $OUT/run<n>/profile, mais o texto do --dump-dom em
# $OUT/run<n>/dom.txt.
# Uso: OUT=<diretório do host> testbench/oracle/measure-reopen.sh probe.html reopen.html ...
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
: "${OUT:?OUT: diretório do host que recebe as cópias}"
[ "$#" -gt 0 ] || { echo "uso: measure-reopen.sh página.html [página.html ...]" >&2; exit 2; }
mkdir -p "$OUT"
docker run --rm -v "$root/testbench/corpus/headless-storage:/pg:ro" -v "$OUT:/out" -e PAGES="$*" pl-oracle-chromium sh -c '
cd /pg && python3 -m http.server 8000 --bind 127.0.0.1 >/dev/null 2>&1 &
sleep 1
n=0
for page in $PAGES; do
  n=$((n + 1))
  mkdir -p /out/run$n
  chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --virtual-time-budget=5000 --dump-dom "http://127.0.0.1:8000/$page" >/out/run$n/dom.txt 2>/dev/null
  cp -a /tmp/profile /out/run$n/profile
done
chmod -R a+rX /out'
