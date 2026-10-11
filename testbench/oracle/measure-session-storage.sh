#!/bin/sh
# Mede o Session Storage do chromium do oráculo: serve testbench/corpus/headless-storage/ em http://127.0.0.1:8000,
# carrega a página dada com --user-data-dir=/tmp/profile e o MODO dado, e copia o leveldb de `Default/Session Storage`
# para $OUT/ss, mais as operações de cada lote do log em $OUT/ss.txt (leveldb-hex.py).
# MODO: `dump` (--dump-dom, o padrão), `devtools` (--remote-debugging-port e Browser.close pelo CDP depois de ESPERA
# segundos) ou `term` (--remote-debugging-port e SIGTERM depois de ESPERA segundos). A página `about:blank` não passa
# pelo servidor; URLBASE troca a base (por exemplo file:///pg).
# Uso: OUT=<dir do host> [MODO=dump|devtools|term] [ESPERA=8] [URLBASE=...] testbench/oracle/measure-session-storage.sh página.html
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
: "${OUT:?OUT: diretório do host que recebe a cópia}"
page=${1:?página.html}
mode=${MODO:-dump}
wait_s=${ESPERA:-8}
base=${URLBASE:-http://127.0.0.1:8000}
mkdir -p "$OUT"
docker run --rm --memory 4g -v "$root/testbench/corpus/headless-storage:/pg:ro" -v "$OUT:/out" -e PAGE="$page" -e BASE="$base" -e MODE="$mode" -e WAIT="$wait_s" pl-oracle-chromium sh -c '
cd /pg && python3 -m http.server 8000 --bind 127.0.0.1 >/dev/null 2>&1 &
sleep 1
if [ "$PAGE" = about:blank ]; then URL=about:blank; else URL="$BASE/$PAGE"; fi
if [ "$MODE" = dump ]; then
  chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --virtual-time-budget=20000 --dump-dom "$URL" >/out/dom.txt 2>/dev/null
else
  chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --remote-debugging-port=9222 --remote-allow-origins=* "$URL" >/dev/null 2>&1 &
  browser=$!
  sleep "$WAIT"
  cp -a "/tmp/profile/Default/Session Storage" /out/live
  if [ "$MODE" = term ]; then
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
  # Espera o navegador sair de fato (até 30 s): copiar antes perde os lotes do desligamento.
  i=0
  while kill -0 "$browser" 2>/dev/null && [ "$i" -lt 60 ]; do sleep 0.5; i=$((i + 1)); done
fi
cp -a "/tmp/profile/Default/Session Storage" /out/ss
ls -la "/tmp/profile/Default/Session Storage" > /out/ls.txt
chmod -R a+rX /out'
for log in "$OUT"/ss/*.log; do python3 "$root/testbench/oracle/leveldb-hex.py" "$log"; done > "$OUT/ss.txt"
# O estado com o navegador vivo (só nos modos com DevTools): o log de antes do sinal.
if [ -d "$OUT/live" ]; then
  for log in "$OUT"/live/*.log; do python3 "$root/testbench/oracle/leveldb-hex.py" "$log"; done > "$OUT/live.txt"
fi
