#!/bin/sh
# Como measure-storage.sh, mas espera as transações do IndexedDB gravarem (--timeout em tempo real, o
# --virtual-time-budget deixava o chromium sair antes do disco) e roda cada página, em ordem, no mesmo perfil.
# Uso: OUT=<dir> testbench/oracle/measure-idb.sh <página.html> [<página.html> ...]
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
: "${OUT:?OUT: diretório do host que recebe a cópia}"
mkdir -p "$OUT"
docker run --rm --memory 4g -v "$root/testbench/oracle/profile-tree.py:/usr/local/bin/profile-tree.py:ro" -v "$root/testbench/corpus/headless-storage:/pg:ro" -v "$OUT:/out" -e PAGES="$*" pl-oracle-chromium sh -c '
cd /pg && python3 -m http.server 8000 --bind 127.0.0.1 >/dev/null 2>&1 &
sleep 1
n=0
for page in $PAGES; do
  n=$((n+1))
  chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --virtual-time-budget=20000 --dump-dom "http://127.0.0.1:8000/$page" >/out/dom$n.txt 2>/dev/null
  mkdir -p /out/run$n && cp -a /tmp/profile/Default/IndexedDB /out/run$n/
done
profile-tree.py /tmp/profile > /out/profile-tree.jsonl
chmod -R a+rX /out'
