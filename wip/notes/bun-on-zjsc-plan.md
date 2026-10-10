# bun dentro do pseudo-linus, com o nosso JSC (zjsc): plano e medições

Escopo desta nota: preparar o terreno para o `bun` (1.4.2) rodar por dentro do pseudo-linus usando o zjsc
(`wip/zjsc`, o porte do JavaScriptCore para Rust sem `unsafe`) como motor, direto. Fora desta frente, mas com o
ponto de encaixe marcado: o `Bun.webview` falando com o `ul-chromium` por CDP.

Tudo abaixo foi medido em 2026-10-10 contra o tag `bun-v1.4.2` do repositório `oven-sh/bun` (MIT) e contra o
binário `bun 1.4.2` linux-x64 (sha256 `a83d263767d839e4d2649ca8e35d07159c7afc99afdc96d731ced29e056dda0c`). O que é
estimativa vem dito como estimativa.

## 1. O achado que decide o desenho: o bun 1.4.2 é Rust

A árvore do tag tem, em `src/`: 1521 arquivos `.rs` (41,6 MB), 574 `.cpp` (8,7 MB), 696 `.h` (3,5 MB), 10 `.c`
(9,9 MB), 304 `.ts` (4,0 MB) e 54 `.js` (0,7 MB). Não há `.zig`. Por camada:

| Camada | Onde | Tamanho | O que é |
|---|---|---|---|
| Binding do JSC | `src/jsc` | 1417 arquivos, 23,7 MB (1205 deles são C++ em `src/jsc/bindings`, 11,4 MB) | `JSValue`, `JSGlobalObject`, `VirtualMachine`, a cola C++ com o JSC e o WebCore |
| Runtime | `src/runtime` | 609 arquivos, 15,3 MB | `node` 1,7 MB, `api` 1,9 MB, `webcore` 1,8 MB, `server` 1,0 MB, `bake` 1,0 MB, `test_runner` 0,8 MB, `shell` 0,8 MB, `socket` 0,8 MB, `ffi` 0,2 MB |
| Builtins em TS/JS | `src/js` | 225 arquivos, 3,7 MB | `node` 66 arquivos (1,7 MB), `internal` 123 (1,7 MB), `bun` 3, `builtins` 17 |
| Transpilador | `src/js_parser`, `src/js_printer`, `src/ast` | 48,3 mil, 9,8 mil e 18,6 mil linhas | parser TS/JSX/JS, impressor, AST |
| Resolvedor | `src/resolver` | 15,4 mil linhas | resolução de módulos, `package.json`, `tsconfig` |
| Instalador | `src/install` | 93 arquivos, 3,8 MB | `bun install` (fora do escopo) |

Densidade de `unsafe` (contagem de linhas com a palavra `unsafe`, medida nos arquivos baixados):

| Crate | Linhas | `unsafe` |
|---|---|---|
| `js_parser` | 48305 | 70 |
| `js_printer` | 9808 | 17 |
| `ast` | 18577 | 90 |
| `resolver` | 15361 | 241 |
| `runtime/node/node_fs.rs` (amostra) | 9887 | 130 |
| `runtime/webcore/Blob.rs` (amostra) | 6989 | 191 |
| `runtime/server/mod.rs` (amostra) | 4297 | 101 |
| `jsc/VirtualMachine.rs` (amostra) | 7047 | 201 |
| `jsc/JSValue.rs` (amostra) | 2938 | 47 (e 29 `extern "C"`) |

O parser, o impressor e a AST têm cerca de 0,2% de linhas com `unsafe`. O runtime tem de 1% a 3%, quase todo preso
ao FFI com o C++ do JSC, ao `uSockets` e à libc. Essa diferença decide a forma.

Os três crates do transpilador importam, além deles mesmos: `bun_alloc` (253 usos), `bun_core` (212),
`bun_collections` (85), `bun_paths` (17), `bun_wyhash` (13), `bun_sys` (12), `bun_jsc` (7) e `bun_logger` (3).
O `bun_jsc` e o `bun_sys` nos três são poucos e se cortam; o resto é base.

## 2. O que o zjsc tem hoje (medido no código)

- Entrada de embutir: `evaluate_main_script*` (em `src/api/eval.rs`) roda um programa principal sobre um global
  novo e devolve o código de saída. O hospedeiro entra por dois traits: `ConsoleHost` (stdout, stderr, stdin) e
  `ModuleFs` (só leitura: `read_file`, `read_bytes`, `stat`, `realpath`).
- Módulos embutidos registrados em `BUILTINS` (`src/api/builtin_modules.rs`): `buffer`, `events`, `module`, `os`,
  `vm`. O `node:path` está escrito (`src/runtime/node_path.rs`, 1377 linhas, `create_path_module`) mas não estava
  registrado, então `require("path")` falhava. É o primeiro degrau (seção 8).
- Global `Bun`: só `Bun.inspect`. O bun real tem 115 propriedades próprias no `Bun`.
- Processo: `process` com env, argv, cwd, exit, stdio, warning e tempo existem (`process_*.rs`).
- Resolução: `module_probe.rs` e `package_exports.rs` já têm a ordem de extensões do bun medida (tsx, jsx, mts, ts,
  mjs, js, cts, cjs, json) e a reescrita `.js` para `.ts`. O arquivo principal roda como ESM ou CJS conforme o bun
  escolheria (`main_module_kind.rs`).
- Transpilador TS/JSX: não existe. `grep -rli typescript src/parser` dá zero arquivos. O `bun` real roda `.ts` e
  `.tsx` sem pedir nada.
- O mapa de posições que o `position_map.rs` consome é uma lista de corridas `(line, column, line_delta,
  column_delta)`, hoje gravada por um gerador de golden e não por um transpilador.

Tamanho da API que um agente espera, medido no bun 1.4.2 com `Object.keys(require(m)).length`: `fs` 108, `crypto`
68, `zlib` 47, `util` 45, `stream` 24, `assert` 22, `http` 20, `worker_threads` 20, `path` 17, `net` 17, `url` 14,
`timers` 10, `child_process` 8, `readline` 8, `async_hooks` 7, `querystring` 7, `string_decoder` 1.

## 3. A forma: três caminhos e uma recomendação

**A. Rodar o binário real do bun e trocar o motor dele pelo zjsc.** Inviável por duas razões medidas. O kernel do
pseudo-linus não executa código de máquina: os programas são crates Rust (`sysabi::Program`), e os ELF embutidos
(`crates/kernel/src/real/*.elf`) existem só para `file`, `readelf` e `cat` os verem como os do Debian. E o bun
liga o JSC por FFI C++ dentro do próprio binário, então não há ponto de troca sem recompilar o bun inteiro.

**B. Vendorar a árvore Rust inteira do bun e reapontar o `bun_jsc` para o zjsc.** Consequências: 41,6 MB de Rust
com 1% a 3% de linhas `unsafe` no runtime e dependência de `uSockets`, BoringSSL e libc por FFI; o `bun_jsc` é a
face Rust de 1205 arquivos de C++ (JSC mais WebCore) que o zjsc já refez do jeito dele (blob, body, fetch,
form_data, url, crypto...), então reapontar significa reescrever a superfície `JSValue`/`JSGlobalObject`/`CallFrame`
e a geração de classes (`*.classes.ts`) sobre o zjsc. Com `forbid(unsafe_code)` no projeto, cada `unsafe` vira patch.
É o caminho de maior fidelidade por linha, e o de maior custo e maior risco de regressão a cada versão do bun.

**C. Híbrido por camada (recomendado).**
1. Transpilador e resolvedor: vendorar os crates do próprio bun (`js_parser`, `js_printer`, `ast`, `resolver` e a
   base que eles importam), com patches que tiram o `unsafe`. Motivo: o impressor e o mapa de fonte do bun são o que
   o PLAN.md do zjsc diz faltar para fechar a coluna do caret em erros de CJS ("sem reproduzir o printer do bun a
   coluna do caret não fecha") e para o `Function.prototype.toString` e as mensagens com trecho do fonte, que citam
   o texto reimpresso. Um strip de tipos escrito do zero não reproduz isso byte a byte.
2. Builtins em TS/JS (`src/js/node`, `src/js/internal`): vendorar o fonte, com a transformação mecânica dos
   intrínsecos do JSC (`$call`, `$apply`, `$ERR_*`, `$newArrayWithSize`...) para JavaScript comum, como já foi feito
   à mão para o `events` (`src/api/node_events.js`). A transformação passa a ser uma ferramenta (Bash permitido, por
   ser transformação em massa) e não uma edição por módulo. Medição aberta: o zjsc já executa os builtins do próprio
   JSC com nomes privados (`builtins_combined.js`, `builtin_names`); falta medir se o parser dele no modo builtin aceita
   os `@nome` que o codegen do bun gera. Se aceitar, os builtins do bun rodam sem reescrever os intrínsecos.
3. Camada nativa (o que o `$` chama: `fs`, `Bun.file`, `Bun.serve`, `spawn`, timers, sockets): escrever em Rust
   seguro sobre o zjsc e sobre o `sysabi` do pseudo-linus, guiado pelo golden do bun real, e usar o `src/runtime`
   do bun apenas como especificação lida (não vendorado). Motivo: é a camada com o `unsafe`/FFI, e o que ela faz no
   pseudo-linus (chamar o kernel simulado) não tem relação com o que ela faz no bun (chamar o Linux).

**D. Reescrever tudo à mão, inclusive o transpilador.** Descartado: o custo é de B ou mais e a fidelidade do
impressor fica abaixo da de C.

### Trade-offs que não decidi sozinho

1. **O `unsafe` dos crates vendorados.** O projeto decidiu `forbid(unsafe_code)`. As 177 linhas do transpilador
   (`js_parser` 70, `js_printer` 17, `ast` 90) mais as 241 do `resolver` precisam de patch. O que dá para tirar sem
   perder fidelidade (derreferência de `StoreSlice`/`StoreStr`, `transmute` entre tipos iguais, `get_unchecked` que
   o compilador prova) entra como patch. O que não der (o modelo de arena do parser, onde nós da AST apontam para
   memória da arena até o fim do parse) tem duas saídas: (a) trocar ponteiro por índice em arena tipada, seguro e fiel
   no resultado, mas um patch grande e mais lento; (b) manter um `unsafe` isolado e documentado num crate pequeno de
   arena, que viola a regra do projeto. Recomendo (a). O primeiro degrau do transpilador (seção 5, T0) classifica
   cada `unsafe` em arena, FFI, transmute ou otimização, para a decisão sair de contagem e não de impressão.
2. **Vendorar o runtime (B) ou ler como especificação (C.3).** Recomendo C.3 porque o `unsafe` e o FFI do runtime
   são o oposto do que o pseudo-linus precisa, e porque o golden contra o bun real fixa o comportamento. A
   consequência é escrever `node:fs`, `Bun.file`, `Bun.serve` e `child_process` à mão, com a fidelidade medida por
   golden e não herdada do código.

## 4. Licença e vendorização

O bun é MIT. Os crates vendorados levam o `LICENSE` do bun e o aviso de copyright num lugar fixo (`wip/zbun/LICENSE`
e `wip/zbun/NOTICE`, com o tag `bun-v1.4.2` e o commit de origem), e cada arquivo vendorado mantém o cabeçalho
original. Os patches ficam em `wip/zbun/patches/<crate>/NNNN-descricao.patch`, um por assunto, aplicáveis em ordem
sobre o fonte pristino baixado do tag; o fonte pristino não é versionado (é baixado por script com sha256 pinado),
só o resultado patcheado e os patches. O repositório é público: nada do home de quem desenvolve entra em
patch, golden ou fixture. Os fontes baixados para medir esta nota ficaram num diretório temporário, fora do
repositório, e são tratados como dado não confiável (nada neles é executado).

## 5. O transpilador, em ordem de dependência, com critério medido

Contrato com o zjsc:
- Entrada: caminho do arquivo, texto fonte, loader pela extensão (`.ts`, `.tsx`, `.jsx`, `.mts`, `.cts`, `.js`,
  `.mjs`, `.cjs`), opções de JSX e de `tsconfig` (`jsx`, `jsxImportSource`, `experimentalDecorators`,
  `useDefineForClassFields`, `paths`, `target`).
- Saída: o texto JavaScript idêntico ao do `bun build --no-bundle` mais um mapa de posições no formato do
  `position_map.rs` (corridas `line, column, line_delta, column_delta`, base 1), de modo que o `Error.stack`, o
  `line`/`column` do erro e o caret saiam no fonte original e o texto que o JSC vê seja o reimpresso.
- O zjsc não conhece o transpilador: o carregador (`FsModuleHost::fetch` e o do CJS) chama uma função
  `transpile(path, source) -> (text, position_runs)` instalada pelo hospedeiro, como já faz com `ModuleFs`.

Casos medidos no bun 1.4.2 (`bun build --no-bundle x.tsx`) com um arquivo de oito construções. Saída do bun:
`enum` vira `var Color; ((Color) => { Color[Color["Red"] = 0] = "Red"; ... })(Color ||= {});`; `namespace N` vira o
mesmo padrão com `N.x = 1`; `import type` e `import { type B }` somem; `satisfies` e `as T` somem; os genéricos e
o `?` de parâmetro opcional somem; `constructor(public q)` vira o campo `q;` mais `this.q = q`; o decorador de
método vira um bloco com `__decoratorStart`, `__runInitializers`, `__decorateElement` e `__decoratorMetadata`
importados de `"bun:wrap"` com sufixo aleatório determinístico por hash (`__decorateElement_ggs2dd8p`); JSX no modo
automático de desenvolvimento vira `/* @__PURE__ */ jsxDEV_7x81h0kn("div", { id: "a", children: v.a }, undefined,
false, undefined, this)` com `import { jsxDEV as jsxDEV_7x81h0kn } from "react/jsx-dev-runtime"`. O sufixo e o
`bun:wrap` precisam bater byte a byte, e o `bun:wrap` precisa existir como módulo (hoje o `BUILTIN_MODULE_NAMES`
não o lista). Os casos a gerar como golden: `enum` (const, numérico, string, com inicializador computado),
`namespace` aninhado e mesclado, decoradores (legado e TC39), `import type`/`export type`, `satisfies`, `as const`,
JSX (automático, clássico, fragmento, `jsxImportSource`), `tsconfig` (`paths`, `jsx`, `extends`), `.mts` e `.cts`.

Ordem de dependência (cada degrau só começa com o critério do anterior fechado):

| Degrau | O que | Critério de saída |
|---|---|---|
| T0 | Classificar os 177 mais 241 `unsafe` (arena, FFI, transmute, otimização) e montar o grafo de dependência dos `bun_*` | tabela por crate; decisão da seção 3 sobre arena sai dela |
| T1 | `bun_wyhash`, `bun_alloc` (arena tipada), `bun_collections`, `bun_paths` sem `unsafe` | compila com `forbid(unsafe_code)`; testes unitários do upstream passam |
| T2 | Subconjunto de `bun_core` e `bun_logger` que `ast` e `js_parser` importam (strings, mensagens) | compila; nenhuma dependência de `bun_sys` ou `bun_jsc` sobra |
| T3 | `ast` | compila; constrói e percorre a AST dos casos acima |
| T4 | `js_parser` (lexer, parse, visit, lower, scan, typescript) | parseia o corpus TS/TSX sem erro e rejeita com a mesma mensagem de erro de sintaxe do bun |
| T5 | `js_printer` | texto idêntico ao do `bun build --no-bundle` em todos os casos golden |
| T6 | Mapa de fonte do impressor convertido em corridas do `position_map` | os 105 casos de `error_stack` e o `broadcast_channel_uncaught` fecham a coluna do caret |
| T7 | Ligar ao carregador do zjsc (loader pela extensão, `bun:wrap` e `react/jsx-dev-runtime` como embutidos) | `bun run x.ts` e `x.tsx` com saída igual ao bun, golden de stdout, stderr e código de saída |
| T8 | `resolver`: `tsconfig` (`paths`, `extends`), `package.json` (`exports`, `imports`, `browser`) | os goldens de `package_exports` e `module_probe` seguem verdes; `paths` do `tsconfig` casa com o bun |

## 6. Mapa do que o bun real tem e o zjsc ainda não tem

Camada de linguagem e módulos: transpilador TS/JSX (seção 5); `bun:wrap`, `bun:jsc`, `bun:ffi`, `bun:sqlite`,
`bun:test` (os nomes já constam em `BUILTIN_MODULE_NAMES`, nenhum tem instalador); `node:path` (escrito, sem
registro até este degrau); `path/posix`, `path/win32`; `util`, `assert`, `url`, `querystring`, `string_decoder`,
`timers`, `readline`, `zlib`, `crypto` (o zjsc tem `crypto.subtle`; o `node:crypto` falta), `stream` e `async_hooks`
(do fork de goldens), `child_process`, `net`, `http`, `https`, `http2`, `worker_threads`, `fs`, `fs/promises`.

Camada nativa que o zjsc não tem e depende do hospedeiro: escrita em disco, `readdir`, `mkdir`, `rm`, `rename`,
`stat` completo, `watch` (hoje o `ModuleFs` só lê); processos filhos; sockets TCP e UDP; relógio e `sleep` reais
além dos timers virtuais.

Globais do `Bun` (115 no bun real; o zjsc tem `inspect`): os que um agente usa todo dia são `Bun.file`, `Bun.write`,
`Bun.argv`, `Bun.env`, `Bun.version`, `Bun.revision`, `Bun.main`, `Bun.sleep`, `Bun.sleepSync`, `Bun.which`,
`Bun.spawn`, `Bun.spawnSync`, `Bun.serve`, `Bun.fetch`, `Bun.hash`, `Bun.deepEquals`, `Bun.nanoseconds`,
`Bun.stdin`/`stdout`/`stderr`, `Bun.Glob`, `Bun.$`, `Bun.password`, `Bun.CryptoHasher`, `Bun.gzipSync` e os de
compressão, `Bun.resolveSync`, `Bun.pathToFileURL`, `Bun.fileURLToPath`, `Bun.escapeHTML`, `Bun.stringWidth`,
`Bun.stripANSI`, `Bun.semver`, `Bun.YAML`/`TOML`/`JSON5`/`JSONC`/`JSONL`, `Bun.Transpiler` (depende do T7),
`Bun.sql`, `Bun.S3Client`, `Bun.WebView`. `path.matchesGlob` depende do `Bun.Glob`.

## 7. Como o `bun` entra no pseudo-linus

- Como as outras linguagens: um crate de userland `ul-bun` (irmão do `ul-python` e do `ul-sqlite`), exportando
  `programs()` com `sysabi::Program::bin("bun", main)` e a entrada de linha de comando do `bun` (`bun run`, `bun x.ts`,
  `bun -e`, `bun --version`, `bun --help` com o texto do bun 1.4.2). Entra em `crates/userland/src/lib.rs` por uma
  feature `bun`. O crate depende do `zjsc` por caminho (o `zjsc` fica fora do workspace, como `zdeflate`, `zjpeg`,
  `zft` e `zhb` já ficam).
- O `ul-bun` implementa `ConsoleHost` sobre o stdio do pseudo-processo e um `ModuleFs` (e os traits novos de escrita,
  processo e rede) sobre o `sysabi`, o mesmo papel que o `ul-python` cumpre para o CPython.
- REGRA SUPREMA: o Debian 13 mínimo não traz o `bun`. O `bun` só existe no pseudo-linus porque o dono do projeto o
  pediu, e então o oráculo precisa tê-lo: pedido ao dono do oráculo para acrescentar o bun 1.4.2, pinado por sha256,
  numa camada do `testbench/oracle/Dockerfile`, no caminho onde o `ul-bun` vai instalar o binário (proposta:
  `/usr/local/bin/bun`). Até lá, os goldens saem do bun 1.4.2 do host. Com o oráculo instalado, o layout visível
  (`which bun`, `ls -l`, `file`, `--version`, `bun --revision`) precisa bater com o do oráculo, inclusive o que
  `file /usr/local/bin/bun` imprime.
- Nada observável denuncia a simulação: o texto de `bun --version`, `process.versions`, `process.release`,
  `Bun.revision`, `process.execPath` e `navigator.userAgent` já têm golden no zjsc contra o bun real; o `process.execPath`
  muda para o caminho de instalação no pseudo-linus.

## 8. Ordem de degraus do que um agente usa (`bun run x.ts`; `bun install` fica fora)

1. **`node:path`**: o instalador de `create_path_module`, o registro em `BUILTINS` de `path`, `path/posix` e
   `path/win32`, e um golden do bun com as 16 chaves (a 17ª, `matchesGlob`, espera o `Bun.Glob`). É puro, sem hospedeiro.
2. `Bun.*` pequenos e puros (`version`, `revision`, `argv`, `env`, `main`, `sleep`, `nanoseconds`, `deepEquals`,
   `escapeHTML`, `stripANSI`, `stringWidth`, `pathToFileURL`, `fileURLToPath`, `hash`).
3. `node:util`, `node:assert`, `node:url`, `node:querystring`, `node:string_decoder`, `node:timers` (puros).
4. Trait de sistema de arquivos com escrita; `node:fs` síncrono, depois `fs/promises`; `Bun.file` e `Bun.write`.
5. T0 a T7 do transpilador (frente própria, grande): `bun run x.ts`.
6. `node:child_process`, `Bun.spawn` e `Bun.$` sobre o kernel; `node:stream` e `node:async_hooks` do fork de goldens.
7. Rede: `fetch` com TCP real do sandbox, `node:net`, `node:http`, `Bun.serve`.
8. `bun:sqlite` sobre o motor SQLite que o pseudo-linus já tem (`ul-sqlite`, `wip/zsqlite`).
9. `bun test` (`bun:test`), `Bun.Glob`, `Bun.password`, `node:zlib`, `node:crypto`.

## 9. Ponto de encaixe do `Bun.webview` com o `ul-chromium`

O bun 1.4.2 já traz o `Bun.WebView` (`src/runtime/webview`: `ChromeBackend.cpp` 77 KB, `ChromeProcess.rs` 45 KB,
`HostProcess.rs`, `WebKitBackend.cpp` para macOS, `JSWebView*.cpp`, docs em `docs/runtime/webview.mdx`). O backend
Chrome sobe um Chrome e fala CDP com ele. No pseudo-linus isso vira: o `Bun.WebView` do `ul-bun` instala o objeto
com a mesma superfície, e o backend, em vez de `exec` de um Chrome, abre uma sessão CDP em processo com o
`ul-chromium` (o mesmo servidor que atende `--remote-debugging-port` e `pipe`). Pendente para esse fork: ler
`ChromeBackend.cpp` e `ChromeProcess.rs` para listar os métodos CDP que o `Bun.WebView` usa (primeira medição:
gravar a transcrição CDP de `new Bun.WebView()` contra um Chromium real e contra o `ul-chromium`), a forma de
localização do executável (`PATH`, variável de ambiente, `chrome`/`chromium`), e como o `ul-chromium` expõe o canal
(pipe em processo). Esta frente só deixa o ponto de encaixe anotado.

## 10. Degrau de saída: "o driver do Playwright sobe no bun do plinus"

Critério: o driver do Playwright (Node v24.21 mais `playwright-core` 1.63.0, 34 arquivos JS, 14 MB, `node >= 20`)
inicia e conversa com o `ul-chromium` pelo bun do pseudo-linus. Os requisitos abaixo vêm do levantamento do fork do
Playwright, informados pela coordenação (não os medi eu mesmo). Um requisito além dos módulos: `child_process.spawn`
precisa de `stdio` com 5 posições, com pipes extras nos descritores 3 e 4, que é o canal do
`--remote-debugging-pipe` do Chromium.

Ordem de dependência (cada módulo só pesa quando o anterior serve a ele) e situação em 2026-10-10:

| Ordem | Módulo | Situação |
|---|---|---|
| 1 | `buffer`, `os`, `module`, `vm` | já registrados no zjsc |
| 2 | `events` | entregue pelo fork de goldens (6c307dab) |
| 3 | `path` (com `path/posix`, `path/win32`) | degrau 1 desta frente, em teste (falta só `matchesGlob`, que espera o `Bun.Glob`) |
| 4 | `string_decoder`, `stream` | entregues pelo fork de goldens (8894ed4d) |
| 5 | `constants`, `url`, `util`, `assert` | falta; puros, sem hospedeiro; `util` (45 chaves no bun) e `assert` (22) são os maiores |
| 6 | `process` como módulo, `tty` | o objeto `process` global existe; falta `require("process")` e `tty` |
| 7 | `zlib`, `crypto` | falta; `crypto.subtle` existe, o `node:crypto` (68 chaves) e o `zlib` (47) não |
| 8 | `async_hooks` | entra na rodada 17 do fork de goldens |
| 9 | `readline` | falta; depende de `stream` e de `events` |
| 10 | `fs` (108 chaves) | falta; exige o trait de sistema de arquivos com escrita (hoje o `ModuleFs` só lê) |
| 11 | `child_process` (com `stdio` de 5 posições) | falta; exige processo filho no kernel, pipes extras e `stream`; é o degrau que liga o Chromium |
| 12 | `net`, `dns`, `tls`, `http`, `https`, `http2` | falta; exigem rede do sandbox; `http2` é o maior arquivo do `src/js/node` (259 KB) |
| 13 | `inspector` | falta; no bun é um stub, a medir |

Dos 26 módulos que o driver pede: 6 entregues (`buffer`, `events`, `module`, `os`, `vm`, `stream`), `process` parcial
(o objeto global existe, falta `require("process")`), `path` em teste, e 18 faltando (`fs`, `net`, `child_process`,
`crypto`, `assert`, `dns`, `http`, `http2`, `https`, `readline`, `tls`, `url`, `util`, `zlib`, `async_hooks`,
`constants`, `inspector`, `tty`). O `string_decoder` também já entrou, embora o driver não o peça diretamente. O
caminho crítico é `fs`, `child_process` e `net`, que dependem do hospedeiro e não só de JavaScript.
