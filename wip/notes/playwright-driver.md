# Driver Node do Playwright no plinus: a incógnita medida

Medido em 2026-10-11, wheel `playwright-1.63.0-py3-none-manylinux1_x86_64` do PyPI (extraída em diretório isolado,
fora do repositório). Fonte de cada fato indicada.

## Como o Python lança o driver (lido em `playwright/_impl/_driver.py` e `_transport.py`)

- `compute_driver_executable()` devolve `(PLAYWRIGHT_NODEJS_PATH ou driver/node, driver/package/cli.js)`.
- O transporte faz `asyncio.create_subprocess_exec(node, cli.js, "run-driver", stdin=PIPE, stdout=PIPE)` com o ambiente
  `PW_LANG_NAME=python`, e fala o protocolo do Playwright: mensagens JSON com prefixo de 4 bytes little-endian.
- **Consequência:** o `driver/node` da wheel é um ELF de 126 MB que o plinus nunca executa (regra do projeto: nada de
  código de máquina de wheel). Basta `PLAYWRIGHT_NODEJS_PATH=/usr/local/bin/bun`; o `bun` aceita os mesmos argumentos
  (`bun cli.js run-driver`). Nenhuma alteração na wheel.

## Linha de base: o bun real roda o driver (oráculo bun 1.4.2)

- `bun /pw/package/cli.js --version` imprime `Version 1.63.0`, código 0.
- `bun /pw/package/cli.js run-driver`, com `initialize` enviado no protocolo de 4 bytes, responde com o
  `__create__` do `BrowserType` (guid, `name: chromium`, `executablePath`). O driver funciona sob bun; o que falta é
  só do nosso bun.

## O que o driver pede do runtime (contagem de `require` em `lib/` e `cli.js`)

Módulos `node:` importados: fs 95, path 88, os 40, events 35, util 27, stream 26, child_process 22, net 20, crypto 14,
process 13, zlib 9, fs/promises 8, url 7, tls 6, http 5, buffer 5, assert 5, https 4, readline 3, tty 2, module 2,
inspector 2, dns 2, vm 1, stream/promises 1, constants 1, async_hooks 1. Mais dependências embutidas (ajv, ws,
enquirer; `bufferutil` é opcional).

Registrados hoje no zjsc (`BUILTINS` em `src/api/builtin_modules.rs`): assert, assert/strict, async_hooks, buffer,
events, fs, fs/promises, module, os, path, path/posix, querystring, stream, stream/promises, string_decoder, timers,
timers/promises, url, util, util/types, vm.

**Faltam:** `child_process`, `net`, `crypto`, `zlib`, `tls`, `http`, `https`, `readline`, `tty`, `inspector`, `dns`,
`constants` e o `process` como módulo (o global existe). `net` e `child_process` são o caminho crítico para o
`initialize` e para lançar o navegador; `crypto` e `zlib` aparecem no caminho de carga; `tls`, `http` e `https`
entram só para conexão por rede e download de navegador, não para o `run-driver` com navegador local.

## Lado do navegador

O Playwright lança o Chromium com `--remote-debugging-pipe` (CDP em fd 3 e 4, mensagens separadas por NUL; também
`--remote-debugging-port=0` em alguns caminhos). O `ul-chromium` já aceita `--remote-debugging-pipe` (`cli.rs`,
`devtools/server.rs`). Esse lado não é incógnita; o `child_process.spawn` do nosso bun precisa suportar `stdio` com
fds extras (`stdio: ['pipe','pipe','pipe','pipe','pipe']`).

## Plano de fechamento (ordem)

1. Rodar `cli.js --version` no bun do plinus e listar o primeiro erro; iterar até imprimir `Version 1.63.0`.
2. `run-driver` + `initialize` no protocolo de 4 bytes, igual ao oráculo.
3. `child_process` (spawn com stdio de fds extras, exec, kill, eventos de saída), `net` (já há TCP no `Host`),
   `crypto` (hash, random, o que o driver usa), `zlib`, `readline`, `tty`, `dns`, `inspector`, `constants`.
4. Playwright Python completo: `chromium.launch()`, `new_page()`, `goto`, `evaluate`, `close`, contra o `ul-chromium`.
5. Comparar a saída do script com a do oráculo (Playwright Python sobre o Chromium 154).
