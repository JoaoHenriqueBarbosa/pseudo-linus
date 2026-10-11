# Pendências consolidadas (fechamento da rodada de 2026-10-11)

Estado: o porte zjsc, o zbun e o chromium headless foram integrados num só ramo, e os crates `wip/z*` foram promovidos
para `crates/` (membros do workspace). Este arquivo reúne o que ficou aberto; o detalhe de cada frente está nos arquivos
indicados.

## Medido e verde na integração
- `ul-chromium`: `frames`, `conformance`, `scripts` (52), `devtools` (3), `paint` (2), `internet` com a regressão do
  carregador de módulos por global corrigida (placar da internet: ver `roadmap-status.md`, só vale o que foi medido).

## Abertas, na ordem em que destravam as metas
1. **M5.6, driver do Playwright no nosso bun:** `bun cli.js --version` para em `Cannot find module 'readline'`. Detalhe e
   ordem do caminho crítico em `driver-pendencias-fork.md` (readline, `node:module`, motor de fluxo do zlib, HTTP/TLS/H2).
2. **M5.6, CDP de frames:** a cena `pw_frames` do oráculo está gravada (127 de 172 mensagens iguais no trabalho em
   andamento do fork, não integrado). Faltam rede por frame, `DOM.describeNode` com `contentDocument` e o layout do iframe.
   `DOM.getDocument`, `querySelector`, `getOuterHTML` e `Page.printToPDF` não começaram (`pendencias-cdp-dom-pdf.md`).
3. **M5.4, WPT:** ~51% medido, meta 90%. `getComputedStyle` (756 subtestes) não iniciado, NOTRUN (1619), assert_equals
   genérico (2388), `:dir()`, `:heading()`, resto do quirks (altura de linha de tabela e de imagem inline)
   (`wpt-pendencias.md`).
4. **M5.5:** `--print-to-pdf` inexistente; pintura em quirks com diferença declarada.
5. **Desempenho:** interpretador 16 a 800 vezes mais lento que o bun (`perf-baseline.md`); nenhuma otimização feita.
6. **Doctype como nó filho do documento:** `childNodes` sai `[1]`, o Chromium tem `[10, 1]` (vale para HTML e XML).
