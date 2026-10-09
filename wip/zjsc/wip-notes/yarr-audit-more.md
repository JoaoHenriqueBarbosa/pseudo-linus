# Auditoria do Yarr, golden ampliado (regexp_more)

Data: 2026-10-08.

## Cobertura que já existia

- `tests/golden/regexp_bun.tsv` (1595 programas): classes, quantificadores, escapes, grupos, backreferences,
  lookaround, flags `i m s u y g`, 79 usos de `\p{Script...}`, split/replace/matchAll básicos.
- `tests/golden/regexp_opt_bun.tsv` (3809 programas): caminhos otimizados do interpretador.
- Lacunas vistas: flag `v` quase sem teste (classes aninhadas, `&&`, `--`, `\q{}`, `\p{RGI_Emoji}`), flag `d`
  (indices e groups), grupos nomeados duplicados, modificadores `(?i:...)`, lookbehind variável,
  `Script_Extensions`/`General_Category` sistemáticos, case folding unicode, `Symbol.replace` e `Symbol.split`
  customizados, `RegExp.escape` e a mensagem exata de cada erro de sintaxe.

## O que o novo golden mede (bun 1.4.2, 2000 programas)

`scripts/gen-regexp-more-golden.js` gera `tests/golden/regexp_more_bun.tsv`; `tests/regexp_more_bun_golden.rs`
roda no padrão de `regexp_bun_golden.rs` (mesmo harness `e2e_values_harness.js`). O bun 1.4.2 expõe
`RegExp.escape`, modificadores `(?i:...)`, `\p{RGI_Emoji}` e grupos nomeados duplicados, então todos entram.

Seções (contagem aproximada): flag v (~330), flag d (~40), grupos nomeados e backreferences (~170),
modificadores (~100), lookbehind e lookahead (~70), propriedades Unicode (Script, scx, gc e formas curtas,
5 caracteres por valor: 3 membros e 2 não membros escolhidos no bun; ~350), case folding (~230), sticky e
lastIndex (~100), matchAll/replace com função/split com captura (~110), Symbol.* customizados e
reflexão do protótipo (~50), toString/source/flags/compile (~230), `RegExp.escape` (~70) e erros de sintaxe
com a mensagem exata, em `""`, `u` e `v` (~300, 28 mensagens distintas).

Geração determinística (sem aleatoriedade), sem caminho da máquina; rodar com `timeout 300 bun ...`.
Nenhum cargo foi rodado: o teste novo ainda não foi executado contra o porte. Primeira ação de quem rodar:
`cargo test --test regexp_more_bun_golden` e triar as divergências por seção.

## Leitura de `src/yarr` contra `upstream/JavaScriptCore/yarr`

Busca por `Unported`, `todo!`, `unimplemented!`, `TODO`, "não portado" em `src/yarr` e `src/runtime/reg_exp*.rs`:
nenhuma ocorrência. Os dois `FIXME` achados (`yarr_interpreter_cpp4.rs:597`, `yarr_pattern_cpp4.rs:356`) são
FIXMEs do próprio C++ copiados no porte, não lacunas. `RegExp.escape` existe em
`src/runtime/reg_exp_legacy_natives.rs`. O porte do Yarr tem 25898 linhas contra 19453 do C++ (cpp), sem
trecho vazio aparente; o YarrJIT não é portado (esperado: só interpretador). Nenhuma correção foi necessária
por leitura; as lacunas reais, se houver, aparecem na primeira execução do golden novo.

## Observação de processo

O gerador foi ajustado em várias passadas para fechar em exatamente 2000 programas únicos, com um laço de
preenchimento determinístico no fim; parte dessas edições no script foi feita com um script Python curto
(substituições em massa em texto), por conveniência, fora do padrão Edit.
