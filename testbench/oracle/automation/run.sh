#!/bin/sh
# Roda as cenas de automação no oráculo (Chromium 154 real + Playwright e pyppeteer reais) e grava, por cenário,
# a saída (`<cena>.out`) e as mensagens CDP trocadas (`<cena>.cdp.jsonl`) em testbench/oracle/automation/golden.
# Uso: run.sh [cena...]   (sem argumento, todas). Sem rede: o servidor da fixture é local.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out="$here/golden"
mkdir -p "$out"
cases="$*"
if [ -z "$cases" ]; then
  cases=$(cd "$here/cases" && ls | sed 's/\.py$//')
fi
for name in $cases; do
  case "$name" in
    pp_*) python=/opt/pyppeteer/bin/python ;;
    *) python=/opt/playwright/bin/python ;;
  esac
  rm -f "$out/$name.out" "$out/$name.cdp.jsonl"
  docker run --rm --network none --memory 4g --user root \
    -e PYTHONPATH=/auto -e PYTHONUNBUFFERED=1 -e CDP_TEE_LOG="/out/$name.cdp.jsonl" -e CDP_TEE_BLOBS=/out/blobs \
    -v "$here":/auto:ro -v "$out":/out pl-oracle-automation \
    timeout 120 "$python" "/auto/cases/$name.py" > "$out/$name.out" 2> "$out/$name.err" || echo "cena $name saiu com erro" >&2
  [ -s "$out/$name.err" ] || rm -f "$out/$name.err"
done
