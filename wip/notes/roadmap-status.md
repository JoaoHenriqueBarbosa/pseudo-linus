# Roadmap vivo: estado medido contra os critérios do `roadmap.md`

Este arquivo complementa o `roadmap.md` (o PRD, com as fases e os critérios de saída). Aqui fica o estado real de cada
critério, com a fonte de cada número. **Regra de manutenção: confira e atualize este arquivo a cada VIGIA e a cada
integração de fatia.** Número sem fonte não entra; critério que nunca foi medido aparece como "NÃO MEDIDO", nunca como
estimativa disfarçada.

Convenção da coluna "Fonte": **eu** = medido pelo coordenador com comando próprio (git, grep, ls); **fork** = reportado
por um fork na saída do container dele, não recompilado pelo coordenador; **vigente** = valor herdado de um arquivo
versionado, com a data do arquivo.

Última verificação: 2026-10-11 18:05 (CEST). Os horários do Registro de atualizações são os dos commits deste arquivo (`git log`); uma versão anterior trazia horários errados, adiantados.

## Ritmo medido

Commits por dia, contados no git (`git log --format=%ad --date=short -- <caminho>`), fonte **eu**:

| Caminho | 05/10 | 06/10 | 07/10 | 08/10 | 09/10 | 10/10 | 11/10 (parcial) | Total |
|---|---|---|---|---|---|---|---|---|
| `wip/zjsc` | | | | 24 | 73 | 151 | 75 | 323 |
| `wip/zbun` | | | | | | 60 | 68 | 128 |
| `crates/ul-bun` | | | | | | | 17 | 17 |
| `crates/ul-chromium` (branch chromium-headless) | | | | | 4 | 70 | 23 | 97 |
| `crates/ul-python` | 53 | 105 | 42 | 12 | | 16 | 41 | 269 |

Tamanho (linhas de `.rs`, fonte **eu**): `wip/zjsc/src` 598 719 (inclui dado gerado de Intl), `wip/zbun` 139 027,
`crates/ul-chromium/src` 20 860, `crates/ul-bun/src` 782. O ritmo com 4 forks Opus em paralelo é de 100 a 150 commits
por dia no zjsc e 60 a 70 no zbun.

## Critérios de saída da fase 5 (navegador headless com JavaScriptCore e Blitz)

| Marco | Critério do PRD | Estado medido | Fonte |
|---|---|---|---|
| M5.1 | JavaScriptCore passando 80% do test262 | **CUMPRIDO: 51 285 de 52 017 (98,6%)** com o corredor fechado (`createRealm` real, fixtures de módulo em memória; commits `0a6c80c9` e `c07fb25f`), test262 no commit `2e0a56762801e275a9fdf96dc49d90ba0cddcf63`. Antes do corredor fechado: 50 706 (97,5%). Por topo: built-ins 98,5%, language 99,1%, annexB 97,1%, intl402 96,0%. Restos, todos falta do motor: `$262.agent` 112, `import source/defer` 90, ArrayBuffer imutável cerca de 50, `Promise.allKeyed` e `allSettledKeyed` 39, módulos assíncronos que não terminam 30. Corredor validado por mim com 6 controles (a versão anterior do corredor; a atual não foi revalidada). 8 crashes fechados em `2653b961` | fork, controle eu |
| M5.2 | `node` com CommonJS e ESM, `fs`, `path`, `http` e `npm install` | Direção trocada para o bun: `bun` é programa do userland, com CJS e ESM, `node:fs` completo, `Bun.serve` e `fetch` em loopback, `Bun.file`, streams, CLI parcial. Comando `node` e `npm install`: não existem (só `bun` está registrado) | eu |
| M5.3 | `--dump-dom` correto em páginas com scripts simples | Cumprido além do critério: 42 de 42 páginas da internet idênticas ao Chromium 154 (meta do goal era 95% por grupo). Confirmado no estado integrado `eb2bd184` (Network http, Sessions, History e `shared_proto_db` juntos): suíte completa do `ul-chromium` e do `ul-common` em árvore limpa, 25 binários ok, 288 testes, 0 falhas, 1 ignorado (screenshot do pyppeteer) | fork |
| M5.4 | Recorte do Web Platform Tests acima de 90% | **49,0%** (13271 de 27063 subtestes, 2045 arquivos; 1136 fora do recorte: reftest 538, wptserve 304, testdriver 288, worker 6). Melhores: console 91,7, html/webappapis 88,0, url 82,7. Piores: dom/ranges 18,5, dom/traversal 20,3, forms 29,1. Maiores causas: assert_equals genérico 2388, NOTRUN 1619, seletor inválido 1008, getComputedStyle 756, interfaces XML ausentes 1167 | fork (não revalidado por mim) |
| M5.5 | `--screenshot` e `--print-to-pdf` com diferença perceptual baixa | **NÃO EXISTE.** O `cli.rs` do `ul-chromium` só trata `--dump-dom`; `Page.captureScreenshot` responde "wasn't found". Pintura de página não existe (o `vello_cpu` só serve ao canvas 2D). O alvo é diferença perceptual, não byte a byte | eu |
| M5.6 | CDP completo para Puppeteer e Playwright; test262 acima de 95% | CDP: 501 de 564 no placar (88,8%) com o cenário novo `playwright_state` (154 de 185: cookies, user agent, locale, fuso, geolocalização, permissões, cabeçalhos extras, histórico; integrado em `4db26cb8`, regressão só rodada antes do rebase, `pyppeteer_goto_and_evaluate` não rodado nessa fatia; sobras: locale não chega ao `Intl`, fuso só vale no documento seguinte, `Cookie` não reenviado por salto de redirect). Antes: 290 de 326 (89%) em `acdd7bba`; o fork reporta 347 de 379 (91,6%) com o cenário novo `playwright_handshake` em 53 de 53 (setWindowBounds, setDownloadBehavior, scripts de documento novo, mundos isolados, emulação de mídia), ainda sem commit. pyppeteer: pip install, launch, goto (com `Response`), title, evaluate e close funcionam com saída igual ao oráculo. Playwright: handshake de CDP em 53 de 53 (fork, integrado em `a2ff72c6`); replay da sessão CDP real do driver do Playwright contra o `ul-chromium`: 725 de 1151 mensagens em cinco cenas do oráculo (63,0%, fork; o denominador cresceu de 664 para 1151 porque o replay passou de 2 para 5 cenas: `pw_evaluate`, `pw_locators`, `pw_actions`, `pw_context`, `pw_navigation`; harness integrado em `36b73524`; `DOM.*` e `Input.*` implementados e integrados em `56815110`, replay em `96a72904`, sem regressão segundo o fork: `devtools` 347 de 379). `pw_evaluate`, `pw_locators` e `pw_actions` já sem método desconhecido; por cena: `pw_evaluate` 128/170, `pw_locators` 153/203, `pw_actions` 192/291, `pw_context` 114/198, `pw_navigation` 138/289; faltam, em `pw_context` e `pw_navigation`: `Browser.grantPermissions`, `Emulation.setGeolocationOverride`/`setLocaleOverride`/`setTimezoneOverride`/`setUserAgentOverride`, `Network.emulateNetworkConditions`/`setExtraHTTPHeaders`, `Runtime.addBinding`, `Storage.getCookies`/`setCookies`/`clearCookies`, `Page.getNavigationHistory`/`navigateToHistoryEntry`; metas são os `golden/pw_*.cdp.jsonl`, gravados com o driver real e o Chromium 154); driver Node: incógnita medida em `wip/notes/playwright-driver.md`. O bun real roda o driver; no nosso, `bun cli.js --version` já passa de `tty` (gerado do `tty.ts` do bun, `282eb505`) e para em `Cannot find module 'crypto'`. Faltam `crypto`, `child_process`, `net`, `zlib`, `tls`, `http`, `https`, `readline`, `inspector`, `dns`; o `.ts` de cada um do bun 1.4.2 está em `/home/john/cache/bun-src/tree/src/js/node`, então saem pelo gerador. test262 acima de 95%: cumprido no agregado (98,6%) e em todos os diretórios de topo (ver M5.1) | fork |

## Estado por frente

### zjsc (JavaScriptCore em Rust)
- Funcional fechado (runtime Web, módulos `node:`, goldens). `function_error` 915 de 915; `error` 2791 de 2797 (as 6 restantes declaradas).
- Suíte completa de 439 binários: na última leitura completa, 223 terminados e 39 vermelhos, sem pânico (fork). Intl e datas: 21 binários vermelhos no baseline; `datetime_range` de 5147 para 3679 divergências e `gaps` de 1220 para 778 (fork).
- Declarados e abertos: coluna do frame de `new X()` (4 colunas), `call_edge` (JIT), `global_semantics`, `error_api`, `dom_exception`, recursão de apply de Proxy, ordem de `ownKeys`.

### bun (zbun e `ul-bun`)
- Transpilador portado: golden do printer 360 de 360, `react_compiler` compila, 0 `unsafe`. Corpus do `ul-bun`: 216 casos (fork; eram 53 na primeira leitura), rodando no kernel real, com `bun run` com `package.json`, CLI base e `node:tty` integrados; as 101 sondas do `-p` estão em KNOWN até o `-p` ser portado (precisa do valor de conclusão no motor).
- Falta: `bun run` com `package.json`, `-p`/`--print`, opções de execução (`--cwd`, `--preload`, `--env-file`, `--define`), subcomandos `test` e `x`, WebSocket, tls/https, inotify, `Bun.Glob`.

### chromium (Blitz, Stylo)
- Perfil em disco: History (inclusive pelo DevTools), Session Storage, Sessions, Cache, Preferences, QuotaManager, Network Persistent State, `shared_proto_db`, `ul-leveldb`.
- Network do documento http completo (ExtraInfo, redirect, favicon): cenário `network_http` 49 de 49 (fork).
- Falta: redirecionamento e navegações posteriores à inicial no DevTools, UKM, IndexedDB em iframe, interior da página de erro (25 dos 74 do `network_navigate`), timing real de rede.

### chromium para Puppeteer e Playwright
- pyppeteer: ver M5.6. Playwright: precisa do driver Node rodando no bun do plinus (não medido), `Browser.setWindowBounds`, `Page.createIsolatedWorld`, `addScriptToEvaluateOnNewDocument`, `DOM.*`, `Input.*`.

### Python (fases 1 a 4 do PRD)
- Fora do foco desta sessão. Placar vigente da bancada: 9320 de 9527 estrito (97,8%), de 06/10 (`docs/conformance.md`).
  Coletor de ciclos e greenlet avançaram em 10 e 11/10 (13 testes do greenlet ainda declarados). Estado das fases de
  pip, numpy e pandas: NÃO RELIDO nesta atualização.

## Fila priorizada

1. **Subir o WPT de 49,0% para 90%** (M5.4, `wip/notes/wpt-baseline.md`). Causas medidas: `assert_equals` genérico (2388), arquivos sem resultado (1619), seletor inválido (1008), `getComputedStyle` (756), interfaces XML ausentes (1167), `type-change` de input week/month (280). Do test262 sobram só faltas do motor (`$262.agent`, `import source/defer`, ArrayBuffer imutável, `Promise.allKeyed`, async de módulo).
2. CDP e Playwright (M5.6): fechar a lista de métodos, depois o driver Node no bun.
3. `--screenshot` e `--print-to-pdf` (M5.5): pintura de página (`blitz-paint` sobre `vello_cpu`), PNG e PDF, medido por diferença perceptual.
4. bun/node (M5.2): `bun run`, `-p`, opções, `test`, `x`; decidir se o comando `node` e o `npm install` entram.
5. Cauda de Intl nos locales de uso real, depois ja e zh; calendários nativos por último.

## Riscos sem medição

- **Desempenho do interpretador (MEDIDO em 18:35, `wip/notes/perf-baseline.md`, fork, release, mediana de 3, variação de até 40% por máquina dividida):** razão zjsc/bun deltablue (reduzido) 803, richards 500, fib(32) 294, regex 27, sort 25, strconcat 23, decodeURI 23 (6,6 s para 2 milhões; os 19,9 s anteriores eram do build de depuração), JSON 16, parse do bundle do Playwright 1,8 a 2,0. O parse não é o gargalo; execução com chamadas e propriedades é. Perfil (callgrind): registro de células RefCell global 22%, operandos e aritmética 10 a 14%, sem cache inline 13 a 17%, SipHash 4 a 7%. Três otimizações propostas, nenhuma feita (ganho estimado 2 a 3 vezes, longe do JIT). O `require` do coreBundle ainda não vale como medida (parava em node:tty, hoje integrado). Sem meta de desempenho no PRD; sem estimativa de prazo.

## Estimativas otimistas

Método: ritmo medido acima, aplicado a itens com escopo conhecido. Item sem baseline não recebe estimativa.

| Item | Estimativa otimista | Base |
|---|---|---|
| test262 | feito | primeira medição em cerca de 1,5 hora desde a ordem; resta limpar crashes e corredor (1 dia, otimista) |
| CDP completo e pyppeteer sem screenshot | 1 dia | 36 cenários faltando, ritmo de 40 a 70 por dia |
| Playwright | 2 a 3 dias | o driver Node no bun é a incógnita |
| Screenshot e PDF perceptuais | 2 a 4 dias | pintura não existe |
| bun: `run`, `-p`, opções, `test`, `x` | 1 a 2 dias | |
| Intl nos locales de uso real | 1 a 2 dias | ritmo de um commit de causa raiz por hora |

A estimativa dada ao user em 2026-10-11 (5 a 7 dias sem screenshot, 7 a 11 com) omitia M5.1, M5.4 e M5.5 como critérios
formais; este arquivo a substitui. Corrigido também: o PRD pede screenshot por diferença perceptual, não byte a byte.

## Frentes ativas (2026-10-11, 18:05 CEST)

| Frente | Fork | Última integração | Em andamento |
|---|---|---|---|
| Printer, zbun, `ul-bun`, driver do Playwright | `ad4a1a75f3d5455f0` | `8be24a85` (Bun.listen e Bun.connect), checkout principal | `node:net` gerado do `net.ts` (fatia B), depois o erro seguinte de `cli.js --version` |
| Motor zjsc (fuso, locale, Intl) | `a7c6a4d11a2d73979` | `e320930e`, nos dois checkouts | ocioso; WPT e Intl a retomar |
| Greenlet, pyppeteer, Playwright | `adccaf30202b0bde1` | `1de9433c` (fuso e locale vivos), worktree chromium-headless | `Fetch.*` e `Network.getResponseBody`, depois diálogos |
| CDP e perfil | `a1f2c7757966af6bf` | `d8c51329` (árvore do Browser.close), worktree chromium-headless | iframe: spike de dois globais no mesmo VM passou; acesso cruzado e realm por frame |

## Registro de atualizações

- 2026-10-11 14:31: arquivo criado. Achados: test262 e WPT nunca foram medidos; `--screenshot` e `--print-to-pdf` não existem; o alvo de screenshot é perceptual.
- 2026-10-11 14:41: test262 clonado (commit `2e0a5676`, 53 910 testes); runner e baseline ainda não existem. Os 4 forks rodando.
- 2026-10-11 14:45: CDP reportado em 347 de 379 e `playwright_handshake` 53 de 53, pendentes de commit (fork do greenlet).
- 2026-10-11 14:50: driver Node do Playwright medido (`playwright-driver.md`). `bun run` com package.json integrado (`90460435`).
- 2026-10-11 14:56: replay do Playwright real em 245 de 664 (36,9%), primeira medida direta do M5.6 para o Playwright. Faltam `Target.createTarget` com contexto, `DOM.*`, `Input.*` e o que o log completo do replay listar.
- 2026-10-11 15:02: replay do Playwright corrigido para 402 de 664 (60,5%): o harness enviava o script injetado como texto literal. `DOM.*` e `Input.*` implementados (fork), placar novo pendente. test262 em 29 500 de 52 017 testes rodados, sem baseline ainda.
- 2026-10-11 15:07: test262 medido: 97,5% (M5.1 e a parte de test262 do M5.6 cumpridos). Corredor validado com 6 controles. Próxima lacuna de medição: WPT (M5.4).
- 2026-10-11 15:09: replay do Playwright em 725 de 1151 (cinco cenas). `bun run` e `node:tty` integrados; driver do Playwright parado em `crypto`. Corpus do `ul-bun` em 216 casos.
- 2026-10-11 15:10: 8 crashes do test262 fechados (`2653b961`); desempenho do interpretador registrado como risco sem medição.
- 2026-10-11 15:13: suíte completa do chromium verde em `eb2bd184` (288 testes, 0 falhas). Redirecionamento (301) do perfil em andamento no fork do CDP.
- 2026-10-11 15:34: DOM.* e Input.* e o replay das cinco cenas integrados no worktree do chromium-headless (`96a72904`). Playwright em 725 de 1151.
- 2026-10-11 15:37: métodos baratos do CDP integrados (`4db26cb8`): CDP em 501 de 564 (o denominador cresceu com o cenário `playwright_state`). Regressão pós-rebase pendente.
- 2026-10-11 16:07: test262 em 98,6% com o corredor fechado (`0a6c80c9`, `c07fb25f`). Restos são falta do motor. Corredor novo ainda sem revalidação por controle.
- 2026-10-11 16:09: redirecionamento de servidor (301) no History e nas Sessions do CDP integrado no worktree do chromium-headless (`5a8d0fbc`, do `2a03eb49` do fork). Medido por mim em container: dump_redirect 1, history_devtools 6, sessions 6, sessions_devtools 3, todos ok (devtools e conformance saíram com exit 0). Fora do medido: cadeias de mais de um salto. Achado do fork: a saída por Browser.close deixa uma árvore (cache, DIPS, Shortcuts, GCM Store, Network Persistent State, ukm_db) bem maior que a do SIGTERM, além do UKM.

- 2026-10-11 16:21: WPT medido pela primeira vez (M5.4): 49,0% contra meta de 90%. Runner e worker integrados no worktree do chromium-headless (`754dcdd4`, do `dc55d53c`; removi da nota o caminho do home). Número do fork, ainda sem controle meu. A lacuna do M5.4 é grande: pesam seletores inválidos, getComputedStyle e as interfaces XML.- 2026-10-11 21:30: integrados e medidos por mim em container: node:crypto (`0c6a936b`) com o corpus do ul-bun em 220 casos ok em checkout limpo (`cd6e518f` corrige dois defeitos antigos de fixture: files/ vazio e node_modules do tsx-automatic ignorado pelo git); fuso vivo do motor (`92f9ee00`, process_time_zone 6 testes ok); Network de subrecurso do CDP (`c7799d48` no chromium-headless: devtools, conformance, history_devtools, dump_redirect ok). Driver do Playwright no nosso bun parou em `Cannot find module net` (printer segue com node:net). Pendentes: locale padrão do motor, Cookie por redirect, UKM e árvore do Browser.close do CDP.
- 2026-10-11 17:12: suíte completa do ul-chromium medida por mim em container no chromium-headless em `1de9433c` (Cookie por redirect, fuso e locale vivos sobre o Network de subrecurso): 25 binários ok, 183 testes, 0 falhas (o internet levou 28 min). Número do fork para o mesmo estado: CDP 636 de 717, playwright_cookies 45 de 51, playwright_locale 37 de 37 (fork, não revalidado). UKM do --dump-dom integrado (`433d7395`, do `e78c5a4c`), ainda sem a suíte completa neste HEAD. Pendentes: árvore do Browser.close (fork do CDP, clean-s22), Fetch.* e getResponseBody (greenlet), node:net do driver do Playwright (printer).
- 2026-10-11 17:55: árvore da saída do Browser.close integrada no chromium-headless (`d8c51329`, do `14511b6d`); medido por mim em container: close_profile, disk_profile (7), dump_profile (3), dump_redirect, history_devtools (6), sessions (6), sessions_devtools (3), devtools e conformance, todos ok (9 binários, 0 falhas). Fatia A do node:net (Bun.listen e Bun.connect, `8be24a85` no checkout principal): corpus do ul-bun ok, 221 casos. Fork do CDP abriu a frente de iframe por decisão minha (serve a M5.6, M5.3 e M5.4): risco de motor declarado, um global zjsc por run_page, frame exige vários globais no mesmo VM. Fork do printer na fatia B do node:net (net.ts gerado) rumo ao driver do Playwright.

- 2026-10-11 18:35: baseline de desempenho integrado (`d2d77a32`, do `e7d3a0db`; caminhos do home trocados por $HOME no script; regexp_replace_gc_regression ok por mim em debug, sem prova de que falha sem a correção). Fatia B do node:net (`43cf9d2f`, node:net gerado do net.ts): corpus do ul-bun ok por mim, 222 casos. node:tls em andamento no printer (223 casos no relato dele).
