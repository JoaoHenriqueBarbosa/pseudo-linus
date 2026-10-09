Erros da rodada 4 (geração de bytecode)

Causa externa, não resolvida:
- `bytecode_generator_cpp4.rs` `add_big_int_constant` (791, 795, 796): `JSBigInt::parse_int` devolve
  `ImplResult` (enum Empty/Heap/BigInt32), mas `big_int_map` e `add_constant_value` trabalham com
  `JSValue`, que ainda não tem célula de BigInt (`js_value.rs` só cobre string/número/booleano).
  Fecha quando `JSValue` ganhar BigInt (FATIA2 de `js_big_int.rs`: `ImplResult` vira `JSValue`).

Já corrigidos por outros agentes antes desta passada (linhas medidas defasadas): cpp5 (as_ref,
Option vs &RegisterRef), part3 (this_register com from_index, clone), generator.rs (generate sem
argumento e tupla).

Corrigidos agora: cpp6 (lista de template strings via borrow de `node`/`next`), generatorification
(empréstimos disjuntos por `&mut **generator`), bytecode_use_def (braço `_` inalcançável removido).
