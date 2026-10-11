# zbun: o transpilador do bun 1.4.2 portado para Rust seguro

Porte, arquivo por arquivo, dos crates do transpilador do bun (`src/ast`, `src/js_parser`, `src/js_printer` e o
subconjunto da base que eles usam) para Rust seguro e estável, para o zjsc rodar `.ts`, `.tsx` e `.jsx` com o mesmo
texto e o mesmo mapa de posições do bun. A decisão e a medição que a sustenta estão em
`wip/notes/bun-transpiler-t0.md`.

## Origem

- Repositório: `oven-sh/bun`, tag `bun-v1.4.2`, commit `744846f844374847c902b5e7fd59b4342a51ef99`.
- Licença: MIT, como declara o `LICENSE.md` do upstream, copiado sem alteração para esta pasta. O upstream não traz
  linha de copyright própria; o crédito é dos autores do bun (`oven-sh/bun`).
- Os nomes dos crates, dos módulos e dos itens são os do upstream, para a ressincronização com versões novas.

## Regras do porte

- `forbid(unsafe_code)` e toolchain estável, como o resto do projeto.
- Arquivo sem `unsafe` e sem recurso do nightly entra igual ao upstream (cópia por ferramenta).
- Ponteiro de arena da AST (`StoreRef`, `StoreSlice`, `Vec<T, AstAlloc>`) vira índice em arena tipada.
- Chamada a biblioteca C (mimalloc, simdutf, highway) vira Rust puro com o mesmo resultado, num trecho comentado.
- A fidelidade é medida contra o `bun build --no-bundle` do bun 1.4.2 do oráculo, byte a byte.
- Mudança sistemática nos arquivos copiados: os travessões dos comentários do upstream viram dois-pontos ou hífen
  (regra do repositório), por ferramenta, na hora da cópia.
- Mudança sistemática nos arquivos de payload copiados (`s.rs`): toda struct ganha `#[derive(Clone)]` (ou `Clone` entra
  no `derive` que já existe), por ferramenta (`awk`), porque o `clone_in` do `Store` pede `T: Clone`. Nenhum outro
  texto muda.
- Mudança sistemática no parser copiado (`js_parser/src`), por ferramenta (`sed`): o `AstAlloc` do upstream (o
  alocador de vetores da arena, `unsafe impl Allocator` sobre o mimalloc) sai, como já saiu dos nós do `bun_ast`, que
  guardam `Vec` e `Box` comuns. `AstAlloc::vec()` vira `Vec::new()`, `AstAlloc::take(x)` vira `core::mem::take(x)`,
  `AstAlloc::vec_from_slice(x)` e `AstVec::<Stmt>::from_slice(x)`/`from_arena_slice(x)` viram `<[_]>::to_vec(x)`,
  `ast_box` vira `Box::new`, e o `StringHashMap<_, AstAlloc>` perde o parâmetro. O `RawSlice` do `ExprOrLetStmt`
  vira `G::DeclList` (faixa `Copy` do `Store`), à mão.
- Mudança sistemática no parser copiado (`js_parser/src`), o passe do Store (acessos a nó e a lista pelo `Store` do
  `P`, no lugar do `Deref` do `StoreRef`): a parte mecânica roda por `scripts/store_fix.py`, guiada pelos diagnósticos
  do `cargo check --message-format=json` (E0609, E0277 de índice por referência e E0599 de `slice`, `slice_mut`,
  `iter`, `get` e `get_mut` em `StoreStr`, `StoreSlice` e `StoreRef`). Cada rodada é conferida pelo compilador e o
  resto é à mão. O `store` é `self.store` ou `p.store`, conforme a assinatura da função que envolve o trecho; depois de
  `let p = self;` (51 métodos) o `self` já foi movido e só `p.store` vale. O subcomando `ctors` troca os construtores
  (`StoreStr::new`, `E::Str::new` e as listas) por `STORE.alloc_str`/`alloc_slice`, e o `selfalias` reescreve os
  `self.store` que ficaram depois do alias (256 trocas); os dois são regras de texto conferidas pelo compilador a cada
  rodada, e um lookbehind impede que `BStr::new` case como `Str::new`.
- Mudança sistemática no parser copiado, empréstimos (`scripts/with_node_fix.py`): o despachante de `visit_stmt.rs`
  passava `p` e `&mut p.store[r]` na mesma chamada. O macro `with_node!` (`js_parser/src/lib.rs`) copia o nó para fora,
  chama sobre a cópia e grava de volta (28 chamadas); com `--opt`, `if let Some(v) = &mut STORE[x].campo { .. }` vira
  copiar o `Option` (valor `Copy`), rodar o corpo e gravar depois do bloco (17 blocos). Vale onde o nó só é alcançável
  pelo `data` durante a chamada (visita em árvore, uma vez por nó); onde a visita reentra no mesmo nó, o lugar tipado
  (`bun_ast::place`) é o certo.
  - Revisão de fidelidade (a copia-e-grava só é fiel se ninguém lê ou escreve o mesmo nó por outro caminho durante a
    chamada). Os 28 pontos do despachante: só o `s_class` entregava o `StoreRef` do próprio nó a quem o relê pelo
    `Store` (`lower_class(StmtOrExpr::Stmt(*stmt))`, que no upstream via o mesmo ponteiro de `data`): lá a cópia estava
    velha para o `lower_class`, e a escrita dele seria sobrescrita pela gravação final. Corrigido com `sync_node!`
    (grava a cópia antes, relê depois). Os outros 27 só passam `stmt` ao `stmts.push`, que guarda o índice. O
    `s_switch` põe `*stmt` numa lista para o `finalize` do `using`, que só olha o tipo do statement. Os 17 blocos
    `--opt` de `lower_decorators.rs` recursam só pelos descendentes do nó (a árvore não tem ciclo), então o pai não é
    alcançável de dentro do corpo. Regra para o resto do passe: antes de usar `with_node!`, procure no corpo qualquer
    função que receba o `Stmt`/`StoreRef` do nó visitado e o releia; achando, `sync_node!`. O teste
    `with_node_tests.rs` fixa as duas vistas (a função que relê vê a cópia velha sem o `sync_node!`, e vê tudo com ele).
- Mudança sistemática no parser copiado, empréstimos duplos de argumento (`scripts/hoist_args.py`): o upstream
  escrevia `Stmt::assign(&mut p.store, p.new_expr(..), rhs)` e `p.new_expr(E::Dot { name: p.store.alloc_str(x) }, loc)`;
  com o `Store` dentro do `p`, o primeiro empréstimo mutável ainda vive quando o argumento pede o segundo (E0499 e
  E0502). O script lê o diagnóstico do rustc (rótulo "first borrow later used by call") e sobe cada argumento que usa
  `p.` ou `self.` para um `let arg_a`, `arg_b`, dentro de um bloco que envolve a chamada; a ordem de avaliação se
  mantém. O que não casa (nenhum argumento usa `p`, o empréstimo é uma faixa `items[i]` segurada) fica para a mão.
  Regenerar o diagnóstico depois de qualquer edição, porque os deslocamentos de byte só valem para o arquivo que o
  rustc viu.
- Padrões da parte 17 (empréstimos do `Store` que o içador não cura), todos à mão ou por script curto:
  - Faixa do `Store` segurada durante um laço que chama método de `p`: o laço vira índice
    (`for index in 0..faixa.len()`), o elemento sai por cópia (`p.store[faixa][index]`) e, se o corpo muda o
    elemento, grava de volta. `rewrite_stmt_range` (`lower_decorators.rs`) é o mesmo molde para `&mut [Stmt]`.
  - Argumento `&p.store[..]` para método `&mut self`: a chamada recebe uma cópia (`.to_vec()` para faixa, `.clone()`
    para nó). A diferença para o upstream: a função não enxerga mais mudanças feitas no nó durante a chamada.
  - Campo de nó visitado por `p.visit_expr(&mut p.store[e].campo)`: copia o campo, visita a cópia, grava de volta
    (`{ let mut arg_a = ..; CHAMADA(&mut arg_a); ..campo = arg_a; }`); só vale para campo `Copy`.
  - `generate_temp_ref`, `generate_temp_ref_with_scope` e `new_symbol` recebem `&[u8]`, não `&'a [u8]`: o nome já é
    copiado para o `Store`, e o `'a` prendia `p.store` emprestado por todo o resto da função. Quem passa
    `p.store.str(..)` copia antes (`.to_vec()`).
- Mudança sistemática no parser copiado (`js_parser/src`), por `sed`: o `VecExt::append(&mut X.generated, y);` e o
  `VecExt::append(&mut X.children, y);` de uma linha viram `X.generated.push(y);` e `X.children.push(y);` (os dois
  campos de `Scope` são `Vec`; o `VecExt::append` saiu do `bun_collections` por ser sinônimo do `push`). Os outros
  `VecExt::append` (listas de `Decl` e de argumentos de `E::Call`) foram à mão: a lista de trabalho é um `Vec` local
  e vira faixa do `Store` com `alloc_slice` quando o nó que a guarda é criado.
- `Log` e `Source` (`bun_ast::logger`) ganham as famílias `add_*` e `range_of_*` do `lib.rs` do upstream que o parser
  chama. O upstream tem dois laços de `range_of_identifier`: o do logger (pula o escape `\u{...}` também no começo,
  zera a largura depois dele e inclui o último ponto de código no fim do arquivo) e o do lexer, que o parser chama e
  que não tem essas três correções. As mensagens de erro do bun saem do segundo, então os dois ficam, num laço só com
  a diferença no parâmetro `FIXED` (`logger::range_of_identifier::<FIXED>`).
- Dependências que o upstream resolve por FFI ou por outra crate e que aqui moram no `ul-common`, para o zjsc e o
  zbun não terem cada um a sua cópia:
  - `bun_ast::math::pow` (no upstream, a FFI `Bun__JSC__operationMathPow`) é `ul_common::libm::operation_math_pow`,
    com o `pow` do glibc, que subiu do zjsc;
  - o `bun_url::file_url_from_string` (a FFI `URL__getFileURLString`, que é o
    `WTF::URL::fileURLWithFileSystemPath` do WebKit) é `ul_common::file_url::file_url_from_path`. O parser de URL
    completo mora no zjsc; um teste do zjsc (`wtf::url::tests::file_url_from_path_matches_the_full_parser`) confere
    as duas num corpus de caminhos com `.`, `..`, controles, delimitadores e não ASCII;
  - o `bun_ptr::BackRef` do `visit_decls` (um ponteiro para a entrada de `replace_exports`, que não muda durante a
    visita) vira um clone da entrada: `ReplaceableExport` é `Clone` e o `Expr` dentro dele é um índice do `Store`.
- Lacunas de API do parser, fechadas sobre o `Store`:
  - `Source.path` é o `bun_paths::fs::Path<'static>` do upstream (o `ast` passa a depender do `bun_paths`), com
    `name()` e `is_node_module()` (a barra `/node_modules/` no diretório, como o `isNodeModule`);
  - `EString::from_static(store, texto)` grava os bytes no `Store` do arquivo, porque a faixa só existe dentro dele;
    o `prefill::string::CHILDREN` const deixou de existir até o `Prefill` entrar (item da seção "Arena tipada");
  - `Scope::put_member_with_hash(store, escopo, nome, hash, membro)` no lugar do `get_or_put_member_with_hash`
    `unsafe`: os membros saem do escopo durante a chamada, o padrão de "Escrita durante a visita";
  - `DeclaredSymbolList::{init_capacity, ensure_total_capacity}` devolvem `Result<_, AllocError>` como o
    `MultiArrayList`, e ganha `append_assume_capacity`;
  - `replace_exports.get_ptr(chave)` vira `replace_exports.get(chave)` do mapa (o `ReplaceableExportMap` tem `Deref`
    para ele), para não haver função de repasse.
- Mudança sistemática no parser copiado, empréstimos de lista e de nó (a rodada que levou o parser de 677 erros de
  empréstimo a zero com a feature `parser`), todos guiados pelo diagnóstico do `cargo check --message-format=json` e
  conferidos pelo compilador a cada rodada:
  - `scripts/loop_to_index.py`: o laço `for x in (&p.store[..])` (ou `.iter()`) que o rustc marca com "this for loop
    borrows" vira laço por índice com `let x = &p.store[lista][index].clone();`, e o corpo não muda (`x` segue `&T`).
    Os laços `&mut`/`iter_mut` e os que mexem na lista ficam à mão, com gravação de volta.
  - `scripts/place_call.py`: `{ let arg_a = &mut p.store[X].F; p.f(arg_a, ..) }` vira copiar o campo (ou o nó, com
    `clone`), chamar com `&mut` da cópia e gravar de volta, que é o "lugar" copia-e-grava de uma chamada que reentra.
  - `scripts/copy_data_arg.py`: `{ let arg_b = &p.store[X].data; f(p, arg_b) }` copia o dado (`Copy`) e passa `&arg_b`.
  - `scripts/dup_str_fix.py`: `alloc_str(str(x))` vira `NodeStore::dup_str(x)` (copia a faixa dentro do vetor de
    bytes, sem emprestar a fatia), e `store_name_in_ref(str(x))` vira `arena_str(x)`.
  - Regra de nomes: `p.arena_str(StoreStr) -> &'a [u8]` não prende o `p.store`; `p.store.str(..)` prende, e é a causa
    mais comum de cascata (a anotação `&'a [u8]` sobre `p.store.str(..)` exige o `Store` emprestado por `'a`).
  - Listas que a varredura compacta no lugar (`ImportScanner::scan`, `visit_decls`, `rewrite_stmts`) saem do `Store`
    por cópia e voltam depois (`ImportScanner::scan_range`, `rewrite_stmt_range`); o upstream compacta a mesma lista,
    então a gravação de volta preserva o que a passada seguinte relê.

## Próximos passos (parser e printer)

Medido no upstream: o `js_parser` tem 48,3 mil linhas (`p.rs` com 10,3 mil, `parse/` com 11 mil, `visit/` com 8,3
mil, `lower/` com 3,4 mil) e o `js_printer` 9,8 mil (`lib.rs` com 8,3 mil, `renamer.rs` com 1,5 mil). O `P` de
`p.rs` tem 25 trechos `unsafe`; `parse/` e `visit/` quase nenhum. A primeira AST de `const x: number = 1` passa por
`Parser::init` (`parse/parse_entry.rs`), `parse_stmts_up_to`, a visita (`visit_stmts_and_prepend_temp_refs`) e
`to_ast`, o que pede quase todo o `P`. Ordem:

1. No `bun_ast`, o que o `P` usa e ainda falta: `import_record.rs` (o `Path` do `bun_paths` vira o subconjunto
   que o transpilador lê), `known_global.rs`, `char_freq.rs`, `runtime.rs`, `ast_result.rs`, `part`, `flags`,
   `DeclaredSymbol`, `ImportKind`, `fold_string_addition.rs`.
2. `parser.rs`, `p.rs` e `parse/` por cópia, com o `P` dono do `Store` e os acessos `p.store[r]` no lugar do
   `Deref` do `StoreRef`; o `log` vem de `p.lexer.log()`.
3. `visit/` e `fold.rs`; `lower/` só o que os oito casos pedem (decorators e namespaces).
4. O teste de cada caso compara a AST do parser com a de `ast/tests/ast_cases.rs`.
5. `js_printer` e o golden de `bun build --no-bundle` gerado na imagem do oráculo.
6. `bun_react_compiler` (o `--react-compiler` do `bun build`), ligando a feature `react_compiler` do parser.

Estado do passo 1: feito (`import_record`, `ImportKind`, `part`, `runtime`, `ast_result`, `char_freq`,
`SlotCounts`). Medida do passo 2 (cópia de todo o `js_parser` restante, 2026-10-10): a primeira compilação para em
169 erros de resolução antes de chegar aos tipos. O que falta, por grupo:

- crates e módulos do upstream que o parser importa e o porte ainda não tem: `bun_crash_handler` e
  `bun_react_compiler` (ficam fora por decisão da coordenação);
- 48 trechos `unsafe` (25 em `p.rs`, 8 em `lower/`, 5 em `visit/mod.rs`, 4 em `parse_entry.rs`, 3 em `parser.rs`,
  e os `unsafe impl Send/Sync` de `defines_table.rs`);
- as assinaturas com tempo de vida (`Ast<'a>`, `import_record::List<'a>`) e o `ToExprWrapper` genérico do porte;
- o `Macro` (FFI com o `bun_js_parser_jsc`), que fica fora.

Estado dos ganchos (commit `cca3d157`): o `Macro` tem as constantes reais (`NAMESPACE`, `is_macro_path`) e o
`MacroContext` é um tipo sem valor, então `options.macro_context` é sempre `None` e os caminhos que chamariam a macro
são inalcançáveis pelo tipo. O `bun_react_compiler` e o `bun_crash_handler` ficam atrás das features
`react_compiler` e `crash_handler`, que só se ligam depois de portar a crate. O `bun build` do oráculo aceita
`--react-compiler`, então recusar a flag denunciaria a simulação: o destino fiel é portar o `bun_react_compiler`,
que entra na fila do porte depois do printer e do golden. Se a CLI for ligada antes disso, a flag fica aceita e sem
efeito até a crate entrar.

Estado do passo 5 (printer, medido em 2026-10-11): o `js_printer` do upstream (`lib.rs`, `renamer.rs`, `error.rs`, 9,8
mil linhas, travessões trocados na cópia) entra em `js_printer/` atrás da feature `printer` (`#![cfg(feature =
"printer")]`, como o `parser`), e `cargo check -p bun_js_printer --features printer` mede 72 erros de resolução, antes
de chegar aos tipos. O que falta, por grupo:

- `bun_core` (entraram nas partes 1 e 2): `printer` (constantes, `bmp_escape`, `surrogate_pair_escape`),
  `fmt::{ItoaBuf, itoa, hex2_upper, hex4_upper, pow10_exp_1e4_to_1e9}` (o `itoa` é escrito à mão, sem o crate),
  `strings::{index_of_needs_escape_for_java_script_string, contains_non_bmp_code_point_or_is_invalid_identifier,
  write_wtf8_as_utf16le}` (a última sem `unsafe`, acrescenta a um `Vec<u8>`; os escapes foram conferidos no bun 1.4.2
  do oráculo), `io::Write` (o `bun_io::Write` do upstream; no printer, `bun_io` virou `bun_core::io`),
  `MutableString` (sem `writable_n_bytes`, `expand_to_capacity` e `owns`, que só o código `unsafe` usava) e
  `FeatureFlags`. O `env_var::BUN_DUMP_SYMBOLS` e o `Map::dump` saem: era um despejo de depuração ativado por variável
  de ambiente do host;
- `bun_collections`: `hashbrown` reexportado e `AutoBitSet` (um `Vec<usize>`, sem os dois braços do upstream);
- `bun_ast`: `Indentation`/`IndentationCharacter` e `symbol::Map` (índice verificado no lugar do `*mut Symbol`; `merge`
  copia os dois campos do símbolo velho em vez de segurar dois empréstimos). `E::JsonValue`, `ObjectJSON` e
  `ArrayJSON` não entram: a `Data` do porte não tem essas variantes (só o loader de JSON as cria), então os braços e
  as três funções `print_*_json` do printer saíram, e voltam junto com o loader;
- **Lacuna registrada: os nós JSON.** Ao imprimir um arquivo `.json` o bun usa `E::ObjectJSON`, `E::ArrayJSON` e
  `E::JsonValue` (o JSON em fita, `JsonTape` e `TapeAlloc` em `ast/e.rs` do upstream), criados só pelo
  `parsers/json_stage2.rs`. O porte não os tem, então o printer perdeu os braços `ExprData::EObjectJSON` e
  `ExprData::EArrayJSON` do `print_expr` e as funções `print_object_json`, `print_array_json` e `print_json_value`
  (cópia em `js_printer/lib.rs` do upstream, logo depois de `number_property_key_must_be_computed`). Para voltar:
  (1) portar `JsonValue`, `JsonTape`, `ObjectJSON` e `ArrayJSON` para o `Store` (a fita vira um `Vec` do `Store`, sem
  `TapeAlloc` nem ponteiro), (2) acrescentar `EObjectJSON(StoreRef<E::ObjectJSON>)` e `EArrayJSON(...)` à `Data` em
  `expr.rs` (a `Tag` já tem as duas), (3) portar o `json_stage2.rs`, que é quem os cria (`.json` não passa pelo
  parser de JS), e (4) copiar de volta os dois braços e as três funções do upstream, trocando `.get()` por acesso ao
  `Store`. Sem isso, `bun build --no-bundle x.json` não tem como bater.
- `bun_sourcemap` (crate `sourcemap/`, sem `unsafe`): `vlq` (codifica e decodifica), `SourceMapState`,
  `append_mapping_to_buffer`, `line_offset_table` (as colunas viram três `Vec`, no lugar do `MultiArrayList`) e `chunk`
  (`Chunk`, `VLQSourceMap`, `LineOffsetTables`, `NewBuilder`/`Builder` com `add_source_mapping`,
  `update_generated_line_and_column` e `generate_chunk`). **Lacunas registradas:** (1) o `InternalSourceMap`, formato
  privado em processo do modo preguiçoso (`GenerateSourceMap::Lazy` com `IS_BUN_PLATFORM`) do runtime do bun, 1,3 mil
  linhas com ponteiros crus; sem ele o `prepend_count` do `Builder` não existe, e o Lazy gera o mesmo VLQ do Eager;
  (2) a leitura de mapas (`Mapping`, `ParsedSourceMap`, `SavedSourceMap`); (3) o `print_source_map_contents` (o JSON do
  `.map`), que entra com o `quote_for_json` movido para `bun_core::printer`, como no upstream. O teste de mapa contra
  `bun build --no-bundle --sourcemap` do oráculo fica para quando o printer compilar (parte 4): as colunas de
  `columns_for_non_ascii` ainda só têm teste de tamanho;
- `bun_core::vec` e `bun_ptr::BackRef` sumiram na parte 3, ao tirar o `unsafe`. `AstAlloc` saiu
  (o printer usa `Default::default()`), `hashbrown` ganhou a feature `raw-entry` e as quatro chamadas a
  `bun_crash_handler::scoped_action` saíram (fora por decisão da coordenação);
- **Parte 3 feita (2026-10-11): `cargo check -p bun_js_printer --features printer` com 0 erros e zero `unsafe`; eram 449 no começo da parte.** O `Printer` ganhou o campo
  `store: &'a Store` (o `Store` do arquivo, só leitura) e `Printer::init` o recebe (`print_ast` passa `&tree.store`,
  `print_with_writer_and_platform` passa `&ast.store`, `print_json` ganhou o parâmetro `store`). A escolha é de
  propósito: um empréstimo que passa por um `&'a Store` não prende o `self`, então `self.print(self.store.str(x))` e
  `let e = &self.store[r]; self.print_expr(..)` compilam; com o `Store` dentro do `Printer` (testado) o rustc já
  acusava 55 conflitos E0502 só nos primeiros diagnósticos. O preço é que o printer não grava nós. O que gravava:
  (1) a transformação `var a = o.x, b = o.y` para `var {x: a, y: b} = o`, que montava um `B::Object` e `E::String`
  temporários, virou `print_destructuring_group`, que imprime direto de (nome no `Store`, posição, binding), com a
  chave UTF-8 de `print_binding_utf8_key` (extraída do `print_binding`, que a usa também); (2) `import_member_binding`
  monta o `Expr` por valor (`EImportIdentifier` é inline); (3) **lacuna registrada:** a dobra de template com
  `mangled_props` (partes `ENameOfSymbol` viram strings e o template é dobrado por `Template::fold` numa cópia local)
  saiu: grava nós no `Store`, e o `bun build` não liga `mangled_props` (a conferir no oráculo). Para voltar, o `Store`
  do printer teria de aceitar nós novos (um `Store` de rascunho com índices separados) ou o fold virar uma função
  pura sobre valores. O `scripts/store_fix.py` rodou três rodadas sobre os diagnósticos (E0609, E0599, E0277, E0616:
  `r.campo` virou `self.store[r].campo`, `r.slice()` virou `self.store.str(r)`).
  **Parte 3a, renamer (feita, 0 erros em `renamer.rs`):** `NameStr` é `Arc<[u8]>` (dono dos bytes, `Clone` barato, hash
  e igualdade pelos bytes; `Arc` e não `Rc` porque o `NestedRenamer` do bundler lê o `NumberRenamer` de várias
  threads). `NoOpRenamer`, `MinifyRenamer` e `NumberRenamer` carregam `&'a Store` para ler `original_name`;
  `ScopeUses` também, e `sees` e `assign_names_recursive` recebem `StoreRef<Scope>` (o `*const Scope` do
  `NestedRenamer` virou `Option<StoreRef<Scope>>`). Os mapas de símbolos deixam de ser `ManuallyDrop`: os renamers
  são donos, sem `owns_symbols` nem `Drop`. `find_unused_name` devolve sempre o nome tomado (sem o `Option` que
  distinguia "igual ao original"; custa uma alocação por nome distinto, que o interner já exigia). `TinyString` é
  `Inline` até 15 bytes ou `Arc<[u8]>`; `NameMinifier::number_to_minified_name` é infalível, então
  `assign_names_by_frequency`, `finish` e `ExportRenamer::next_minified_name` deixam de devolver `Result`;
  `ExportRenamer` perde a arena e devolve `Vec<u8>`. `NestedRenamer::new` perde o parâmetro da arena. Em `lib.rs`,
  `print_ast` deixa de reapontar o `parent` dos filhos do escopo do módulo (o índice do pai já vale) e passa
  `&tree.store` aos renamers; `print_json`, `print` e `print_with_writer*` ganham `'a` no `Ast` e no `Store`.
  **Parte 3b, zero `unsafe` (feita, 0 `unsafe` em `js_printer`, sem `#![feature]`):** `Encoding` deixou de ser parâmetro
  const (`ConstParamTy`, a feature `adt_const_params`) e virou argumento de execução de
  `write_pre_quoted_string_inner` e `write_json_string`; a fachada const `write_pre_quoted_string`, que só
  repassava, saiu. Os seis `unsafe impl bytemuck` do `ModuleInfo` não tinham uso e saíram. O `Writer` perdeu
  `reserve`/`advance` (ponteiro cru em `*mut u8`): `WriterTrait::reserve_capacity` só pré-dimensiona e
  `write_reserved` é `print_slice` mais o erro do escritor, sem `reserve_spare_bytes` nem `commit_spare`. Os três
  ponteiros apagados viraram objetos: `SourceMapHandler` e `RequireOrImportMetaCallback` guardam um
  `Box<dyn ... + 'a>` (com `impl` para `&mut T`; `Box` e não `&'a mut dyn` porque atrás de `&mut` o `'a` seria
  invariante e `print_ast` precisa encolher o `Options<'a>` para criar o renamer na pilha), e
  `runtime_transpiler_cache` é `Option<&'a mut RuntimeTranspilerCache>`. `Renamer` guarda só empréstimo
  compartilhado, é `Copy` e `name_for_symbol(self)` devolve `&'r [u8]`, o que dispensou o helper do `Printer` que
  descolava o empréstimo com ponteiro cru: os 30 chamadores usam `self.renamer.name_for_symbol` direto. `slice_of`
  (que o `store_fix.py` deixou com `self` num fn livre) saiu: `self.store[x]` (ou `tree.store`/`ast.store` nos drivers).
  **Parte 3c, `BackRef`:** os três usos saíram sem cópia: com o `Renamer` `Copy` (`symbols(self)` devolve `&'r Map`),
  `self.renamer.symbols().get_const(..)` já não prende `self`, e a chave de `commonjs_named_exports` é `&'a`. O helper
  `Printer::symbols`, que só repassava, saiu.
  **Partes 3d a 3f, `EString`, `StoreStr` e campos de `s::*`:** o `flattened(&self, &mut Store)` do `EString` grava a
  cadeia de cordas no `Store`, que o printer (só `&Store`) não tem; o `EString::flat(&self, &'s Store)` devolve um
  `FlatString<'s>` (UTF-8 emprestado, UTF-8 de corda juntado num `Cow::Owned`, ou `&[u16]`) com `is_utf8`, `slice8`,
  `slice16`, `string` e `prefer_template`, e `print_string_literal_e_string`, `print_string_characters_e_string` e
  `best_quote_char_for_e_string` passaram a recebê-lo. O resto foi mecânico: `StoreStr` virou `self.store.str(x)`
  (nome do `EDot`, `value` de `EBigInt` e `RegExp`, `comment` de `EInlinedEnum`, caminho do import record, alias),
  `Raw(raw)` do template também, `Option<StoreRef<Scope>>` e `StoreRef<If>`/`For`/... viraram `self.store[r]`
  (`wrap_to_avoid_ambiguous_else` e `is_identifier_or_numeric_constant_or_property_access` recebem o `Store`),
  `InlinedEnumValueDecoded::String` já era `StoreRef<EString>` (sem `NonNull::from` nem `cast_mut`), o
  `left_binary` do visitor binário é o `StoreRef<Binary>` e não uma cópia do nó, e as chamadas ao `ModuleInfo`
  copiam `self.store` antes de `self.module_info()` (que empresta o `self` inteiro). `Path::is_file` entrou no
  `bun_ast` (namespace vazio ou `file`). Os testes do workspace (com e sem a feature) seguem verdes.
  **Próximo (parte 4):** golden byte a byte contra `bun build --no-bundle` do oráculo, e o teste do mapa de posições.

Os `Define` do parser guardam `ExprData` com índices: como cada arquivo tem o seu `Store`, o valor de um define precisa
ser gravado no `Store` do arquivo quando o parser o usa.

Ordem de mapa: a tabela de membros do escopo usa o `hashbrown` 0.15.5 do upstream com o mesmo hash, então a
iteração (que o renamer e o `p.rs` percorrem) tem a ordem do upstream. O `wyhash` sem o `write_length_prefix` só
muda o hash dos `HashMap` do porte, cuja ordem não sai na saída; isso se confere de novo quando o renamer entrar.

## Arena tipada: o lugar do `StoreRef` e do `StoreSlice`

No upstream, os nós da AST vivem num bloco do mimalloc liberado de uma vez. `StoreRef<T>` é um `NonNull<T>` com
`Deref` e `DerefMut` (`nodes.rs`), `StoreSlice<T>` é um par ponteiro e tamanho, e `StoreStr` é o mesmo para bytes.
`expr::Data` e `stmt::Data` guardam esses ponteiros por valor e são `Copy`; `Expr::Data::clone_in` copia o nó com
`ptr::read`, e as duas cópias passam a apontar para os mesmos vetores internos. Tudo isso é `unsafe` porque a
validade do ponteiro e a ausência de `&mut` duplicado são contratos de quem chama, não do tipo.

No porte, o ponteiro vira índice e a memória vira um dono explícito:

- **Índice.** `StoreRef<T>` é `{ index: NonZeroU32, PhantomData<fn() -> T> }`: 4 bytes, `Copy`, com o nicho que
  mantém `Option<StoreRef<T>>` do mesmo tamanho. O índice é a posição mais um no vetor do tipo `T`. A igualdade é a
  do índice, como a do upstream é a do endereço. `StoreSlice<T>` é `{ start: u32, len: u32 }`, uma faixa contígua do
  mesmo vetor, e `StoreStr` é a mesma faixa sobre o vetor de bytes. `truncate` só encurta a faixa, como no upstream.
- **Dono.** `new_store::Store` tem um `Vec<T>` por tipo de nó e um `Vec<u8>` para as strings. Os campos vêm de uma
  macro (`define_store!`), e cada tipo portado entra com uma linha; o traço `Stored` liga o tipo ao seu vetor, e
  `Index`/`IndexMut` sobre `StoreRef<T>` e `StoreSlice<T>` dão `store[r].op` e `store[s][i]`. Um `Store` por arquivo
  analisado: o parser é o dono durante a análise, entrega o `Store` ao resultado (`Ast`), e o printer recebe
  `&Store` junto com a AST. O `reset()` do upstream vira soltar o `Store`. Não há estado global nem `thread_local`.
- **`clone_in` sem aliasing escondido.** Copiar um nó é `store.alloc(store[r].clone())`: o nó novo ganha outro
  índice, e as faixas internas são copiadas como números. As duas cópias continuam vendo os mesmos elementos, que é
  o comportamento observável do upstream, mas qualquer escrita passa pelo `&mut Store`, então o compilador
  garante que não existem dois `&mut` ao mesmo tempo.
- **Escrita durante a visita.** O upstream escreve no nó enquanto visita os filhos (`b.left = p.visit_expr(b.left)`).
  Com índice, o padrão é copiar o filho para fora (`Expr` e `Stmt` são `Copy`), visitar com `&mut` no dono e
  devolver: `let left = p.store[b].left; let left = p.visit_expr(left); p.store[b].left = left;`. A ordem das
  visitas e o texto gerado não mudam.
- **Lugar tipado (`bun_ast::place`).** O upstream guarda `StoreRef::from_bump(&mut node.campo)`, um ponteiro para um
  campo dentro de outro nó, e escreve por ele enquanto aloca e visita. Com índices não há ponteiro para campo: o
  "lugar" é o par (nó dono, qual campo), resolvido contra o `Store` a cada leitura (`get`) e a cada escrita (`set`),
  sem empréstimo de pé entre as duas. Há dois tipos, `ExprPlace` (`E::Dot.target`, `E::Binary.left`, o valor de
  `SExpr`, `Throw` e `Return`, o `test` de `If` e `Switch`, o valor de uma `Decl`) e `PropertyListPlace`
  (`properties` de `E::JSXElement` e de `E::Object`, com `push` e `take`). Regra de escolha: copiar o valor para
  fora, mexer e gravar de volta basta onde o código lê, muda e grava numa janela curta e nada mais alcança o campo
  (a visita de `substitute_single_use_symbol_in_stmt` grava `result` no campo antes, visita a cópia e grava o que
  ela deixou: o dono é o statement que o chamador processa, e nada o lê durante a visita); onde o ponteiro para o
  campo vive por mais de uma alocação (a caminhada pela cadeia de `E::Dot` e de `E::Binary` do `maybe_defined_helper`,
  a descida pelos spreads de `<div {...{...x}}>`), o lugar é o tipo certo. Cada variante tem teste em
  `ast/tests/place.rs`. O `from_bump(&mut T)` que só embrulhava uma alocação (`ClassStaticBlock`) virou
  `store.alloc`.
- **Listas.** O upstream monta a lista num `ArenaVec` e a vaza para um `StoreSlice`. No porte ela é montada num
  `Vec` local e copiada em bloco para o vetor do tipo (`store.alloc_slice`), o que dá a mesma faixa contígua.
- **Constantes pré-carregadas.** O upstream aponta `StoreRef::from_static` para nós `'static` (as tabelas de
  `Prefill`). No porte, `Store::new` grava esses nós primeiro, em posições fixas, e as constantes são índices
  `const` para essas posições; nenhum nó é compartilhado entre `Store`s.
- **Strings de literais.** `StoreStr` de um literal do programa (`"default"`, `"exports"`) é gravado no vetor de
  bytes na primeira vez em que o parser o pede; as fatias do código-fonte são gravadas uma vez, com o fonte inteiro
  no começo do vetor, e viram faixas dele.
- **Consequências.** As asserções de tamanho do upstream (`expr::Data` com 12 bytes, `Expr` com 16) mudam, porque o
  índice tem 4 bytes e não 8; o tamanho não aparece na saída, então a fidelidade não muda. As funções do upstream
  que recebem só o nó passam a receber também `&Store` ou `&mut Store`; essa é a mudança sistemática do porte nos
  arquivos de `ast`, `js_parser` e `js_printer`.
- **Listas de trabalho que saíam do parser como `&'a mut [T]`.** Quem as consome sem guardá-las no nó recebe
  `Vec<T>` ou `&[T]` (`lower_class` devolve `Vec<Stmt>`, `append_part` lê `&[Stmt]`, `parse_arrow_body` e
  `generate_closure_for_type_script_namespace_or_enum` recebem `Vec`). Quem guarda a lista no nó a grava com
  `store.alloc_slice` no ponto em que o nó nasce: `ImportClause.items` e `ExportClauseResult.clauses` já são
  `StoreSlice<ClauseItem>` (o `'a` desses dois tipos saiu), e `Part.scopes` é um `Vec<StoreRef<Scope>>`.
- **`ScopeOrder.scope` é `StoreRef<Scope>`** (Copy), e o escopo do módulo nasce no `Store` do arquivo
  (`define.store.clone()` antes do `P`). `scope_ref()` saiu: era um repasse sobre o campo. O `'arena` do
  `ScopeOrder` fica até as faixas `&'a [ScopeOrder<'a>]` (`scope_order_to_visit` e `scopes_in_order_for_enum`)
  migrarem para o `Store`.
## `Define` com um `Store` por arquivo (passo 2, decidido (B))

No upstream, `DefineData.value` é um `ExprData` cujos `StoreRef` apontam para nós de vida de processo (tabelas
`'static`, o `bump` do bundler ou o `Store` de thread onde o `parse_env_json` monta objetos). O `Define` é montado
uma vez, fica só leitura e é lido por todos os arquivos, inclusive em threads diferentes (os `unsafe impl
Send/Sync` de `DefineData` e de `defines_table.rs` existem para isso). O `value_for_define` devolve o próprio
`ExprData`: todos os usos de um define com objeto ou lista apontam para o mesmo nó; o caso `EString` já copia o nó
(`new_expr(&*str_)`).

No porte não há nó fora de um `Store`, e um índice só vale no `Store` em que nasceu. Duas formas fiéis:

- **(A) Cópia no uso.** O `Define` é dono de um `Store` próprio (o `define_store`), e `DefineData.value` é um
  `ExprData` com índices nele. O `value_for_define` copia o valor em profundidade do `define_store` para o `Store`
  do arquivo a cada uso. Custo só onde o define aparece; cada uso ganha um nó próprio, então dois usos do mesmo
  define com objeto deixam de compartilhar o nó (só observável se o visitante alterar um deles e o outro refletir a
  alteração, o que no upstream seria um efeito colateral de compartilhamento).
- **(B) Prefixo pré-carregado.** O `define_store` é montado a partir de `Store::new()` (com o `Prefill` nas
  posições fixas), e o `Store` de cada arquivo nasce como cópia dele (`define.store.clone()`). Os índices de
  `DefineData.value` valem em qualquer `Store` de arquivo sem conversão, o `value_for_define` fica igual ao do
  upstream, e dentro de um arquivo os usos compartilham o nó como lá. É a mesma técnica das constantes
  pré-carregadas acima. Custo: todo arquivo paga a cópia do `define_store` inteiro, usado ou não; com os defines
  padrão (literais curtos e a tabela de globais puros, quase toda sem valor) são poucas centenas de bytes, mas um
  `--define` com objeto JSON grande pesa em cada arquivo.

Nas duas, o `Define` deixa de precisar de `unsafe impl Send/Sync`: um `Store` é `Send + Sync` por construção, e
`OnceLock` guarda a tabela de globais puros sem o embrulho `SyncDefineData`. Os nomes e a estrutura do upstream
(`DefineData`, `DotDefine`, `Define`, `Flags`) ficam; muda só a origem dos índices.

Decidido: (B), pela coordenação. Feito em `js_parser/src/lib.rs` (`mod defines`, com `Define.store`) e
`defines_table.rs` (sem o `SyncDefineData` e os seus dois `unsafe impl`), com teste em `defines_tests.rs`. O
`DefineData::init_static_string` recebe o `store` do `Define` no lugar do `&'static E::EString`. O `Store` e o
`ArrayHashMap` ganharam `Clone`. Com defines grandes (um `--define` de objeto JSON grande) a cópia por arquivo vira
custo, e nesse dia a (A) volta à mesa. O `Prefill` em posições fixas ainda não existe; quando entrar, o
`define_store` passa a nascer de `Store::new()`.

## Estado por arquivo

| Crate | Arquivo do upstream | Estado | O que mudou |
|---|---|---|---|
| `bun_ast` | `op.rs` | portado | nada (cópia) |
| `bun_ast` | `target.rs` | portado | nada (cópia) |
| `bun_ast` | `nodes.rs` | parcial: `AssignTarget`, `OptionalChain`, `StoreRef`, `StoreSlice`, `StoreStr`, os nomes de índice e lista, `LocRef`, `ClauseItem`, `EnumValue`, `Catch`, `Finally`, `Case`, `ArrayBinding`, `StmtOrExpr` | os três `Store*` viram índices (seção "Arena tipada"); `ExprNodeList` é `StoreSlice<Expr>` |
| `bun_ast` | `new_store.rs` | substituído | a arena de blocos do mimalloc vira `Store` com um `Vec` por tipo (`define_store!`), sem `unsafe`; ganha um `Vec<u16>` para as strings UTF-16 e `append` (o `VecExt::append` das listas) |
| `bun_ast` | `store.rs` (novo) | o `Store` concreto | uma linha por tipo de nó, de payload (`S::*`, `B::*`, o `expr::Data` do `SLazyExport`) e de lista |
| `bun_ast` | `lib.rs` | parcial: `RefTag`, `Ref`, `Loc`, `Range`, `flags`, os reexports | trecho igual ao upstream, menos `range_of_identifier`, que pede as tabelas de identificador; o `#[derive(EnumTag)]` do `bun_core` vira a macro local `enum_tag!`, e os acessores `e_*`/`s_*` saem de `data_accessors!`; o `DebugOnlyDisabler` (guarda do `Store` de thread) não tem papel sem `Store` global |
| `bun_ast` | `base.rs` | portado | nada (cópia), menos o travessão de um comentário |
| `bun_ast` | `symbol.rs` | parcial: `Symbol`, `SymbolFlags`, `SlotNamespace`, `Kind`, `Use` | `namespace_alias` é `Option<Box<_>>` no lugar do `AstBox`; o `Map` de duas camadas do bundler (com `unsafe`) fica fora, o transpilador usa a tabela plana |
| `bun_ast` | `g.rs` | portado | listas viram faixas do `Store`; `can_be_moved` e `init_return_expr` recebem o `Store`; os `deep_clone` entram com o de `expr.rs` |
| `bun_ast` | `ts.rs` | portado | `TSNamespaceScope.property_accesses` e `TSNamespaceMemberMap` são o `StringArrayHashMap` do porte, sem contexto nem alocador genéricos; o mapa compartilhado entre namespaces irmãos é o mesmo índice no `Store` |
| `bun_ast` | `scope.rs` | portado, com teste dos membros | os nomes dos membros são `StoreStr`, então as buscas recebem `&Store` e `get_or_put_hashed`/`put` deixam de ser `unsafe` (o nome já é uma faixa do `Store`); `AstBox`/`AstVec` viram `Box`/`Vec`; a tabela é o `hashbrown` 0.15.5 do upstream com o mesmo hash, para a ordem de iteração ser a dele; `recursive_set_strict_mode` recebe o `Store` e o índice; saem os sinônimos de repasse `Members::count` e `Scope::get_member_hash` e o `get_or_put_member_with_hash` (repasse para `members`) |
| `bun_ast` | `use_directive.rs` | portado (cópia) | nada |
| `bun_ast` | `import_record.rs` | portado (cópia) | o `bun_paths::fs::Path` vira um `Path` local com `pretty` e `text` como faixas do `Store` e namespace `&'static`; `original_path` é `StoreStr`; `List` é `Vec` |
| `bun_ast` | `lib.rs` (`ImportKind`, `part`) | portado | nada |
| `bun_ast` | `runtime.rs` | portado (cópia), com o teste do upstream | sem `Runtime::source_code` (o `runtime.out.js` embutido, só do bundler); saem os repasses `count`, `get_ptr` e `contains` do `ReplaceableExportMap`, que já tem `Deref` para o mapa |
| `bun_ast` | `ast_result.rs` | portado | sem tempo de vida: listas em `Vec`, mapas no `ArrayHashMap`/`HashMap` do porte; `module_scope` é `Option<StoreRef<Scope>>` (o escopo vive no `Store` que o parser entrega com a `Ast`); `empty_in(arena)` vira `Default`, e `from_parts` recebe o `Vec` |
| `bun_ast` | `char_freq.rs` | portado (cópia) | nada |
| `bun_ast` | `nodes.rs` (`SlotCounts`, `NameMinifier`) | portado | as duas funções de nome curto do upstream (o mesmo laço com cabeça e cauda diferentes) viram uma só com a tabela como parâmetro, e perdem o `Result` de alocação |
| `bun_ast` | `nodes.rs` (`Part`, `DeclaredSymbolList`, `ExportsKind`, `TlaCheck`, `InlinedEnumValue`, `NamedImport`, `NamedExport`, `Dependency`, `PropertyUse`) | portado | `DeclaredSymbolList` troca o `MultiArrayList` por dois `Vec` (as mesmas colunas); `Part.scopes` é `Vec<StoreRef<Scope>>`; o `InlinedEnumValue` codifica o índice da `E::String` no lugar do endereço, com o mesmo NaN-boxing para números; os métodos de capacidade perdem o `Result` de alocação que nunca falha aqui |
| `bun_ast` | `nodes.rs` (`StrictModeKind`) | portado | nada |
| `bun_ast` | `e.rs` | portado, menos os nós de JSON | ver "e.rs" abaixo |
| `bun_ast` | `expr.rs` | parcial: `Expr`, `Query`, `IntoExprData`, `init`, `Tag`, `PrimitiveType`, `Data`, os acessores, `can_be_const_value`, `can_be_moved`, `is_safe_to_string`, `EFlags`, `Equality`, `LooseEql`/`StrictEql` e `Data::eql` (com teste) | `init` recebe o `Store`; `allocate` (via bump) some, há um dono só; `Data` mantém 12 bytes e `Expr` 16; o `EqlParser` dá o `Store` no lugar da arena, e achatar a corda de um `EString` é copiar o nó, achatar e gravar de volta no mesmo índice; `known_primitive` e `to_string_expr_without_side_effects` recebem o `Store` (o texto fixo, como `"null"`, é gravado nele). Pendentes: `deep_clone`, `write_to_hasher`, as outras dobras e o acesso por caminho |
| `bun_ast` | `fold_string_addition.rs` | portado, com teste | as cordas são índices no `Store` e cada nó é alterado no próprio índice; `concat_parts` copia as partes para o `Store` (`alloc_slice`) no lugar do `ArenaVec` vazado |
| `bun_core` | `util.rs` (`StackCheck`) | portado, com teste | sem `asm!` nem `pthread_getattr_np`: a posição é o endereço de uma variável local e o fim da pilha é a posição na configuração da thread menos o tamanho dela (`RLIMIT_STACK` de `/proc/self/limits` na principal, ou o tamanho dado em `configure_thread_with_size`); o limiar de 128 KiB é o do upstream |
| `bun_ast` | `transpiler_cache.rs` | parcial, com teste | sem o `bun_jsc` o despacho é o caso `r#impl == None` do upstream (cache desligado: `get` nunca acerta, `put` guarda a saída quando há `input_hash`); saem `entry` e `r#impl`, e o `get` perde as `parser_options` sem tipo que só o despacho lia |
| `bun_ast` | `nodes.rs` (`Flags`) | portado | o `pub use crate::flags as Flags` do upstream |
| `bun_ast` | `s.rs` | portado (cópia) | todos os tipos derivam `Clone` para o `clone_in` (inserido por ferramenta), `ExportDefault::can_be_moved` recebe o `Store` |
| `bun_ast` | `stmt.rs` | portado | `alloc` recebe o `Store`; `allocate`/`allocate_expr` (bump) somem; `Data` tem 8 bytes e `Stmt` 12 (16 no upstream) |
| `bun_ast` | `b.rs` | portado, menos `write_to_hasher` | `B` tem 8 bytes e `Binding` 12; os acessores de lista recebem o `Store` |
| `bun_ast` | `binding.rs` | portado | `ToExprWrapper<C>` guarda um `fn(&mut C, Loc, Ref) -> Expr` no lugar do trampolim com `*mut c_void` e do `BackRef` do bump; o `Store` vem do contexto (`C: AsMut<Store>`) |
| `bun_ast` | `nodes.rs` (`ImportItemStatus`) | portado | nada |
| `bun_ast` | `loader.rs` | portado (cópia) | sem os sinônimos de grafia `is_type_script` e `is_java_script_like` (repasse de uma linha, proibido no repositório); um travessão de comentário trocado |
| `bun_core` | `assert_ffi_discr!` (de `src/opaque/lib.rs`) | portado | nada (cópia) |
| `bun_ast` | `lexer_tables.rs` | portado (cópia), com os testes do upstream | `is_identifier_continue` vira reexportação de `is_identifier_part` (era repasse de uma linha); travessões trocados por ferramenta |
| `bun_core` | `identifier.rs` (`string/identifier.rs`) | portado (cópia) | as tabelas de duas etapas do Unicode e `is_identifier`/`is_identifier_utf16` sobre o `CodepointIterator` do porte; travessões trocados por ferramenta |
| `bun_core` | `lexer_tables` (de `string/mod.rs`) | portado | as 9 palavras reservadas do modo estrito; o módulo `lexer` vizinho fica fora (só repasses) |
| `bun_wyhash` | `lib.rs` | portado, com os 8 testes do upstream (vetores do wyhash e códigos do SMHasher) | as 5 leituras desalinhadas com `unsafe` viram `from_le_bytes` sobre a fatia (`read4`/`read8` deixam de existir, o `read_bytes` do módulo cobre os dois); sai o `#![feature(hasher_prefixfree_extras)]` e o `write_length_prefix` vazio do `OneShotHasher`, o que muda só o hash interno dos mapas; travessões trocados por ferramenta |
| `bun_collections` | `array_hash_map.rs` | parcial: `ArrayHashMap` e `StringArrayHashMap` (alias de `ArrayHashMap<Box<[u8]>, V>`) | entradas em dois `Vec` na ordem de inserção, com a remoção por troca do Zig, e índice no `HashMap` da biblioteca padrão no lugar do `ArrayHashMap` com 25 trechos `unsafe`; a ordem observável é a mesma. Sem contexto nem alocador genéricos, sem o mapa sem caixa do Windows e sem os sinônimos `contains_key` e `put_assume_capacity` |
| `bun_core` | `comptime_string_map.rs` | portado, com os 8 testes do upstream | sem o `impl HasLength for &crate::String`, que volta com o porte de `bun_core::String` |
| `bun_core` | `strings` (`string/immutable.rs`, `string/immutable/unicode.rs`, `strings_impl` de `lib.rs`) | parcial: ASCII, igualdade e ordem, surrogates, WTF-8, `CodepointIterator`, UTF-8 para UTF-16 e volta, as buscas do lexer (`index_of*`, `contains`, `has_prefix_with_word_boundary`, `is_all_whitespace`, `is_unicode_space_separator`, `peek_n_codepoints_wtf8`, `lexer_step`), `copy_utf16_into_utf8` | onde o upstream chama o simdutf, roda o laço escalar que o próprio upstream usa quando o simdutf recusa a entrada; para entrada válida os dois dão as mesmas unidades, então o porte usa sempre o laço. As buscas SIMD do highway viram laços escalares com o mesmo resultado; o `copy_nonoverlapping` de `next_codepoint_multibyte` vira `copy_from_slice`. `eql_long` compara fatias no lugar da leitura por palavras com ponteiro. `to_utf8_alloc_with_type` (repasse de uma linha) vira reexportação de `to_utf8_alloc` |
| `bun_core_macros` | `comptime_string_map.rs` | portado | nada (cópia) |
| `bun_core` | `fmt.rs` | parcial: `FormatDouble`, `hex_digit_value(_u32)`, `ParseIntError`, `parse_int`, `parse_double` | o `WTF__dtoa` do WebKit vira os dígitos mais curtos do formatador do Rust montados pela regra do Number::toString do ECMAScript, com testes contra o `String(n)` do JavaScript. O `WTF__parseDouble` (fast_float do WebKit, por FFI) vira o reconhecimento do mesmo prefixo (`-` opcional, dígitos com ponto, expoente só com dígito) seguido da conversão do Rust, que arredonda corretamente como o fast_float |
| `bun_core` | `util.rs`, `result.rs` e `lib.rs` | parcial: `GenericIndex`, `GenericIndexOptional`, `trim`, `impl_tag_error!`, os reexports `parse_int`, `ParseIntError` e `wtf::parse_double` | nada (cópia); `Environment` só com `IS_NATIVE = true` (o porte roda nativo) |
| `bun_core_macros` | `lib.rs` | parcial: só os dois proc-macros do mapa | `pretty_fmt!` e os derives de contagem de referência entram quando pedidos |
| `bun_collections` | `bit_set.rs` | parcial: `IntegerBitSet` | nada (cópia do subconjunto que o lexer usa) |
| `bun_collections` | `array_hash_map.rs` (`StringHashMap`, `StringSet`) | parcial, com teste | `StringHashMap` com chave `Box<[u8]>` (o `Hash` é o de `[u8]`, igual ao do `StringHashMapKey`, e o `hashbrown` é o 0.15.5, então a ordem de iteração é a do upstream) e `entry_ref` no lugar do `raw_entry_mut`; sem alocador genérico nem `put_static_key`. O `AllocError` passa a ser o do `bun_alloc` |
| `bun_collections` | `zig_hash_map.rs`, `identity_context.rs` | portado (cópia por Bash, travessões trocados por ferramenta), com teste | sai o `get_or_put_context` (repasse) e, do `identity_context`, os `ArrayIdentityContext*` (o `ArrayHashMap` do porte não tem contexto); o corte foi feito com um script Python, não com Edit |
| `bun_collections` | `vec_ext.rs` | parcial, com teste | só os métodos que fazem algo além de renomear um método do `Vec`; os sinônimos de uma linha (`append`, `slice`, `init_capacity` e afins) saem pela regra de repasse, e os chamadores passam ao nome do `Vec` no passo 3; os de ponteiro cru ficam fora |
| `bun_collections` | `pool.rs` (`object_pool!`, `ObjectPoolType`, `PoolGuard`) | parcial, com teste | a lista ligada de nós em ponteiro cru vira um `Vec<T>` por thread usado como pilha, com o mesmo `MAX_COUNT`; `INIT` é obrigatório e devolve `Self` (o upstream devolve `Result<Self, bun_core::Error>`; o único `INIT` do parser não falha) |
| `bun_alloc` | `lib.rs` | parcial: `Arena`, `ArenaVec`, `ArenaVecExt`, `ArenaString`, `arena_format!`, `vec_from_iter_in`, `AllocError`, com teste | o bump do mimalloc vira um `typed_arena` para bytes e outro para `u16`, sem `unsafe`; `ArenaVec` guarda um `Vec` comum e a arena (só tempo de vida e `bump()`); `into_bump_slice` só para `u8`/`u16`, as listas de nós vão ao `Store` por `into_vec` + `store.alloc_slice` |
| `bun_ast` | `lexer_log.rs` | portado (cópia) | nada |
| `bun_ast` | `lib.rs` (o `logger`) | parcial: `Kind`, `Level`, `Location`, `Data`, `Msg`, `Log`, `Path`, `Source`, `range_data`, `alloc_print` e as posições de erro | `Msg` sem `metadata`; o `Log` não tem o `LineColumnTracker` (só acelera a conta de linha, o resultado é o mesmo) nem o `tracked_range_data`, que virou a chamada direta de `range_data` (era repasse); `Span` entra em `nodes.rs` |
| `bun_js_parser` | `lexer.rs` | portado (cópia por Bash, travessões e en dashes trocados por ferramenta), com testes dos oito casos do plano | ver "lexer.rs" abaixo |
| `bun_options_types` | `jsx.rs`, `bundle_enums.rs` | portado (cópia por Bash, travessões trocados por ferramenta), com testes | nada; o `lib.rs` só declara estes dois e o `schema` |
| `bun_options_types` | `schema.rs` | parcial: `api::{Target, Loader, JsxRuntime, Jsx}` | só os tipos que `jsx.rs` e `bundle_enums.rs` leem; o resto do esquema é da CLI e do instalador |
| `bun_core` | `strings` (`split`, `tokenize`) | portado, com teste | o `next` inerente do `SplitIterator` e o do `Iterator` (repasse) viram um só, no `Iterator` |
| `bun_core` | `fmt` (`fmt_identifier`, `FormatValidIdentifier`) | portado, com teste | o `write_bytes` sem checagem vira `from_utf8` checado (só recebe codepoints de identificador); o `has_needed_gap`, que ninguém lia, sai |
| `bun_paths` | `lib.rs` (`NODE_MODULES_NEEDLE`, `fs::{PathName, Path}`) | parcial, com testes | só POSIX; `Path` só com `init` e `name`; `dir_with_trailing_slash` (ponteiro cru, o parser não usa) e `into_static` ficam fora |
| `bun_core` | `feature_flags.rs` | parcial: as quatro flags que o parser e o printer leem | nada |
| `bun_core` | `output.rs` (`panic`, `error_writer`, `todo_panic!`) | parcial, com teste | `error_writer` é o `stderr` do processo (o upstream lê o fluxo do thread-local da CLI); o `todo_panic!` não marca o contador do relatório de falha |

### `lexer.rs`

- O `log` é `&'a mut Log` com dono único, no lugar do `NonNull<Log>` compartilhado com o parser; o parser chega a ele
  por `p.lexer.log()`. Os dois acessos `unsafe` ao ponteiro e o `get_unchecked` de `next_codepoint_with` somem.
- As fatias que viram `StoreStr` (comentários preservados, argumentos de pragma, `sourceMappingURL`, literal ASCII) são
  faixas do fonte, que ocupa o começo do vetor de bytes do `Store` (`source_str`). Por isso `PragmaArg::scan` e
  `scan_source_mapping_url_value` recebem `chunk_start`, o deslocamento do pedaço no fonte. O `range` deles segue a
  conta do upstream, que só serve às mensagens de erro.
- `to_e_string` e `to_utf8_e_string` recebem o `Store` e gravam nele o texto decodificado.
- Os três kernels do highway (`index_of_newline_or_non_ascii_or_hash_or_at` e os dois "interesting character")
  viram laços escalares com a mesma semântica do C++.
- Sai `oom_from_alloc!(Error)`: sem alocador falível no porte, não há `AllocError` para converter.

### `e.rs`

- Os nós de JSON (`JsonValue`, `JsonTape`, `TapeAlloc`, `ObjectJSON`, `ArrayJSON`, com o `unsafe` deles) ficam fora até o
  loader de JSON; `expr::Tag` mantém `EObjectJSON` e `EArrayJSON` para os discriminantes não mudarem, e só o `Data`
  não tem as variantes.
- `Rope.next` é `Option<StoreRef<Rope>>`, no lugar do `*mut Rope`; `next_ref` (o único `unsafe` do `Rope`) some.
- `EString` deriva `Copy` (todos os campos são índices e escalares) e `shallow_clone` fica; `Flattened` vira a cópia
  por valor. Com `is_utf16`, `data` é faixa do vetor UTF-16 do `Store`, e `slice16` lê dele sem `unsafe`.
- `init`, `init_utf16` e `init_re_encode_utf8` recebem faixas já gravadas no `Store`; `Number::to_string` grava o
  texto no `Store` (as tabelas de -100 a 100 do upstream só evitavam alocação).
- Os métodos que mudam nó guardado (`Object::put`, `get_or_put_object`, `Array::push`, `Template::fold`) recebem
  `(store, this: StoreRef<_>)`; a corda de `fold` é montada numa cópia da `EString` e gravada de volta.
- `alphabetize_*` e `package_json_sort` usam o `sort_by` estável, que dá a mesma ordem do `index_sort` estável.
- Pendente: o `Display` de `EString`. `EString::hash` entrou com o `bun_wyhash`, `is_identifier` com as tabelas
  do lexer e `Import::import_record_loader` com o `loader.rs`; os três recebem o `Store`.
