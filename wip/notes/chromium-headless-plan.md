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
  (`tape-replay.py`, HTTP e HTTPS, `--host-resolver-rules`), com `--virtual-time-budget=15000`, oito vezes
  (`RUNS`). Cada saída distinta vira variante (`<id>.dom`, `<id>.<n>.dom`). O que o oráculo não repete nem assim
  (o retrato de uma animação no meio: o trabalho de CPU do carregamento anda o relógio de verdade, e cada rodada
  para num instante diferente) vira intervalo: `golden-ranges.py` marca o número que muda entre as rodadas, sempre
  com o mesmo texto logo antes e logo depois, e grava o mínimo e o máximo observados em `<id>.ranges.json` (um por
  texto distinto do oráculo); o que não variou fica exato. Só
  vai para `unstable.txt` a página de despejo vazio.
- Placar: `PAGE=<id> DIFF_DIR=<dir> cargo test --release --test internet -- --nocapture` no `ul-chromium`. Passa o
  despejo igual a uma variante, ou com o texto de um grupo de intervalos, os números exatos iguais e os variáveis
  dentro do intervalo observado. Nunca se calibra custo de CPU para cair numa amostra, nem se alarga intervalo.

## Placar

(atualizado a cada medição)

- 2026-10-10, commit 46711eb6: lab 6/6, spa 13/16 (vuejs-org, nextjs-org, vite-dev), static 18/20
  (wikipedia-linus, wikipedia-debian). Scripts 17/17.
- 2026-10-10, commit 5425f906: lab 6/6, spa 14/16 (nextjs-org, vite-dev), static 20/20. Scripts 23/23.

## Modelo de quadros (medido no oráculo)

- Sob `--virtual-time-budget` o relógio virtual para enquanto a página espera a rede, e os quadros saem nesse meio
  tempo. Fora disso, quadros numa grade de 16ms enquanto a página carrega, mais um depois do `load`, e nenhum depois:
  rAF pedido depois desse último não roda (sonda `frames2.html`: `raf t30` ausente).
- Enquanto a rede não responde a página está ociosa: o quadro pedido e os idle callbacks pendentes saem antes da
  resposta (wikipedia: o rIC pedido aos 94ms roda aos 106, antes dos scripts).
- Depois do `load` o navegador busca o `link rel=icon` (sem ele, nada aparece no resource timing); essa busca fica
  em voo e mantém os quadros saindo (round trip calibrado em 50ms, pela wikipedia).
- Em cada quadro: rAF, depois ResizeObserver, depois IntersectionObserver; o idle callback espera o quadro seguinte.
- Alvo sem caixa (`display: none`) não intersecta.
- Sonda injetada na fita do oráculo (cópia em scratch, `<script>` no `<head>` que grava um `<meta name=plog>`): é a
  forma de comparar a ordem de eventos do Chromium com a nossa, com o `INSPECT=` do lado de cá.

## Divergências conhecidas

- vite-dev: o Rive (`@rive-app/webgl2`) só segue com `getContext('webgl2')`; aqui os três contextos voltam null e o
  `onLoad` nunca roda, então o canvas fica com o atributo (641 em vez de 293). Com um contexto 2D falso ele nem o
  usa; com um WebGL2 falso passa de `pixelStorei` e para. Precisa do WebGL2 de verdade (abaixo).
- nextjs-org: a hidratação já comita; sobram `border-radius` do framer (0.233831 contra 0.132387) e `scale()` de SVG
  (0.900135 contra 0.944158), que vêm de layout e métricas de fonte, e a ordem do meta `theme-color`.
- Codificação sem declaração: o Chromium usa o detector CED, que erra em amostras minúsculas
  (`testbench/corpus/headless/s05`). Portar o CED, ou aproximar com o `chardetng`, é item próprio.
- `<template>`, `DocumentFragment` e PI: o Blitz não tem esses tipos de nó; fragmento emulado por elemento marcado,
  `template.content` compartilha o nó do template.
- `document.write` durante o parse insere depois do script corrente em vez de alimentar o tokenizador.
- Perfil, estado de rede (decisão do coordenador, 2026-10-11): o `Network Persistent State` e o `TransportSecurity` do
  `--dump-dom` saem na forma sem rede do oráculo (116 bytes, sem `TransportSecurity`; com rede o oráculo grava de
  1049 a 1079 bytes com servidores do Google e o HSTS deles). Essa forma vem do tráfego de fundo do Chromium, que o
  sandbox não emula e não deve emular; nenhum agente lê o arquivo para decidir algo.
- Perfil, IndexedDB em `iframe`: o oráculo cria bucket no QuotaManager (próprio no mesmo site, particionado em outro
  site); o motor ainda não executa script de `iframe`, então não nasce (`crates/ul-chromium/src/quota_manager.rs`).
- Perfil, UKM (`segmentation_platform/ukm_db`, `crates/ul-chromium/src/ukm.rs`): o `--dump-dom` grava `urls` (url_id =
  MD5, atraso do `last_timestamp` sorteado das 44 execuções medidas) e o evento da saída pela regra de duração medida
  (nenhum abaixo de 46 ms, todos acima de 90 ms); o `metric_value` é sorteado na faixa medida, sem a relação com a
  página. Fora do medido: o navegador de pé (SIGTERM deixa o banco de 4096 bytes e o WAL de 90 KB, o `Browser.close` o de
  49152 bytes com a fonte 49 e sempre um evento).
- Perfil, saída do `Browser.close` do navegador de pé: a árvore da saída limpa vem do molde `template-close.pack`
  (quatro execuções do oráculo sem rede, `about:blank`; `tests/disk_profile.rs`), e com página o cache de disco (página
  e favicon), o `Preferences` com o engajamento, o QuotaManager, o `ukm_db` (fonte 49, sempre um evento) e o
  `Network Persistent State` sem rede saem como no oráculo (`tests/close_profile.rs`). Fora do medido: o
  `Site Characteristics Database/000003.log` (112 bytes com página contra 40 do molde), o `shared_proto_db/000003.log`
  com página (6787 bytes contra 7194 do molde) e as sessões com mais de uma navegação no mesmo `Browser.close`.
- Perfil, redirecionamento: medido com um salto de servidor (`301`); cadeias maiores e o redirecionamento do cliente
  depois do de servidor seguem sem medida (`crates/ul-chromium/src/history.rs`).

## Canvas (decisão do coordenador, 2026-10-10: os dois de verdade)

O oráculo (`--disable-gpu`) entrega WebGL e WebGL2 por ANGLE sobre SwiftShader, e o 2D por Skia. Tabela medida em
`testbench/oracle/measured/webgl-swiftshader.json` (gerada por `webgl-swiftshader.html`): constantes, 102/167
parâmetros, métodos, extensões, atributos de contexto, precisões de shader e o renderer desmascarado.

1. WebGL/WebGL2 em estado: objetos (programa, shader, buffer, textura, framebuffer, renderbuffer, VAO, query,
   sampler, sync, transform feedback), `getParameter` pela tabela medida mais o estado corrente, `getError` com as
   regras de validação do Chromium, extensões com os próprios objetos. Compilação de GLSL com validação de sintaxe
   e `getShaderInfoLog` como o ANGLE.
2. Rasterização por software quando a página lê pixels (`readPixels`, `toDataURL`, `drawImage` do canvas).
3. Canvas 2D: estado, caminhos e rasterização por vello_cpu; `measureText` com as fontes do oráculo
   (`measureText('Hello')` em `10px sans-serif` = 22.7880859375); `toDataURL` com os bytes do codificador PNG do Skia
   (canvas vazio 300x150: 2118 caracteres).

## Pintura (M5.5: `--screenshot`, depois `Page.captureScreenshot` e `--print-to-pdf`)

`crates/ul-chromium/src/paint.rs`: o Blitz resolve o layout e o `blitz-paint` desenha pelo `anyrender_vello_cpu` num RGBA
de viewport, composto sobre o branco do Chromium e codificado em PNG. Fontes: as da VFS (Liberation e DejaVu do Debian), as
mesmas do oráculo, sem fallback inventado.

Métrica fixa e versionada: `paint-ssim-v1` em `crates/ul-chromium/tests/paint.rs` (SSIM médio, luminância Rec. 709, janela
uniforme 8x8 com passo 4, L=255, K1=0,01, K2=0,03). Razão: pesa estrutura (bordas, glifos, alinhamento) e não o valor de cada
pixel; o antisserrilhado e o hinting do texto diferem do Skia sem a página parecer outra, e o SSIM cai quando um elemento muda
de lugar, de tamanho ou some, o que o erro médio quadrático esconde numa página quase toda branca. Os goldens são screenshots
do Chromium 154 do oráculo (`--headless --hide-scrollbars --window-size=780,493`, páginas em `tests/paint/pages`). O limiar de
cada página é o medido menos 0,02 (catraca: só sobe).

Medido na primeira pintura (SSIM contra o oráculo): texto 0,871; caixa e borda 0,884; imagem 0,891; flex e grid 0,834; site
local 0,740 (as imagens `data:` só pintaram depois de servidas pelo corpo da própria URL). O que falta, todo de layout do `blitz-dom` e não da pintura: `vertical-align: top` em imagem inline (a imagem fica
na linha de base), a tabela (larguras de coluna e a espessura da borda colapsada), alturas de linha 1 a 2 px diferentes em
parágrafos e 1 px a mais na altura do cabeçalho.

## Ordem

1. Placar da internet a 95% em cada grupo (agora).
2. Screenshot (blitz-paint + anyrender_vello_cpu) e `--print-to-pdf`.
3. CDP (`--remote-debugging-port`) para Puppeteer e Playwright.
