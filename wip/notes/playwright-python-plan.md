# Playwright e Puppeteer em Python por dentro do pseudo-linux: plano

Medido em 2026-10-10. Oráculo: imagem `pl-oracle-automation` (`testbench/oracle/Dockerfile.automation`), a
`pl-oracle-chromium` (Debian 13, Chromium 154.0.8037.92, Python 3.13.5) com `python3-pip`, `python3-venv` e dois venvs:
`/opt/playwright` (playwright 1.63.0, greenlet 3.5.6, pyee 13.0.1, typing_extensions 4.16.0, pytest-playwright 0.10.0) e
`/opt/pyppeteer` (pyppeteer 2.0.0, websockets 10.4, pyee 11.1.1, urllib3 1.26.20, tqdm, appdirs, certifi,
importlib_metadata, zipp). Cenas, gravador e goldens em `testbench/oracle/automation/`.

## Objetivo

`pip install playwright` (e `pyppeteer`) dentro do pseudo-linux, em venv, e os scripts de exemplo (as cenas
`cases/*.py`) produzirem a mesma saída que no oráculo, falando CDP com o `chromium` do pseudo-linux
(`crates/ul-chromium`). CDP com paridade completa é a prioridade número um do Chromium (memória
`project-chromium-cdp-priority`).

## O que cada biblioteca exige (medido no oráculo)

### Playwright (Python)

- **Driver Node**: a wheel traz `playwright/driver/node` (ELF de 126 MB, Node v24.21.0, ligado só a libc, libm,
  libdl, libpthread, libstdc++, libgcc_s) e `playwright/driver/package` (o `playwright-core` 1.63.0 em JS: 34 arquivos,
  14 MB, a maior parte em `lib/coreBundle.js` e `lib/utilsBundle.js`). O Python inicia o driver com
  `asyncio.create_subprocess_exec(node, cli.js, "run-driver", stdin=PIPE, stdout=PIPE)` e fala com ele por mensagens JSON
  com prefixo de 4 bytes (`_impl/_transport.py`). Não existe caminho puro Python: `launch` e `connect_over_cdp` passam
  sempre pelo driver. Quem fala CDP com o Chromium é o driver (JS), não o Python.
- **Módulos embutidos do Node que o driver requer** (varredura de `require(...)`, `node-modules-used.sh`, número de
  arquivos): `path` 10, `fs` 7, `os` 7, `net` 5, `child_process` 4, `crypto` 4, `events` 3, `stream` 3, `assert` 2,
  `dns` 2, `http` 2, `http2` 2, `https` 2, `module` 2, `readline` 2, `tls` 2, `url` 2, `util` 2, `zlib` 2, e com 1 cada
  `async_hooks`, `buffer`, `constants`, `inspector`, `process`, `tty`, `vm`. O driver exige `node >= 20`.
- **Lançamento do Chromium**: o driver abre o navegador com `--remote-debugging-pipe` e dois fds extras (3: comandos,
  4: respostas e eventos, mensagens JSON separadas por NUL). Precisa de `child_process.spawn` com `stdio` de cinco
  posições (pipes extras) no lado do Node e do pipe do lado do `ul-chromium` (já existe: `cli.rs`, `server.rs`).
  Com `connect_over_cdp` usa WebSocket em `/devtools/browser/<id>`.
- **greenlet**: `import playwright.async_api` já importa `greenlet` (`_impl/_greenlets.py` e `_page.py`), não só a API
  síncrona. É uma extensão nativa (`greenlet/_greenlet.cpython-313-x86_64-linux-gnu.so`), sem versão em Python puro.
- Demais dependências: `pyee`, `typing_extensions` (Python puro).

### Puppeteer em Python: `pyppeteer` 2.0.0

- Escolha medida: é o único com a API do Puppeteer (`launch`, `newPage`, `$eval`, `waitForSelector`, `setRequestInterception`...).
  A última release é de 2024-02-18 e o projeto não é mantido, mas **funciona contra o Chromium 154**: a cena `pp_basic`
  (47 passos) roda no oráculo sem erro. `pydoll-python` 3.0.0 e `zendriver` 0.17.1 estão mantidos mas têm outra API
  (não medidos, não são Puppeteer).
- Python puro: nenhuma extensão nativa no venv. Usa `websockets` 10.4, `asyncio`, `subprocess`, `urllib3` 1.26,
  `pyee` 11.1.1, `tqdm`, `appdirs`, `certifi`, `importlib_metadata`.
- Lança o `chromium` com `--remote-debugging-port=<N>` e lê o `DevTools listening on ws://...` do stderr; liga com
  `websockets` direto (sem driver Node). Fala o protocolo **antigo, sem `flatten`**: os comandos da página vão em
  `Target.sendMessageToTarget` e voltam em `Target.receivedMessageFromTarget`. O servidor do `ul-chromium` já trata esse
  modo (`server.rs`, sessão sem `flatten`).
- É o **primeiro degrau**: não depende do Bun nem de greenlet.

## O que o pseudo-linux já tem e o que falta

Fatos verificados no repositório (HEAD `ee1b4234`):

- Tem: Python 3.13 em Rust (`crates/ul-python`), `pip` com espelho do PyPI da bancada (`testbench/mirror`,
  `wip/notes/python-pip.md`), `subprocess` sobre `_posixsubprocess.fork_exec` (`wip/notes/python-subprocess.md`),
  `asyncio` e threads, e um modelo de troca simétrica de pilha em `gthread.rs` (`wip/notes/python-threads.md`, "como o
  `greenlet`"). O `ul-chromium` fala `--remote-debugging-port` e `--remote-debugging-pipe`.
- Não tem módulo `greenlet` (nenhuma menção fora de `gthread.rs` e da nota).
- Não tem programa `bun` nem `node` registrado no userland (nenhuma ocorrência em `crates/kernel/src/exec.rs`; o
  zjsc é consumido como biblioteca pelo `ul-chromium`).
- O registro de módulos embutidos do zjsc (`wip/zjsc/src/api/builtin_modules.rs`) só instala `buffer`, `module`, `os`,
  `vm` (e `events`, na rodada 16). Faltam, do que o driver usa: `child_process`, `net`, `http`, `https`, `http2`, `tls`,
  `dns`, `crypto` (a base existe em `runtime/crypto.rs`), `fs`, `stream`, `readline`, `zlib`, `assert`, `async_hooks`,
  `inspector`, `tty`, `url`, `util`, `constants`, `path`, `process` como módulo.
- Hipótese ainda não medida: se o `playwright/driver/node` (ELF do Node) puder ser trocado, no pseudo-linux, pelo
  `bun` do plinus rodando o `cli.js`. O `playwright-core` declara `node >= 20` e não declara suporte a Bun; só a
  medição dirá (depende da frente do Bun).
- Espelho (`testbench/mirror`): acrescentado em 2026-10-10, sem binário no git (o `wheels.lock` guarda só nome e
  sha256, o diretório `wheels/` está no `.gitignore` e `fetch.sh` baixa e confere; conferido: 117 arquivos, sha256 ok).
  Entradas novas, com tamanhos medidos no PyPI real: `playwright-1.63.0-py3-none-manylinux1_x86_64.whl` **48,22 MB**
  (traz o Node de 126 MB e o `playwright-core`; é o único grande), `greenlet-3.5.6-cp313-cp313-manylinux_2_24_x86_64`
  0,62 MB, `pyppeteer-2.0.0` 83 KB, `urllib3-1.26.20` 144 KB (o pyppeteer exige `<2`; a 2.8.0 segue no espelho),
  `pyee-13.0.1` (playwright) e `pyee-11.1.1` (pyppeteer) 16 KB cada, `appdirs-1.4.4` 10 KB e `websockets-10.4.tar.gz`
  85 KB. `playwright` e `pyppeteer` entraram em `packages.txt`; `tqdm`, `certifi`, `importlib_metadata`, `zipp` e
  `typing_extensions` já estavam.
  **`websockets` 10.4 só existe como sdist para cp313**: o PyPI real só tem wheels cp37 a cp311 e o `.tar.gz`, e o
  `pip install pyppeteer` no oráculo constrói a partir dele (`setup.py`, extensão C opcional que cai no Python puro).
  O servidor do espelho (`crates/mirror`) já indexa `.tar.gz`, mas o `pip` do pseudo-linux precisa construir uma sdist
  (isolamento de build, `setuptools` e `wheel` do espelho, e a falha do compilador C virando o fallback em Python puro).
  Isto é um trabalho do `pip` (degrau 3) a medir, não do espelho.

## CDP: o que o servidor do ul-chromium ainda não trata

`python3 -I testbench/oracle/automation/cdp-methods.py` compara os métodos que cada cena enviou ao Chromium real com os
nomes tratados em `src/devtools/*.rs` e `prelude/*.js`. Em 2026-10-10, sobre as 11 cenas: **52 métodos faltam**:

- **Handshake do Playwright** (toda cena): `Network.enable`, `Log.enable`, `Page.addScriptToEvaluateOnNewDocument`,
  `Page.createIsolatedWorld`, `Page.setFontFamilies`, `Browser.setWindowBounds`, `Browser.setDownloadBehavior`,
  `Emulation.setDeviceMetricsOverride`, `Emulation.setEmulatedMedia`, `Emulation.setFocusEmulationEnabled`.
- **Ações**: `Input.dispatchMouseEvent`, `Input.dispatchKeyEvent`, `Input.insertText`, `DOM.getContentQuads`,
  `DOM.getBoxModel`, `DOM.scrollIntoViewIfNeeded`, `DOM.describeNode`, `DOM.resolveNode`.
- **Histórico, captura e impressão**: `Page.getNavigationHistory`, `Page.navigateToHistoryEntry`, `Page.captureScreenshot`,
  `Page.getLayoutMetrics`, `Page.printToPDF`, `IO.read`, `IO.close`, `Emulation.setDefaultBackgroundColorOverride`,
  `Page.handleJavaScriptDialog`.
- **Rede e `route`**: `Fetch.enable`, `Fetch.disable`, `Fetch.continueRequest`, `Fetch.fulfillRequest`, `Fetch.failRequest`,
  `Network.setCacheDisabled`, `Network.setExtraHTTPHeaders`, `Network.getResponseBody`, `Network.emulateNetworkConditions`,
  `Network.getCookies`, `Network.setCookies`, `Network.deleteCookies`, `Network.setUserAgentOverride`,
  `Storage.getCookies`, `Storage.setCookies`, `Storage.clearCookies`.
- **Contexto**: `Emulation.setGeolocationOverride`, `setLocaleOverride`, `setTimezoneOverride`, `setUserAgentOverride`,
  `setTouchEmulationEnabled`, `Browser.grantPermissions`, `Runtime.addBinding`, `Performance.enable`, `Security.enable`.

Observações medidas no oráculo que o servidor precisa reproduzir:

- Com o `--enable-automation` que o Playwright passa, `navigator.webdriver` vale `true` e `navigator.plugins.length` vale 5.
- O Playwright lança o Chromium em modo `--headless` com janela de 1280x720 por padrão; `set_viewport_size` muda
  `innerWidth` e `innerHeight`; `new_context(device_scale_factor=2)` muda `devicePixelRatio`.
- `page.goto("http://127.0.0.1:1/")` falha com `net::ERR_UNSAFE_PORT`: a lista de portas inseguras do Chromium faz parte
  do comportamento visível.
- `/json/list` com um navegador novo traz dois alvos `browser_ui` (os popups do omnibox) mais as páginas.

## Contrato do `greenlet` (medido no oráculo, greenlet 3.5.6, CPython 3.13.5)

Frente própria do `ul-python`, sem dono hoje: um módulo nativo `greenlet` (e `greenlet._greenlet`, o que o pacote do PyPI
importa) sobre o `crates/ul-python/src/gthread.rs`. Cena e golden: `cases/gl_api.py` e `golden/gl_api.out`
(`testbench/oracle/automation/run.sh gl_api`); o critério de pronto é a saída idêntica, linha a linha, mais a suíte
`greenlet/tests` do pacote que o `pip` instala.

- **Superfície do módulo** (nomes públicos, na ordem de `sorted`): `CLOCKS_PER_SEC`, `GREENLET_USE_CONTEXT_VARS`,
  `GREENLET_USE_GC`, `GREENLET_USE_TRACING`, `GreenletExit`, `enable_optional_cleanup`, `error`,
  `get_clocks_used_doing_optional_cleanup`, `getcurrent`, `gettrace`, `greenlet`, `settrace`; `__version__ == "3.5.6"`.
  `greenlet.greenlet` tem `GreenletExit`, `dead`, `error`, `getcurrent`, `gettrace`, `gr_context`, `gr_frame`, `parent`,
  `run`, `settrace`, `switch`, `throw`; base `object`. `GreenletExit` herda de `BaseException`; `greenlet.error` de `Exception`.
- **`switch`**: `g.switch(*args, **kwargs)` entrega os argumentos ao `run` na primeira vez e, nas seguintes, devolve ao
  ponto suspenso: um argumento vira o valor, vários viram `tuple`, nenhum vira `()`; só com `kwargs` vira `dict`.
  Quando o `run` termina, o valor de retorno vai ao pai como retorno do `switch`; trocar para um greenlet morto troca
  direto para o pai e devolve o valor (`g.switch()` retorna `[]`, que é `()`).
- **`getcurrent`**: devolve o greenlet atual; o principal tem `parent is None`. `g.parent` começa no criador e aceita
  `parent=` no construtor.
- **`parent`**: `ValueError("cyclic parent chain")` ao formar ciclo (inclusive `g.parent = g`);
  `AttributeError("cannot set the parent of a main greenlet")`;
  `TypeError("GreenletChecker: Expected any type of greenlet, not int")` (também no construtor com `parent=5`).
- **`dead`, `bool(g)`, `gr_frame`, `gr_context`, `run`**: `bool(g)` é verdadeiro só com o greenlet iniciado e não
  terminado (falso antes de iniciar e depois de morto); `gr_frame` é `None` antes de iniciar, o quadro suspenso depois;
  `gr_context` é `None` sem `contextvars` ativas; `g.run` existe antes de iniciar e depois de iniciado
  `g.run = 1` levanta `AttributeError("run cannot be set after the start of the greenlet")`; sem `run`, `switch`
  levanta `AttributeError("run")`; `run` não chamável: `TypeError("'int' object is not callable")` ao `switch`.
  Subclasse que define `run` como método é chamada com os argumentos do primeiro `switch` (o padrão do Playwright).
- **`throw`**: `g.throw(exc)` num greenlet suspenso levanta `exc` no ponto suspenso e devolve o retorno do `run`; num
  greenlet não iniciado levanta `exc` no chamador e o greenlet fica morto; `g.throw()` sem argumentos lança
  `GreenletExit` e o `switch` pai recebe a **instância** `GreenletExit`; `getcurrent().throw(ValueError(...))` levanta
  no próprio chamador. Exceção não tratada no `run` propaga ao pai como se o `switch` a tivesse levantado
  (`KeyError('k')`, `ZeroDivisionError`).
- **`settrace` e `gettrace`**: o gancho recebe `(evento, (origem, destino))` com `evento` em `"switch"` e `"throw"`;
  a sequência medida para `switch` ao filho, retorno ao principal, `throw(GreenletExit)` e retorno é
  `switch, switch, throw, switch`; `settrace` devolve o gancho anterior.
- **Threads**: trocar para um greenlet criado em outra thread (que já terminou) levanta
  `greenlet.error("cannot switch to a different thread (which happens to have exited)")`.
- **Coleta**: soltar a última referência a um greenlet suspenso (`del g; gc.collect()`) lança `GreenletExit` nele, e os
  `finally` do corpo rodam.
- **Quem usa assim**: `playwright/_impl/_greenlets.py` (`MainGreenlet`, `RouteGreenlet`, `LocatorHandlerGreenlet`,
  `EventGreenlet` herdam de `greenlet.greenlet` e usam `greenlet.settrace`), `_impl/_sync_base.py` e `_disposable.py`
  (`getcurrent`, `switch`, `parent`), `sync_api/_context_manager.py` (greenlet despachante). Os padrões estão em
  `cases/gl_api.py` (`dispatcher`).

## Degraus, do menor para o maior (cada um com o critério medido)

1. **pyppeteer sobre o CDP do `ul-chromium`, no host** (sem Python do plinus): rodar `cases/pp_basic.py` com o Python do
   host (venv com `uv`) contra um `ul-chromium` de teste, e comparar a saída com `golden/pp_basic.out`. Critério: 47 de
   47 passos iguais. Depende só do CDP (a lista de 18 métodos que o `cdp-methods.py pp_basic` mostra).
2. **Playwright sobre o CDP do `ul-chromium`, no host**: o mesmo com o driver Node do host e `connect_over_cdp`
   (e `launch` com o `ul-chromium` como `executable_path`, pelo pipe). Critério: as 10 cenas `pw_*` iguais aos `.out`
   do oráculo. Depende dos 52 métodos acima.
3. **pyppeteer por dentro do pseudo-linux**: `pip install pyppeteer` no espelho, `python` do plinus, `chromium` do plinus.
   Critério: `pp_basic` igual ao golden. Depende do degrau 1 e de o `ul-python` rodar `websockets` 10.4 e `asyncio` com
   `subprocess` e sockets (a verificar).
4. **Módulo `greenlet` nativo no `ul-python`**, sobre o `gthread.rs` (mesma semântica de `switch`, `getcurrent`, `parent`,
   `throw`, `settrace`, `GreenletExit`, `dead`, `run`). Critério: `import greenlet` e a suíte pública do greenlet 3.5.6
   (os testes de `greenlet/tests`) iguais ao oráculo. Pré-requisito de `import playwright`, até na API assíncrona.
5. **Frente do Bun**: `bun` e `node` como programas do pseudo-linux sobre o zjsc, com os módulos `child_process`, `net`,
   `fs`, `stream`, `http(s)`, `tls`, `zlib`, `crypto`, `readline`, `async_hooks` etc. da lista acima. Critério: o
   `playwright/driver/package/cli.js` roda `run-driver` e responde ao `initialize` do cliente Python (hipótese a medir).
   O driver ELF `playwright/driver/node` que o `pip` instala: no pseudo-linux a chamada tem de cair no `node` do plinus
   (Node v24 no oráculo; `node --version` e o comportamento têm de bater, REGRA SUPREMA).
6. **Playwright por dentro do pseudo-linux**: `pip install playwright`, cena `pw_navigation`, depois as demais.
   Depende dos degraus 2, 4 e 5.
7. **`Bun.WebView`** (fork futuro): o `Bun` usando o `ul-chromium` por CDP. Fora deste plano.

## O que depende de quem

- **Fork do CDP** (`crates/ul-chromium/src/devtools`, `prelude/65-devtools.js`): os 52 métodos acima, na ordem do
  handshake. É o caminho crítico dos degraus 1, 2, 3 e 6.
- **Frente do Bun / zjsc**: degrau 5.
- **`ul-python`**: degrau 4 (`greenlet`) e a verificação do degrau 3.
- **Esta frente**: oráculo, cenas, goldens, `cdp-methods.py`, plano; a seguir um executor das cenas contra o
  `ul-chromium` (degrau 1).

## Como reproduzir

```sh
docker build -f testbench/oracle/Dockerfile.automation -t pl-oracle-automation testbench/oracle   # uma vez
testbench/oracle/automation/run.sh [cena...]          # grava golden/<cena>.out e golden/<cena>.cdp.jsonl
python3 -I testbench/oracle/automation/cdp-methods.py [cena...]   # métodos que o ul-chromium ainda não trata
```

`<cena>.cdp.jsonl`: uma mensagem por linha (`"out"` do cliente ao navegador, `"in"` o contrário), strings acima de 2 KB
trocadas por `<sha256:HEX len=N>` com o texto em `golden/blobs/`. `pw_connect` não tem transcrição (liga por WebSocket,
que o `cdp_tee` não intercepta).

## Backlog do `greenlet` (a suíte oficial do 3.5.6 no sandbox)

O teste `greenlet_test_suite_fails_only_where_declared` (`crates/host/tests/greenlet.rs`) roda a suíte inteira e exige
que os testes que falham sejam exatamente os de `DECLARED_DIVERGENCES`, cada um com a causa. Estado de 2026-10-11:
156 testes executados (os módulos `test_cpp` e `test_extension_interface` nem importam, o oráculo roda 172), 20 divergências declaradas.

1. **Coletor de ciclos (projeto à parte).** O plinus não acha ciclos de instâncias: `gc.collect()`, `__del__` e
   `weakref` de objetos em ciclo só rodam na saída do interpretador. Atinge `test_circular_greenlet`,
   `test_inactive_ref`, `test_threaded_leak`, `test_threaded_adv_leak`, `test_issue251_killing_cross_thread_leaks_list`.
   Desenho: registrar todos os contêineres (instâncias, listas, dicts, tuplas, cells, frames); a coleta faz a
   eliminação por tentativa (_trial deletion_: subtrai das contagens as referências internas ao conjunto, usando o
   grafo de `gc.get_referents`, e o que sobra com contagem zero é lixo); finaliza o lixo (`__del__`, na ordem de
   criação) e quebra o ciclo limpando os contêineres, para o `Rc` cair.
2. **Trashcan.** Os 30 subtestes de `TestTrashCanReEnter` (a destruição de uma cadeia profunda de greenlets) passam no
   estado atual; ficam no backlog só para reabrir se voltarem a falhar.
3. **"Expected to leak but it did not".** `test_finalizer_crash`, `test_permutations`, `test_recursive_startup`,
   `test_throw_to_dead_thread_doesnt_crash` e os três `test_issue251_issue252_*` esperam um vazamento que o CPython
   tem (referência ao greenlet preso numa pilha de C) e o plinus não tem. Divergência declarada: o plinus libera mais.
4. **`smaps_rollup`**: feito (`/proc/<pid>/smaps_rollup`).
5. **Switch reentrante e morte entre threads**: feito (`settrace` do lado de quem chega, `__del__` como quadro do laço,
   `deleteme` por thread, principal de thread encerrada, pai perverso).
6. **Extensão C `_test_extension` (.so).** `test_cpp`, `test_extension_interface`, `test_get_stack_with_nested_c_calls`
   e os dois `test_*_at_module_gc_time` importam o `.so` de teste do greenlet, que o sandbox não executa. Declarado.
7. **Desempenho do `switch`.** `_save_stacks` varre todos os greenlets suspensos da thread a cada troca: criar e trocar
   para N greenlets é O(N^2) (2000 greenlets: 31 s; no CPython, milissegundos). Os dois
   `test_untracked_memory_doesnt_increase_unfinished_thread_dealloc_*` (20 mil greenlets) estouram os 30 s por isso.
8. **`gr_context` dentro de `Context.run`** (`test_context_propagated_by_context_run`): devolve o Context em vez de
   `None`; causa ainda não analisada.
