# Auditoria de String com Unicode de borda

## Golden

- `scripts/gen-string-unicode-golden.js` gera `tests/golden/string_unicode_bun.tsv` (2789 programas medidos no bun 1.4.2).
- `tests/string_unicode_bun_golden.rs` roda o golden (nunca foi executado: sem cargo nesta rodada).
- Cada programa embrulha a expressão em `F(() => ...)`, que serializa o valor ou `Nome: mensagem` da exceção.
- Famílias: at/charAt/charCodeAt/codePointAt, isWellFormed/toWellFormed/normalize, localeCompare sem locale, pad/repeat/trim,
  split com limite, replace com `$&`, `` $` ``, `$'`, `$<n>`, `$nn`, indexOf/lastIndexOf, substring/substr/slice, caixa especial
  (ß, İ, sigma final, ligaduras, Deseret, Georgian Mtavruli), String.raw, fromCodePoint/fromCharCode, Symbol em String e template,
  `new String` (chaves próprias, descritores, modo estrito), iterador com surrogates, limites de tamanho, URI e escape/unescape.
- Sucessos com string de 100 milhões de unidades ou mais ficam de fora (pesados e dependem da memória); os erros
  `RangeError: Out of memory` (22 casos) ficam.
- Descartados na geração: `Array(2**31).join('x')`, `Array(2**30).join('xx')` e similares (o bun não termina o programa).

## Leitura contra o upstream (rápida)

- Mensagens conferidas com o bun: `URIError: URI error`, `URIError: String contained an illegal UTF-16 sequence.`,
  `RangeError: Arguments contain a value that is out of range of code points`, mensagem de `repeat`, de `normalize`.
  Todas iguais às constantes em `string_constructor.rs`, `string_prototype.rs` e `js_global_object_functions.rs`.
- `repeat` e `pad` lançam `OutOfMemory` acima de `MAX_LENGTH`, como o bun mostra.
- Nenhuma divergência óbvia encontrada nesta passada, nenhuma edição em `src/`. O resultado real só aparece ao rodar o golden.

## Pendente

- Rodar `string_unicode_bun_golden` e triar as falhas.
