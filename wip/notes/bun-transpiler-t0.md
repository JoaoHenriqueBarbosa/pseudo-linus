# Transpilador TS/JSX do bun no zjsc: degrau T0 (medição do `unsafe` e do grafo)

Medido em 2026-10-10 sobre o tag `bun-v1.4.2` de `oven-sh/bun`, commit
`744846f844374847c902b5e7fd59b4342a51ef99` (tarball do codeload com sha256
`25e09a8804535b8fbea1dad9f95af8402fc9133d28127d5594ff00df1549590e`). Os fontes ficaram fora do repositório, em
cache local, tratados como dado não confiável: só os `Cargo.toml` e o texto dos `.rs` foram lidos, nada foi
executado. Esta nota é o critério de saída do T0 do `bun-on-zjsc-plan.md` (seção 5).

## 1. O grafo real é maior que o plano supunha

O plano contou o `unsafe` só dentro de `js_parser`, `js_printer` e `ast` (177 linhas) e listou a base que eles
importam por contagem de uso. O fecho transitivo das dependências declaradas nos `Cargo.toml` dá **44 crates,
297.936 linhas e 3.553 linhas com `unsafe`**. Ele inclui crates `*_sys` de FFI com bibliotecas C e C++:
`bun_mimalloc_sys`, `bun_simdutf_sys`, `bun_boringssl_sys`, `bun_libuv_sys`, `bun_uws_sys`, `bun_zlib_sys`,
`bun_zstd` e `bun_highway` (SIMD).

Parte do fecho entra só por arestas finas e se corta sem perda de fidelidade no texto gerado:

| Aresta | Usos nos três crates | O que puxa |
|---|---|---|
| `bun_crash_handler` (parser 5, printer 5) | `scoped_action` e `handle_oom` | `bun_io`, `bun_zlib`, `bun_threading`, `bun_sys`, `bun_analytics` |
| `bun_io` (printer 4) | saída em buffer | `bun_uws_sys` (uSockets, BoringSSL), `bun_spawn_sys` |
| `bun_sys` (ast 10), `bun_perf` (ast 2) | caminhos e tempo | `bun_errno`, `bun_libuv_sys`, `bun_windows_sys` |
| `bun_react_compiler` (parser 17) | o React Compiler (58.168 linhas) | só a base |

Sem essas arestas, o mínimo que o transpilador precisa é: `js_parser`, `js_printer`, `ast`, `bun_alloc`,
`bun_core`, `bun_collections`, `bun_ptr`, `bun_paths`, `bun_wyhash`, `bun_sourcemap`, `bun_base64`, `bun_url`,
`bun_options_types`, `bun_highway`, `bun_dispatch`, `bun_opaque`, `bun_hash`, `bun_output_tags`,
`bun_core_macros`, `bun_simdutf_sys` e `bun_mimalloc_sys`. São **cerca de 152.700 linhas e 1.610 linhas com
`unsafe`**, nove vezes o que o plano estimou.

## 2. Classificação do `unsafe` por padrão (linhas não comentadas)

| Crate | Linhas `unsafe` | FFI | `unsafe impl` Send/Sync | fatia ou str crua | sem checagem | ponteiro cru | `unsafe fn` | outros |
|---|---|---|---|---|---|---|---|---|
| `js_parser` | 59 | 1 | 4 | 0 | 5 | 16 | 0 | 33 |
| `js_printer` | 16 | 0 | 6 | 0 | 0 | 6 | 0 | 4 |
| `ast` | 80 | 2 | 15 | 3 | 1 | 16 | 5 | 38 |
| `bun_alloc` | 214 | 9 | 18 | 17 | 5 | 55 | 20 | 90 |
| `bun_core` | 500 | 66 | 57 | 34 | 9 | 128 | 25 | 180 |
| `bun_collections` | 188 | 0 | 7 | 6 | 8 | 62 | 22 | 83 |
| `bun_ptr` | 158 | 0 | 10 | 2 | 0 | 87 | 8 | 51 |
| `bun_paths` | 45 | 4 | 0 | 5 | 4 | 10 | 2 | 20 |
| `bun_wyhash` | 5 | 0 | 0 | 0 | 0 | 2 | 0 | 3 |
| `bun_sourcemap` | 62 | 1 | 2 | 0 | 0 | 40 | 3 | 16 |

O "outros" é quase todo bloco `unsafe { ... }` que derreferencia os ponteiros de arena da AST
(`StoreRef`/`StoreSlice`, 328 usos só em `ast`).

## 3. O que não sai por patch: o modelo de memória da AST

- **Arena com cópia bit a bit.** Os nós da AST vivem num `MimallocArena` (um `mi_heap_t` do mimalloc, por FFI) e
  são liberados em bloco, sem `Drop`. Os vetores internos usam `Vec<T, AstAlloc>`, cujo `deallocate` não faz nada.
  `Expr::Data::clone_in` copia os nós com `ptr::read` supondo que dois cabeçalhos de `Vec` podem apontar para o
  mesmo buffer (comentário de `bun_alloc/ast_alloc.rs`). É `unsafe` por construção: não há um ponto isolado para
  trocar, o invariante atravessa `ast`, `js_parser` e `js_printer`.
- **Rust nightly.** O tag fixa `nightly-2026-07-20` e usa `allocator_api` (cinco crates), `thread_local`,
  `adt_const_params`, `macro_metavar_expr`, `freeze` e `arbitrary_self_types_pointers`. O zjsc e o resto do
  workspace compilam no estável (rustc 1.99.0 da imagem `rust-mold`). `Vec<T, A>` com alocador próprio não existe no
  estável.
- **FFI na base.** `bun_alloc` chama o mimalloc; `bun_core` chama o simdutf (UTF-8/UTF-16) e o highway (busca de
  bytes com SIMD). Trocar por Rust puro é fiel no resultado, e é patch localizado.

## 4. Consequência para o plano

A seção 4 do plano supõe "fonte pristino mais patches aplicáveis em ordem". Isso funciona para o FFI da base e para
as arestas finas do item 1. Não funciona para o modelo de arena: tirar o `unsafe` dele é trocar ponteiro de arena
por índice em toda a AST, o que reescreve a maior parte dos `.rs` de `ast`, `js_parser` e `js_printer`. Um patch
desse tamanho não reaplica sobre a próxima versão do bun; na prática ele é um porte.

As opções, com a consequência de cada uma, estão no relato do T0 ao coordenador. Nada foi vendorado ainda.
