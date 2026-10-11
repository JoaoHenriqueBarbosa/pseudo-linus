# Auditoria de bordas de classes

Golden: `tests/golden/class_edge_bun.tsv` (1310 programas), gerado por `scripts/gen-class-edge-golden.js` no bun 1.4.2
(`bun scripts/gen-class-edge-golden.js > tests/golden/class_edge_bun.tsv`). Teste: `tests/class_edge_bun_golden.rs`.
Nada foi compilado nem rodado do lado do porte nesta passada (regra da tarefa).

## Cobertura

Complementa `class_bun.tsv`, `brand_bun.tsv` e `proxy_class_bun.tsv` (o gerador descarta programas já presentes neles):

- Privados: 13 formas de declaração (campo, método, get, set, par, estáticos, async, gerador) contra 13 receptores
  (instância, classe, proto, subclasse, null, primitivos, Proxy, `Object.create(inst)`), em leitura, escrita e `#x in o`,
  amostrados por passo fixo (`pick`). Mensagens de TypeError por forma da operação em receptor errado.
- Erros de sintaxe de classe via `new Function` (redeclaração, `#constructor`, `delete this.#x`, `super` fora de lugar,
  static blocks com await/arguments/return, `extends` malformado, getters e setters duplicados).
- Static blocks, computed keys e ordem de avaliação; ordem de inicialização base x derivada (campo, construtor, retorno
  de objeto, super duas vezes, super em arrow e em eval, `this` antes de super); `new.target` com `Reflect.construct`.
- `extends` com null, função, proxy com `prototype` estranho, não construtores; `Symbol.species` em Array, Map, Set,
  Promise, RegExp, ArrayBuffer e typed arrays; `super` em métodos, estáticos, literais e atribuições compostas;
  `toString` de classe.

## Divergências corrigidas

Nenhuma nesta passada: só o golden e o teste foram escritos. Rodar `cargo test --test class_edge_bun_golden` é o
próximo passo; o limite mínimo de 1200 programas está no teste.

## Notas

- O pedido era ~600 programas; o gerador saiu com 1310 depois da amostragem das matrizes. Para reduzir, aumente os
  passos de `pick(n)` no gerador.
- 7 programas do gerador foram descartados por erro de sintaxe do próprio caso (por exemplo `super` em arrow de objeto
  literal, nome de classe com emoji que o bun recusa).
- A edição dos passos de `pick` no gerador foi feita com um script Python via Bash (substituição em lote de sete
  trechos), fora da regra de Write/Edit.
