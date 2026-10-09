# Lacuna do objeto global `Bun` e dos módulos `node:` no porte zjsc

Medido em 2026-10-09: `Object.getOwnPropertyNames(Bun)` no bun 1.4.2 (112 nomes) e grep em `wip/zjsc/src`.
Nada compilado nem rodado. O PLAN.md já lista `Bun`, `Buffer`, `process` e os módulos `node:` como "ainda ausentes"
no global; `require("node:X")` hoje só resolve `node:vm` e `fs` de teste (`src/api/eval.rs`, `RUN_IN_THIS_CONTEXT_BOOT`).

Peso: P (até ~300 linhas), M (300 a 1500), G (acima, ou subsistema inteiro).

## Bun: o que o porte já tem de base

Base existente em `src/runtime`: `util_inspect.rs` (2429), `node_buffer.rs` (966), `event_emitter_core.rs` (121),
`crypto.rs` (3041, com ec/rsa/kdf/pq), `fetch.rs` (403), `file.rs` (101), `blob.rs`, `process_*.rs`, `timers.rs`,
`url.rs`, `zstd/` (só descompressão), `compression_streams.rs` (gzip só no cabeçalho), `worker_host.rs`, `module_fs.rs` (45, trait).
Não existe nenhum módulo chamado `bun*` nem objeto `Bun`.

## Bun: propriedade por propriedade

Equivalente parcial existe:
- `inspect` : `util_inspect.rs`. Falta ligar ao objeto e as opções de `Bun.inspect` (M, só ligação: P).
- `env`, `argv`, `cwd`, `version`, `revision`, `main`, `isMainThread` : `process_env/process_shape/process_object`. P.
- `stdin`, `stdout`, `stderr` : `process_stdio.rs` cobre o `process.*`; o `BunFile` de `Bun.stdout` falta. P.
- `file`, `write` : `file.rs`/`blob.rs` (File de web). `BunFile` com `text/json/arrayBuffer/exists/stream` e `Bun.write` precisam do `ModuleFs` com VFS real. M.
- `fetch` : `fetch.rs`. Rede real falta (sandbox: provavelmente sem rede). M já feito em parte.
- `CryptoHasher`, `hash`, `MD4`, `MD5`, `SHA1`, `SHA224`, `SHA256`, `SHA384`, `SHA512`, `SHA512_256`, `sha` : `crypto.rs` e `wtf/sha1.rs` têm digests; falta confirmar md4/md5/sha512_256 e a classe. M.
- `randomUUIDv5`, `randomUUIDv7` : base em `crypto.rs` (`randomUUID`). P.
- `fileURLToPath`, `pathToFileURL`, `resolveSync`, `resolve`, `origin` : `wtf/url.rs::file_url_path`, `module_probe.rs`. P.
- `sleep`, `sleepSync`, `nanoseconds`, `gc`, `peek`, `shrink` : `timers.rs`/`performance.rs`/GC do VM. P cada.
- `zstdCompress*`/`zstdDecompress*` : só descompressão em `zstd/`. Compressão M, descompressão P.
- `readableStreamTo*` (9) : streams em andamento (`install_streams`). P cada, depois dos streams.
- `ArrayBufferSink`, `concatArrayBuffers`, `allocUnsafe`, `mmap`, `unsafe`, `embeddedFiles`, `isStandaloneExecutable` : base de ArrayBuffer. P cada.

Sem nada no porte:
- P: `escapeHTML`, `stripANSI`, `stringWidth` (a lógica de largura existe em `ul-common::width`, não no zjsc), `sliceAnsi`, `wrapAnsi`, `color`, `enableANSIColors`, `deepEquals`, `deepMatch`, `indexOfLine`, `semver`, `which`, `openInEditor`, `generateHeapSnapshot`, `registerMacro`, `jest`, `revision`, `version_with_sha`, `password` (argon2/bcrypt: M), `CSRF`, `Cookie`, `CookieMap`, `JSON5`, `JSONC`, `JSONL`, `TOML`, `YAML`, `XML`, `markdown`.
- M: `Glob`, `Transpiler` (o bun transpila TS/JSX; o porte tem parser mas não emissor), `build`, `plugin`, `deflateSync`/`inflateSync`/`gzipSync`/`gunzipSync` (inflate+deflate+crc, ~800 linhas), `Archive`, `Image`, `secrets`, `dns`.
- G: `$` (shell do bun, precisa de parser de shell e de processos), `spawn`/`spawnSync` (precisa de processos do SO simulado), `serve`/`listen`/`connect`/`udpSocket` (rede), `FileSystemRouter`, `SQL`/`sql`/`postgres`, `RedisClient`/`redis`, `S3Client`/`s3`, `FFI`, `cron`, `Terminal`, `WebView`.

## Módulos `node:` (propriedades próprias no bun 1.4.2)

| módulo | props no bun | no porte | peso |
|---|---|---|---|
| `node:util` | 45 | `util_inspect.rs` (inspect, format via `console_format.rs`); faltam `promisify`, `callbackify`, `types`, `inherits`, `deprecate`, `parseArgs`, `styleText`, `TextEncoder/Decoder` (os dois como globais), `isDeepStrictEqual` | M |
| `node:path` | 17 | nada (posix puro; `ul-common::fsutil` não é do zjsc) | P |
| `node:os` | 23 | nada; precisa dos valores do sandbox (`process_system.rs` ajuda) | P |
| `node:events` | 20 | `event_emitter_core.rs` (121); falta `EventEmitter` completo (`once`, `on`, `captureRejections`, `errorMonitor`, `getEventListeners`) | M |
| `node:buffer` | 14 | `node_buffer.rs` (966), `base64_globals.rs`; falta `Buffer` global e o módulo, `transcode`, `Blob`/`File` ligados | M |
| `node:fs` | 108 | nada (só `readFileSync` de teste); precisa do VFS do sandbox: sync (`readFileSync`, `writeFileSync`, `readdirSync`, `statSync`, `mkdirSync`, `existsSync`, `rmSync`...), `fs/promises`, callbacks, `Stats`, `Dirent`, streams, `watch` | G |
| `node:crypto` | 72 | `crypto.rs` (3041): digests, kdf, ec, rsa, pq e `crypto.subtle`; falta `createHash`/`createHmac`/`randomBytes`/`randomUUID` como módulo, `createCipheriv`, `scrypt`, `timingSafeEqual` | M (hash/hmac/random: P) |
| `node:child_process` | 8 | nada; precisa de processos no sandbox | G |

Pré-requisito comum a todos: um registro `node:` no `require` e no `FsModuleHost` (PLAN.md: "`node:` no `FsModuleHost`" pendente), o global `process` completo e o global `Buffer`.

## Ordem recomendada de porte (o que um agente de IA mais usa em scripts)

1. Registro de módulos `node:` no `require`/`import` (P), mais `process` e `Buffer` como globais. Destrava tudo abaixo.
2. `node:path` (P): puro, usado em quase todo script.
3. `node:fs` sync + `fs/promises` + `Bun.file`/`Bun.write` (G, mas é o item mais usado; fatiar em: leitura/escrita, readdir/stat/mkdir/rm, `Stats`/`Dirent`, streams). Depende do `ModuleFs` sobre o VFS.
4. `node:util` (`inspect`, `format`, `promisify`, `types`, `parseArgs`, `isDeepStrictEqual`) e `Bun.inspect` (M, base pronta).
5. `node:os` (P) e `Bun.env/argv/cwd/version/main` ligados ao objeto `Bun` (P).
6. `node:events` completo (M, base pronta).
7. `node:buffer` módulo + `node:crypto` (`createHash`, `createHmac`, `randomBytes`, `randomUUID`, `Bun.CryptoHasher`, `Bun.hash`) (M).
8. `Bun.sleep`, `Bun.sleepSync`, `Bun.nanoseconds`, `Bun.deepEquals`, `Bun.escapeHTML`, `Bun.stripANSI`, `Bun.stringWidth`, `Bun.which`, `Bun.semver`, `Bun.pathToFileURL`/`fileURLToPath` (todos P, lote único).
9. `Bun.spawn`/`spawnSync`, `$` e `node:child_process` (G): agentes usam muito, mas só fazem sentido quando o sandbox tiver processos; ficam depois de fs.
10. `Bun.fetch`/`serve` com rede, `Bun.gzipSync` e companhia (M), `Bun.YAML/TOML/JSON5/JSONL` (P cada), `Glob` (M).
11. Por último, sem prioridade de agente: `SQL`/`sql`/`postgres`, `Redis`, `S3`, `FFI`, `Transpiler`/`build`/`plugin`, `WebView`, `Terminal`, `cron`, `Image`, `Archive`, `FileSystemRouter`, `udpSocket`/`listen`/`connect`.
