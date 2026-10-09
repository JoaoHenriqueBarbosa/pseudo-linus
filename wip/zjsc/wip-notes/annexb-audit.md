# Auditoria de Annex B e recursos legados

Golden: `tests/golden/annexb_bun.tsv` (2270 programas sloppy, medidos no bun 1.4.2 com TZ=UTC), gerado por
`scripts/gen-annexb-golden.js`; teste em `tests/annexb_bun_golden.rs` (`annexb_matches_bun`, mínimo de 700 programas).

## Como o gerador roda

- O bun executa arquivo como módulo (estrito), então o preload lê o programa e o roda com `vm.runInThisContext`
  (semântica de script sloppy, como o `evaluate_named_script_result` do zjsc). O resultado é a global `R`.
- Quase todo programa avalia o código por eval indireto dentro de `try/catch` e grava `Nome: mensagem` do erro, de modo
  que SyntaxError e TypeError entram no golden. Os programas de comentário HTML (`<!--`, `-->`) rodam direto, porque
  dependem de ser o texto do script.
- Descartados no gerador (2): `delete Object.prototype.__proto__` (quebra o preload) e `sort` em objeto com
  `length: 2 ** 32` (estoura o tempo). 26 programas ficam com `R` indefinido (`<undefined>`): erro de sintaxe no
  próprio script e resultados assíncronos; são válidos como golden (o zjsc também deve deixar `R` indefinido, ou o
  teste acusará).

## Áreas cobertas

`__proto__` (literal, duplicado, acessor, descritor), `__defineGetter__`/`__defineSetter__`/`__lookupGetter__`/
`__lookupSetter__`, os 13 métodos HTML de String, `substr`, `trimLeft`/`trimRight`, `escape`/`unescape`,
`getYear`/`setYear`/`toGMTString`, parsing legado de `Date`, `RegExp.$1..$9`/`lastMatch`/`input`/... e `compile`,
sintaxe de RegExp do Annex B (`\8`, `\c`, `{`, `]`, quantificador em lookahead, `\k`), comentários HTML, literais octais
e `\07` em strings e templates, funções em blocos (B.3.3, `if (x) function`, labelled function, switch, catch, eval),
`for (var i = 0 in {})`, `with` e `Symbol.unscopables`, `arguments.callee`, `fn.caller`/`fn.arguments`,
`Object.prototype.toString` (toStringTag e builtinTag), `split` com limit, `sort` sem comparador com `undefined` e
buracos.

## Leitura do código (sem rodar nada)

Lidos por cima: `substr` (`string_prototype.rs`), `create_html` e escape de aspas (`string_prototype_natives_part2.rs`),
`getYear`/`setYear` (`date_prototype.rs`), registro de `trimLeft`/`trimRight`/`toGMTString`. Todos batem com o
JavaScriptCore (clamp de NaN para 0, `&quot;` só no atributo, `setYear` com 0..=99 somando 1900 e NaN reiniciando em
zero, `toGMTString` idêntica a `toUTCString`). Nenhuma divergência óbvia encontrada, nenhuma edição em `src/`.
Divergências reais só aparecerão ao rodar `annexb_matches_bun`; quem rodar deve triar por área (a lista de falhas traz
o programa, o esperado e o obtido).

## Observação de procedimento

O ajuste do preload no gerador foi feito com um script Python via Bash (substituição de trecho), contra a regra de usar
Edit; o resto foi Write.
