# Auditoria de buffers (ArrayBuffer, SharedArrayBuffer, TypedArray, DataView)

## Golden novo

- `scripts/gen-buffer-golden.js` (bun 1.4.2) gera `tests/golden/buffer_bun.tsv` com 3922 programas.
- `tests/buffer_bun_golden.rs` roda o golden no padrão de `tests/function_error_bun_golden.rs` (global `R`).
- Complementa `buffers_bun.tsv` e `typedarray_more_bun.tsv`. Cobre: `maxByteLength`, `resize`, `transfer`,
  `transferToFixedLength`, `detached`, `slice`; SharedArrayBuffer `grow`/`growable`; TypedArray e DataView
  length-tracking com encolhe/cresce, iteração e callbacks que redimensionam; getters/setters do DataView com
  offsets e mensagens exatas; Float16Array e `Math.f16round` (arredondamento nos limites); `from`/`of`/`set`/
  `subarray`/`copyWithin`/`toSorted`/`toReversed`/`with`/`findLast`; `fromBase64`/`toBase64`/`fromHex`/`toHex`/
  `setFromBase64`/`setFromHex` (o bun 1.4.2 tem todos); Atomics (inclui `pause`, `waitAsync`); `structuredClone`
  de buffers; mensagens exatas de buffer detached.
- Todos os programas são autocontidos (IIFE) e o gerador avalia no mesmo processo do bun, sem caminho de máquina.

## Estado

Nenhum cargo, rustc nem teste foi rodado (regra da tarefa), então o golden novo ainda não foi medido contra o motor.

## Leitura do código contra o golden

Conferidas por busca as mensagens que o golden espera: `Receiver must be ArrayBuffer/SharedArrayBuffer`,
`Receiver is detached`, `ArrayBuffer resize failed with new byte length N`, `grow failed with new byte length N`,
as quatro mensagens de `fromBase64`/`fromHex`/`setFromHex` (inclusive o `Uint8Array.prototype.fromHex` do bun, que
aparece assim também em `uint8_array_base64.rs`), `calling X constructor without new is invalid`,
`X cannot be negative` e `larger than (2 ** 53) - 1` (por `js_value_conversions.rs`),
`DataView.prototype.X expects |this| to be a DataView object`. Nenhuma divergência óbvia encontrada, então nenhuma
edição foi feita em `src/runtime`.

## Mensagens do golden sem equivalente literal no código (a conferir quando rodar)

- `The object can not be cloned.` e `Transfer list contains duplicate ArrayBuffer` (structuredClone, global do bun).
- `Receiver of DataView method must be a DataView`, `Receiver should be a typed array view` (conferir a origem).
- `TypedArray.prototype.reduce callback must be a function` e `... of empty array with no initial value`
  (existem só no builtins JS com outro texto: conferir o caminho de TypedArray).
- `Atomics is not a constructor (evaluating 'new Atomics()')` e `Atomics is not a function. (In 'Atomics()', ...)`.
- `null is not an object (evaluating 'u.set(null)')`.
- `Atomics.pause argument needs to be either undefined or integer number`.

## Próximo passo

Rodar `cargo test --test buffer_bun_golden` e triar as divergências por grupo.
