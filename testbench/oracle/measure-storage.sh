#!/bin/sh
# Mede o que o chromium do oráculo grava no disco quando uma página usa localStorage e IndexedDB: serve
# testbench/corpus/headless-storage/ em http://127.0.0.1:8000 dentro do contêiner, carrega probe.html com
# --user-data-dir=/tmp/profile e --dump-dom, e copia o perfil final (bytes) para $OUT/profile, mais a árvore em
# $OUT/profile-tree.jsonl.
# Uso: OUT=<diretório do host> testbench/oracle/measure-storage.sh [página.html]
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
: "${OUT:?OUT: diretório do host que recebe a cópia}"
page=${1:-probe.html}
mkdir -p "$OUT"
docker run --rm -v "$root/testbench/oracle/profile-tree.py:/usr/local/bin/profile-tree.py:ro" -v "$root/testbench/corpus/headless-storage:/pg:ro" -v "$OUT:/out" -e PAGE="$page" pl-oracle-chromium sh -c '
cd /pg && python3 -m http.server 8000 --bind 127.0.0.1 >/dev/null 2>&1 &
sleep 1
chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --virtual-time-budget=5000 --dump-dom "http://127.0.0.1:8000/$PAGE" >/out/dom.txt 2>/dev/null
cp -a /tmp/profile /out/profile
profile-tree.py /tmp/profile > /out/profile-tree.jsonl
chmod -R a+rX /out'
