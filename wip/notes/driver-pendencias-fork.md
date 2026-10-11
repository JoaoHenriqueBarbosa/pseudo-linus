# Pendências do driver do Playwright (fork agent-ad4a1a75, 2026-10-11)

Nada em andamento não commitado. Tudo abaixo é o que falta, na ordem do caminho crítico.

1. `bun /pw/package/cli.js --version`: último erro medido `Cannot find module 'readline'` (coreBundle.js). Depois dele, ainda
   não medidos: `inspector`, `tty` já existe; rodar `PW_ARGS="--version" sh /tmp/zbun-pr/pw.sh` e seguir o erro seguinte até
   `Version 1.63.0`. Fonte: `src/js/node/readline.js` do bun 1.4.2, via `scripts/gen-node-modules.js` (módulo novo na tabela).
2. `run-driver` + `initialize` (protocolo de 4 bytes LE + JSON) e o ciclo Playwright Python: não começados.
3. `node:module`: faltam `createRequire`, `isBuiltin`, `Module._load`, `_resolveFilename`, `_cache`. Caso
   `module-module-createrequire` em `KNOWN` (corpus.rs).
4. `node:zlib` motor de fluxo: `NativeZlib`, `NativeBrotli`, `NativeZstd` lançam not implemented no `new`; caso
   `module-zlib-engine` em `KNOWN` com bytes medidos. Reusar `crates/ul-archive` (gzip, zstd, deflate) e o zdeflate/brotli
   que o `CompressionStream` do zjsc já usa; fatias: deflate/inflate/gzip, depois brotli e zstd.
5. `node:http`: HTTPParser real, servidor sobre `Bun.serve` (`onNodeHTTPRequest`), TLS (SecureContext e handshake) seguem
   lançando not implemented. `http2`: `H2FrameParser` e upgrade TLS idem.
6. Spawn: `Bun.spawn` sem `resourceUsage` real, sem `ipc`/`send`, sem `terminal`, stdin de `ReadableStream` só por bombeamento
   simples; erro de executável relativo com `/` não medido.
7. `Bun.dns`: sem resolução por rede; `reverse`/`lookupService` só o caso medido.
8. Depois do driver: `process_stdio.rs` (remover `process-stdout-shape` de KNOWN), `-p/--print`, opções de execução, `test`
   e `x`, fatia pbkdf2/scrypt/hkdf/argon2/cifras/KeyObject do crypto.

Divergência declarada: mensagem do `TypeError` de `ChildProcess.connected` (PLAN.md, seção child_process e module).
