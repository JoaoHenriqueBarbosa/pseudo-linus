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
  (regra do repositório), por ferramenta, na hora da cópia. Nenhum outro texto muda.

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
| `bun_ast` | `nodes.rs` | parcial: `AssignTarget`, `OptionalChain`, `StoreRef`, `StoreSlice`, `StoreStr` | os três `Store*` viram índices (seção "Arena tipada"); o arquivo cresce item a item |
| `bun_ast` | `new_store.rs` | substituído | a arena de blocos do mimalloc vira `Store` com um `Vec` por tipo (`define_store!`), sem `unsafe` |
| `bun_ast` | `lib.rs` | parcial: `RefTag`, `Ref`, `Loc`, `Range` | trecho igual ao upstream, menos `range_of_identifier`, que pede o iterador de codepoints e as tabelas de identificador do `bun_core`; o logger e `Source` entram em seguida |
| `bun_ast` | `base.rs` | portado | nada (cópia), menos o travessão de um comentário |
| `bun_ast` | `symbol.rs` | parcial: `Symbol`, `SymbolFlags`, `SlotNamespace`, `Kind`, `Use` | `namespace_alias` é `Option<Box<_>>` no lugar do `AstBox`; o `Map` de duas camadas do bundler (com `unsafe`) fica fora, o transpilador usa a tabela plana |
| `bun_ast` | `g.rs` | parcial: `NamespaceAlias` | o arquivo cresce item a item |
| `bun_ast` | `nodes.rs` (`ImportItemStatus`) | portado | nada |
| `bun_ast` | `loader.rs` | pendente | pede `bun_core::assert_ffi_discr!` e `bun_collections::StringArrayHashMap` |
| `bun_core` | `comptime_string_map.rs` | portado, com os 8 testes do upstream | sem o `impl HasLength for &crate::String`, que volta com o porte de `bun_core::String` |
| `bun_core` | `strings` (`string/immutable.rs`, `string/immutable/unicode.rs`, `strings_impl` de `lib.rs`) | parcial: ASCII, igualdade e ordem, surrogates, WTF-8, `CodepointIterator`, UTF-8 para UTF-16 e volta | onde o upstream chama o simdutf, roda o laço escalar que o próprio upstream usa quando o simdutf recusa a entrada; para entrada válida os dois dão as mesmas unidades, então o porte usa sempre o laço. `eql_long` compara fatias no lugar da leitura por palavras com ponteiro |
| `bun_core_macros` | `comptime_string_map.rs` | portado | nada (cópia) |
| `bun_core_macros` | `lib.rs` | parcial: só os dois proc-macros do mapa | `pretty_fmt!` e os derives de contagem de referência entram quando pedidos |
