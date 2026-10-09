# chromium --headless (fase 5): plano e estado

Branch `chromium-headless`. Consumidores: `wip/zjsc` (motor JS) e `crates/ul-python`. Oráculo: chromium do Debian 13
(`testbench/oracle/Dockerfile.chromium`, imagem `pl-oracle-chromium`, golden por `testbench/oracle/gen-chromium.sh`).

## Arquitetura

- `crates/ul-chromium`: programa `chromium` (feature `browser` do `userland`).
  - `document.rs`: `BaseDocument` do Blitz (blitz-dom, blitz-html, stylo) como DOM de verdade; parser do html5ever
    com scripting ligado e doctype preservado; serialização de fragmento da especificação; sniff do doctype
    (o Blitz não tem nó de doctype).
  - `encoding.rs`: BOM, `Content-Type`, pré-varredura de `<meta>`, palpite UTF-8 ou windows-1252.
  - `page.rs`: busca (`file:`, `http(s):` pelo `ul_net::net::client`), decodifica, monta, serializa.
  - `cli.rs`: bandeiras do headless; `--dump-dom` pronto.
- `wip/zjsc/src/runtime/dom_bridge.rs` + `api/embed.rs`: ponte `DomHost` (uma função nativa `__pl_dom(op, ...)`)
  e `run_page`/`PageRunner` para embutir páginas.

## Estado

- `--dump-dom` de página estática idêntico ao oráculo em 5 de 6 páginas do corpus (`tests/conformance.rs`).
- Falta: scripts (ponte DOM e prelúdio JS), eventos, timers, `fetch`/XHR, screenshot (blitz-paint +
  anyrender_vello_cpu), `--print-to-pdf`, CDP (`--remote-debugging-port`).

## Divergências conhecidas (`testbench/corpus/headless/known-divergences.txt`)

- Codificação sem declaração: o Chromium usa o detector CED, que erra em amostras minúsculas (`s05`).
  Portar o CED, ou aproximar com o `chardetng`, é item próprio.
- Parse não incremental: o documento inteiro é parseado antes dos scripts. O correto é pausar o parser em
  `<script>` (`document.write`, scripts que leem DOM parcial). Entra junto com a execução de scripts.
- `<template>`, `DocumentFragment` e PI: o Blitz não tem esses tipos de nó; contorno planejado por elemento marcado.

## Ordem

1. Ponte DOM (`bridge.rs`) e prelúdio JS; scripts inline e externos em ordem; `DOMContentLoaded` e `load`.
2. Laço de eventos e timers no relógio virtual; `fetch` e XHR pela pilha do `ul-net`.
3. Screenshot e PDF. 4. CDP para Puppeteer e Playwright.
