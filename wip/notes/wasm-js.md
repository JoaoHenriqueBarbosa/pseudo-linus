# wasm_js_bun_golden: 37 de 589 divergências (2026-10-08)

Fonte: `/tmp/now4_wasm_js_bun_golden.txt`. Nenhum cargo rodado; a correção abaixo não foi compilada.

## Classificação

1. Funcref não nulo entre instâncias/Table/Global (11): `Table.set/get` com função exportada, `Global anyfunc`,
   `exports.t.get(0)` e `ToJSValue` de referência não nula (panics `host_call.rs:202`). Pedem a ponte
   função wasm exportada <-> `JSWebAssemblyFunction` com identidade estável (`t.get(0) === exports.add`), mais
   `ToWebAssemblyValue` com a mensagem "Argument value did not match the reference type". Não feito: fatia grande.
2. Objeto GC em tipo de referência definido sem instância (4 GG + 1 XX): `ToWebAssemblyValue`/`ToJSValue` de
   struct/array em Table/Global/importação. Depende do item 1 (resolver o tipo definido na instância dona).
3. `Memory.toResizableBuffer`/`toFixedLengthBuffer` (7): CORRIGIDO por leitura (ver abaixo).
4. JSPI (11): `Suspending` com resultado múltiplo iterável (6 panics, `host_call.rs`), ordem de microtarefas
   (3 casos: `imp1/after/imp2`, `m1/p/m2`, `i64arg/i64neg`), thenable (`ok:0` em vez de `9`), `then` próprio
   (`ownthen` ok:77 em vez de 1, `then` chamado a mais), "JavaScript frames found" no caso `direct`, e o sufixo
   `(evaluating 'e.g(1)')` na mensagem de `SuspendError` (apêndice de erro do bun, não do JSC puro).
5. SIMD (3): `elementCount de v128` unreachable em `wasm_simd_opcodes.rs:35` e "pilha vazia" em `wasm_ipint.rs`
   (1380, 2208): operando v128 em lane/splat/const não tratado.
6. GC: `ref.cast` de null deveria lançar "access to a null reference" (veio "failed to cast"); `0xfb 27/20`
   (`any.convert_extern`/`ref.test`) devolve 0 em vez de 1.
7. Pilha: recursão de cauda `return_call` (`0x12`) com 1000000 iterações dá "Stack overflow" (esperado 2000000):
   `return_call` precisa reaproveitar o quadro.

## Corrigido: Memory.toResizableBuffer (src/runtime/js_web_assembly.rs)

C++ (`JSWebAssemblyMemory.cpp`): o getter `buffer` devolve o buffer já associado, de qualquer tipo, e só cria um de
comprimento fixo se não há nenhum; `toFixedLengthBuffer` e `toResizableBuffer` trocam o associado quando o tipo difere.
O porte tratava `buffer` como "fixo" e destacava o redimensionável a cada `m.buffer`, o que gerava
`r === m.buffer` falso, "Buffer is already detached" e `byteLength` 0 após `grow`. `memory_buffer` agora recebe
`Option<bool>` (`None` = getter), `toFixedLengthBuffer` ganhou corpo próprio, e o teto de `maxByteLength` é
`min(maximum, max_allocatable_bytes)` (4 GiB sem máximo, antes saía 2^64).

Nota: a edição foi aplicada com script Python (impedimento de tempo), não com Edit.

## Próximas fatias

Item 1 (identidade de funcref) destrava 11 + 5; depois o resultado múltiplo de `Suspending`, depois SIMD v128.

## Funcref não nulo: fatias (1), (2) e (3) feitas, (4) planejada (2026-10-08, sem cargo)

Fatia 3 feita (não compilada): `FuncRefRegistry::owners` guarda `Weak<Instance>` por `InstanceId`;
`register_instance(&Rc<Instance>)` (chamado em `link_instance`, `js_web_assembly.rs`, logo após o `Rc::new`) e
`instance_of(id)`. O `call_indirect`/`return_call_indirect` em `wasm_ipint.rs` resolve a dona, confere a assinatura
pelo `canonical_type_id` (o registro de tipos é global, vale entre módulos) com a info da dona, e devolve
`Outcome::CallOther { owner, callee, arguments }`, que `run_frames` executa com `owner.invoke` (resultado ou
`Resume::Raise`). `take_call_arguments` agora recebe a `ModuleInformation` da dona. Pendente na 3: instância criada
fora de `link_instance` (testes de `wasm_ipint`, que usam `Instance` sem `Rc`) não se registra, então vira
`BadSignature` ao ser alvo de outra; o `Weak` caído (instância liberada) também.
Pontos que a fatia 4 resolve em `js_web_assembly.rs`: linhas ~440, 518 (`ToJSValue`), 1206 e 1255 (Table), e o
`Thrown::Unported` que `host_call.rs:202` (`throw_thrown`) transforma em panic.

Feito em `src/wasm/wasm_instance.rs` e `wasm_ipint.rs` (não compilado; a edição foi aplicada com script Python por
pressa, não com Edit): uma referência de função é `FUNC_REF_TAG | posição` num `FuncRefRegistry` do thread que guarda
`(InstanceId, índice no espaço de funções)`, uma posição por par (identidade estável, como o `FuncRefTable` do C++).
`Instance::id` vem de `allocate_instance_id()`. API: `func_ref(instance, índice)`, `func_ref_target`,
`func_ref_local(instance, ref)`, `is_func_ref`, e o cache do wrapper JS por referência (`function_wrapper` /
`set_function_wrapper`, o `Instance::getFunctionWrapper`/`setFunctionWrapper`; guarda o `JSValue` codificado).
O `call_indirect` de função de outra instância continua em `BadSignature`.

Fatia 3 (Table/Global/call_indirect entre instâncias): o registro precisa guardar também a `Rc<Instance>` dona (ou
um `Weak`), para o `Outcome::Call { callee, arguments }` virar chamada na instância `owner`; a checagem de assinatura
compara `canonical_type_id` do tipo da função na instância dona com o `type_position` pedido (hoje só a local).
`ref.func`, `table.get/set/fill/grow` e `Global get/set` já passam pelo `u64`, nada a mudar neles.

Fatia 4 (`ToJSValue`/`ToWebAssemblyValue`, `host_call.rs:202`): `ToJSValue` de funcref não nulo: `function_wrapper(ref)`;
se não há, criar a `JSWebAssemblyFunction` (nome = índice, `length` = nº de parâmetros, chama a instância dona),
`set_function_wrapper` e devolver; null vira `js_null()`. `ToWebAssemblyValue`: se o `JSValue` é wrapper de função
wasm (procurar no mapa inverso wrapper -> referência, a criar junto do `set_function_wrapper`) e o tipo atende
(`reference_matches` com o tipo da Table/Global), devolve a referência; senão `TypeError`
"Argument value did not match the reference type". Pontos de uso: `Table.get/set/grow`, `Global` com `anyfunc`,
`WebAssembly.Table` com `element` inicial, importação de função wasm exportada (hoje `HostFunction`; deve reconhecer o
wrapper e chamar direto, `JSToWasm`/`WasmToJS` evitados).
Cache do wrapper é raiz de GC: o `u64` guardado no registro precisa ser marcado quando o GC existir.

## Funcref não nulo: fatia 4 feita (2026-10-08, sem cargo, não compilada)

`js_web_assembly.rs`: `exported_function_wrapper(global_object, instance, index)` é o único caminho que cria a função
exportada: consulta `function_wrapper(func_ref(id, index))`, senão `create_exported_function` + `set_function_wrapper`
(os exports também passam por ela, daí `t.get(0) === exports.add`). `to_js_value` de funcref não nulo chama
`func_ref_to_js` (`instance_of` + wrapper). `to_wasm_value` aceita função exportada em `funcref` abstrato via o mapa
inverso `EXPORTED_FUNCTIONS` (`exported_func_ref`); `to_wasm_value_in` confere tipo definido com
`Instance::gc_reference_matches`; o resto lança "Argument value did not match the reference type". Table
(construtor, `set`, `grow`) e Global (`funcref`) usam esse caminho; `can_store_js_value` saiu. Novos acessores
`Instance::id()` e `Instance::info()`. Pendente: tipo definido sem instância dona (Table/Global soltos) devolve o
TypeError; função importada de JS em funcref (`WebAssemblyWrapperFunction`) não existe; `exnref` ainda é `Unported`.
Falta rodar cargo e o golden wasm_js_bun.

## Funcref de import JS e tipo definido sem instância (2026-10-08, sem cargo, não compilada)

Função JS importada em funcref: não precisou de tipo novo. O `WebAssemblyWrapperFunction` do C++ é aqui o
`ExportedFunction` do índice importado: `exported_function_wrapper(instance, index)` cria uma função nativa (nome =
índice, `length` = parâmetros) e a guarda no cache do `func_ref`, então `exports.imp === exports.imp`,
`ref.func` de import e `Table.get` devolvem a mesma identidade. Chamá-la (do JS ou por `call_indirect` via
`Outcome::CallOther`) cai em `Instance::invoke`, que roteia o índice importado para o `HostFunction` de
`host_function_for` (`WasmToJS`: ToJSValue nos argumentos, `this` indefinido, ToWebAssemblyValue no resultado,
múltiplos resultados por iterável). Importar o wrapper em outro módulo chama direto a instância dona (assinatura
conferida em `read_imports`). Uma função JS pura continua sem entrar em Table/Global (o C++ também exige
`isWebAssemblyHostFunction`, "Argument value did not match the reference type").

Tipo definido sem instância dona: no C++ o RTT canônico é global (`TypeInformation`), então `toWebAssemblyValue` não precisa de instância. Aqui também: `is_subtype_index` e `is_strict_sub_rtt` viraram funções livres em
`wasm_module_information.rs` (liam só o registro global), `gc_cell_matches` perdeu o parâmetro `info`, e
`reference_matches_type(reference, ty)` (em `wasm_ipint.rs`, substitui `Instance::gc_reference_matches`) confere objeto
GC pelo heap compartilhado e função pela assinatura na instância dona (`instance_of`). Em `js_web_assembly.rs`
`to_wasm_value_in` e `to_wasm_value_owned` saíram; `to_wasm_value` cobre tudo, e `TableState`/`GlobalState` perderam o
campo `owner` (e `table_wrapper`/`global_wrapper` o parâmetro). Falta compilar, rodar o golden wasm_js_bun e conferir
que nenhum teste de `wasm_ipint` usava `gc_reference_matches` ou `ModuleInformation::is_subtype_index`.
