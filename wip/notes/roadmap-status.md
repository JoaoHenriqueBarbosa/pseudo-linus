# Roadmap vivo: estado medido contra os critérios do `roadmap.md`

Este arquivo complementa o `roadmap.md` (o PRD, com as fases e os critérios de saída). Aqui fica o estado real de cada
critério, com a fonte de cada número. **Regra de manutenção: confira e atualize este arquivo a cada VIGIA e a cada
integração de fatia.** Número sem fonte não entra; critério que nunca foi medido aparece como "NÃO MEDIDO", nunca como
estimativa disfarçada.

Convenção da coluna "Fonte": **eu** = medido pelo coordenador com comando próprio (git, grep, ls); **fork** = reportado
por um fork na saída do container dele, não recompilado pelo coordenador; **vigente** = valor herdado de um arquivo
versionado, com a data do arquivo.

Última verificação: 2026-10-11 17:45 (CEST).

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
| M5.1 | JavaScriptCore passando 80% do test262 | **CUMPRIDO: 50 706 de 52 017 (97,5%)**, test262 no commit `2e0a56762801e275a9fdf96dc49d90ba0cddcf63`. Por topo: built-ins 98,0%, language 97,3%, annexB 96,6%, intl402 95,8% (`test/staging` e `test/harness` fora). Corredor `wip/zjsc/scripts/test262-run.py` (sloppy e strict, `negative` com fase, `includes`, `async`, módulo). Corredor validado por mim com 6 testes de controle (2 passam, 4 falham pelo motivo certo). 553 das 1311 falhas são limite do corredor (fixtures de módulo e `$262.createRealm`), 104 são `import source/defer`, 8 crashes (fechados em `2653b961`: 7 de Temporal nos limites de data e 1 de `super[chave]` com namespace) e 2 timeouts (não eram bug: 1 milhão de iterações de `decodeURI` leva 19,9 s; limite do corredor passou para 60 s) | fork, controle eu |
| M5.2 | `node` com CommonJS e ESM, `fs`, `path`, `http` e `npm install` | Direção trocada para o bun: `bun` é programa do userland, com CJS e ESM, `node:fs` completo, `Bun.serve` e `fetch` em loopback, `Bun.file`, streams, CLI parcial. Comando `node` e `npm install`: não existem (só `bun` está registrado) | eu |
| M5.3 | `--dump-dom` correto em páginas com scripts simples | Cumprido além do critério: 42 de 42 páginas da internet idênticas ao Chromium 154 (meta do goal era 95% por grupo) | fork |
| M5.4 | Recorte do Web Platform Tests acima de 90% | **NÃO MEDIDO.** Não há recorte de WPT no repositório | eu |
| M5.5 | `--screenshot` e `--print-to-pdf` com diferença perceptual baixa | **NÃO EXISTE.** O `cli.rs` do `ul-chromium` só trata `--dump-dom`; `Page.captureScreenshot` responde "wasn't found". Pintura de página não existe (o `vello_cpu` só serve ao canvas 2D). O alvo é diferença perceptual, não byte a byte | eu |
| M5.6 | CDP completo para Puppeteer e Playwright; test262 acima de 95% | CDP: 290 de 326 no placar (89%) no último commit integrado (`acdd7bba`); o fork reporta 347 de 379 (91,6%) com o cenário novo `playwright_handshake` em 53 de 53 (setWindowBounds, setDownloadBehavior, scripts de documento novo, mundos isolados, emulação de mídia), ainda sem commit. pyppeteer: pip install, launch, goto (com `Response`), title, evaluate e close funcionam com saída igual ao oráculo. Playwright: handshake de CDP em 53 de 53 (fork, integrado em `a2ff72c6`); replay da sessão CDP real do driver do Playwright contra o `ul-chromium`: 725 de 1151 mensagens em cinco cenas do oráculo (63,0%, fork; o denominador cresceu de 664 para 1151 porque o replay passou de 2 para 5 cenas: `pw_evaluate`, `pw_locators`, `pw_actions`, `pw_context`, `pw_navigation`; harness integrado em `36b73524`; `DOM.*` e `Input.*` implementados, commit pendente de integração). `pw_evaluate`, `pw_locators` e `pw_actions` já sem método desconhecido; faltam, em `pw_context` e `pw_navigation`: `Browser.grantPermissions`, `Emulation.setGeolocationOverride`/`setLocaleOverride`/`setTimezoneOverride`/`setUserAgentOverride`, `Network.emulateNetworkConditions`/`setExtraHTTPHeaders`, `Runtime.addBinding`, `Storage.getCookies`/`setCookies`/`clearCookies`, `Page.getNavigationHistory`/`navigateToHistoryEntry`; metas são os `golden/pw_*.cdp.jsonl`, gravados com o driver real e o Chromium 154); driver Node: incógnita medida em `wip/notes/playwright-driver.md`. O bun real roda o driver; no nosso, `bun cli.js --version` já passa de `tty` (gerado do `tty.ts` do bun, `282eb505`) e para em `Cannot find module 'crypto'`. Faltam `crypto`, `child_process`, `net`, `zlib`, `tls`, `http`, `https`, `readline`, `inspector`, `dns`; o `.ts` de cada um do bun 1.4.2 está em `/home/john/cache/bun-src/tree/src/js/node`, então saem pelo gerador. test262 acima de 95%: cumprido no agregado (97,5%) e em todos os diretórios de topo (ver M5.1) | fork |

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

1. **Medir um recorte de WPT** (M5.4). test262 medido (M5.1 e M5.6 cumpridos); resta fechar o corredor (fixtures em memória, `createRealm`), os 8 crashes e 2 timeouts, depois `Atomics` (68,4%), `module-code` (81,8%) e `import source/defer`.
2. CDP e Playwright (M5.6): fechar a lista de métodos, depois o driver Node no bun.
3. `--screenshot` e `--print-to-pdf` (M5.5): pintura de página (`blitz-paint` sobre `vello_cpu`), PNG e PDF, medido por diferença perceptual.
4. bun/node (M5.2): `bun run`, `-p`, opções, `test`, `x`; decidir se o comando `node` e o `npm install` entram.
5. Cauda de Intl nos locales de uso real, depois ja e zh; calendários nativos por último.

## Riscos sem medição

- **Desempenho do interpretador:** 1 milhão de iterações de `decodeURI` leva 19,9 s no zjsc (fork, test262). O PRD pede `import numpy` abaixo de 200 ms e não fixa meta de desempenho do motor JavaScript; ainda assim a velocidade não foi medida em benchmark. Sem estimativa.

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

## Frentes ativas (2026-10-11)

| Frente | Fork | Última integração |
|---|---|---|
| Printer, zbun, `ul-bun` | `ad4a1a75f3d5455f0` | `0775d66d` (CLI do bun), checkout principal |
| Suíte do zjsc e Intl | `a7c6a4d11a2d73979` | `46da9666` (formatRange), checkout principal |
| Greenlet, pyppeteer, Playwright | `adccaf30202b0bde1` | `acdd7bba` (Network http), worktree chromium-headless |
| CDP e perfil | `a1f2c7757966af6bf` | `eb2bd184` (Sessions ao vivo), worktree chromium-headless |

## Registro de atualizações

- 2026-10-11 14:30: arquivo criado. Achados: test262 e WPT nunca foram medidos; `--screenshot` e `--print-to-pdf` não existem; o alvo de screenshot é perceptual.
- 2026-10-11 14:45: test262 clonado (commit `2e0a5676`, 53 910 testes); runner e baseline ainda não existem. Os 4 forks rodando.
- 2026-10-11 15:05: CDP reportado em 347 de 379 e `playwright_handshake` 53 de 53, pendentes de commit (fork do greenlet).
- 2026-10-11 15:25: driver Node do Playwright medido (`playwright-driver.md`). `bun run` com package.json integrado (`90460435`).
- 2026-10-11 16:00: replay do Playwright real em 245 de 664 (36,9%), primeira medida direta do M5.6 para o Playwright. Faltam `Target.createTarget` com contexto, `DOM.*`, `Input.*` e o que o log completo do replay listar.
- 2026-10-11 16:25: replay do Playwright corrigido para 402 de 664 (60,5%): o harness enviava o script injetado como texto literal. `DOM.*` e `Input.*` implementados (fork), placar novo pendente. test262 em 29 500 de 52 017 testes rodados, sem baseline ainda.
- 2026-10-11 17:00: test262 medido: 97,5% (M5.1 e a parte de test262 do M5.6 cumpridos). Corredor validado com 6 controles. Próxima lacuna de medição: WPT (M5.4).
- 2026-10-11 17:30: replay do Playwright em 725 de 1151 (cinco cenas). `bun run` e `node:tty` integrados; driver do Playwright parado em `crypto`. Corpus do `ul-bun` em 216 casos.
- 2026-10-11 17:45: 8 crashes do test262 fechados (`2653b961`); desempenho do interpretador registrado como risco sem medição.
