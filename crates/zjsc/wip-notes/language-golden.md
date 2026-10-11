# Golden de semântica da linguagem (zjsc)

Gerado por `scripts/gen-language-golden.js` no bun 1.4.2 (`bun scripts/gen-language-golden.js > tests/golden/language_bun.tsv`,
a contagem por categoria sai no stderr). Cada programa roda num processo bun próprio, com timeout de 10 s, e o
resultado é serializado pelo harness `tests/golden/language_bun_harness.js`. O teste é `tests/language_bun_golden.rs`
(cada programa num realm novo; a exceção do próprio harness sai por `describe_exception`).

Total: 2351 programas de uma linha, determinísticos (duas gerações seguidas dão arquivos idênticos), sem caminho
da máquina. Ficam de fora extensões do bun que não são do JavaScriptCore (`structuredClone`, globais de hospedeiro,
chaves extras de `Error`).

| Categoria | Programas |
|---|---|
| var/let/const e TDZ | 55 |
| closures e captura por iteração | 34 |
| hoisting | 37 |
| classes (campos, privados, static blocks, herança, super, new.target, extends null) | 115 |
| destructuring | 67 |
| spread | 45 |
| template literals e tagged templates | 41 |
| optional chaining e atribuição lógica | 63 |
| controle de fluxo (labels, switch, loops, with) | 52 |
| try/catch/finally | 52 |
| getters/setters e objetos | 81 |
| símbolos e protocolos (iterator, toPrimitive, hasInstance, species) | 76 |
| geradores e async | 80 |
| operadores e ordem de avaliação | 229 |
| coerção, `==` e comparação | 262 |
| strict vs sloppy (this, arguments, with, eval direto/indireto, `Function`) | 192 |
| funções (toString, call/apply/bind, name, length) | 230 |
| recursão, tail calls, pilha | 41 |
| mensagens de erro de runtime | 361 |
| semântica miscelânea | 110 |
| metadados de funções internas | 75 |
| protótipos e escopo | 74 |

Limite conhecido: o harness é síncrono, então async/await só é observado pela ordem síncrona do que roda antes do
primeiro `await` (microtarefas não são drenadas antes da serialização).
