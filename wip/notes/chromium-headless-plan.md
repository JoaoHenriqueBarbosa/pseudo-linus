# chromium --headless (fase 5): plano e estado

Branch `chromium-headless`. Consumidores: `wip/zjsc` (motor JS) e `crates/ul-python`. Oráculo: Chromium 154 do Debian 13
(`testbench/oracle/Dockerfile.chromium`, imagem `pl-oracle-chromium`).

## Meta de entrega

O `chromium --headless --dump-dom` só conta como entregue com **95% ou mais** de páginas idênticas ao oráculo no
conjunto da internet, **em cada grupo** (páginas estáticas e SPAs), medido por `crates/ul-chromium/tests/internet.rs`.

## Arquitetura

- `crates/ul-chromium`: programa `chromium` (feature `browser` do `userland`).
  - `document.rs`: `BaseDocument` do Blitz (blitz-dom, blitz-html, stylo) como DOM; parser do html5ever com
    scripting ligado e doctype preservado; serialização de fragmento da especificação; sniff do doctype.
  - `encoding.rs`: BOM, `Content-Type`, pré-varredura de `<meta>`, palpite UTF-8 ou windows-1252.
  - `bridge.rs`: o `DomHost` do zjsc sobre o documento (árvore, atributos, seletores, `innerHTML`, rede com CORS).
  - `prelude/*.js`: DOM e janela em JavaScript sobre a ponte (Node, Element, interfaces HTML e SVG, eventos com
    captura e bolha, custom elements, MutationObserver, location, history, navigator, storage, XHR, fetch).
  - `page.rs`: carrega, roda scripts na ordem do navegador (síncronos com o resto do documento destacado, como o
    parser incremental veria; `defer` e `module` depois; `async` depois de `DOMContentLoaded`), ciclo de vida
    (`readystatechange`, `DOMContentLoaded`, `load`, `pageshow`), laço de eventos no tempo virtual até o orçamento.
  - `fetch.rs`: de onde vêm os bytes (rede do `ul-net` em produção; fita nos testes).
  - `cli.rs`: bandeiras do headless; `--dump-dom`, `--virtual-time-budget`, navegação por `location`.
- `wip/zjsc`: `runtime/dom_bridge.rs` (a função nativa `__pl_dom`), `api/embed.rs` (`run_page`, `PageRunner`,
  `__pl_eval` para scripts dinâmicos no escopo global, laço com orçamento virtual).

## Conjunto da internet (goldens)

`testbench/corpus/internet/pages.txt`: 36 páginas públicas, 20 estáticas e 16 SPAs (TodoMVC em React, Vue,
Preact, Svelte, Backbone, jQuery, Angular, Lit; react.dev, vuejs.org, svelte.dev, angular.dev, nextjs.org,
vite.dev, htmx.org, solidjs.com).

- Fitas: `testbench/oracle/record-internet.sh` grava, com o Chromium real via CDP (`tape-record.py`), cada resposta
  que a página recebeu (documento, scripts, folhas, XHR e fetch). Só cabeçalhos que mudam o comportamento entram;
  nada da máquina de quem grava. Corpos deduplicados por sha256 em `<id>/bodies/`.
- Golden: `testbench/oracle/gen-internet.sh` roda o Chromium do oráculo **sem rede**, sobre o replay da fita
  (`tape-replay.py`, HTTP e HTTPS, `--host-resolver-rules`), com `--virtual-time-budget=15000`, duas vezes. Saídas
  iguais viram `testbench/golden/chromium-internet/<id>.dom`; diferentes vão para `unstable.txt` e saem do placar
  (o próprio oráculo não se repete: aleatório, relógio ou ordem de rede).
- Placar: `PAGE=<id> DIFF_DIR=<dir> cargo test --release --test internet -- --nocapture` no `ul-chromium`.

## Placar

(atualizado a cada medição)

- 2026-10-10, commit 46711eb6: lab 6/6, spa 13/16 (vuejs-org, nextjs-org, vite-dev), static 18/20
  (wikipedia-linus, wikipedia-debian). Scripts 17/17.

## Modelo de quadros (medido no oráculo)

- Sob `--virtual-time-budget` o relógio virtual para enquanto a página espera a rede, e os quadros saem nesse meio
  tempo. Fora disso, quadros numa grade de 16ms enquanto a página carrega, mais um depois do `load`, e nenhum depois:
  rAF pedido depois desse último não roda (sonda `frames2.html`: `raf t30` ausente).
- Em cada quadro: rAF, depois ResizeObserver, depois IntersectionObserver; o idle callback espera o quadro seguinte.
- Alvo sem caixa (`display: none`) não intersecta.
- Sonda injetada na fita do oráculo (cópia em scratch, `<script>` no `<head>` que grava um `<meta name=plog>`): é a
  forma de comparar a ordem de eventos do Chromium com a nossa, com o `INSPECT=` do lado de cá.

## Divergências conhecidas

- vite-dev: largura de canvas medida por ResizeObserver (293 no oráculo, 641 aqui): layout de grid/flex.
- vuejs-org: os prefetch do VitePress dependem do `load` vir depois da montagem do Vue (no Chromium o `load` espera
  imagens e fontes de rede real); aqui o `load` sai antes da montagem e o quadro final já passou quando o idle
  callback observa os links.
- nextjs-org: a hidratação nunca comita (nenhum `__reactFiber`), embora o stream do RSC seja lido até o fim e os
  chunks registrem; no Chromium ela comita em ~170ms e insere `2g70aufap1-f8.js`. Causa ainda não achada.
- wikipedia-*: o mw.loader (cheio de requestIdleCallback) não injeta parte dos `<style>` e a classe
  `vector-feature-toc-pinned-clientpref-1` sai diferente.
- Codificação sem declaração: o Chromium usa o detector CED, que erra em amostras minúsculas
  (`testbench/corpus/headless/s05`). Portar o CED, ou aproximar com o `chardetng`, é item próprio.
- `<template>`, `DocumentFragment` e PI: o Blitz não tem esses tipos de nó; fragmento emulado por elemento marcado,
  `template.content` compartilha o nó do template.
- `document.write` durante o parse insere depois do script corrente em vez de alimentar o tokenizador.

## Ordem

1. Placar da internet a 95% em cada grupo (agora).
2. Screenshot (blitz-paint + anyrender_vello_cpu) e `--print-to-pdf`.
3. CDP (`--remote-debugging-port`) para Puppeteer e Playwright.
