# Mapa de cobertura da linguagem (goldens contra o bun 1.4.2)

Gerado em 2026-10-08 por grep nos 100 `tests/golden/*.tsv` (`*_bun.tsv` mais `language`, `scope`, `statements`, `class`,
`control_flow`, `async`, `iterator`, `module`, `module_more`, `accessor`). Contagem = linhas (programas) que casam; é
uma estimativa por regex, não uma auditoria de asserções. Escopo: a LINGUAGEM, não os built-ins.

## Resumo por construção

| Área | Cobertura | Onde / observação |
|---|---|---|
| Operadores aritméticos, bitwise, `**` | boa (>5000 linhas com `**`, 434 com `>>>`) | `language`, `coercion`, `bigint*`; ordem de coerção operando a operando era rala |
| Coerção `valueOf`/`toPrimitive` | boa (1800 / 826) | `coercion`, `object_model` |
| BigInt misto | média (494) | `bigint_bun`, `bigint`; os erros "Invalid mix" por operador agora em `language_gap` |
| `in`, `instanceof`, `hasInstance` | boa (279 hasInstance) | `class`, `proxy` |
| Atribuição composta/lógica em membro, ordem de referência | FRACA (44 `]op=`, 2 `a[k()]=`, 11 `++` em membro, 161 `\|\|=`) | buraco A, coberto por `language_gap` |
| Destructuring (decl, atribuição, param, for-of, catch) | média em volume, FRACA em ordem de efeitos (IteratorClose, defaults, alvos membro, rest com proxy) | buraco B, coberto por `language_gap` |
| Parâmetros default, `arguments` mapeado, escopo de parâmetros | FRACA (21) | coberto por `language_gap` |
| Spread (chamada, array, objeto) | boa (811) | `language`, `iterator` |
| Generators (`yield*`, return/throw) | boa (310 / 769) | `iterator`, `control_flow` |
| async/await, `for await` | boa (289 `for await`, 623 async gen) | `async`, `promise`; ordem de microtasks em `async_bun` |
| Classes (herança, super, accessors, static, privados, brand, `new.target`, `extends null`, species) | boa (3265 linhas em `class_bun`; 390 `new.target`, 40 `extends null`, 822 species) | `class`, `brand`, `typedarray` |
| Labels, switch, try/catch/finally (break/continue/return em finally) | boa (1690 / 345 / 4092) | `control_flow`, `statements` |
| Getters/setters/computed | boa (2995 em `accessor_bun`, 299 computed) | `accessor` |
| Tagged templates (cache de strings) | boa (347 / 627 raw) | `language`, `string` |
| Optional chaining, `??`, `??=` | boa (999 / 532); `this` em `(a?.b)()` e erros de sintaxe rasos | reforçado em `language_gap` |
| Closures em loops, TDZ, hoisting | boa (366 / 444) | `scope`, `statements`; Annex B em `annexb` |
| Strict vs sloppy | boa | `scope`, `global_semantics`, `with` (2388) |
| `using` / `await using` | média (54 / 24, 139 `Symbol.dispose`) | `recent_features`, `iterator` |
| Módulos ES (import/export, live bindings, ciclos, TLA, `import.meta`, `import()`) | boa (`module` 461 + `module_more` 1551; 90 `import.meta`, 273 `import()`) | |

## Os dois maiores buracos

1. **Operadores e referências**: ordem de coerção por operador e por tipo de operando (`<` vs `>` avaliam ToPrimitive
   em ordens diferentes, BigInt misto, Symbol), atribuição composta e lógica em `o[k()] op= v()` com Proxy registrando
   `get/set/has`, `++/--` em membro com BigInt e não numéricos, base null/undefined (ordem do TypeError contra a avaliação
   da chave e do valor), nome de função inferido em `??=`, optional chaining com `this`, `delete a?.b`, `with` e proxy.
2. **Destructuring e parâmetros**: ordem de `iter/next/return` do protocolo de iteração em padrões de array (IteratorClose
   em erro de alvo, de default e de `next`; `return` que devolve não objeto ou lança), defaults com efeito, chaves
   computadas, `...rest` de objeto com Proxy (ownKeys/gopd/get), alvos que são membros, os cinco contextos, e parâmetros
   default (TDZ, escopo separado do corpo, `arguments` não mapeado, `length`).

## Golden criado

- Gerador: `scripts/gen-language-gap-golden.js` (mede o bun; cada caso roda em `new Function` dentro de `run`, log de
  efeitos no array global `L`).
- Dados: `tests/golden/language_gap_bun.tsv` (1160 programas, um pouco acima dos 700 pedidos porque a combinação
  padrão x fonte x contexto foi amostrada, não exaustiva).
- Teste: `tests/language_gap_bun_golden.rs` (não rodado nesta passada; ninguém compilou).

## Lacunas menores que ficaram fora

- `for await` com ordem de microtasks de `return()` em destructuring assíncrono; `await using` com erros múltiplos
  (SuppressedError) em padrões.
- Cache de strings de tagged template por site em loops e em `eval` repetido (existe, mas raso).
- Ordem de avaliação de decoradores e de campos de classe com computed keys que lançam.

## Mensagens de erro conferidas contra `src/`

Conferidas por grep (existem com o mesmo texto): `Cannot destructure property 'x' from null or undefined value`,
`Cannot destructure null or undefined value`, `Invalid mix of BigInt and other type in <operação>.`,
`BigInt does not support >>> operator`, `Iterator result interface is not an object.`, `Spread syntax requires ...`,
`Cannot convert a symbol to a number`, `Attempted to assign to readonly property.`, `Unable to delete property.`,
`Invalid destructuring assignment target`, `Left side of assignment is not a reference.`. Nenhuma divergência óbvia de
texto encontrada; divergências de ordem de efeitos só aparecem quando o teste rodar.
