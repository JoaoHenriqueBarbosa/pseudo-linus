#!/bin/sh
# Quais módulos embutidos do Node o driver do Playwright (o JavaScript dentro da wheel) usa. Medido por varredura dos
# `require("...")` e `from "node:..."` em package/lib, só os nomes que o Node tem como builtin.
D=$(/opt/playwright/bin/python -c "import playwright,os;print(os.path.dirname(playwright.__file__))")/driver/package
echo "== arquivos"; find "$D" -name '*.js' | wc -l; du -sh "$D"
echo "== builtins usados (contagem de arquivos que os requerem)"
for m in assert async_hooks buffer child_process cluster console constants crypto dgram diagnostics_channel dns domain events fs http http2 https inspector module net os path perf_hooks process punycode querystring readline repl stream string_decoder timers tls tty url util v8 vm worker_threads zlib; do
  n=$(grep -rlE "require\(['\"](node:)?$m(/[a-z_]+)?['\"]\)" "$D" --include=*.js | wc -l)
  [ "$n" -gt 0 ] && echo "$m $n"
done
echo "== entrada"
ls "$D"; cat "$D/package.json" | head -30
echo "== como o python o inicia"
sed -n 1,60p "$(dirname "$D")/../_impl/_transport.py" 2>/dev/null | head -0
grep -n "driver\|PLAYWRIGHT\|stdin\|stdout\|Popen\|create_subprocess" "$(dirname "$D")/../_impl/_transport.py" | head -30
