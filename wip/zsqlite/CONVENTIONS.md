# zsqlite: convenções do porte do SQLite 3.46.1 para Rust (modelo v2)

Objetivo: o sqlite3 do Debian 13 (3.46.1) em Rust seguro, byte a byte: mesmo arquivo `.db`, mesmo
journal e WAL, mesma saída, mesmas mensagens de erro, mesma ordem de linhas do planejador. Fonte:
`upstream/sqlite-amalgamation-3460100/sqlite3.c`, fatiado em `chunks/` (um trecho por arquivo de
origem e faixa de linhas; `chunks/manifest.json`).

## Por que existe um modelo v2

A primeira tradução (guardada em `legacy/`, fora do crate, só como material de consulta) usou
`Rc<RefCell<T>>` e `Weak` em toda estrutura do C e deixou cada agente inventar os acessores que
faltavam. Medido: 564 nomes distintos sem definição, `value_text` definido duas vezes com tipos
diferentes, `Sqlite3Context` nunca definido e, pior, `MemPage.a_data` é uma CÓPIA dos bytes da
página do pager (`pager_get_data(p).to_vec()`): toda edição do b-tree se perderia e o `.db` nunca
sairia certo. Além disso `RefCell` aliasado onde o C aliasa (cursores, páginas, VDBE) vira pânico
em tempo de execução. O modelo v2 troca ponteiros por posse em árvore e por índices (handles).

## Regras gerais

- `#![forbid(unsafe_code)]`. Nenhum `unsafe`, nenhuma FFI, nenhuma crate de SQLite.
- Tradução fiel, função por função, na mesma ordem do C. Nada de "simplificar", trocar algoritmo,
  pular ramo de erro ou deixar `todo!()`. Ramo que só existe em outra plataforma (Windows, VxWorks)
  ou sob `SQLITE_DEBUG`/`SQLITE_TEST`/`SQLITE_COVERAGE_TEST` some; o resto fica.
- Opções do Debian 13 valem como `#ifdef` resolvidos. A lista é exatamente a de
  `PRAGMA compile_options` do sqlite3 do oráculo (conferida em 2026-10-08); o que não está nela
  está DESLIGADO (em particular: sem `GEOPOLY`, sem `STMT_SCANSTATUS`, sem `SORTER_REFERENCES`,
  sem `STAT4`, sem `NULL_TRIM`, sem `CURSOR_HINTS`):
  `ALLOW_ROWID_IN_VIEW`, `DEFAULT_AUTOVACUUM`, `DEFAULT_CACHE_SIZE=-2000`, `DEFAULT_FILE_FORMAT=4`,
  `DEFAULT_JOURNAL_SIZE_LIMIT=-1`, `DEFAULT_MMAP_SIZE=0`, `DEFAULT_PAGE_SIZE=4096`,
  `DEFAULT_PCACHE_INITSZ=20`, `DEFAULT_RECURSIVE_TRIGGERS`, `DEFAULT_SECTOR_SIZE=4096`,
  `DEFAULT_SYNCHRONOUS=2`, `DEFAULT_WAL_AUTOCHECKPOINT=1000`, `DEFAULT_WAL_SYNCHRONOUS=2`,
  `DEFAULT_WORKER_THREADS=0`, `DIRECT_OVERFLOW_READ`, `ENABLE_COLUMN_METADATA`,
  `ENABLE_DBPAGE_VTAB`, `ENABLE_DBSTAT_VTAB`, `ENABLE_FTS3`, `ENABLE_FTS3_PARENTHESIS`,
  `ENABLE_FTS3_TOKENIZER`, `ENABLE_FTS4`, `ENABLE_FTS5`, `ENABLE_LOAD_EXTENSION`,
  `ENABLE_MATH_FUNCTIONS`, `ENABLE_PREUPDATE_HOOK`, `ENABLE_RTREE`, `ENABLE_SESSION`,
  `ENABLE_STMTVTAB`, `ENABLE_UNLOCK_NOTIFY`, `ENABLE_UPDATE_DELETE_LIMIT`, `HAVE_ISNAN`,
  `LIKE_DOESNT_MATCH_BLOBS`, `MALLOC_SOFT_LIMIT=1024`, `MAX_ATTACHED=10`, `MAX_COLUMN=2000`,
  `MAX_COMPOUND_SELECT=500`, `MAX_DEFAULT_PAGE_SIZE=32768`, `MAX_EXPR_DEPTH=1000`,
  `MAX_FUNCTION_ARG=127`, `MAX_LENGTH=1000000000`, `MAX_LIKE_PATTERN_LENGTH=50000`,
  `MAX_MMAP_SIZE=0x7fff0000`, `MAX_PAGE_COUNT=0xfffffffe`, `MAX_PAGE_SIZE=65536`,
  `MAX_SCHEMA_RETRY=25`, `MAX_SQL_LENGTH=1000000000`, `MAX_TRIGGER_DEPTH=1000`,
  `MAX_VARIABLE_NUMBER=250000`, `MAX_VDBE_OP=250000000`, `MAX_WORKER_THREADS=8`,
  `MUTEX_PTHREADS`, `SECURE_DELETE`, `SOUNDEX`, `SYSTEM_MALLOC`, `TEMP_STORE=1`, `THREADSAFE=1`,
  `USE_URI`.
- `ENABLE_UPDATE_DELETE_LIMIT` muda a GRAMÁTICA: o `parse.c` do amalgamation foi gerado sem ela,
  então as tabelas LALR e as ações de `parse_tables.rs`/`parse_reduce*.rs` precisam ser regeradas
  pelo lemon de 3.46.1 com `-DSQLITE_ENABLE_UPDATE_DELETE_LIMIT` (o oráculo aceita
  `DELETE ... ORDER BY ... LIMIT`).
- Comentários em português acentuado, sem travessão. Identificadores em inglês.
- Sem `.expect()`/`.unwrap()` que possa disparar em entrada de usuário: o C devolve código de erro,
  o Rust também. `unwrap` só onde o C tem `assert` de invariante.

## Nomes (determinísticos)

- `sqlite3XxxYyy` vira `xxx_yyy`; `static` vira snake_case do nome; cada arquivo C é um módulo
  (`crate::btree`, `crate::pager`, `crate::vdbe`, ...), sem sufixo `_c`/`_h`. Nada de glob
  `prelude` reexportando tudo: cada módulo importa o que usa (`use crate::pager::{Pager, PgId};`).
  Isso elimina as ambiguidades e deixa o compilador apontar o que falta.
- Função `static` do C é privada (`fn`); só o que o C declara em cabeçalho é `pub`/`pub(crate)`.
- Tipos mantêm o nome do C em PascalCase (`Btree`, `BtCursor`, `MemPage`, `Vdbe`, `Mem`, `Expr`).
  Campo `pBt` vira `p_bt` quando continuar existindo; quando o ponteiro vira handle, o campo ganha o
  sufixo do handle (`pgno`, `cursor_id`).
- `#define` de constante vira `pub const NOME: tipo` (em `crate::consts`, já extraído); de expressão
  vira `#[inline] pub fn`. Opcodes: `pub const OP_XXX: u8`.

## Modelo de dados (fechado, não se inventa outro)

1. **Posse em árvore, sem `Rc`/`RefCell`/`Weak`.** A conexão `Connection` (o `sqlite3` do C) possui
   tudo por valor: `dbs: Vec<DbSlot>` (cada um com `Option<Btree>` e `Schema`), `stmts: Slab<Vdbe>`,
   funções, módulos virtuais, hooks. Funções recebem `&mut Connection` (ou o pedaço que precisam) e
   handles. Ciclos do C (pai de volta) viram parâmetro explícito, nunca ponteiro.
2. **Página de banco de dados = índice.** O `PCache` possui `Vec<PgSlot>`; `PgSlot { data: Vec<u8>,
   extra: PgExtra, flags, n_ref, pgno, ... }`. `PgId(u32)` é o handle. `MemPage` NÃO guarda bytes:
   guarda só o cabeçalho decodificado (`is_init`, `leaf`, `n_cell`, `hdr_offset`, `max_local`, ...) e
   vive no `extra` do slot, como o C faz. Os bytes são sempre `pager.page_data(pg)`/`page_data_mut`;
   quando o código precisa de metadados e bytes ao mesmo tempo, usa-se `pager.page_parts(pg) ->
   (&mut MemPage, &mut [u8])` (borrow dividido). O `a_data + off` do C é `data[off..]`.
3. **Cursores por handle.** `BtShared` possui `cursors: Slab<BtCursor>`; `CursorId(u32)` é o handle.
   A lista `pCursor` do C (para `saveAllCursors`) é iterar o slab. `VdbeCursor` guarda `CursorId`
   do btree, não referência.
4. **Btree == BtShared 1:1** (cache compartilhado desligado; é o padrão do sqlite3 e do Debian.
   `cache=shared` por URI fica como lacuna registrada, última prioridade).
5. **VFS por trait.** `trait Vfs { fn open(...) -> Result<Box<dyn VfsFile>, i32>; ... }` e
   `trait VfsFile` no módulo `os`, com os mesmos códigos `i32` do C. O registro de VFS é um
   `thread_local!`/`OnceLock` seguro no módulo `os`. `os_unix` implementa sobre `std::fs`/`std::io`
   mais as syscalls de `sysabi` quando o projeto já as expõe; fcntl/flock com `rustix`-like só se a
   dependência já for permitida pelo workspace (consultar antes; sem `libc` cru).
6. **Árvores de sintaxe por posse.** `Expr`, `ExprList`, `SrcList`, `Select`, `With`, `Window`,
   `Upsert`, `Trigger*` são `Box`/`Vec`/`Option<Box>`; o compartilhamento do C (`EP_Static`,
   `pWhere` apontado de dois lugares) vira cópia explícita (`expr_dup`) no ponto onde o C copia
   ou `Rc<Expr>` SOMENTE onde o C documenta subárvore compartilhada e imutável.
7. **Esquema por `Rc<Table>`/`Rc<Index>` imutáveis após fechar.** A construção (CREATE TABLE em
   andamento) usa um `TableBuilder` mutável em `Parse`; `Schema` guarda `Rc<Table>`. A tabela hash
   do C (`sqlite3Hash`) é traduzida fielmente (mesma ordem de iteração, que aparece na saída).
8. **Mem**: `struct Mem { flags: u16, enc: u8, e_subtype: u8, n: i32, u_i: i64, u_r: f64,
   z: Vec<u8>, z_p_type..., agg: Option<Box<dyn Any>> }`; semântica de `MEM_*` idêntica ao C. Texto
   e blob são bytes, nunca `String`.
9. **Funções de usuário**: `type ScalarFn = fn(&mut Context, &[Mem])`; `Context { out: Mem,
   agg: &mut Option<Box<dyn Any>>, db: &mut Connection ... }` definido em `crate::func_ctx`.
   O `aggregate_context<T>` é método do `Context`.
10. **Erros**: `i32` com as constantes do C, como no C; `Result` só dentro de helpers locais.
11. **Estado global** (`sqlite3GlobalConfig`, mutex estático, PRNG) fica em `crate::global` com
    `thread_local!`/`Mutex` seguro; sem `static mut`.
12. Inteiros: `i64` para `sqlite3_int64`, `u32` para `Pgno`, resto como no C; overflow definido do
    C vira `wrapping_*`. Ponto flutuante `f64`; `REAL` formatado pela tradução de `printf.c`
    (`%!.15g`, `sqlite3FpDecode`), sem `format!` do Rust.

## Ordem de fechamento (camadas, cada uma só compila com as de baixo)

0. `consts` (extraído do C), `ctype`, `hash`, `bitvec`, `utf`, `util` (varint, atoi, atof, strnicmp),
   `printf` (+`StrAccum`), `random`, `rowset`, `tokenize`/`keywordhash`, `complete`.
1. `os` (traits), `os_unix`, `memjournal`, `memdb`, `pcache`, `pcache1`.
2. `pager`, `wal`.
3. `btree` (+ `btmutex` desnecessário em Rust: some).
4. `mem`/`vdbemem`, `vdbeaux`, `vdbe`, `vdbeapi`, `vdbesort`, `vdbeblob`.
5. Árvores e esquema: `expr`, `resolve`, `walker`, `build`, `callback`, `prepare`, `parse` (lemon),
   `select`, `insert`, `update`, `delete`, `where*`, `window`, `trigger`, `fkey`, `alter`, `analyze`,
   `pragma`, `attach`, `vacuum`, `func`, `date`, `json`, `main`, `legacy`, `table`.
6. Extensões: `fts3`, `fts5`, `rtree`, `geopoly`, `dbstat`, `dbpage`.
7. Binário `zsqlite-shell` (CLI) ligado e comparado ao oráculo.

Cada camada fecha com `cargo check` limpo, testes unitários contra o oráculo e um commit.
O oráculo é o `sqlite3` da imagem `pseudo-linus-oracle:894fe4065523`.
