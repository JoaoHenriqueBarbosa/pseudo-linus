# Plano do WebAssembly GC contra o bun

## Triagem de /tmp/now3_wasm_gc_bun_golden.txt (build antigo, 361 de 868 divergem)

Agrupado por causa raiz (a saída é dominada por pânicos, não por mensagens de validação):

| Causa | Casos | Situação |
|---|---|---|
| Tipo de bloco de um byte lido como índice de tipo (`anyref` 0x6e, `eqref`, `i31ref`, `structref`, `arrayref`, `nullref`...): `rtt(4294967278)` em `wasm_module_information.rs:294` | 251 + 31 + 12 (len 15, 28, 8) | Corrigido em `read_block_type` (`wasm_ipint.rs`): todo byte `0x41..=0x7f` (exceto `0x63`/`0x64`) é tipo de valor |
| Importação JS com resultado múltiplo (`Thrown::Unported`) | 31 | Corrigido: `multi_result_values` em `js_web_assembly.rs` (iterableToList, tamanho "Incorrect number of values returned to Wasm from JS", conversão por tipo) |
| `to_js_value` com referência não nula (função, exnref, ponte entre instâncias) | 10 | Pendente: `js_web_assembly.rs`, ramo final de `to_js_value`; precisa de acesso à instância dona da função |
| `gc_object_to_wasm` com tipo heap definido sem instância (Table/Global/importação) | 2 | Pendente: exige o `ModuleInformation` do tipo, hoje só há o abstrato |

Resíduos que dependem de rerodar (os pânicos escondiam): `ref.cast failed...` esperado em 24 casos,
`unsupported instruction 0x14`/`0xd6` (5 e 3 casos, vieram do bun como esperados de validação),
`Stack overflow` (4), `null is not an object` / `undefined is not an object` (4).

## Próximo passo

Rerodar `wasm_gc_bun_golden` (build novo) e reagrupar o que sobrar; as três primeiras causas acima não foram
verificadas por execução (sem cargo nesta rodada).

## Rodada de 2026-10-08 (sem cargo)

- Overflow aritmético: o `bytes()` de `PageCount` já satura; o que restava era `memory.maximum().page_count() as usize * 65536`
  no buffer redimensionável de `Memory` (`js_web_assembly.rs`), agora `usize::try_from(maximum().bytes())` com saturação.
  No bun 1.4.2 `address: 'i64'` está desligado ("requires Memory64 to be enabled"), então o caminho só dispara no porte.
- Ordem das promessas (`m1`/`p`, `imp2`/`after`): os casos não são de `WebAssembly.instantiate`, e sim de JSPI
  (`Suspending`/`promising`, linhas 467 a 515 de `wasm_js_bun.tsv`). Ordem medida no bun: importação que devolve
  valor síncrono ou `Promise.resolve(x)` ainda suspende e a promessa do `promising` assenta DEPOIS de `m1`
  (`a b c m1 p m2`); importação `async` com dois `await null` assenta depois de `m3`; importação síncrona sem
  `Suspending` assenta antes de `m1`. Pendente: `suspend`/`finish` em `js_web_assembly_jspi.rs` ligam `then` direto
  e resolvem a promessa de saída na mesma reação; o JSC passa por `promiseResolve` + um salto extra. Calibrar
  rodando o golden para ver o desvio de saltos de microtarefa.
- `to_js_value`/`to_wasm_value` para funcref não nulo: NÃO feito. Bloqueio de projeto: `func_ref` é só
  `FUNC_REF_TAG | índice local` (`wasm_instance.rs`), sem identidade de instância, e o `call_indirect` resolve o
  índice na instância corrente. Precisa de um registro global de funções (id global -> instância, índice) usado
  por `func_ref`, `call_indirect`, `ref.func` e Table/Global, mais cache `(instância, índice) -> ExportedFunction`
  em `create_exported_function` para identidade estável. Mensagem do bun a medir para função não wasm:
  "Argument value did not match the reference type" (já usada em Global).
