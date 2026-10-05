# ul-sqlite: status

Dono: agente net. Programa: `sqlite3` (CLI do SQLite 3.46.1 do Debian 13).

## Como é feito

- Motor: a libsqlite3 3.46.1 do Debian 13 (pacote libsqlite3-dev), ligada pelo rusqlite sem
  `bundled` (decisão do coordenador): mesmo motor, mesmas opções de compilação e mesmo cabeçalho de
  arquivo do sqlite3 do oráculo. Link dinâmico; pra binário hermético, `SQLITE3_STATIC=1` na config da
  raiz. A imagem de build precisa de libsqlite3-dev e a de runtime de libsqlite3-0.
- `rusqlite` com `modern_sqlite` (bindings do 3.53.2): necessário pro deslocamento do erro de sintaxe
  (`sqlite3_error_offset`, o que alimenta o `^--- error here`). Só funções que existem no 3.46.1 são
  chamadas; nenhuma das que a feature expõe a mais é usada.
- VFS (`src/vfs.rs`): porte do `os_unix.c` sobre o `sysabi` com a trait segura do sqlite-plugin.
  Abertura com os mesmos flags e modos (journal e WAL herdam modo e dono do banco), temporários
  `etilqs_*` no diretório temporário do sandbox apagados logo depois de abertos, caminho absoluto com
  symlinks resolvidos, tamanho 1 relatado como 0, setor 4096, POWERSAFE_OVERWRITE, e o protocolo de
  travas do `unixLock` sobre as travas OFD do kernel (`ofd_setlk`/`ofd_getlk`).
- CLI (`src/cli`): porte do shell.c 3.46.1: argumentos em duas passadas, abertura tardia do banco,
  `~/.sqliterc`, leitura linha a linha com `quickscan` e `sqlite3_complete`, terminadores `go` e `/`,
  `shell_exec` com a moldura de erro ("Parse error near line N:", "Runtime error near line N:",
  "Error: in prepare, ...", "Error: stepping, ... (N)") e o trecho `^--- error here`, códigos de saída,
  saída sem `sqlite3_close` quando o C sai sem fechar (journal quente fica), buffer do stdio da glibc
  no stdout (ordem certa com o stderr num `2>&1`).
- Modos: list, csv, json, line, column, table, box, markdown, qbox, insert, quote, tabs, ascii,
  html, tcl, count, off, explain (com recuo) e o grafo do EXPLAIN QUERY PLAN; REAL pelo `printf` do
  próprio SQLite (`%!.15g` e `%!.20g`).
- Comandos de ponto: .auth, .backup/.save, .bail, .binary, .cd, .changes, .connection (só a ativa),
  .crnl, .databases, .dbconfig, .dump (com --data-only, --newlines, --nosys, --preserve-rowids,
  padrões LIKE), .echo, .eqp, .exit, .explain, .fullschema, .headers, .help (texto exato do Debian),
  .import (CSV e ASCII, --csv, --ascii, --skip, --schema, -v, criação da tabela com renomeação de
  duplicatas como o `zAutoColumn`), .indexes, .limits, .load (falha como arquivo inexistente), .mode
  (com --wrap, --wordwrap, --ww, --quote), .nullvalue, .once, .open (--new, --readonly,
  --deserialize), .output (arquivo, stdout, stderr, `|comando`), .parameter, .print, .progress,
  .prompt, .quit, .read (arquivo e `|comando`), .restore, .schema (--indent, --nosys, comentário de
  colunas das views), .separator, .shell/.system, .show, .tables, .timeout, .timer, .version,
  .vfsname, .vfslist, .vfsinfo, .width.
- Opções: -ascii, -bail, -batch, -box, -column, -cmd, -csv, -deserialize, -echo,
  -eqp, -eqpfull, -header, -noheader, -help, -html, -init, -interactive, -json, -line, -list,
  -markdown, -newline, -nofollow, -nullvalue, -quote, -readonly, -safe, -separator, -table, -tabs,
  -version, -vfs (só os nossos), e as de configuração aceitas sem efeito.
- Funções trocadas pra não depender do host: `julianday`, `unixepoch`, `date`, `time`, `datetime`,
  `strftime`, `timediff`, `current_*` (porte do date.c com o relógio e o fuso do sandbox; `%f` e `%g`
  pelo printf do SQLite), `random()` e `randomblob()` (getrandom do sandbox), `load_extension()`
  (nunca abre biblioteca). Do shell.c: `shell_add_schema`, `shell_putsnl`, `strtod`, `dtostr`,
  `usleep`, `edit` (falha), colação `uint`. `generate_series` vem do crate vendorizado `pl-series`.

## Segurança e isolamento

- Todo callback chamado pelo C (VFS, funções, authorizer, progress e busy handler) roda em
  `unwind::guard`: um `KillUnwind`/`ExitUnwind` vira erro pro SQLite e é relançado quando o controle
  volta pro Rust. Sem isso o unwind atravessaria `extern "C"` e abortaria o processo host.
- A libsqlite3 do Debian tem `USE_URI`: o nosso VFS é registrado como `unix` (padrão) e também como
  `unix-excl`, `unix-dotfile` e `unix-none`, e o authorizer nega ATTACH (e VACUUM INTO) de URI com
  `vfs=` que não seja nosso ou `memdb`. Nenhum caminho de SQL chega ao VFS do host.
- `load_extension` está sempre desligada; a função SQL e o `.load` respondem como arquivo inexistente.
- Progress handler a cada 1000 passos da VM chama `checkpoint()` (preempção e sinais) e trata SIGINT
  como o CLI (interrompe o comando; o segundo SIGINT sai).

## Conformidade

- Corpus original do F12 (86 casos): 80/86 estrito no testkit e no kernel real. Os 6 que faltam são
  casos `script` (precisam do `bash` do crate `shell`).
- Corpus estendido `cli_extra.toml` (56 casos novos, golden do oráculo): no último placar completo,
  131/142 estrito no testkit; faltavam os 8 casos `script`, `generate_series` com passo negativo (ver
  pendências) e 2 casos já corrigidos depois (funções inócuas no DEFAULT e um caso com fuso + faketime
  ambíguo, reescrito).
- Testes: `cargo test -p ul-sqlite` (unitários + `tests/conformance.rs` com testkit e kernel).

## Pendências

- WAL compartilhado: sem `xShmMap` (o da trait devolve ponteiro com contrato que o compilador não
  verifica, equivale a unsafe nosso). O CLI liga `locking_mode=EXCLUSIVE` antes de abrir um banco em
  WAL e antes de `PRAGMA journal_mode=WAL` (anotado pelo authorizer), então o WAL funciona, mas com o
  banco preso ao processo enquanto ele estiver aberto, e `PRAGMA locking_mode` responde `exclusive`.
- `generate_series` (passo negativo, LIMIT/OFFSET empurrados, primeiro argumento obrigatório) vem do
  crate vendorizado `vendor/series` (`pl-series`), porte do series.c do 3.46.1. A tabela virtual exige
  `unsafe impl VTab`, que não cabe aqui (forbid(unsafe_code)); o crate vendorizado não herda o lint.
  Outras tabelas virtuais do shell.c podem seguir o mesmo caminho.
- Outras extensões embutidas no shell.c que dependem de tabela virtual (fsdir, completion, zipfile,
  sqlar, .archive, -A, -zip, -append, .recover, .dbinfo, .expert, .session, .intck): ausentes pelo
  mesmo motivo ou por dependerem de código C do shell.c que não está na libsqlite3.
- Funções escalares do shell.c ainda não portadas: `regexp`/REGEXP, `sha3`/`sha3_query`/`.sha3sum`,
  `decimal_*`, `base64`, `base85`, `ieee754*`, `readfile`/`writefile`.
- `.databases`: estado da transação aproximado (sem `read-txn`; o rusqlite só expõe
  `sqlite3_txn_state` com bindings que exigiriam 3.53).
- `xSleep`: o `PRAGMA busy_timeout` usa o busy handler nativo, que dorme no host (fora do
  escalonador); o `.timeout` usa um busy handler nosso com `nanosleep` do sandbox. `xRandomness` semeia
  o PRNG interno do SQLite uma vez pelo `/dev/urandom` do host (afeta só nonces de journal).
- Memória alocada pelo SQLite (malloc do C) não passa pelo allocator rastreado do sandbox.
- SQL com bytes que não são UTF-8 válido vai pro motor com troca por U+FFFD (o rusqlite só aceita
  `&str` em `prepare`).
- `-nofollow` não recusa symlink (a trait não deixa devolver `SQLITE_OK_SYMLINK`).
- Modo interativo (stdin terminal): prompts e banner como o original, sem edição de linha nem histórico.
