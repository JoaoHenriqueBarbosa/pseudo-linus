#!/bin/sh
# Mede o que uma navegação grava no perfil do chromium do oráculo (History, Sessions, UKM, Preferences por site, cache
# de disco): serve testbench/corpus/headless-storage/ em http://127.0.0.1:8000, carrega a página (ou about:blank) com
# --user-data-dir=/tmp/profile e copia o perfil inteiro, depois da saída, para $OUT/profile, mais a árvore em
# $OUT/profile-tree.jsonl.
# MODO: `dump` (--dump-dom, o padrão), `term` (--remote-debugging-port e SIGTERM depois de ESPERA segundos) ou `close`
# (o mesmo, mas encerra pelo `Browser.close` do DevTools).
# REDE: `none` roda o contêiner sem rede (só o loopback); o padrão tem a rede do host.
# RUNS: quantas execuções seguidas no mesmo perfil (só no modo dump; o padrão é 1).
# Uso: OUT=<dir do host> [MODO=dump|term] [ESPERA=8] [RUNS=1] testbench/oracle/measure-navigation.sh página.html|about:blank
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
: "${OUT:?OUT: diretório do host que recebe a cópia}"
page=${1:?página.html ou about:blank}
mode=${MODO:-dump}
wait_s=${ESPERA:-8}
runs=${RUNS:-1}
network=${REDE:-bridge}
mkdir -p "$OUT"
docker run --rm --network "$network" --memory 4g -v "$root/testbench/corpus/headless-storage:/pg:ro" -v "$root/testbench/oracle/profile-tree.py:/usr/local/bin/profile-tree.py:ro" -v "$OUT:/out" -e PAGE="$page" -e MODE="$mode" -e WAIT="$wait_s" -e RUNS="$runs" pl-oracle-chromium sh -c '
cd /pg && python3 -m http.server 8000 --bind 127.0.0.1 >/dev/null 2>&1 &
sleep 1
if [ "$PAGE" = about:blank ]; then URL=about:blank; else URL="http://127.0.0.1:8000/$PAGE"; fi
if [ "$MODE" = dump ]; then
  n=0
  while [ "$n" -lt "$RUNS" ]; do
    chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --virtual-time-budget=20000 --dump-dom "$URL" >/out/dom.txt 2>/dev/null
    n=$((n + 1))
  done
else
  chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --remote-debugging-port=9222 --remote-allow-origins=* "$URL" >/dev/null 2>&1 &
  browser=$!
  sleep "$WAIT"
  if [ "$MODE" = close ]; then
    python3 -c "
import asyncio, json, urllib.request, websockets
async def main():
    url = json.load(urllib.request.urlopen(\"http://127.0.0.1:9222/json/version\"))[\"webSocketDebuggerUrl\"]
    async with websockets.connect(url) as ws:
        await ws.send(json.dumps({\"id\": 1, \"method\": \"Browser.close\"}))
        try:
            await asyncio.wait_for(ws.recv(), 5)
        except Exception:
            pass
asyncio.run(main())"
  else
    kill -TERM "$browser"
  fi
  i=0
  while kill -0 "$browser" 2>/dev/null && [ "$i" -lt 60 ]; do sleep 0.5; i=$((i + 1)); done
fi
cp -a /tmp/profile /out/profile
profile-tree.py /tmp/profile > /out/profile-tree.jsonl
(cp -a /root/.config /out/config 2>/dev/null || true)
profile-tree.py /root/.config > /out/config-tree.jsonl
chmod -R a+rX /out'
