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

## Estado por arquivo

| Crate | Arquivo do upstream | Estado | O que mudou |
|---|---|---|---|
| `bun_ast` | `op.rs` | portado | nada (cópia) |
| `bun_ast` | `target.rs` | portado | nada (cópia) |
| `bun_ast` | `nodes.rs` | parcial: `AssignTarget` | o arquivo cresce item a item, na ordem em que os outros módulos pedem |
| `bun_ast` | `lib.rs` | parcial: `RefTag` e `Ref` | trecho igual ao upstream; o resto do arquivo (logger, `Source`, `Range`, `Loc`) entra em seguida |
| `bun_ast` | `base.rs` | pendente | pede `symbol.rs` |
| `bun_ast` | `loader.rs` | pendente | pede `bun_core::assert_ffi_discr!` e `bun_collections::StringArrayHashMap` |
| `bun_core` | `comptime_string_map.rs` | portado, com os 8 testes do upstream | sem o `impl HasLength for &crate::String`, que volta com o porte de `bun_core::String` |
| `bun_core_macros` | `comptime_string_map.rs` | portado | nada (cópia) |
| `bun_core_macros` | `lib.rs` | parcial: só os dois proc-macros do mapa | `pretty_fmt!` e os derives de contagem de referência entram quando pedidos |
