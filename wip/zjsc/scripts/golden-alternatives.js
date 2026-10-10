// Medição de programas com saída não determinística no bun, para os geradores de golden.
//
// Algumas mensagens do JSC dependem da ordem de um HashSet de ponteiros (por exemplo, qual chave não configurável o
// Proxy cita em "has the non-configurable property 'X' that was not in the result from the 'ownKeys' trap"), e essa
// ordem muda entre execuções do bun. Excluir o caso é proibido, então o oráculo passa a medir várias vezes.
//
// Formato no tsv (escrito por `emitFactored` de scripts/golden-prelude.js, lido por `check` em tests/common/mod.rs):
//   JSON(sufixo) <TAB> JSON(resultado) <TAB> índice do prelúdio <TAB> JSON([outras saídas aceitas])
// A quarta coluna só existe quando as execuções divergiram. `resultado` é a menor saída observada (ordem de código de
// unidade UTF-16), as alternativas são as demais, também ordenadas, para a regeneração ser estável quando o conjunto
// observado é o mesmo. Um conjunto incompleto (K pequeno demais) só estreita o que o teste aceita, nunca o alarga.
//
// Uso nos geradores:
//   const { measureStable } = require("./golden-alternatives.js");
//   const { ok, out, err, alternatives } = await measureStable(() => runChild(job), 7);
//   rows.push({ source, result: out, alternatives });  // para emitFactored
//
// Saída que nunca se repete (o valor de `Math.random`, o texto de `Date()`) não cabe num conjunto finito. Para ela,
// `measureVarying` roda o programa duas vezes no relógio real e duas com o relógio deslocado (libfaketime, datas que
// diferem em todos os campos) e compara as saídas token a token (`tokenize`): número, palavra ou outro caractere. Se as
// saídas têm os mesmos tokens e só números ou palavras mudam, esses índices de token são os que variam no bun, e a
// quarta coluna ganha um array de inteiros com eles (`["alt", [3, 7]]`). O teste aceita no índice que varia qualquer
// token da mesma classe e exige igualdade em todo o resto (`matches_varying` em tests/common/mod.rs, a mesma
// tokenização). Goldens de formato cru (`fonte<TAB>resultado`) levam o array numa terceira coluna.
"use strict";

const DEFAULT_RUNS = 7;

// Executa `runOnce` (async, devolve { ok, out, err }) `runs` vezes em sequência, cada uma num processo bun separado
// (responsabilidade de `runOnce`). Devolve o primeiro resultado com `out` = menor saída e `alternatives` = as demais
// distintas. Se alguma execução falha, devolve a falha (o chamador decide descartar e reportar).
// `options.suspect` (RegExp) marca saídas de famílias sabidamente instáveis; ao vê-las o programa passa a ser medido
// `options.suspectRuns` vezes (padrão 40), porque com duas opções equiprováveis sete execuções ainda deixam 1/64 de
// chance de nunca ver a outra.
async function measureStable(runOnce, runs = DEFAULT_RUNS, options = {}) {
  const seen = new Set();
  let first = null;
  for (let i = 0; i < runs; i++) {
    const result = await runOnce();
    if (!result.ok) return { ...result, alternatives: [] };
    first = first || result;
    seen.add(result.out);
    if (i === runs - 1 && options.suspect && [...seen].some((out) => options.suspect.test(out))) runs = options.suspectRuns || 40;
  }
  const sorted = [...seen].sort();
  return { ok: true, out: sorted[0], err: first.err, alternatives: sorted.slice(1) };
}

// Um token é um número (`1`, `0.25`, `5e-7`), uma palavra ASCII ou qualquer outra unidade UTF-16 sozinha.
function tokenize(text) {
  const tokens = [];
  for (const match of text.matchAll(/(\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|([A-Za-z]+)|([\s\S])/g)) {
    tokens.push({ kind: match[1] !== undefined ? "number" : match[2] !== undefined ? "word" : "other", text: match[0] });
  }
  return tokens;
}

// Os índices de token em que as saídas diferem, se todas têm a mesma sequência de classes e só números ou palavras
// mudam; `null` quando a diferença não é dessa forma (então vale o conjunto finito de `measureStable`).
function varyingTokens(outputs) {
  const [first, ...rest] = outputs.map(tokenize);
  const varying = new Set();
  for (const tokens of rest) {
    if (tokens.length !== first.length) return null;
    for (let i = 0; i < tokens.length; i++) {
      if (tokens[i].kind !== first[i].kind) return null;
      if (tokens[i].text === first[i].text) continue;
      if (tokens[i].kind === "other") return null;
      varying.add(i);
    }
  }
  return [...varying].sort((a, b) => a - b);
}

// Os relógios deslocados: datas que diferem do presente e entre si em ano, mês, dia, dia da semana, hora, minuto e
// segundo, para que todo campo de data que vem do relógio apareça como variável.
const SHIFTED_CLOCKS = ["@2003-04-01 07:08:09", "@2011-12-15 19:21:43"];

// Os ambientes das quatro execuções de `measureVarying`. A libfaketime vem de `LIBFAKETIME` (o `.so` do pacote
// libfaketime do Debian, que `apt-get download libfaketime && dpkg -x` extrai sem instalar nada); sem ela o gerador
// não consegue medir o relógio e para.
function varyingEnvs(baseEnv) {
  const library = process.env.LIBFAKETIME;
  if (!library) throw new Error("LIBFAKETIME não definido: aponte para o libfaketimeMT.so.1 do pacote libfaketime do Debian");
  return [baseEnv, baseEnv, ...SHIFTED_CLOCKS.map((clock) => ({ ...baseEnv, LD_PRELOAD: library, FAKETIME: clock, FAKETIME_DONT_RESET: "1" }))];
}

// Roda o programa nas quatro execuções (`runOnce(env)` devolve { ok, out, err }). A primeira, no relógio real, é a
// saída gravada. Uma execução deslocada que falhe não entra na comparação (o programa pode depender da data).
// Devolve `varying` (índices de token) quando a diferença é só de números e palavras, senão `alternatives`.
async function measureVarying(runOnce, baseEnv) {
  const results = [];
  for (const env of varyingEnvs(baseEnv)) results.push(await runOnce(env));
  if (!results[0].ok) return { ...results[0], alternatives: [] };
  const outputs = [...new Set(results.filter((result) => result.ok).map((result) => result.out))];
  if (outputs.length === 1) return { ...results[0], alternatives: [] };
  const varying = varyingTokens([results[0].out, ...outputs.filter((out) => out !== results[0].out)]);
  if (varying) return { ...results[0], alternatives: [], varying };
  const sorted = outputs.sort();
  return { ...results[0], out: sorted[0], alternatives: sorted.slice(1) };
}

module.exports = { measureStable, measureVarying, varyingTokens, tokenize, DEFAULT_RUNS };
