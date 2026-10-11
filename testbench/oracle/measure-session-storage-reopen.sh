#!/bin/sh
# Mede o Session Storage do chromium do oráculo em execuções seguidas com o MESMO --user-data-dir: cada argumento é
# `modo:página` (modo `dump`, `term` ou `close`; `term` e `close` esperam ESPERA segundos com o DevTools aberto e saem por
# SIGTERM ou Browser.close). Depois de cada execução copia `Default/Session Storage` para $OUT/run<n>/ss e imprime as
# operações do log em $OUT/run<n>/ss.txt.
# Uso: OUT=<dir do host> [ESPERA=3] testbench/oracle/measure-session-storage-reopen.sh dump:ss-none.html term:ss-quick.html ...
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
: "${OUT:?OUT: diretório do host que recebe as cópias}"
[ "$#" -gt 0 ] || { echo "uso: measure-session-storage-reopen.sh modo:página [modo:página ...]" >&2; exit 2; }
wait_s=${ESPERA:-3}
mkdir -p "$OUT"
docker run --rm --memory 4g -v "$root/testbench/corpus/headless-storage:/pg:ro" -v "$OUT:/out" -e RUNS="$*" -e WAIT="$wait_s" pl-oracle-chromium sh -c '
cd /pg && python3 -m http.server 8000 --bind 127.0.0.1 >/dev/null 2>&1 &
sleep 1
n=0
for run in $RUNS; do
  n=$((n + 1))
  mode=${run%%:*}
  page=${run#*:}
  url="http://127.0.0.1:8000/$page"
  mkdir -p /out/run$n
  if [ "$mode" = dump ]; then
    chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --virtual-time-budget=20000 --dump-dom "$url" >/out/run$n/dom.txt 2>/dev/null
  else
    chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --remote-debugging-port=9222 --remote-allow-origins=* "$url" >/dev/null 2>&1 &
    browser=$!
    sleep "$WAIT"
    if [ "$mode" = term ]; then
      kill -TERM "$browser"
    else
      python3 - <<PY
import json, urllib.request, socket, base64, os
ws = json.load(urllib.request.urlopen("http://127.0.0.1:9222/json/version"))["webSocketDebuggerUrl"]
host, rest = ws[5:].split("/", 1)
h, p = host.split(":")
s = socket.create_connection((h, int(p)))
key = base64.b64encode(os.urandom(16)).decode()
s.send(("GET /%s HTTP/1.1\r\nHost: %s\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: %s\r\nSec-WebSocket-Version: 13\r\n\r\n" % (rest, host, key)).encode())
s.recv(4096)
msg = json.dumps({"id": 1, "method": "Browser.close"}).encode()
mask = os.urandom(4)
s.send(bytes([0x81, 0x80 | len(msg)]) + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(msg)))
try:
    s.recv(4096)
except Exception:
    pass
PY
    fi
    i=0
    while kill -0 "$browser" 2>/dev/null && [ "$i" -lt 60 ]; do sleep 0.5; i=$((i + 1)); done
  fi
  cp -a "/tmp/profile/Default/Session Storage" /out/run$n/ss
done
chmod -R a+rX /out'
for dir in "$OUT"/run*; do
  for log in "$dir"/ss/*.log; do python3 "$root/testbench/oracle/leveldb-hex.py" "$log"; done > "$dir/ss.txt"
done
