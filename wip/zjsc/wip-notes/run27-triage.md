# Triagem da run 27 (/tmp/zjsc10-run27.txt)

O log tinha 1718 linhas na leitura final e ainda crescia (parou em `builtins_bun_golden`). Falhas vistas até aí:

| Alvo | Resultado | Classe | Causa |
|---|---|---|---|
| `accessor_bun_golden` | 2 de 2999 | (a) golden velho | O snapshot da run tinha, nas linhas 2604 e 2615 de `accessor_bun.tsv`, o esperado `"<undefined>"` para `'abc'.length=1` em modo estrito. O golden em `wip/zjsc/tests/golden/accessor_bun.tsv` (02:41:56) já foi regenerado e traz `T:TypeError: Attempted to assign to readonly property.`, igual ao que o porte devolve. Só precisa de nova run. |
| `array_edge_bun_golden` | "golden com só 587 programas" | (c) harness | O mínimo do teste era 600, mas o gerador, depois de descartar repetidos e os que já estão em `array_bun.tsv`, deixa 587. Mínimo baixado para 580 em `tests/array_edge_bun_golden.rs`. |
| `base64_globals_bun_golden` | 10 de 183 | (b) bug do runtime | `DOMException is not defined`. Ver abaixo. A última divergência (linha/coluna de `atob` via `Reflect.apply`, esperado 4:20, veio 1:11) é outra coisa, ver "Pendente". |
| `array_exotic_bun_golden` | 2 de 9172 | (b) bug do runtime | Mesmo sintoma do `array_readonly_length_messages`, ver abaixo. |
| `array_readonly_length_messages` | 1 asserção | (b) bug do runtime | Única diferença entre `left` (porte) e `right` (bun): `proto pop`. O porte devolve `ok`; o bun lança `TypeError: Array length is not writable`. |

## DOMException (corrigido)

`install_dom_exception` (`src/runtime/js_dom_exception.rs`) e `install_global`
(`src/runtime/native_class_support.rs`, usado por `TextEncoder`, `TextDecoder` e `performance.rs`) gravavam o
global com `global_object.global_this().put_direct(...)`. Só que `global_this` é um `JSGlobalProxy`
(`js_global_object.rs`, `set_global_this`), e `putDirect` no proxy não encaminha ao alvo: a propriedade ficava
no proxy e a resolução de identificador, que consulta o objeto global, nunca a via. Todo o resto de
`js_global_object_init.rs` usa `global_object.put_direct`. O mesmo `dom_exception_bun_golden` já falhava 217 de
219 na run 26.

Correção: os dois pontos agora fazem `global_object.put_direct(vm, &key, value, 0)`. O mesmo conserto vale para
`TextEncoder`, `TextDecoder` e `Performance*`, que passam por `install_global`. Falta rodar para confirmar.

A duplicata `key` de `js_dom_exception.rs` saiu: agora é `property_key as key` de `native_class_support`.

## proto pop (corrigido, falta rodar)

Causa: `set_length` de `array_prototype.rs` (cópia do `setLength` de ArrayPrototypeInlines.h) no caminho não-`isJSArray` chamava `JSObject::put` direto, pulando `JSArray::put`; o `Array.prototype` (`DerivedArrayType`) nunca via o `length` somente leitura. Agora usa `put_through_method_table` (o `methodTable()->put` do C++). Texto original da investigação abaixo.

Programa: `Object.defineProperty(Array.prototype, 'length', {value: 4, writable: false}); Array.prototype.pop()`.
No JSC o `pop` cai no caminho lento (get 3, delete 3) e `setLength(3, true)` bate no `lengthIsReadOnly` do mapa
esparso: `Array length is not writable`. O porte devolve `undefined`.

Conferido e fiel ao C++ (`JSArray.cpp`): `JSArray::pop` (`src/runtime/js_array.rs:678`), `set_length` e
`set_length_with_array_storage` (`js_array.rs:430-520`), `set_length_writable`, `is_length_writable`,
`array_proto_func_pop` (`array_prototype.rs:789`). O desvio então está antes: ou o `defineOwnProperty` de
`length` do `Array.prototype` não deixa o `length` somente leitura no mapa esparso (e o 4 chega com o array
ainda em `ArrayClass`/`Undecided`), ou `Array.prototype` não é visto por `JSArray::from_value` e cai num caminho
que ignora o erro. Próximo passo: logar `indexing_type` e `is_length_writable()` do `Array.prototype` logo após o
`defineProperty` (`JSArray::defineOwnProperty`, caso `length`, em `js_array.rs`).

## Pendente

- Linha/coluna do erro de `atob('!')` chamado por `Reflect.apply(atob, null, ['!'])`: esperado `line 4, column 20`
  (posição do `Reflect.apply` no programa multilinha), veio `1:11`. O `throw_dom_exception_from_host` de
  `js_dom_exception.rs` provavelmente pega o frame errado quando o chamador é um nativo (`Reflect.apply`) e não
  o código JS. Só dá para avaliar depois da correção do global.
- Comentário de `tests/base64_globals_bun_golden.rs` ainda diz que `DOMException` não existe no porte; ficou
  velho (o gerador agora cobre a classe). Atualizar o cabeçalho quando a run passar.
- Alvos depois de `bigint_static_props` ainda não tinham saído no log.
