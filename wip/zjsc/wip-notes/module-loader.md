# Carregador de módulos: estado real e plano em fatias (2026-10-09)

## Correção da triagem

A afirmação "não há carregador, `import()` nem `import` estático ligado ao interpretador" é falsa. Está tudo
ligado, só que a porta de entrada é `api::module`, não `api::eval`:

- `src/runtime/js_module_loader.rs` (676 linhas): `JSModuleLoader` + `ModuleRegistry` (mapa chave/tipo para
  registro), trait `ModuleHost` (`resolve`, `fetch`, `import_meta_url`, `source_type`, agora `is_main_module`),
  `install_module_loader`, `load_and_evaluate_module`, `request_import_module` (o `requestImportModule`),
  `global_func_import_module` (o `importModule`, alvo do `ImportNode`), `create_import_meta_properties`, passos
  assíncronos do `DynamicImport*` em `js_microtask.rs`, `import defer`, JSON/Text modules, atributos `with`.
- `src/api/module.rs`: `evaluate_module(source, specifier, host)` (usado nos testes unitários) e
  `evaluate_module_map(files, entry)` (usado pelos três goldens). Hosts: `MemoryModuleHost` (resolução do Bun
  sobre mapa em memória) e `FileUrlHost` (`import.meta.url = file:///chave`).
- Goldens: `module_bun.tsv` (445), `module_edge_bun.tsv` (600), `module_more_bun.tsv` (1699), com os testes
  `tests/module_*_bun_golden.rs`. Cada caso é um mapa de arquivos num realm novo; erros de `BuildMessage`,
  `ResolveMessage` e `AggregateError` (camada do Bun) só comparam o `log`.
- `import()` dentro de módulo chega ao carregador (referrer = chave do módulo, via `caller_source_origin`).

## O que falta (comparado ao upstream e ao Bun)

1. **Script (não módulo) sem host**: `evaluate_script` e os `evaluate_*_script_*` não instalam host, só o
   `importModule` (via `init_link_time_constants`). `import('./x.js')` num script rejeita com
   `TypeError: No module loader is installed`, mensagem que não existe em Debian/Bun e denuncia o sandbox. O
   Bun rejeita com `ResolveMessage: Cannot find module './x.js' imported from <caminho>` (código
   `ERR_MODULE_NOT_FOUND`), `Cannot find package 'x' imported from ...`, e `No such built-in module: node:x`
   (`ERR_UNKNOWN_BUILTIN_MODULE`).
2. **`import.meta` só tinha `url`**, enumerável. No Bun existem `url`, `path`, `filename`, `dirname`, `dir`,
   `file`, `main`, `env`, `require`, `resolve`, `resolveSync`; nenhuma aparece em `Object.keys`,
   `getOwnPropertyNames`, `getOwnPropertyDescriptor` (undefined) nem `JSON.stringify` (`{}`), mas todas
   respondem a `in` e a leitura (objeto exótico com slots virtuais).
3. **Sem host de verdade**: só há host em memória. Falta o host do sandbox (VFS do pseudo-linus): resolução com
   `node_modules`, `package.json` (`exports`, `main`, `type`), extensões implícitas, `index.js`, `file:` URLs.
4. **Builtins `node:`**: `import fs from 'node:fs'` etc. não têm módulo sintético (existe `synthetic_module_record`
   para JSON/Text; falta registrar builtins como módulos com `default` + exports nomeados).
5. **CommonJS interop**: `import cjs from './x.cjs'`, `require` de ESM, `module.exports` como `default`.
6. **`import.meta.hot`/`require` no escopo do módulo, `Bun.plugin`, loaders de TypeScript/JSX**: fora do JSC,
   camada do Bun; só o que o agente de IA usa entra.
7. Ordem de macrotask do carregamento de arquivo (`schedule_module_load`) já existe para o laço de eventos.

## Fatias

1. **(feita) `import.meta` do Bun**: `path`, `filename`, `dirname`, `dir`, `file` (derivados de `file://`) e
   `main` (novo `ModuleHost::is_main_module`), todos não enumeráveis, `url` também não enumerável. Teste:
   `api::module::tests::import_meta_main_is_true_only_for_the_entry_and_the_extras_stay_hidden`. Pendência
   conhecida: `getOwnPropertyNames`/`getOwnPropertyDescriptor` ainda enxergam as propriedades (no Bun, vazio).
2. **(feita, sem compilar) `import.meta.resolve(spec)` e `resolveSync`**: nativas (comprimento 0, não enumeráveis)
   em `create_import_meta_properties`, referrer pelo `caller_source_origin`; novo método
   `ModuleHost::import_meta_resolve(specifier, referrer, sync)` (padrão: `resolve` + `import_meta_url`);
   `FileUrlHost` sobrescreve (medido no bun 1.4.2: `resolve` devolve URL sem conferir existência nem sondar
   extensão, `node:`/`file:` passam direto, `.` e vazio lançam, `./` mantém a barra final; `resolveSync` devolve
   caminho e lança se não achar). Golden: `scripts/gen-module-meta-resolve-golden.js` ->
   `tests/golden/module_meta_resolve_bun.tsv` (67 casos), teste `import_meta_resolve_matches_bun` em
   `tests/module_edge_bun_golden.rs` (agora com `check_golden` compartilhado). Pendências medidas e fora do golden:
   `resolveSync` do Bun sonda extensão (`./a` acha `a.mjs`) e aceita `?query`; `resolve` de bare `fs` devolve
   `node:fs` e de pacote instalável resolve no cache, ambos dependem do host do sandbox (fatias 4 e 5).
   Medido e portado depois (sem compilar; golden agora com 212 casos):
   - `this`: `resolve` exige o próprio `import.meta` (chamada solta, `{resolve: r}.resolve()`, `.call({})`,
     `.call(1)` e `Object.create(import.meta)` lançam `TypeError: import.meta.resolve must be bound to an
     import.meta object`; `.bind(import.meta)` e `.call(import.meta)` valem). `resolveSync` só recusa `this` que
     não é célula (`null`, número; mensagem `import.meta.resolveSync must be bound ...`); aceita `{}` e string.
     Porte: registro `IMPORT_META_OBJECTS` em `js_module_loader.rs`. `resolveSync.call(undefined)` lança no bun, e
     a chamada solta `r()` só vale quando `r` é variável fora de registrador (capturada ou de módulo): o
     `FunctionCallResolveNode` entrega o objeto de escopo como `this`, que é célula. Esse `this` (e `{}`,
     `Object.create(import.meta)`) não tem chamador: a falha cita `imported from undefined`, e `this` string
     cita a string (absoluta, resolve contra ela). O porte recusa `undefined` e trata o resto assim; fecha no
     golden (a chamada solta depende de o porte emitir o `this` de escopo, conferir no primeiro build).
     Argumentos são convertidos depois da checagem de `this`.
   - Segundo argumento de `resolve` (parent): só vale se for string; outro tipo (`undefined`, `null`, número,
     objeto, mesmo com `toString` que lança) é ignorado e o chamador é a origem. A string perde o `file://`, vale
     como caminho enraizado em `/` (nunca no diretório de trabalho), só o diretório conta (`./s/` e `s/x.mjs`
     dão `/s/`; `./s` e `""` dão `/`) e `.`/`..` são resolvidos. Porte: `parent_referrer`. `resolveSync` só usa
     parent string absoluta (relativa é ignorada), e a mensagem de falha cita a string como veio (argumento não
     string cita `undefined`; parent `""` dá `... from ''`). Barras duplas: o bun não colapsa
     (`http://h/p/q.mjs` dá `file:///http://h/p/b.mjs`, `..` consome também o segmento vazio, `.`/`..` no fim dão
     a barra final); porte: `join_relative(.., keep_empty = true)` em `api/module.rs`. Especificador `//x`
     (medido e portado, sem compilar; golden com 499 casos): `resolve("//x/p")` lê `x` como host e dá `file://x/p`
     (o resto normalizado como absoluto, `..` nunca consome o host: `//x/../y` dá `file://x/y`, `//` dá `file:///`,
     `///x` dá `file:///x`, `////x` dá `file:////x`, `//x//y` mantém `//`, `.`/`..` no fim dão a barra final);
     `resolveSync("//...")` devolve o texto como veio, sem conferir existência (inclusive `//` e `///`), com ou sem
     parent. `\x`, `\\x`, `C:\x` são caminho (`Cannot find module '\x'`), `a\b` e `x\` são pacote; porte em
     `unresolved_specifier_message` e `MemoryModuleHost::resolve`. `import('//x')` no bun rejeita com `BuildMessage`
     (`ENOENT reading "//x"`; só barras, como `//` e `///`, dá `EISDIR reading "//"`), não `ResolveMessage`: porte
     (sem compilar) em `build_message_text`/`throw_build_message`, nos dois caminhos de falha do `import()` (sem host
     e `resolve` do host). A `BuildMessage` é nativa e global como a `ResolveMessage`; as duas dividem tudo em
     `js_module_loader.rs` (`MessageState` com `kind`, nativos genéricos `message_*::<KIND>`, `MessageClassSpec`,
     macro `message_getters!`, `install_message_classes`). Diferenças medidas da `BuildMessage`: protótipo só com
     `column`, `level`, `line`, `message` (com setter), `notes` (`[]`), `position`, `toJSON` (4 campos), `toString`;
     sem `code`, `specifier`, `referrer`, `stack`; `e.stack = x` cria propriedade própria. Pendência: com host de
     arquivos real, `//x` deve consultar o host (existe: `EISDIR` se for diretório); hoje só a raiz é `EISDIR`.
   - `resolveSync` falha com `ResolveMessage` (mensagem, `code`, `specifier`, `referrer` do bun) pelo mesmo
     `throw_resolve_message` do `import()`; `resolveSync("")` é `TypeError` `ERR_INVALID_ARG_VALUE`; `.` e `..`
     são "Cannot find module".
3. **(feita, sem compilar) `import()` em script sem host**: rejeita com `ResolveMessage` (`Cannot find module './x.js'
   imported from /main.js`, `Cannot find package 'x' imported from ...`, `No such built-in module: node:x`, `code`
   `ERR_MODULE_NOT_FOUND`/`ERR_UNKNOWN_BUILTIN_MODULE`), classe global `ResolveMessage` instalada no init do
   global (`install_message_classes`). Falha de `resolve` de qualquer host usa o mesmo caminho, e o
   `MemoryModuleHost` passou a dizer `imported from`. Golden `import_no_host_bun.tsv` (gerador
   `scripts/gen-import-no-host-golden.js`, teste `tests/import_no_host_bun_golden.rs`, referrer neutro `/main.js`).
   Pendências: sob `node:vm` o
   bun rejeita com `TypeError [ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING]: A dynamic import callback was not specified.`
   (o porte não tem `node:vm`); `import('https://...')` é `BuildMessage ENOENT reading "<url>"` no bun, não coberto;
   `import('fs')` sem prefixo resolve no bun e aqui cai em `Cannot find package`.
4. `ModuleHost` do sandbox sobre o VFS (resolução de caminho, `index`, extensões, `package.json`).
   - **(feita, sem compilar) peça 1: sonda pura** em `src/api/module_probe.rs` (`probe(path, trailing_slash,
     exists)`, 3 testes em memória). Medido no bun 1.4.2: ordem implícita `tsx, jsx, mts, ts, mjs, js, cts, cjs,
     json`, igual para `index.*`; arquivo exato vence; arquivo vence diretório; `./c/` vai direto ao index;
     `./b.js`/`.jsx` sem o exato tenta `ts, tsx, mts` (nunca `jsx`/`mjs`/`cts`), `.mjs` só `mts`, `.cjs` só `cts`;
     `./b.ts` com só `b.js` falha. O agente anterior não deixou nada em `src/api/` (sem sonda prévia).
   - **(feita, sem compilar) peça 2: `probe_directory(dir, read)`** em `module_probe.rs` (4 testes em memória).
     Medido no bun 1.4.2 (`import m from "./d"`, `package.json` do diretório):
     - `main` é sondado com ordem PRÓPRIA, diferente da implícita: `js, cjs, cts, tsx, ts, jsx, json`; `mjs` e
       `mts` nunca entram na sonda (`main:"lib/x"` com só `x.mjs` cai no `index` da raiz), só por nome exato ou
       pela reescrita `.mjs` para `.mts` (`main:"lib/x.mjs"` acha `x.mts`). O `index.*` do diretório apontado por
       `main` usa a mesma ordem curta. Arquivo vence diretório (`main:"lib"` com `lib.js` e `lib/index.js` dá
       `lib.js`). `main` com barra final (`lib/`) pula extensões e vai ao `index`, mas arquivo exato ainda vale
       (`lib/x.js/` acha `x.js`; `lib/x/` com só `x.js` não acha).
     - Cai no `index.*` do próprio diretório (ordem implícita, `index.mjs` vale) quando `main` é ausente, `""`,
       não string, `null`, inexistente, `.`, `./`, ou com espaços (não faz trim), ou diretório sem `index`.
       Sem `main` e sem `index`: `Cannot find module`.
     - `main` aceita `./lib/x.js`, caminho absoluto e `../out.js` (sai do diretório).
     - JSON inválido (`{main:`), vazio, array, chave `Main` (maiúscula): sem erro, vale como sem `main`, tanto em
       `import` estático quanto em `import()` e `require`. BOM, comentários e vírgula final são aceitos. Chave
       `main` repetida: o resultado bateu com "vale a primeira" (medido com a primeira inexistente).
     - `exports` e `module` são ignorados em import relativo (`exports` + `main` usa `main`; só `exports` cai no
       `index`). `package.json` dentro do diretório de `main` não é consultado.
     - Pendente: chaves sem aspas/aspas simples no `package.json` não medidas; `type` ainda não entra (afeta
       formato do `.js`, não a resolução).
   - Falta: trait de leitura de arquivos (`exists`/`is_dir`/`read`) sobre o VFS, o host que chama `probe`
     (tratar `?query` antes), `package.json` (`main`, `exports`, `type`), `node_modules` subindo diretórios,
     `file:` URLs, e conferir `resolveSync` com query. Medir ainda: `.json`/diretório sem index vs `package.json`.
5. Módulos sintéticos de `node:*` (`fs`, `path`, `os`, `process`, `url`, `child_process`) com `default` + nomeados.
6. Interop CJS (`default` = `module.exports`, nomeados por análise estática) e `require` em ESM.
7. `import.meta.env`/`require`, e as propriedades virtuais exatas (descritor undefined) via objeto exótico.

## Verificação pendente

Nada compilou nem rodou nesta fatia (regra: sem cargo). Conferir `cargo test --test module_more_bun_golden` e
`--lib api::module` quando houver build: `put_direct` com `DONT_ENUM` em `create_import_meta_properties` e o
closure `define` (empresta `object` e `vm`).
