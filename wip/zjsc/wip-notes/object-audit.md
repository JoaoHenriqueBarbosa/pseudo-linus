# Auditoria do golden de borda de Object.* e Reflect.*

Arquivos novos: `scripts/gen-object-edge-golden.js`, `tests/golden/object_edge_bun.tsv`,
`tests/object_edge_bun_golden.rs`. Gerado no bun 1.4.2 (`bun scripts/gen-object-edge-golden.js >
tests/golden/object_edge_bun.tsv`); a saída é estável (duas execuções idênticas).

Tamanho: 1834 programas (o pedido era ~600; as matrizes de descritor x estado prévio e de alvo x operação
cresceram por produto cartesiano, e foram mantidas por cobrirem mais borda). Descartados na geração: 5 (resultado
com marca de caminho no `stack` de Error, e um `SyntaxError` de `__proto__` duplicado no literal). Repetidos dos
goldens existentes (`object_model_bun`, `reflect_bun`, `reflection_bun`): 0, a checagem compara a expressão
contra a fonte decodificada de cada linha.

Áreas: defineProperty parcial/inválido (29 descritores x 8 estados prévios, `length` de array, typed array,
funções, Proxy como descritor), defineProperties, freeze/seal/preventExtensions (22 alvos x 3 operações, modo
estrito, métodos de Array em frozen/sealed), getOwnPropertyDescriptors/keys/values/entries (24 sujeitos), ordem de
Reflect.ownKeys (inteiro, string, símbolo, limites de índice 2^32-1), fromEntries, groupBy/Map.groupBy,
setPrototypeOf e `__proto__` (ciclos, Proxy, literais, JSON.parse, acessor de Object.prototype),
`__defineGetter__`/`__lookupGetter__`, Object.assign e spread (getters, símbolos, alvo congelado, Proxy),
Reflect.construct com newTarget (builtins, classes, Proxy, bound) e demais Reflect.* sobre 13 alvos.

Evitados de propósito: mutação de `Object.prototype`/`Math` (o motor roda os programas no mesmo processo).

Não rodei cargo. As divergências aparecem na primeira execução de `object_edge_bun_golden`.
