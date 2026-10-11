#!/bin/sh
# Roda DENTRO do contêiner do oráculo, sem rede: sobe o servidor de replay da fita montada em /tape e despeja o DOM
# da URL dada com o chromium real. `--virtual-time-budget` faz o relógio da página andar até ela ficar ociosa (ou
# gastar o orçamento), então o resultado não depende de corrida entre rede e timers. O relógio de parede começa no
# instante da gravação (`recorded_at` da fita), via libfaketime, para páginas que mostram "há N horas". O
# `Math.random` do V8 sai de uma semente fixa (`--random-seed`): o placar semeia o motor com a mesma semente e o
# mesmo gerador, e páginas que sorteiam (ids, destaques) passam a ter um golden só.
# Uso: replay-dump.sh <url>
python3 /oracle/tape-replay.py /tape &
sleep 1
start=$(python3 -c 'import json; print(json.load(open("/tape/tape.json"))["recorded_at"])')
exec faketime -f "@$(date -u -d "@$start" '+%Y-%m-%d %H:%M:%S')" \
  chromium --headless --no-sandbox --disable-gpu --disable-quic --ignore-certificate-errors \
  --host-resolver-rules="MAP * 127.0.0.1" --js-flags=--random-seed=1 --virtual-time-budget=15000 --dump-dom "$1" 2>/dev/null
