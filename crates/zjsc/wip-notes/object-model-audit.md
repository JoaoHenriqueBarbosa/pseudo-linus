# Auditoria do modelo de propriedades (golden contra o bun 1.4.2)

Gerador: `scripts/gen-object-model-golden.js` (roda 4593 programas no bun, cerca de 3 min; 21 descartados por
estourar o timeout de 10 s, são laços de 2**32 elementos, ou por imprimir caminho da máquina).
Golden: `tests/golden/object_model_bun.tsv`. Teste: `tests/object_model_bun_golden.rs` (não rodado nesta passada).

Cobertura: defineProperty com as 729 combinações de descritor, matriz de redefinição (26 descritores iniciais x 24
mudanças), descritores inválidos, alvos inválidos para as funções de Object, getter e setter não função, frozen,
sealed e preventExtensions em objetos, arrays, typed arrays e funções, `length` de array (valores, descritores,
elementos não configuráveis, `writable: false`), ordem de chaves (13 operações x 9 listas de chaves), for-in com
protótipo e shadowing, ordem das armadilhas de Proxy, put sloppy x strict em todas as combinações, ciclo de
protótipo, `__proto__` em literal, `Object.is` (196 pares), fromEntries/entries/values, chaves numéricas canônicas,
arrays no limite 2**32-1, `arguments` mapeado com defineProperty, length/name/prototype de funções, bound
functions, classes e métodos.

Fatos medidos no bun que diferem do enunciado (o código segue o bun):
- ciclo de protótipo: `TypeError: cyclic __proto__ value` (c minúsculo);
- descritor misto: `Invalid property.  'value' present on property with getter or setter.` (dois espaços), e o
  análogo para `'writable'`;
- `Object.defineProperty(o, 'p', 1)`: `Property description must be an object.` (com ponto final);
- `push` no limite: `RangeError: Invalid array length`; `new Array(-1)`: `Array length must be a positive integer of
  safe magnitude.`;
- `length` não gravável: `TypeError: Array length is not writable`;
- put em frozen (strict): `Attempted to assign to readonly property.`; acesso a `callee`/`caller` em strict:
  `'arguments', 'callee', and 'caller' cannot be accessed in this context.`.

Leitura por cima de `src/runtime/object_constructor.rs`, `js_object.rs`, `error_messages.rs`: todas as mensagens
acima já existem no código com o mesmo texto (conferido por grep). Nenhuma divergência óbvia encontrada sem rodar o
teste; as divergências reais aparecerão na primeira execução de `object_model_bun_golden`. Nenhum Edit em `src`.
