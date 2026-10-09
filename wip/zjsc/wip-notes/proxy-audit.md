# Auditoria de Proxy e Reflect (golden contra o bun)

Data: 2026-10-08. Oráculo: bun 1.4.2.

## O que foi criado

- `wip/zjsc/scripts/gen-proxy-golden.js`: gerador (mede o bun programa a programa, arquivo `proxy_case.js`).
- `wip/zjsc/tests/golden/proxy_bun.tsv`: 2010 programas (já existia só `proxy_class_bun.tsv`, de classes).
- `wip/zjsc/tests/proxy_bun_golden.rs`: teste (molde de `function_error_bun_golden.rs`), exige no mínimo 1500 programas.

## Cobertura

- Cada um dos 13 traps x operações que o disparam x 11 variações de handler (normal, retorna false, retorna
  undefined, lança Error, lança primitivo, trap número/objeto/string, null, undefined, getter que lança).
- Violação de invariante, uma a uma, com a mensagem exata do bun (get, set, has, deleteProperty, defineProperty,
  getOwnPropertyDescriptor, ownKeys, getPrototypeOf, setPrototypeOf, isExtensible, preventExtensions, construct, apply).
- Construtor, `Proxy.revocable`, revogação (3 alvos x 43 operações, mais função, mais revogar dentro de trap).
- Proxy de proxy (profundidades 2 a 10), de função, de array, de classe; protótipo cíclico.
- Ordem de chamada das traps com handler que é um Proxy registrando cada consulta (4 alvos x 54 operações).
- `Reflect.*` (aridade, nomes, erros de argumento, receiver), `with` e `Symbol.unscopables`, `in`, JSON, for-in,
  spread, Object.keys/assign, instanceof, typeof, `Object.prototype.toString`, `class extends` de Proxy e `super`.

## Comparação com src/runtime/proxy_object.rs

As 37 mensagens de TypeError literais de `proxy_object.rs` aparecem todas, byte a byte, nas saídas do bun. As
mensagens "'X' property of a Proxy's handler should be callable" são montadas por `format!`, também iguais. Nenhuma
divergência óbvia de texto encontrada, então nenhuma edição no código.

## Golden de sequência de traps (proxy_trace)

- `scripts/gen-proxy-trace-golden.js`, `tests/golden/proxy_trace_bun.tsv` (745 programas) e
  `tests/proxy_trace_bun_golden.rs`. Cada programa registra nome e chave de cada trap disparada por uma operação
  interna (spread, for-in, `Object.keys/values/entries`, `JSON.stringify`, `Array.prototype.*` em proxy de array,
  `instanceof`, `in`, `with`, `delete`, `class extends`, `Symbol.toPrimitive`, `Object.assign`, destructuring), em seis
  alvos, mais cerca de 130 casos de invariante violada, revogação e handler que lança (TypeError com mensagem).
- `structuredClone` ficou de fora (não é do JavaScriptCore). O teste Rust não foi rodado (sem cargo).

## Pendências

- O teste Rust não foi rodado (regra da tarefa: sem cargo). A primeira rodada vai mostrar as divergências de
  comportamento (ordem de traps, `with`, `class extends`), que ainda não foram medidas contra o zjsc.
- O golden usa `run`/`fmt` copiados no fonte de cada programa (2010 vezes), 1,7 MB no total.
