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

module.exports = { measureStable, DEFAULT_RUNS };
