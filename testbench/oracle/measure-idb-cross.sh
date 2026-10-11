#!/bin/sh
# Leitura cruzada do IndexedDB com o Chromium do oráculo. Roda as páginas dadas, em ordem, num perfil novo e depois
# `idb-dump.html`, que lê todos os bancos da origem sem escrever; o texto lido vai para $OUT/dump.txt e o leveldb da
# origem para $OUT/db. Com SEED=<diretório de um leveldb>, o perfil começa com esse banco no lugar do da origem (o que
# o plinus gravou), e o dump mostra o que o Chromium lê nele.
# Uso: OUT=<dir> [SEED=<dir>] testbench/oracle/measure-idb-cross.sh [<página.html> ...]
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
: "${OUT:?OUT: diretório do host que recebe a cópia}"
mkdir -p "$OUT"
seed_mount=""
if [ -n "${SEED:-}" ]; then seed_mount="-v $(cd "$SEED" && pwd):/seed:ro"; fi
# shellcheck disable=SC2086
docker run --rm --memory 4g -v "$root/testbench/corpus/headless-storage:/pg:ro" -v "$OUT:/out" $seed_mount -e PAGES="$*" pl-oracle-chromium sh -c '
cd /pg && python3 -m http.server 8000 --bind 127.0.0.1 >/dev/null 2>&1 &
sleep 1
db=/tmp/profile/Default/IndexedDB/http_127.0.0.1_8000.indexeddb.leveldb
if [ -d /seed ]; then mkdir -p "$db" && cp -a /seed/. "$db/"; fi
for page in $PAGES dump; do
  [ "$page" = dump ] && page=idb-dump.html
  chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --virtual-time-budget=20000 --dump-dom "http://127.0.0.1:8000/$page" >/out/last-dom.txt 2>/dev/null
done
python3 -c "
import html, re, sys
dom = open(\"/out/last-dom.txt\").read()
m = (re.findall(r\"<pre id=\\\"out\\\">(.*?)</pre>\", dom, re.S) or [None])[-1]
sys.stdout.write(html.unescape(m) + \"\n\" if m else \"sem saída\n\")
" > /out/dump.txt
rm -rf /out/db && cp -a "$db" /out/db
chmod -R a+rX /out'
