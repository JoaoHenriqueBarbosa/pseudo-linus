# Auditoria de acessores e descritores

Golden: `tests/golden/accessor_bun.tsv` (2995 programas, bun 1.4.2), gerado por `scripts/gen-getter-setter-golden.js`,
rodado por `tests/accessor_bun_golden.rs`. Não duplica `iterator_bun.tsv` nem `promise_bun.tsv`. O teste NÃO foi
rodado (regra da tarefa: sem cargo).

## Medições do bun que diferem do enunciado

- `Object.freeze`/`Object.seal` em `Uint8Array(2)` lança `TypeError: Attempting to store non-configurable property on a
  typed array at index: 0`, não `RangeError: Cannot freeze array buffer views with elements` (essa é mensagem do V8).
- Delete de propriedade não configurável em strict: `TypeError: Unable to delete property.`
- Set em acessor só com getter em strict: `TypeError: Attempted to assign to readonly property.`
- `__proto__` cíclico: `cyclic __proto__ value`; protótipo imutável: `Cannot set prototype of immutable prototype object`.

## Conferência das mensagens no porte

Todas as mensagens mais frequentes do golden existem literalmente em `src/runtime/error_messages.rs`,
`object_constructor.rs`, `js_array.rs`, `js_generic_typed_array_view.rs`, `reflect_object.rs` e `object_prototype.rs`
(`Attempting to change ...`, `Getter must be a function.`, `Invalid property.  'value' present on property with getter or
setter.`, `Properties can only be defined on Objects.`, `Array length is not writable`, etc.). Sem divergência de texto
encontrada por grep; a divergência de comportamento só aparece rodando o teste.

## Correção feita

- `object_constructor.rs` redefinia `READONLY_PROPERTY_WRITE_ERROR` localmente; passou a importar de
  `runtime::error_messages` (duplicata, regra DRY). Mudança sem efeito de comportamento.

## Pendências

- Rodar `accessor_bun_golden` e triar as divergências de comportamento (ordem de leitura dos campos do descritor,
  Proxy, `Reflect.set` com receiver, typed arrays).
- Possível pontos de atenção: mensagem com espaço final em `Unable to delete property. ` e `Attempting to define
  property on object that is not extensible. ` aparecem em alguns caminhos (Reflect/Proxy) e sem espaço em outros no bun.
