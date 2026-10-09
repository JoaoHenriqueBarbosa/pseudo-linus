# Golden subclass_edge_bun: 14 divergências em 1283

Saída analisada: `/tmp/now4_subclass_edge_bun_golden.txt`. Teste: `wip/zjsc/tests/subclass_edge_bun_golden.rs`.

## Causa 1: harness (3 casos), corrigido

- `new C(0)` com `toString` de subclasse de `Date`: o golden saiu de um bun em `America/Sao_Paulo`, e o motor
  sozinho resolve UTC. O teste agora chama `set_time_zone_spec_override(Some("America/Sao_Paulo"))`, como já
  faz `date_edge_bun_golden.rs`.
- `String(Date(0))`: `Date()` sem `new` devolve a hora do relógio, então o esperado é a hora da geração do
  golden e nunca bate. O teste pula esse programa (não-determinístico, não é limitação do oráculo).

## Causa 2: comportamento do bun sobre o JavaScriptCore (11 casos), reproduzido

Pela regra do projeto o oráculo é o JSC do bun, inclusive as partes que o bun acrescenta. As cinco divergências
foram portadas (sem rodar cargo; falta compilar e rodar o golden):

- `Object.getOwnPropertyNames(Error)` termina em `appendStackTrace,prepareStackTrace` (`error_natives.rs`,
  `init_error_classes`). `prepareStackTrace` nasce `undefined`. A semântica de `appendStackTrace(source,
  destination)` (acrescenta as linhas de frame de `source` ao `stack` de `destination`) é dedução, sem medição
  própria: os goldens só provam a presença da chave; medir no bun e ajustar.
- O cabeçalho `name: message` do `stack` de um `ErrorInstance` é montado na leitura (`materialize_stack` usa
  `error_header_of_object`, o `Error.prototype.toString` do objeto no momento), não mais pelo tipo interno.
- `Error.captureStackTrace(err)` em `ErrorInstance` troca os frames e refaz o cabeçalho na próxima leitura
  (`replace_pending_stack`); em objeto simples o cabeçalho é sempre `Error`, sem `message`.
- `captureStackTrace` com argumento que não é objeto lança `TypeError: invalid_argument`
  (`CAPTURE_STACK_TRACE_NOT_OBJECT`).
- `Reflect.construct(Error, [..], F)` com `F.prototype` fora da cadeia de `Error.prototype` não guarda frames, então
  `e.stack` fica indefinido (`prototype_chain_contains` em `create_error_from_arguments`).

## Rodada 6: 3 de 1282 divergem (`/tmp/now6_subclass_edge_bun_golden.txt`), corrigido sem compilar

- `Reflect.construct(Error, [..], F)` com `F` fora da pilha: o `bun` não deixa `stack` própria (`Object.getOwnPropertyNames`
  dá só `message`), seja qual for o `F.prototype`. A regra do `prototype_chain_contains` estava errada (removida, com
  `structure_prototype`, que era código morto). Agora `create_error_from_arguments` não guarda frames quando
  `subclass_caller` existe, a lista saiu vazia (o caller não está na pilha) e o tipo é `Error`. `TypeError` etc. com
  `newTarget` classe mantêm `stack` (medido).
- Cabeçalho de `stack` com `get name()`/`get message()`: o `bun` não chama o acessor e trata como ausente
  (`Error: m`); `String(e)` continua chamando o getter. `error_header_of_object` ganhou o parâmetro `invoke_getters`
  (falso no `materialize_stack`, lê por `PropertySlot` `VMInquiry`).
- Pendente: compilar e rodar o golden; confirmar que `new K` (`class K extends Error`) com frames vazios por
  `stackTraceLimit` não é afetado (o filtro `limit != 0` já devolve `None` antes).

Pendente anterior: conferir `stack_format`, `error_message` e `error_edge` depois de compilar (o cabeçalho agora lê `name` e
`message` por propriedade, o que pode acionar getters).
