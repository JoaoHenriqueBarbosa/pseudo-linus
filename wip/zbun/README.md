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
| `bun_ast` | `nodes.rs` (`StrictModeKind`) | portado | nada |
| `bun_ast` | `e.rs` | portado, menos os nós de JSON | ver "e.rs" abaixo |
| `bun_ast` | `expr.rs` | parcial: `Expr`, `Query`, `IntoExprData`, `init`, `Tag`, `PrimitiveType`, `Data`, os acessores, `can_be_const_value`, `can_be_moved`, `is_safe_to_string` | `init` recebe o `Store`; `allocate` (via bump) some, há um dono só; `Data` mantém 12 bytes e `Expr` 16. Pendentes: `deep_clone`, `write_to_hasher`, `known_primitive`, as dobras e o acesso por caminho |
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
| `bun_collections` | `array_hash_map.rs` | parcial: `StringArrayHashMap` | entradas em dois `Vec` na ordem de inserção, com a remoção por troca do Zig, e índice no `HashMap` da biblioteca padrão no lugar do `ArrayHashMap` com 25 trechos `unsafe`; a ordem observável é a mesma. Sem contexto nem alocador genéricos, sem o mapa sem caixa do Windows e sem os sinônimos `contains_key` e `put_assume_capacity` |
| `bun_core` | `comptime_string_map.rs` | portado, com os 8 testes do upstream | sem o `impl HasLength for &crate::String`, que volta com o porte de `bun_core::String` |
| `bun_core` | `strings` (`string/immutable.rs`, `string/immutable/unicode.rs`, `strings_impl` de `lib.rs`) | parcial: ASCII, igualdade e ordem, surrogates, WTF-8, `CodepointIterator`, UTF-8 para UTF-16 e volta, as buscas do lexer (`index_of*`, `contains`, `has_prefix_with_word_boundary`, `is_all_whitespace`, `is_unicode_space_separator`, `peek_n_codepoints_wtf8`, `lexer_step`), `copy_utf16_into_utf8` | onde o upstream chama o simdutf, roda o laço escalar que o próprio upstream usa quando o simdutf recusa a entrada; para entrada válida os dois dão as mesmas unidades, então o porte usa sempre o laço. As buscas SIMD do highway viram laços escalares com o mesmo resultado; o `copy_nonoverlapping` de `next_codepoint_multibyte` vira `copy_from_slice`. `eql_long` compara fatias no lugar da leitura por palavras com ponteiro. `to_utf8_alloc_with_type` (repasse de uma linha) vira reexportação de `to_utf8_alloc` |
| `bun_core_macros` | `comptime_string_map.rs` | portado | nada (cópia) |
| `bun_core` | `fmt.rs` | parcial: `FormatDouble`, `hex_digit_value(_u32)`, `ParseIntError`, `parse_int`, `parse_double` | o `WTF__dtoa` do WebKit vira os dígitos mais curtos do formatador do Rust montados pela regra do Number::toString do ECMAScript, com testes contra o `String(n)` do JavaScript. O `WTF__parseDouble` (fast_float do WebKit, por FFI) vira o reconhecimento do mesmo prefixo (`-` opcional, dígitos com ponto, expoente só com dígito) seguido da conversão do Rust, que arredonda corretamente como o fast_float |
| `bun_core` | `util.rs`, `result.rs` e `lib.rs` | parcial: `GenericIndex`, `GenericIndexOptional`, `trim`, `impl_tag_error!`, os reexports `parse_int`, `ParseIntError` e `wtf::parse_double` | nada (cópia); `Environment` só com `IS_NATIVE = true` (o porte roda nativo) |
| `bun_core_macros` | `lib.rs` | parcial: só os dois proc-macros do mapa | `pretty_fmt!` e os derives de contagem de referência entram quando pedidos |
| `bun_collections` | `bit_set.rs` | parcial: `IntegerBitSet` | nada (cópia do subconjunto que o lexer usa) |
| `bun_alloc` | `lib.rs` | parcial: `Arena` | o bump do mimalloc vira um `typed_arena` para bytes e outro para `u16`, sem `unsafe`; `alloc_slice_copy` e `alloc_slice_fill_default` |
| `bun_ast` | `lexer_log.rs` | portado (cópia) | nada |
| `bun_ast` | `lib.rs` (o `logger`) | parcial: `Kind`, `Level`, `Location`, `Data`, `Msg`, `Log`, `Path`, `Source`, `range_data`, `alloc_print` e as posições de erro | `Msg` sem `metadata`; o `Log` não tem o `LineColumnTracker` (só acelera a conta de linha, o resultado é o mesmo) nem o `tracked_range_data`, que virou a chamada direta de `range_data` (era repasse); `Span` entra em `nodes.rs` |
| `bun_js_parser` | `lexer.rs` | portado (cópia por Bash, travessões e en dashes trocados por ferramenta), com testes dos oito casos do plano | ver "lexer.rs" abaixo |

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
