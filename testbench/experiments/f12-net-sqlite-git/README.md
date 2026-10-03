# F12: rede, sqlite e git sobre o FS do sandbox

Experimento das hipóteses H34 (rede), H35 (sqlite) e H36 (git). Resultado completo em
`testbench/results/f12-net-sqlite-git.json`; corpus em `corpus/cases/{sqlite,git}` e golden em
`golden/{sqlite,git}`.

```sh
cd testbench
cargo run -q -p oracle -- gen --tool sqlite
cargo run -q -p oracle -- gen --tool git
cargo run --release --manifest-path experiments/f12-net-sqlite-git/Cargo.toml   # ~3 min, grava o JSON
cargo test --release --manifest-path experiments/f12-net-sqlite-git/Cargo.toml
```

## Hipóteses

| Id | Frase | Critério |
|---|---|---|
| H34 | curl/wget sobre ureq aplicam a allowlist num ponto só, sem tokio | servidor local: permitido passa, negado não conecta, DNS pra IP privado bloqueado, redirect pra negado bloqueado; árvore sem tokio |
| H35 | rusqlite permite registrar VFS próprio e o banco mora no sandbox | compara rusqlite serialize, rusqlite + sqlite-plugin e turso: API segura, corpus SQL contra sqlite3, interoperabilidade de arquivo nos dois sentidos |
| H36 | gix aceita customizar o acesso ao repositório | gix alto nível sobre FS em memória; protótipo com gix-* de baixo nível validado por `git fsck` no oráculo |

## Método

Nada de unsafe no nosso código (`unsafe_code = "forbid"`; o depscan do próprio crate dá 0). Tudo roda
sobre o `harness::MemTree` ou sobre um FS em memória nosso; o disco do host só aparece onde a medição
é justamente "isso exige disco".

**Rede.** Um objeto `Policy` (allowlist exata ou `*.sufixo`, IP literal só se escrito, bloqueio de
faixas internas: loopback, RFC 1918, link-local, CGNAT, multicast, IPv4 mapeado em IPv6, ULA) é usado
por dois ganchos do `ureq` 3.4.2 (fixado, porque são do módulo `unversioned`):

- `PolicyResolver` (trait `Resolver`): nega o nome antes de qualquer DNS, resolve pela tabela de hosts
  do sandbox ou pelo `DefaultResolver`, e descarta endereços internos quando a política manda. O ureq
  chama o resolvedor a cada salto de redirect.
- `PolicyGate` (trait `Connector`, primeiro da cadeia `().chain(PolicyGate).chain(TcpConnector).chain(RustlsConnector)`):
  confere host e endereços já resolvidos imediatamente antes do TCP (defesa em profundidade).

Servidores HTTP de verdade (`std::net`) em 127.0.0.1, 127.0.0.2 e ::1 contam as conexões que recebem.
São 12 cenários (tabela abaixo), mais um GET HTTPS real em `https://example.com` (rustls +
webpki-roots), mil requisições negadas pra medir custo, `cargo metadata` da árvore e o mesmo teste de
redirect com `attohttpc` 0.31 e `minreq` 3.0 (as alternativas sem tokio).

**sqlite.** Um CLI `sqlite3` mínimo nosso (modo list) é o mesmo pros três motores, pra separar o que é
motor do que é CLI: porte do `sqlite3_complete` (tabela de estados do complete.c) pra acumular linhas e
separar comandos, moldura de erro do shell.c ("Parse error near line N", "Error: in prepare, ...",
"Error: stepping, ... (19)"), trecho com `^--- error here` a partir do deslocamento que o motor
informa, código de saída (no modo argv é o código do SQLite), REAL como `%!.15g`, texto cortado no
primeiro NUL, `-header`, `-separator`, `-bail`, `-readonly`, `.headers`, `.tables`, `.schema` (com o
comentário de colunas da view), desenho do `EXPLAIN QUERY PLAN`, e a saída sem `sqlite3_close` quando
um comando da linha de comando falha (o journal fica quente, como no sqlite3 real). Os modos
csv/json/column ficaram de fora de propósito e contam como não suportados.

Os três caminhos:

1. `rusqlite` 0.40.2 com `bundled` e `serialize`: cada comando abre um banco em memória com
   `deserialize` dos bytes do arquivo e grava de volta com `serialize`.
2. `rusqlite` + `sqlite-plugin` 0.11.0: VFS nosso (`MemVfs`, trait `Vfs` segura, `register_static`
   seguro) sobre um FS compartilhado em memória com as travas do SQLite (SHARED, RESERVED, PENDING,
   EXCLUSIVE) por arquivo.
3. `turso_core` 0.8.1 (sem `simd` e `encryption`, que puxam C): `IO`, `File` e `Clock` nossos sobre o
   mesmo FS compartilhado.

Corpus: 86 casos em `corpus/cases/sqlite` (DDL/DML, tipos e afinidade, formatação de REAL, funções de
texto, data e matemáticas, LIKE/GLOB, agregados, joins, subconsultas, views e triggers, autoincrement,
CHECK e DEFAULT, ALTER TABLE, WITHOUT ROWID, WITH RECURSIVE (contador, Fibonacci, árvore, Mandelbrot),
janelas (ranking e frames ROWS/RANGE/GROUPS com EXCLUDE), JSON (escalares, modificação, json_each,
json_tree, agregados), upsert, RETURNING, colunas geradas, STRICT, PRAGMA comuns, índices, EXPLAIN QUERY
PLAN, transações e savepoints, erros de prepare e de execução em argv e stdin, comandos do CLI e
encadeamentos de vários `sqlite3` no mesmo arquivo via script). O golden vem do sqlite3 3.46.1 do
Debian 13 e foi gerado duas vezes com resultado idêntico. Como o motor empacotado é 3.53.2, o arquivo
do banco é comparado pelo retrato lógico (esquema, linhas ordenadas, `user_version`,
`application_id`); a igualdade de bytes (ignorando os campos de versão do cabeçalho, offsets 92..100) é
medida à parte.

O SQLite empacotado foi alinhado ao do Debian com
`LIBSQLITE3_FLAGS="-USQLITE_DEFAULT_FOREIGN_KEYS -DSQLITE_ENABLE_MATH_FUNCTIONS"` em `.cargo/config.toml`:
sem isso, `PRAGMA foreign_keys` responde 1 e `sqrt`/`pow`/`ln` não existem.

Além do corpus: interoperabilidade de arquivo nos dois sentidos (banco criado aqui aberto pelo sqlite3
do oráculo com `PRAGMA integrity_check` e a mesma consulta de verificação; bancos criados pelo sqlite3
em modo rollback, em modo WAL e com TEXT de UTF-8 inválido abertos aqui), dois "processos" (threads) no
mesmo banco (contador com 2 x 100 incrementos em comandos separados, leitura durante transação aberta,
escrita concorrente sem espera) e `datetime('now')` com o relógio do sandbox fixado em
2026-01-15 12:00:00.

**git.** (1) `gix` 0.88 alto nível: tentativa de abrir e criar o repositório que só existe no
`MemTree`, e o mesmo repositório materializado num diretório do host. (2) Protótipo com `gix-object`,
`gix-hash`, `gix-zlib`, `gix-pack`, `gix-index`, `gix-ref` e `gix-diff` sobre o `MemTree`, com um CLI
nosso por comando (`init`, `hash-object`, `add`, `write-tree`, `commit-tree`, `update-ref`, `commit`,
`log`, `status`, `diff`, `cat-file`, `ls-files`, `ls-tree`, `rev-parse`). Corpus de 15 casos `script`
em `corpus/cases/git`, todos terminando em `rm -rf .git` (o índice guarda ctime, inode e device, o que
deixaria o golden não determinístico); um mini-shell nosso (`src/shell.rs`) roda os encadeamentos
(`;`, `&&`, `||`, `|`, `<`, `>`, `>>`, `$(...)`, `VAR=...`, `$?`). Validação no oráculo: repositório
construído pelo protótipo (blobs, executável, symlink, binário, árvores aninhadas, commit-tree e
update-ref em ramos, `commit -a` com remoção) exportado e checado com `git fsck --strict --no-dangling`
e 8 comandos rodados dos dois lados. (3) Repositório criado pelo git real com 7 commits, tag leve, tag
anotada e `git gc --aggressive` (pack com deltas, `packed-refs`, `commit-graph`), capturado em bytes e
lido pelo protótipo: todo objeto comparado com `git cat-file -p`, e `log`, `ls-tree -r`, `status` e
`rev-parse` comparados com o git real. (4) Linhas de código por comando. (5) `git2` 0.21 descartado sem
compilar libgit2: um manifesto de sonda (`probes/`) só pra `cargo metadata` baixar as fontes e o
depscan e a varredura de API rodarem.

## Candidatos

| Papel | Candidato | Versão | Categoria | Encaixe |
|---|---|---|---|---|
| net | ureq com Resolver e Connector nossos | 3.4.2 | b (árvore c por causa do ring) | serve |
| net | attohttpc | 0.31.0 | b | não serve |
| net | minreq | 3.0.0 | b | não serve |
| sqlite | rusqlite serialize/deserialize | 0.40.2 (SQLite 3.53.2) | c | serve com trabalho |
| sqlite | rusqlite + sqlite-plugin | 0.40.2 + 0.11.0 | c | serve com trabalho |
| sqlite | turso_core com IO próprio | 0.8.1 | b (árvore c: aegis, generator) | não serve |
| git | gix alto nível | 0.88.0 | b | não serve |
| git | gix-* baixo nível | object 0.65, pack 0.75, index 0.56, ref 0.68, diff 0.68 | b | serve com trabalho |
| git | git2 | 0.21.0 | c (libgit2-sys, libz-sys) | não serve |

## Resultado

### Rede (H34)

| Cenário | Resultado | Conexões no servidor |
|---|---|---|
| nome permitido | 200 | 1 |
| nome negado (mesmo resolvendo pra um servidor vivo) | bloqueado, zero DNS | 0 |
| nome permitido que resolve pra 127.0.0.1, com bloqueio de internos | bloqueado | 0 |
| `localhost` permitido, pelo DNS do host (::1 e 127.0.0.1) | bloqueado, 1 DNS | 0 |
| redirect pra nome negado | bloqueado no segundo salto | front 1, alvo 0 |
| redirect pra IP literal negado | bloqueado | front 1, alvo 0 |
| redirect pra nome permitido (controle) | 200 | front 1, alvo 1 |
| IP literal fora da allowlist | bloqueado | 0 |
| IP literal na allowlist mas interno | bloqueado | 0 |
| IPv6 literal `[::1]` | bloqueado | 0 |
| `http://allowed.test@denied.test/` | bloqueado (o host é denied.test) | 0 |
| `ALLOWED.Test.` (caixa e ponto final) | normalizado, 200 | 1 |

12/12 cenários certos. Requisição negada custa ~1,7 µs e não toca o DNS (1000 de 1000 negadas, zero
consultas). HTTPS real: `https://example.com` respondeu 200 com rustls e as raízes do webpki-roots
embutidas (sem ler o `/etc/ssl` do host). A árvore do ureq não tem tokio nem outro runtime assíncrono
(53 dependências). O que é C nela é o `ring` (provedor de cripto padrão do rustls); o
`wasm-bindgen-shared` que o depscan também marca é de outro alvo e não compila no Linux. Pegadinha
medida no código do ureq: `Config::default()` lê `HTTP_PROXY` do ambiente do host
(`Proxy::try_from_env`), então o agente do sandbox nasce com `proxy(None)`.

Alternativas: `attohttpc` e `minreq` não têm gancho de resolvedor nem de conexão. Com o invólucro
checando só a URL inicial, seguir redirect (o padrão das duas) levou a uma conexão no servidor negado
(127.0.0.2). Com redirects desligados, o `attohttpc` devolve o 302 e o invólucro consegue checar o
`Location`; o `minreq` com `with_max_redirects(0)` nem devolve a resposta (erro "too many
redirections"). Nas duas, o DNS fica dentro da crate: dá pra resolver antes e checar, mas o nome é
resolvido de novo depois (TOCTOU), e nome permitido que aponta pra IP interno não tem como ser barrado.

### sqlite (H35)

| | rusqlite serialize | rusqlite + sqlite-plugin | turso_core |
|---|---|---|---|
| corpus estrito (86 casos) | 82 | 82 | 27 |
| stdout e exit iguais ao golden | 82 | 82 | 77 |
| banco final com mesmas tabelas e linhas (53 bancos) | 53 | 52 | 49 |
| banco final idêntico byte a byte, fora a versão | 0 (só o offset 27 diverge) | 46 | 0 |
| daqui pro sqlite3 (integrity_check + consulta igual) | sim | sim | sim |
| sqlite3 (rollback) pra cá | sim | sim | sim |
| sqlite3 (WAL) pra cá | sim, com o cabeçalho trocado na carga | sim, com `locking_mode=EXCLUSIVE` | sim |
| TEXT com UTF-8 inválido gravado pelo sqlite3 | lê igual | lê igual | "Corrupt database: TEXT value contains invalid UTF-8" |
| contador 2 x 100 em comandos separados | sem trava: perde atualizações (de 19 a 100 nas rodadas medidas); com trava de arquivo inteiro: 200 | 200, zero perdidas | 200, zero perdidas, com retentativas por "busy" (de 9 a 191 nas rodadas medidas) |
| leitor durante transação aberta | vê o estado antigo | vê o estado antigo | vê o estado antigo |
| commit visível pra outro processo | só quando o processo que confirmou termina | na hora | na hora |
| escrita concorrente sem espera | não detecta: o último a sair sobrescreve (1 linha de 2) | "database is locked" | "Database is busy" |
| `datetime('now')` com relógio do sandbox | relógio do host | relógio do host | relógio do host |

As 4 falhas dos dois caminhos com o SQLite real são as mesmas: os 3 modos de saída fora da camada
mínima e `sqlite-pragma-journal-mode`. Nesse caso o banco em memória do caminho 1 responde `memory` a
todo `PRAGMA journal_mode`, e o caminho 2 aceita `journal_mode=WAL` e falha no INSERT seguinte com
"disk I/O error", porque o WAL compartilhado exige `shm_map`. Com o mesmo CLI, o stdout do caminho 1 é
idêntico ao do caminho 2 em todos os casos menos esse: o CLI nosso cobre o que o corpus pede, e o que
sobra é configuração do motor.

O que é de cada parte, medido:

- **Motor**: mensagens de erro, deslocamento do erro (o que alimenta o `^---`), texto guardado em
  `sqlite_schema`, recursos SQL, `journal_mode`, layout do arquivo. Com o mesmo CLI, o turso produz
  stdout diferente do SQLite real em 6 casos (WITHOUT ROWID, colunas geradas e VACUUM atrás de flags
  experimentais, `ALTER TABLE` e `.schema` mostrando o SQL reescrito como `CREATE TABLE t (a ...)`,
  `journal_mode` sempre `wal`); o resto das falhas dele está no stderr (não informa deslocamento, então
  não há trecho com `^---`; "misuse of aggregate function count(*)" em vez de "misuse of aggregate:
  count()"; "non-terminated literal" em vez de "unrecognized token"; o código "(19)" já vem dentro da
  mensagem) e no arquivo (o SQL reescrito em `sqlite_schema` muda o retrato lógico).
- **CLI**: tudo que está listado no Método. Isso é nosso em qualquer caminho.

API e segurança:

- `rusqlite` não tem API pra registrar VFS: só `open_with_flags_and_vfs`, que recebe o nome de um VFS
  já registrado (zero ocorrências de `sqlite3_vfs_register` ou `register_vfs` no crate). Registrar um é
  FFI unsafe. O v1 errou aqui.
- `sqlite-plugin`: `register_static` e a trait `Vfs` são seguras, e o nosso VFS não tem unsafe. Mas: o
  `shm_map` (necessário pro WAL compartilhado) devolve `NonNull<u8>` com contrato de validade que o
  compilador não verifica; `xCurrentTime`, `xRandomness` e `xSleep` não fazem parte da trait e vão pro
  VFS unix do host (por isso o `datetime('now')` do caminho 2 é o do host); o build roda bindgen e
  precisa de libclang (neste host só existe a libclang versionada, então o `Cargo.toml` liga a feature
  `runtime` do bindgen).
- `turso_core`: `IO`, `File` e `Clock` são traits seguras e bastam pro banco morar no FS do sandbox,
  mas o `datetime('now')` chama `SystemTime::now()` direto (o `Clock` do IO só serve pro WAL e
  timeouts). O crate `turso` de alto nível também aceita IO próprio (`Builder::with_io_impl`), com API
  async. A árvore do `turso_core` inclui `aegis` (cripto em C sem a feature `pure-rust-crypto`) e
  `generator` (troca de contexto em assembly).
- Nenhum dos três segue o relógio do sandbox em `datetime('now')`. Nos caminhos com o SQLite real isso
  só sai com um VFS em C ou trocando as funções de data por funções nossas registradas com o mesmo nome.

### git (H36)

- `gix` alto nível: `gix::open` no caminho que só existe no `MemTree` dá `does not appear to be a git
  repository`; `gix::init` tenta criar o diretório no disco do host (`Could not create directory`). O
  mesmo repositório materializado num diretório do host abre e lê o HEAD. Não há trait de FS: o
  repositório é feito de `gix_odb::Store::at`, `gix_ref::file::Store::at` e `gix_index::File::at`.
- `gix-*` baixo nível: tudo que o protótipo usa aceita bytes. `gix_pack::index::File::from_data` e
  `gix_pack::data::File::from_data` aceitam qualquer `Deref<Target = [u8]>`;
  `gix_ref::packed::Buffer::from_bytes`; `gix_index::State::from_bytes` e `write_to` (que não grava o
  checksum final: quem grava é `gix_index::File`, que exige caminho; o protótipo calcula o SHA-1). O
  recurso `blob` do `gix-diff` puxa `gix-worktree`, `gix-filter` e `gix-command` (que tocam o host),
  mas `diff_with_slider_heuristics` e `UnifiedDiff` são puros; a moldura do `git diff` (cabeçalho
  `@@ -a +b @@` sem `,1`, contexto de função do xdiff, `\ No newline at end of file`) é nossa.
- Corpus: 15/15 estrito (stdout, stderr, exit e árvore final), incluindo o hint do `git init`, o resumo
  do `git commit` (`[master (root-commit) 7a08811] first`, estatística, `create mode`/`delete mode`),
  `status` curto e longo, `diff` e `diff --cached`, `log` com formatos, `rev-parse --short` e o erro
  "Needed a single revision".
- Repositório feito pelo protótipo e exportado: `git fsck --strict --no-dangling` sai com 0, e os 8
  comandos comparados (`log` com `%H %P %T %an <%ae> %at %s`, `log` de outro ramo, `ls-files -s`,
  `status --porcelain`, `cat-file -p HEAD`, `ls-tree -r`, `rev-parse` de três refs, `diff`) dão a
  mesma saída no git real, com o índice escrito pelo protótipo (stat zerado: o git real recalcula o
  hash e confirma).
- Repositório do git real depois de `gc --aggressive` (nenhum objeto solto, tudo num pack com 6
  entradas delta, refs em `packed-refs`, tag anotada): 32/32 objetos com tipo, tamanho e `cat-file -p`
  iguais; `log`, `ls-tree -r`, `status --porcelain` (lendo o índice escrito pelo git) e `rev-parse` das
  tags iguais.
- Linhas de código (sem brancas, comentários e testes), total 1314:

| Comando | Linhas | Comando | Linhas |
|---|---|---|---|
| init | 67 | log | 130 |
| hash-object | 57 | status | 147 |
| add | 43 | diff | 73 |
| write-tree | 47 | cat-file | 42 |
| commit-tree | 38 | ls-files | 12 |
| update-ref | 18 | ls-tree | 26 |
| commit | 141 | rev-parse | 36 |
| store sobre gix-* (compartilhado) | 334 | diff de blobs (compartilhado) | 64 |
| despacho e utilitários | 39 | | |

- `git2` 0.21: binding de libgit2 (libgit2-sys e libz-sys compilam C). `Repository::open` e `init`
  recebem caminho. O único backend que não é disco é o mempack (objetos em memória, sem ler o nosso
  FS), via `Odb::add_new_mempack_backend` + `Repository::from_odb`. Backend de objetos ou de refs
  próprio só implementando `git_odb_backend`/`git_refdb_backend` pela FFI crua do libgit2-sys (o
  `git2` não expõe `git_refdb_backend` em lugar nenhum e tem 988 ocorrências de `unsafe` nas fontes),
  ou seja, unsafe nosso.

## Veredito

- **H34: confirmada.** O ureq 3 aceita Resolver e Connector próprios por `Agent::with_parts`, e a mesma
  `Policy` nos dois pontos barra nome negado sem DNS nem conexão, nome permitido que resolve pra
  endereço interno, redirect pra host ou IP negado e os truques de URL testados. Sem tokio. TLS com
  rustls funcionou num GET real. Ressalvas que vão pro design: fixar a versão (`unversioned` não segue
  semver), desligar o proxy do ambiente, e o `ring` é C (trocar de provedor de cripto é decisão à
  parte). `attohttpc` e `minreq` não servem: sem gancho, vazaram no redirect.
- **H35: parcial.** O rusqlite sozinho não registra VFS. Com o `sqlite-plugin` dá, sem unsafe nosso, e
  é o melhor caminho medido: 82/86 com CLI nosso, banco idêntico byte a byte ao do sqlite3 em 46 de 53
  casos, interop nos dois sentidos, travas reais entre processos. Faltam WAL compartilhado (o `shm_map`
  tem contrato de unsafe) e relógio do sandbox. O `serialize`/`deserialize` dá a mesma conformidade sem
  VFS, mas a semântica entre processos é pior (commit só visível no fim do processo, escritor
  concorrente sobrescreve) e exige trava de arquivo inteiro por comando. O `turso` tem a melhor API de
  IO, mas como motor fica em 27/86 e não lê TEXT inválido que o SQLite aceita.
- **H36: parcial.** O `gix` de alto nível não aceita: exige diretório real. Os `gix-*` de baixo nível
  aceitam bytes e bastam pra um git sobre o VFS que o git real valida (`fsck --strict` limpo) e que lê
  repositório empacotado pelo git real, com uns 1300 linhas de CLI e store nossos pra 14 comandos.

**Recomendação.** Rede: `ureq` 3 fixado, com `PolicyResolver` e `PolicyGate` como o único caminho de
saída do sandbox (curl e wget são CLIs nossos por cima). sqlite: `rusqlite` + `sqlite-plugin` com o VFS
sobre o tmpfs do kernel e o CLI nosso, WAL em modo exclusivo (`locking_mode=EXCLUSIVE` ao abrir banco
que já está em WAL) até valer a pena implementar `shm_map`, e as funções de data com "now" trocadas
por funções nossas se o relógio do sandbox for requisito; o caminho `serialize` fica como plano B sem
build de bindgen. Acompanhar o `turso`: a API de IO é a certa, o motor ainda não. git: `gix-*` de baixo
nível com refs, índice e worktree nossos, como o protótipo; `gix` e `git2` fora.

## Arquivos

- `src/net/`: `Policy`, `PolicyResolver`, `PolicyGate`, agente do sandbox; servidores e cenários.
- `src/sqlite/cli.rs`: CLI `sqlite3` modo list; `memfs.rs`: FS compartilhado com travas;
  `rusq.rs`: caminhos 1 e 2; `turso_engine.rs`: caminho 3; `experiments.rs`: conformidade, interop,
  concorrência, relógio.
- `src/git/store.rs`: repositório sobre `MemTree` com `gix-*`; `cmd/`: um arquivo por comando;
  `textdiff.rs`: diff no formato do git; `gix_high.rs`: sonda do `gix` alto nível; `experiments.rs`:
  fsck, pack, linhas.
- `src/shell.rs`: mini-shell dos casos `script`.
- `probes/`: manifesto só de metadados pra `git2` e `turso` (nunca compilado).
- `tests/`: política de rede, persistência e travas do sqlite, protótipo git contra o golden e o
  `git fsck`.
