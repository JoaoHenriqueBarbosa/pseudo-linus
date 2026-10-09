// Fatora o prelúdio comum dos goldens `tests/golden/<nome>_bun.tsv`.
// Cada linha do tsv guardava o programa inteiro, e o prelúdio (a função de serialização `S` etc.) se repetia em todas.
// Formato novo: `JSON(sufixo)<TAB>JSON(resultado)[<TAB>índice]`. O programa avaliado é `preludes[índice] + sufixo`, com
// `preludes` é o array JSON de strings em `tests/golden/<nome>.preludes.json` (lido em Rust por `preludes_from_json` em
// tests/common/mod.rs, sem lista de arquivos nos testes). O índice é omitido quando vale 0. Os programas se agrupam pelo começo do fonte e cada grupo tem o seu prelúdio (ver `factorRows`).
// Saída não determinística no bun (ordem de HashSet de ponteiros, por exemplo): `scripts/golden-alternatives.js` mede o
// programa várias vezes e, se as saídas diferem, a linha ganha uma quarta coluna `JSON([outras saídas aceitas])`, com a
// terceira coluna (índice) sempre presente. O teste passa se o porte devolve a segunda coluna ou qualquer alternativa.
// Quinta coluna (meta `[modo, índiceDaCauda, ...corridasDoSufixo]`): as corridas do mapa de posições que caem no prelúdio
// vivem no `*.preludes.json` (entradas `{text, runs, tails}`), ver `factorMeta`; `readRows` devolve o meta completo.
// Quem lê o tsv (testes em tests/common/mod.rs e geradores que deduplicam contra goldens vizinhos) só depende de a
// primeira coluna ser o JSON do sufixo.
const fs = require("fs");
const path = require("path");

const GOLDEN_DIR = path.join(__dirname, "..", "tests", "golden");
const STRICT = '"use strict";\n';

// Prefixo comum mais longo, cortado depois do último '\n' (o prelúdio termina sempre em linha completa).
function commonPrelude(sources) {
  let prefix = sources[0];
  for (const source of sources) {
    let i = 0;
    while (i < prefix.length && i < source.length && prefix[i] === source[i]) i++;
    prefix = prefix.slice(0, i);
  }
  return prefix.slice(0, prefix.lastIndexOf("\n") + 1);
}

// rows: [{ source, result }] com os textos já decodificados. Devolve os prelúdios e as linhas do tsv, na mesma ordem.
// Os programas se agrupam pelo começo do fonte (a diretiva e os primeiros 160 caracteres): o fonte canônico do bun varia
// de formato (indentação, aspas, minificado) e um único programa fora do padrão zeraria o prefixo comum de todos. Grupo
// com menos de MIN_GROUP programas cai no grupo reserva (um por tipo de diretiva), cujo prefixo comum pode ser vazio.
const HEAD_CHARS = 160;
const MIN_GROUP = 3;

function factorRows(rows) {
  const strict = (row) => row.source.startsWith(STRICT);
  const head = (row) => (strict(row) ? "1" : "0") + row.source.slice(0, HEAD_CHARS);
  const counts = new Map();
  for (const row of rows) counts.set(head(row), (counts.get(head(row)) || 0) + 1);
  const keyOf = (row) => (counts.get(head(row)) >= MIN_GROUP ? head(row) : (strict(row) ? "1" : "0"));
  const groups = new Map(); // chave do grupo -> programas, na ordem da primeira aparição
  for (const row of rows) {
    const key = keyOf(row);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(row.source);
  }
  const preludes = [];
  const indexOfGroup = new Map();
  for (const [key, sources] of groups) {
    indexOfGroup.set(key, preludes.length);
    preludes.push(commonPrelude(sources));
  }
  const indexes = rows.map((row) => indexOfGroup.get(keyOf(row)));
  const { preludeRuns, metas } = factorMeta(preludes, rows.map((row, i) => ({ index: indexes[i], meta: row.meta })));
  const lines = rows.map((row, i) => {
    const index = indexes[i];
    const suffix = row.source.slice(preludes[index].length);
    const alternatives = row.alternatives && row.alternatives.length ? "\t" + JSON.stringify(row.alternatives) : "";
    // Com alternativas a quarta coluna exige a terceira, mesmo valendo 0. A quinta coluna (meta) exige as duas.
    if (metas[i]) {
      return JSON.stringify(suffix) + "\t" + JSON.stringify(row.result) + "\t" + index + "\t" + JSON.stringify(row.alternatives || []) + "\t" + JSON.stringify(metas[i]);
    }
    return JSON.stringify(suffix) + "\t" + JSON.stringify(row.result) + (index || alternatives ? "\t" + index : "") + alternatives;
  });
  return { preludes, lines, preludeRuns };
}

// A quinta coluna (meta `[modo, ...corridas]`, ver `positionRuns`) repetia em toda linha as corridas da parte do prelúdio,
// que são quase as mesmas em todas. Fatoração exata: as corridas com linha dentro do prelúdio (linhas 1..P, o prelúdio
// termina em '\n') são a "cabeça" da linha e o resto o "sufixo". Em cada grupo, `runs` é o maior prefixo comum das cabeças
// distintas e `tails` o que sobra de cada cabeça distinta. A coluna passa a ser `[modo, índiceDaCauda, ...corridasDoSufixo]`
// (índice -1: a linha não tem corrida nenhuma, o caso `[]` de `positionRuns`), com as coordenadas absolutas do programa
// completo. O programa montado é `runs ++ tails[índice] ++ sufixo`, idêntico à lista original. `texts`: textos dos
// prelúdios; `entries`: `[{ index, meta }]` (meta no formato completo, ou ausente). Devolve `preludeRuns[índice]`
// (`{ runs, tails }`, sempre presente) e as `metas` novas, alinhadas a `entries`.
function countLines(text) {
  return (text.match(/\n/g) || []).length;
}
function factorMeta(texts, entries) {
  const split = entries.map(({ index, meta }) => {
    if (!meta) return null;
    const runs = meta.slice(1);
    if (runs.length === 0) return { mode: meta[0], head: null, rest: [] };
    const limit = countLines(texts[index]);
    let k = 0;
    while (k < runs.length && runs[k] <= limit) k += 4;
    return { mode: meta[0], head: runs.slice(0, k), rest: runs.slice(k) };
  });
  const preludeRuns = texts.map(() => ({ runs: [], tails: [], tailIndex: new Map() }));
  const heads = texts.map(() => new Map()); // índice do prelúdio -> chave -> cabeça distinta
  split.forEach((item, i) => {
    if (item && item.head) heads[entries[i].index].set(item.head.join(","), item.head);
  });
  heads.forEach((distinct, index) => {
    const list = [...distinct.values()];
    if (list.length === 0) return;
    let common = list[0].length;
    for (const head of list) {
      let k = 0;
      while (k < common && head[k] === list[0][k]) k++;
      common = k - (k % 4);
    }
    const info = preludeRuns[index];
    info.runs = list[0].slice(0, common);
    for (const head of list) {
      info.tailIndex.set(head.join(","), info.tails.length);
      info.tails.push(head.slice(common));
    }
  });
  const metas = split.map((item, i) => {
    if (!item) return null;
    const tail = item.head ? preludeRuns[entries[i].index].tailIndex.get(item.head.join(",")) : -1;
    return [item.mode, tail, ...item.rest];
  });
  return { preludeRuns: preludeRuns.map(({ runs, tails }) => ({ runs, tails })), metas };
}

// Entrada de `*.preludes.json`: string (prelúdio sem corridas, formato antigo) ou `{ text, runs, tails }`.
function normalizePrelude(entry) {
  return typeof entry === "string" ? { text: entry, factored: false } : { text: entry.text, runs: entry.runs || [], tails: entry.tails || [], factored: true };
}

// Inverso de `factorMeta` para uma linha: devolve o meta completo `[modo, ...corridas]`.
function joinMeta(prelude, meta) {
  if (!meta || !prelude.factored) return meta;
  const [mode, tail, ...rest] = meta;
  return [mode, ...(tail < 0 ? [] : [...prelude.runs, ...prelude.tails[tail]]), ...rest];
}

// O repositório é público: nenhum golden pode carregar caminho da máquina de quem gerou. Esta checagem é o ponto único
// por onde todo texto de golden passa (`emitFactored`, `emitFactoredLines`, `emitRow` e a escrita direta de linhas).
// Falha com erro, nunca descarta em silêncio.
function leakPatterns() {
  const os = require("os");
  const patterns = [
    [/\/home\//, "/home/"],
    [/\/Users\//, "/Users/"],
    // Diretório aleatório do mkdtemp: `/tmp/<nome>-XXXXXX` (seis caracteres alfanuméricos de sufixo).
    [/\/tmp\/[A-Za-z0-9_.-]*-[A-Za-z0-9]{6}(?![A-Za-z0-9])/, "diretório aleatório em /tmp/"],
  ];
  const literals = [];
  try {
    const user = os.userInfo().username;
    // Só como componente de caminho (`/john`, `~john`): um e-mail de exemplo com o mesmo nome não é vazamento.
    if (user && user.length >= 3) patterns.push([new RegExp("[/\\\\~]" + user.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "(?![A-Za-z0-9])"), "nome do usuário em caminho"]);
  } catch {}
  for (const [label, value] of [["HOME", process.env.HOME], ["diretório home", os.homedir()]]) {
    if (value && value.length > 1 && value !== "/") literals.push([label, value]);
  }
  return { patterns, literals };
}

function findLeak(text) {
  const { patterns, literals } = leakPatterns();
  for (const [re, label] of patterns) {
    const m = re.exec(text);
    if (m) return { label, at: m.index, sample: text.slice(Math.max(0, m.index - 40), m.index + 60) };
  }
  for (const [label, value] of literals) {
    const at = text.indexOf(value);
    if (at >= 0) return { label, at, sample: text.slice(Math.max(0, at - 40), at + 60) };
  }
  return null;
}

// Devolve o próprio texto quando está limpo; lança quando contém caminho da máquina.
function assertPublicResult(text) {
  const leak = findLeak(String(text));
  if (leak) throw new Error(`golden vaza ${leak.label} (posição ${leak.at}): ...${leak.sample}...`);
  return text;
}

// Uma linha de tsv na saída padrão, já checada.
function emitRow(line) {
  console.log(assertPublicResult(line));
}

function preludesPath(name) {
  return path.join(GOLDEN_DIR, `${name}.preludes.json`);
}

// Um prelúdio por linha do arquivo, na ordem dos índices; o JSON não tem espaço dentro das strings, então o diff é legível.
// Com `preludeRuns` (de `factorRows`, quando alguma linha tem meta) cada entrada é `{ text, runs, tails }`.
function writePreludes(name, preludes, preludeRuns) {
  const entries = preludeRuns ? preludes.map((text, i) => ({ text, runs: preludeRuns[i].runs, tails: preludeRuns[i].tails })) : preludes;
  // `GOLDEN_OUT_DIR` desvia a escrita (execução de conferência para /tmp). Os `*.preludes.json` de tests/golden são lidos
  // pelos geradores vizinhos (`knownPrograms`); sobrescrevê-los com os de uma execução cujo tsv foi para outro lugar deixa
  // o par tsv/prelúdios incoerente e faz a execução seguinte divergir ou travar.
  const outDir = process.env.GOLDEN_OUT_DIR || GOLDEN_DIR;
  fs.writeFileSync(path.join(outDir, `${name}.preludes.json`), assertPublicResult("[\n" + entries.map((entry) => JSON.stringify(entry)).join(",\n") + "\n]\n"));
}

// Uso dos geradores: grava os prelúdios e devolve o texto do tsv novo.
function emitFactored(name, rows) {
  const { preludes, lines, preludeRuns } = factorRows(rows);
  writePreludes(name, preludes, rows.some((row) => row.meta) ? preludeRuns : undefined);
  return assertPublicResult(lines.join("\n") + "\n");
}

// Como `emitFactored`, para quem já monta as linhas do formato antigo (`JSON(programa)<TAB>JSON(resultado)[<TAB>JSON(meta)]`).
function emitFactoredLines(name, lines) {
  return emitFactored(name, lines.map((line) => {
    const [source, result, meta] = line.split("\t");
    const row = { source: JSON.parse(source), result: JSON.parse(result) };
    if (meta !== undefined) row.meta = JSON.parse(meta);
    return row;
  }));
}

// Lê um tsv (formato novo ou antigo) e devolve [{ source, result }] com o programa completo.
// Goldens que reaproveitam os prelúdios de outro (o teste inclui o `preludes.json` dele).
const SHARED_PRELUDES = { timers: "microtask_order" };

function readRows(name, tsvText) {
  name = SHARED_PRELUDES[name] || name;
  const preludes = (fs.existsSync(preludesPath(name)) ? JSON.parse(fs.readFileSync(preludesPath(name), "utf8")) : []).map(normalizePrelude);
  return tsvText.split("\n").filter(Boolean).map((line) => {
    const [suffix, result, index, alternatives, meta] = line.split("\t");
    const prelude = index === undefined ? (preludes[0] || { text: "", factored: false }) : preludes[Number(index)];
    const row = { source: prelude.text + JSON.parse(suffix), result: JSON.parse(result) };
    if (alternatives !== undefined) row.alternatives = JSON.parse(alternatives);
    if (meta !== undefined) row.meta = joinMeta(prelude, JSON.parse(meta));
    return row;
  });
}

// Formatos de tsv que não são o JSON(programa) da primeira coluna (antigo ou fatorado, tratado por `readRows`).
// Todo arquivo fora destas duas tabelas é lido por `readRows`, e um erro de leitura ali é bug real e se propaga.

// Programa em texto puro, sem JSON e sem prelúdio, numa coluna que não é a primeira.
// column: coluna do programa; json: a coluna é JSON(programa) (sem prelúdio).
const RAW_PROGRAM = {
  // coluna 0: programa cru (resto: tipo/resultado/campos extras)
  "async_bun.tsv": { column: 0 }, "async_gen_bun.tsv": { column: 0 }, "async_iter_grid_bun.tsv": { column: 0 },
  "async_order_bun.tsv": { column: 0 }, "available_locales_bun.tsv": { column: 0 }, "await_context_bun.tsv": { column: 0 },
  "buffers_bun.tsv": { column: 0 }, "builtins_bun.tsv": { column: 0 }, "coercion_bun.tsv": { column: 0 },
  "collator_bun.tsv": { column: 0 }, "collection_async_bun.tsv": { column: 0 }, "control_flow_bun.tsv": { column: 0 },
  "control_flow_more_bun.tsv": { column: 0 }, "datetime_edge_bun.tsv": { column: 0 }, "datetime_gaps_bun.tsv": { column: 0 },
  "datetime_more_bun.tsv": { column: 0 }, "e2e_numeric.tsv": { column: 0 }, "e2e_values.tsv": { column: 0 },
  "errors_bun.tsv": { column: 0 }, "intl_bun.tsv": { column: 0 }, "intl_collator_bun.tsv": { column: 0 },
  "intl_edge_bun.tsv": { column: 0 }, "intl_locale_bun.tsv": { column: 0 }, "intl_misc_bun.tsv": { column: 0 },
  "intl_more_bun.tsv": { column: 0 }, "intl_more_locales_bun.tsv": { column: 0 }, "intl_object_bun.tsv": { column: 0 },
  "internal_fields_bun.tsv": { column: 0 },
  "json_number_bun.tsv": { column: 0 }, "language_bun.tsv": { column: 0 }, "locale_more_bun.tsv": { column: 0 },
  "microtask_bun.tsv": { column: 0 }, "number_format_more_bun.tsv": { column: 0 }, "number_parts_bun.tsv": { column: 0 },
  "number_regional_bun.tsv": { column: 0 }, "plural_bun.tsv": { column: 0 }, "promise_bun.tsv": { column: 0 },
  "promise_grid_bun.tsv": { column: 0 }, "promise_more_bun.tsv": { column: 0 }, "recent_apis_bun.tsv": { column: 0 },
  "reflection_bun.tsv": { column: 0 }, "regexp_bun.tsv": { column: 0 }, "regexp_more_bun.tsv": { column: 0 },
  "regexp_opt_bun.tsv": { column: 0 }, "reltime_more_bun.tsv": { column: 0 }, "resolved_locale_bun.tsv": { column: 0 },
  "temporal_bun.tsv": { column: 0 }, "temporal_calendars_bun.tsv": { column: 0 }, "temporal_duration_bun.tsv": { column: 0 },
  "temporal_edge_bun.tsv": { column: 0 }, "temporal_locale_bun.tsv": { column: 0 }, "temporal_zoned_bun.tsv": { column: 0 },
  "text_locale_bun.tsv": { column: 0 }, "typedarray_bun.tsv": { column: 0 }, "typedarray_more_bun.tsv": { column: 0 },
  "wasm_api_bun.tsv": { column: 0 }, "wasm_exceptions_bun.tsv": { column: 0 }, "wasm_gc_bun.tsv": { column: 0 },
  "wasm_js_bun.tsv": { column: 0 }, "wasm_numeric_bun.tsv": { column: 0 }, "wasm_simd_bun.tsv": { column: 0 },
  "wasm_ctor_bun.tsv": { column: 0 }, "wasm_callref_bun.tsv": { column: 0 },
  "temporal_to_zoned_bun.tsv": { column: 0 }, "collator_locales_bun.tsv": { column: 0 },
  "number_unit_scandinavian_bun.tsv": { column: 0 }, "likely_subtags_bun.tsv": { column: 0 },
  "typed_array_species_bun.tsv": { column: 0 }, "blob_bun.tsv": { column: 0, json: true }, "crypto_bun.tsv": { column: 0, json: true },
  "fetch_types_bun.tsv": { column: 0, json: true }, "streams_bun.tsv": { column: 0, json: true },
  // coluna 2: as colunas 0 e 1 são o modo de avaliação e o nome declarado
  "global_var_decl_bun.tsv": { column: 2 },
  // coluna 1: a coluna 0 é fuso/nome do caso
  "date_bun.tsv": { column: 1 }, "date_edge_bun.tsv": { column: 1 }, "date_legacy_parse_bun.tsv": { column: 1 },
  "date_proto_bun.tsv": { column: 1 }, "date_tz_bun.tsv": { column: 1 }, "reentrancy_bun.tsv": { column: 1 },
  // coluna 1 em JSON(programa)
  "timezone_bun.tsv": { column: 1, json: true }, "tailcall_bun.tsv": { column: 1, json: true },
  "cjs_module_load_bun.tsv": { column: 1, json: true }, "cjs_require_bun.tsv": { column: 1, json: true },
  "esm_module_load_bun.tsv": { column: 1, json: true }, "dialogs_io_bun.tsv": { column: 1, json: true },
  // coluna 0 em JSON(programa) sem prelúdio; as outras colunas são stdout/stderr em hex e código de saída
  "uncaught_bun.tsv": { column: 0, json: true }, "post_message_uncaught_bun.tsv": { column: 0, json: true }, "broadcast_channel_uncaught_bun.tsv": { column: 0, json: true }, "event_target_main_bun.tsv": { column: 0, json: true }, "message_channel_loop_bun.tsv": { column: 0, json: true },"console_object_bun.tsv": { column: 0, json: true },
  "console_object_more_bun.tsv": { column: 0, json: true }, "console_error_main_bun.tsv": { column: 0, json: true }, "console_primitive_bun.tsv": { column: 0, json: true }, "console_dir_bun.tsv": { column: 0, json: true },
  // coluna 0 em JSON(programa) sem prelúdio; a terceira coluna é a marca `bunonly`, não índice de prelúdio
  "global_semantics_bun.tsv": { column: 0, json: true },
  // coluna 0 em JSON(programa), resto cru (resultado não é JSON)
  "syntax-errors.tsv": { column: 0, json: true }, "syntax-error-positions.tsv": { column: 0, json: true },
};

// Goldens cuja linha não carrega programa JS completo (dados de tabela, locale, padrão de regexp): nunca entram na dedup.
const NOT_PROGRAM = {
  "bigint.tsv": "operação e operandos em hex",
  "calendar_bun.tsv": "locale, instante e campos do calendário",
  "case_mapping.tsv": "code points em hex",
  "date_pattern_bun.tsv": "locale, instante e opções do Intl.DateTimeFormat",
  "date_parse_v8_bun.tsv": "fuso e texto de data entre aspas, não programa",
  "display_names_bun.tsv":"locale, tipo e código do Intl.DisplayNames",
  "duration_format_bun.tsv": "locale, estilo e duração do Intl.DurationFormat",
  "locale_getters_bun.tsv": "tag de locale e listas de getters",
  "math_bun.tsv": "nome da função e operandos em hex",
  "node_modules_resolve_bun.tsv": "importador, especificador e caminho resolvido",
  "package_exports_bun.tsv": "campo exports, subcaminho e alvo resolvido",
  "number_to_string.tsv": "bits em hex e saídas de formatação",
  "options_parity_bun.tsv": "nome de construtor e typeof",
  "parse_double.tsv": "texto e bits em hex",
  "regexp-exec.tsv": "entrada e padrão de regexp, não programa",
  "regexp-syntax.tsv": "padrão de regexp, não programa",
  "reltime_bun.tsv": "locale, estilo e valor do Intl.RelativeTimeFormat",
  "segmenter_bun.tsv": "granularidade, locale e texto",
  "segmenter_locales_bun.tsv": "granularidade, locale e texto",
  "timezone_names_bun.tsv": "id de fuso, instante e nome",
};

function sourcesOf(file, text) {
  if (NOT_PROGRAM[file]) return [];
  const raw = RAW_PROGRAM[file];
  if (!raw) return readRows(file.replace(/(_bun)?\.tsv$/, ""), text).map((row) => row.source);
  return text.split("\n").filter(Boolean).map((line) => {
    const cell = line.split("\t")[raw.column];
    if (cell === undefined) throw new Error(`${file}: linha sem a coluna ${raw.column}`);
    return raw.json ? JSON.parse(cell) : cell;
  });
}

// Programas completos (prelúdio recomposto) dos goldens vizinhos. `own` é o nome do golden do chamador (`x_bun.tsv`) e é
// SEMPRE excluído, qualquer que seja o filtro: sem isso, ao regenerar, o próprio golden fazia todo programa virar
// "repetido" e o resultado saía com uma linha. `filter` é uma lista de nomes de arquivo `x_bun.tsv` ou um predicado sobre
// o nome do arquivo; arquivos ausentes são ignorados. Goldens fora de formato de programa (`NOT_PROGRAM`) contribuem com
// nada; um arquivo não catalogado que não parseia levanta erro.
function knownPrograms(own, filter) {
  if (typeof own !== "string" || !own.endsWith(".tsv")) throw new Error("knownPrograms(own, filter): own é o nome do golden do chamador, ex. \"x_bun.tsv\"");
  const accepts = Array.isArray(filter) ? (name) => filter.includes(name) : filter;
  return fs.readdirSync(GOLDEN_DIR).filter((name) => name.endsWith(".tsv") && name !== own && accepts(name)).flatMap((name) =>
    sourcesOf(name, fs.readFileSync(path.join(GOLDEN_DIR, name), "utf8")));
}

// Amostra determinística de até `target` itens: os de menor SHA-1 do texto (`keyOf(item)`, o próprio item por padrão),
// devolvidos na ordem original. A escolha de cada item depende só do texto dele e do tamanho do conjunto, nunca da posição
// nem de outros goldens: tire a amostra do conjunto candidato inteiro e só depois desconte os goldens vizinhos
// (`knownPrograms`), senão uma linha a mais ou a menos num vizinho troca centenas de programas. Amostra por índice
// (`floor((i + 1) * T / n)`, `i % passo`) e PRNG com semente deslocam tudo quando entra ou sai um item no começo da lista.
function sampleByHash(items, target, keyOf = (item) => item) {
  if (items.length <= target) return items.slice();
  const crypto = require("crypto");
  const ranked = items.map((item, index) => ({ index, hash: crypto.createHash("sha1").update(String(keyOf(item))).digest("hex") }));
  ranked.sort((a, b) => (a.hash < b.hash ? -1 : a.hash > b.hash ? 1 : a.index - b.index));
  return ranked.slice(0, target).map((entry) => entry.index).sort((a, b) => a - b).map((index) => items[index]);
}

// Coletor de candidatos com densidade: `push(passo, ...itens)` guarda os itens com a densidade `1/passo` (passo 1 ou menor:
// entram todos) e `resolve(keyOf)` devolve, na ordem original, a amostra de cada densidade escolhida por `sampleByHash`.
// Troca o `if ((i + j) % passo === 0) add(x)` dentro do laço de geração por `thin(passo, x)` sem tirar a escolha do hash.
// `pushIn(grupo, passo, ...itens)` separa a amostra por família: cada grupo mantém a própria proporção.
function stepSampler() {
  const entries = [];
  const pushIn = (group, step, items) => {
    for (const item of items) entries.push({ group, step, item });
  };
  return {
    push(step, ...items) {
      pushIn("passo " + step, step, items);
    },
    pushIn(group, step, ...items) {
      pushIn(group, step, items);
    },
    resolve(keyOf = (item) => item) {
      const groups = new Map();
      for (const entry of entries) {
        if (!groups.has(entry.group)) groups.set(entry.group, []);
        groups.get(entry.group).push(entry);
      }
      const kept = new Set();
      for (const group of groups.values()) {
        const step = group[0].step;
        const chosen = step <= 1 ? group : sampleByHash(group, Math.ceil(group.length / step), (entry) => keyOf(entry.item));
        for (const entry of chosen) kept.add(entry);
      }
      return entries.filter((entry) => kept.has(entry)).map((entry) => entry.item);
    },
  };
}

// O bun transpila todo arquivo antes de entregar ao JSC, e as mensagens de erro com texto do fonte ("evaluating '...'",
// "In '...'") e `Function.prototype.toString` citam o texto transpilado. Medido contra o runtime do bun 1.4.2 (arquivos
// com `true`, `false`, `undefined`, `x=>x`, classes, métodos, comparados via toString): o texto que o JSC vê é o de
// `Bun.Transpiler` com `target: "bun"` e `deadCodeElimination: true` (com `minifySyntax` ligado ou não, a saída é a mesma
// nos casos medidos): `true` vira `!0`, `false` vira `!1`, `undefined` vira `void 0`, `typeof a === "undefined"` vira
// `typeof a > "u"`, `1 + 2` vira `3`, `'a' + 'b'` vira `"ab"`. Sem `target: "bun"` ou sem `deadCodeElimination` nada disso
// acontece. Whitespace e identificadores ficam intactos. Geradores que rodam o programa como arquivo gravam
// `canonicalSource(original)`: bun e porte passam a ver o mesmo texto. Geradores via `vm.runInThisContext` não precisam.
//
// Duas diferenças que o texto não carrega e que este módulo não consegue reproduzir:
// - O transpilador descarta toda diretiva "use strict" (também as de dentro de função). A semântica estrita continua
//   valendo no runtime (o arquivo vira módulo ESM, ou função de wrapper CJS quando a diretiva está no topo), então o
//   texto canônico PRESERVA a diretiva do topo do original: o porte precisa dela para rodar o arquivo em modo estrito.
//   Diretivas de função são descartadas, como o runtime faz no texto (`toString` não as mostra); o porte as perde também.
// - Todo arquivo que o bun roda como CJS é embrulhado numa função (ver `cjsRuntimeBody`): o `toString` mostra 2 espaços
//   a mais de recuo e o runtime reordena declarações. Resolvido: para CJS, `canonicalSource` devolve o corpo exato que o
//   runtime põe na função de wrapper, e o harness do porte avalia esse corpo dentro do mesmo wrapper.
//
// Modo de execução (ESM ou CJS), medido no bun 1.4.2 com arquivo `.js` sem package.json (2026-10-08). A regra:
// - ESM (estrito, sem wrapper, indentação original) é o PADRÃO: um arquivo sem nenhum marcador de CJS é ESM mesmo sem
//   `import`/`export`. `import`, `export`, `import.meta` e `await` no topo não são necessários para ser ESM.
// - CJS (wrapper, `toString` com +2 espaços) quando o arquivo tem qualquer um destes marcadores:
//     * a diretiva "use strict" como primeira instrução (aspas simples ou duplas; precedida de comentário também vale;
//       depois de outra instrução não vale). Este CJS é estrito, via a diretiva;
//     * referência livre (não declarada no arquivo) a `module`, `exports`, `__dirname` ou `__filename`, inclusive em
//       `typeof module`/`typeof exports` e dentro de função;
//     * referência livre a `require`, em qualquer posição EXCETO a instrução `typeof require;` sozinha (código morto que
//       o transpilador elimina; `var r = typeof require` marca);
//     * `this` no topo do arquivo, inclusive dentro de arrow function no topo (`() => this`); `this` dentro de função
//       comum, método ou inicializador de campo de classe não marca;
//     * instrução `with` (então o arquivo é sloppy: `with` e CJS sem diretiva rodam sem "use strict").
//   Não marcam: `arguments` (no topo é ReferenceError em ESM, `typeof arguments` dá "undefined"), `new.target`, `return`
//   no topo, `o.require`, `{require: 1}`, 'require' em string, `globalThis.module`, declarar `var/let require/module/
//   exports` ou parâmetro com esses nomes (sombra desfaz o marcador), `import()` dinâmico.
// - CJS sem diretiva e sem `with`... é sloppy (`this === undefined` em função solta é falso); ESM é sempre estrito.
// - `import`/`export`/`await` no topo com a diretiva: ESM vence (estrito, sem wrapper).
// Em ESM, sintaxe sloppy-only (`delete x`, `with`, `let` como identificador, `yield`/`await` como identificador) é
// SyntaxError no runtime; o golden então registra o erro estrito.
// Como a detecção por regex erra (escopo, `this` em arrow), `moduleMode` pergunta ao próprio `bun build`, que embrulha o
// módulo CJS em `__commonJS`; um pré-filtro por palavra evita o custo quando nenhuma palavra suspeita aparece.
// `canonicalSource` embute "use strict" no ESM e preserva a diretiva do CJS estrito, de modo que o porte (que roda o
// texto como script) rode no mesmo modo de estrictness do bun.
const canonicalTranspiler = new Bun.Transpiler({
  loader: "js",
  target: "bun",
  minifyWhitespace: false,
  minifyIdentifiers: false,
  minifySyntax: false,
  deadCodeElimination: true,
  inline: false,
});
const LEADING_STRICT = /^\s*(["'])use strict\1\s*;?/;
const LEADING_COMMENTS = /^(?:\s+|\/\/[^\n]*(?:\n|$)|\/\*[\s\S]*?\*\/)*/;
const MARKER_WORDS = /\b(require|module|exports|__dirname|__filename|this|with)\b/;
const ESM_WORDS = /\b(import|export|await)\b/;
const modeCache = new Map();
let modeDir = null;
// "esm" | "cjs" | "cjs-strict" (CJS cuja estrictness vem da diretiva do topo).
function moduleMode(source) {
  let mode = modeCache.get(source);
  if (mode !== undefined) return mode;
  if (LEADING_STRICT.test(source.replace(LEADING_COMMENTS, ""))) {
    mode = ESM_WORDS.test(source) && bundlesAsEsm(source) ? "esm" : "cjs-strict";
  } else if (!MARKER_WORDS.test(source)) {
    mode = "esm";
  } else {
    mode = bundlesAsEsm(source) ? "esm" : "cjs";
  }
  modeCache.set(source, mode);
  return mode;
}
// O `bun build` só embrulha em `__commonJS` com `require`/`module` em uso; `with`, `__dirname`, `__filename` e `exports`
// soltos o runtime trata como CJS e o bundler não. Estes marcadores complementam, desfeitos pela declaração do nome.

function hasFreeMarker(source) {
  const free = (name) => !new RegExp(`\\b(?:var|let|const|function|class)\\s+${name}\\b|\\bfunction\\s*[\\w$]*\\s*\\([^)]*\\b${name}\\b`).test(source)
    && new RegExp(`(?<![.\\w$'"\`])${name}\\b(?!\\s*:)`).test(source);
  if (/\bwith\s*\(/.test(source)) return true;
  if (free("__dirname") || free("__filename") || free("exports") || free("module")) return true;
  // `typeof require;` sozinho como instrução é código morto que o transpilador descarta: não marca.
  return free("require") && !/^\s*typeof\s+require\s*;?\s*$/m.test(source);
}
// Verdadeiro quando o arquivo roda como ESM: o `bun build` não embrulha em `__commonJS`, ou nem compila.
function bundlesAsEsm(source) {
  if (hasFreeMarker(source)) return false;
  const os = require("os");
  if (modeDir === null) modeDir = fs.mkdtempSync(path.join(os.tmpdir(), "golden-mode-"));
  const file = path.join(modeDir, "probe.js");
  fs.writeFileSync(file, source);
  const run = Bun.spawnSync([process.execPath, "build", "--target=bun", file], { stdout: "pipe", stderr: "pipe" });
  if (run.exitCode !== 0) return true;
  return !run.stdout.toString().includes("__commonJS");
}
// Texto que o runtime do bun entrega ao JSC para um programa CJS. Medido (bun 1.4.2): é a função
// `function(exports, require, module, __filename, __dirname) {` + corpo + `}`, o `arguments.callee.toString()` do topo.
// O corpo NÃO é o de `Bun.Transpiler`: o runtime (a) recua cada linha 2 espaços (menos o miolo de template literal),
// (b) sobe toda declaração `class` para o início, (c) não funde declaradores (`var a = 1; var b = 2;` em vez de
// `var a = 1, b = 2;`), (d) põe o `var x;` içado logo depois da instrução que o continha e não no fim, (e) descarta
// "use strict" (do topo e de função). Em vez de imitar isso por regra, a sonda pergunta ao próprio bun: roda o programa
// com uma primeira instrução `__CJS_PROBE__(arguments.callee.toString(), module)` (preload `cjs-probe-preload.js`, que
// imprime e sai antes de o programa executar), tira essa linha do texto e devolve o corpo recuado, com "\n" no fim.
// `arguments.callee` falha em modo estrito, por isso a diretiva do topo sai do fonte da sonda (o texto não a mostra).
// Sem a diretiva e sem outro marcador de CJS no resto do programa o bun classificaria o arquivo da sonda como ESM, sem
// wrapper (`arguments is not defined`); a referência livre a `module` na própria sonda força o CJS, e a linha inteira
// sai do texto devolvido.
const CJS_HEADER = "function(exports, require, module, __filename, __dirname) {";
const CJS_PROBE_LINE = "  __CJS_PROBE__(arguments.callee.toString(), module);";
function cjsRuntimeBody(source) {
  const os = require("os");
  if (modeDir === null) modeDir = fs.mkdtempSync(path.join(os.tmpdir(), "golden-mode-"));
  const comments = source.match(LEADING_COMMENTS)[0];
  const rest = source.slice(comments.length).replace(LEADING_STRICT, "");
  const file = path.join(modeDir, "cjs-text.cjs");
  fs.writeFileSync(file, "__CJS_PROBE__(arguments.callee.toString(), module);\n" + comments + rest);
  const preload = path.join(__dirname, "cjs-probe-preload.js");
  const run = Bun.spawnSync([process.execPath, "--preload", preload, file], { stdout: "pipe", stderr: "pipe" });
  const line = run.stdout.toString().split("\n").find((l) => l.startsWith("\u0001"));
  if (!line) throw new Error("sonda CJS sem saída: " + run.stderr.toString() + JSON.stringify(source));
  const lines = JSON.parse(line.slice(1)).split("\n");
  const probe = lines.indexOf(CJS_PROBE_LINE);
  if (lines[0] !== CJS_HEADER || lines[lines.length - 1] !== "}" || probe < 0) {
    throw new Error("forma inesperada do wrapper CJS: " + JSON.stringify(lines.slice(0, 3)));
  }
  lines.splice(probe, 1);
  return lines.slice(1, -1).join("\n") + "\n";
}
// Arquivo que o bun deve executar para ver o texto de `canonicalSource(source)`. ESM: o próprio texto canônico. CJS: o
// o programa ORIGINAL: o texto canônico é o recuo da reimpressão do runtime e não é um arquivo válido para reexecutar
// (o recuo mexeria no miolo de template literal, e o `Bun.Transpiler` reordena `var` içado diferente do runtime), então o
// que o bun vê é por construção o corpo da sonda.
function executableSource(source, forcedMode) {
  if ((forcedMode || moduleMode(source)) !== "esm") return source;
  // O "use strict" embutido pelo `canonicalSource` NÃO pode ir para o arquivo que o bun executa: com a diretiva e sem
  // import/export o bun classifica o arquivo como CJS estrito e o embrulha numa função (nasce `arguments`, `this` vira
  // `module.exports`), mudando a semântica do ESM original (`arguments` no topo é ReferenceError em ESM). Sem a diretiva,
  // o bun o trata como ESM, que já é estrito.
  const canonical = canonicalSource(source);
  return canonical.startsWith(STRICT_PREFIX) ? canonical.slice(STRICT_PREFIX.length) : canonical;
}
const STRICT_PREFIX = '"use strict";\n';
// ESM: texto de `Bun.Transpiler` com "use strict" embutido (idempotente). CJS: exatamente o corpo que o runtime do bun
// põe na função de wrapper (recuo e ordem de declarações incluídos), precedido de "use strict" só no CJS estrito, porque o
// runtime descarta a diretiva do texto mas o porte precisa dela para rodar em modo estrito. O harness do porte avalia
// `function(exports, require, module, __filename, __dirname) {\n` + corpo + `}` (ver PLAN/relato), sem renormalizar.
function canonicalSource(source, forcedMode) {
  const mode = forcedMode || moduleMode(source);
  if (mode !== "esm") {
    return (mode === "cjs-strict" ? '"use strict";\n' : "") + cjsRuntimeBody(source);
  }
  const out = canonicalTranspiler.transformSync(source);
  const canonical = '"use strict";\n' + out;
  if ('"use strict";\n' + canonicalTranspiler.transformSync(canonical) !== canonical) throw new Error("transpilação não idempotente: " + JSON.stringify(source));
  return canonical;
}

// ---- Mapa de posições: texto gravado (reimpresso pelo bun) -> fonte original.
// O bun remapeia as posições de `Error.stack` e de `line`/`column` pelo source map do transpilador, então o stack mostra
// linha:coluna do fonte ORIGINAL enquanto o programa do golden é o texto reimpresso (recuo de 2 colunas no CJS, `class`
// içada, `var` desmembrado, `new` removido, `true` virando `!0`). O gerador alinha os tokens dos dois textos e grava as
// corridas `[linha, coluna, dlinha, dcoluna, ...]` (flat, coordenadas do texto gravado, base 1); o formato e a leitura
// estão em `src/parser/position_map.rs`. Uma posição pertence à última corrida que começa nela ou antes; só há corrida
// onde o deslocamento muda. Mapa que não desloca nada (`[1, 1, 0, 0]`) vira lista vazia.
const PUNCTUATORS = />>>=|\.\.\.|===|!==|\*\*=|<<=|>>=|>>>|&&=|\|\|=|\?\?=|=>|==|!=|<=|>=|&&|\|\||\?\?|\?\.|\+\+|--|\+=|-=|\*=|\/=|%=|&=|\|=|\^=|<<|>>|\*\*|[{}()\[\];,<>+\-*\/%&|^!~?:=.@#]/y;
const IDENTIFIER = /[\p{ID_Start}$_][\p{ID_Continue}$‌‍]*/uy;
const NUMBER = /0[xXbBoO][\da-fA-F_]+n?|(?:\d[\d_]*\.?[\d_]*|\.\d[\d_]*)(?:[eE][+-]?\d+)?n?/y;
const REGEX_AFTER_WORD = new Set(["return", "typeof", "instanceof", "in", "of", "new", "delete", "void", "throw", "case", "do", "else", "yield", "await"]);
function scanString(text, start) {
  const quote = text[start];
  let k = start + 1;
  while (k < text.length) {
    if (text[k] === "\\") k += 2;
    else if (text[k] === quote) return k + 1;
    else if (text[k] === "\n") return k;
    else k++;
  }
  return text.length;
}
function scanTemplate(text, start) {
  let k = start + 1;
  while (k < text.length) {
    if (text[k] === "\\") k += 2;
    else if (text[k] === "`") return k + 1;
    else if (text[k] === "$" && text[k + 1] === "{") k = scanBraces(text, k + 2);
    else k++;
  }
  return text.length;
}
function scanBraces(text, start) {
  let depth = 1;
  let k = start;
  while (k < text.length) {
    const c = text[k];
    if (c === "{") { depth++; k++; }
    else if (c === "}") { depth--; k++; if (depth === 0) return k; }
    else if (c === '"' || c === "'") k = scanString(text, k);
    else if (c === "`") k = scanTemplate(text, k);
    else k++;
  }
  return text.length;
}
function scanRegex(text, start) {
  let k = start + 1;
  let inClass = false;
  while (k < text.length && text[k] !== "\n") {
    const c = text[k];
    if (c === "\\") k += 2;
    else if (c === "[") { inClass = true; k++; }
    else if (c === "]") { inClass = false; k++; }
    else if (c === "/" && !inClass) {
      k++;
      while (k < text.length && /[\w$]/.test(text[k])) k++;
      return k;
    } else k++;
  }
  return start + 1;
}
// Tokens `{ t, line, col }` (linha e coluna de base 1, a coluna em unidades UTF-16 como o JSC), sem comentários.
function tokenize(text) {
  const tokens = [];
  let line = 1;
  let lineStart = 0;
  let i = 0;
  let prev = null;
  const advanceTo = (end) => {
    for (let k = i; k < end; k++) if (text.charCodeAt(k) === 10) { line++; lineStart = k + 1; }
    i = end;
  };
  while (i < text.length) {
    const c = text[i];
    if (/\s/.test(c)) { advanceTo(i + 1); continue; }
    if (c === "/" && text[i + 1] === "/") {
      const end = text.indexOf("\n", i);
      advanceTo(end < 0 ? text.length : end);
      continue;
    }
    if (c === "/" && text[i + 1] === "*") {
      const end = text.indexOf("*/", i + 2);
      advanceTo(end < 0 ? text.length : end + 2);
      continue;
    }
    let end;
    if (c === '"' || c === "'") end = scanString(text, i);
    else if (c === "`") end = scanTemplate(text, i);
    else if (c === "/" && (prev === null || (/^[^\w$'"`)\]}]/.test(prev) || REGEX_AFTER_WORD.has(prev)))) end = scanRegex(text, i);
    else {
      IDENTIFIER.lastIndex = i;
      NUMBER.lastIndex = i;
      PUNCTUATORS.lastIndex = i;
      let m;
      if ((m = IDENTIFIER.exec(text)) !== null) end = i + m[0].length;
      else if (/[\d.]/.test(c) && (m = NUMBER.exec(text)) !== null && m[0].length > 0) end = i + m[0].length;
      else if ((m = PUNCTUATORS.exec(text)) !== null) end = i + m[0].length;
      else end = i + 1;
    }
    const token = { t: text.slice(i, end), line, col: i - lineStart + 1 };
    tokens.push(token);
    prev = token.t;
    advanceTo(end);
  }
  return tokens;
}
// Para cada token do texto gravado, o índice do token do original a que corresponde (ou -1). Duas cabeças andam juntas;
// no descompasso tenta, nesta ordem: pular tokens do original (janela 32), pular tokens do gravado, voltar ao ponto de
// onde saiu um salto, saltar para a próxima ocorrência não usada dos três tokens seguintes em qualquer parte do original
// (a `class` que o runtime iça para o começo), e por fim deixar o token sem par.
function alignTokens(recorded, original) {
  const match = new Int32Array(recorded.length).fill(-1);
  const used = new Uint8Array(original.length);
  const WINDOW = 32;
  const gram = (tokens, k) => tokens[k].t + "\u0000" + (k + 1 < tokens.length ? tokens[k + 1].t : "") + "\u0000" + (k + 2 < tokens.length ? tokens[k + 2].t : "");
  let grams = null;
  const gramIndex = () => {
    if (grams === null) {
      grams = new Map();
      for (let k = 0; k < original.length; k++) {
        const key = gram(original, k);
        const list = grams.get(key);
        if (list) list.push(k); else grams.set(key, [k]);
      }
    }
    return grams;
  };
  const skipUsed = (p) => { while (p < original.length && used[p]) p++; return p; };
  const nextEqual = (i, j) => i + 1 >= recorded.length || j + 1 >= original.length || recorded[i + 1].t === original[j + 1].t;
  let i = 0;
  let j = 0;
  let saved = -1;
  while (i < recorded.length) {
    j = skipUsed(j);
    if (j < original.length && recorded[i].t === original[j].t) { match[i] = j; used[j] = 1; i++; j++; continue; }
    if (saved >= 0) {
      const back = skipUsed(saved);
      if (back < original.length && recorded[i].t === original[back].t) { j = back; saved = -1; continue; }
    }
    let found = -1;
    for (let d = 1; d <= WINDOW && j + d < original.length; d++) {
      if (!used[j + d] && original[j + d].t === recorded[i].t && nextEqual(i, j + d)) { found = j + d; break; }
    }
    if (found >= 0) { j = found; continue; }
    let skip = 0;
    if (j < original.length) {
      for (let e = 1; e <= WINDOW && i + e < recorded.length; e++) {
        if (recorded[i + e].t === original[j].t && nextEqual(i + e, j)) { skip = e; break; }
      }
    }
    if (skip > 0) { i += skip; continue; }
    const target = (gramIndex().get(gram(recorded, i)) || []).find((p) => !used[p]);
    if (target !== undefined) {
      if (saved < 0) saved = j;
      j = target;
      continue;
    }
    i++;
  }
  return match;
}
function positionRuns(original, recorded) {
  const recordedTokens = tokenize(recorded);
  const originalTokens = tokenize(original);
  const match = alignTokens(recordedTokens, originalTokens);
  const runs = [];
  let lineDelta = null;
  let columnDelta = null;
  recordedTokens.forEach((token, k) => {
    if (match[k] < 0) return;
    const source = originalTokens[match[k]];
    const dl = source.line - token.line;
    const dc = source.col - token.col;
    if (dl !== lineDelta || dc !== columnDelta) {
      runs.push(token.line, token.col, dl, dc);
      lineDelta = dl;
      columnDelta = dc;
    }
  });
  return runs.length === 4 && runs[2] === 0 && runs[3] === 0 ? [] : runs;
}
// O programa de um caso de golden: `source` (o que se grava, canônico), `executable` (o que o bun executa, para o
// resultado e o stack saírem do fonte original) e `meta` (`[modo, ...corridas]`, a quinta coluna do tsv; modo 0 ESM, 1 CJS,
// 2 CJS estrito). Fonte que o bun nem parseia: fica o original nos dois e sem meta (o harness o roda como script).
// `sloppy` força o modo CJS sem "use strict" (o `.cjs` que o bun roda em modo não estrito) para o programa que só vale
// fora do modo estrito (`function static() {}`, `function await() {}` como identificador): o ESM do bun o rejeita com
// SyntaxError e o caso inteiro seria descartado. `prepared.file_extension` diz a extensão que o arquivo executável
// precisa ter para o bun honrar o modo (`.cjs` quando forçado, `.js` no resto).
const preparedCache = new Map();
function prepareProgram(original, sloppy = false) {
  const key = sloppy ? "\u0000sloppy\u0000" + original : original;
  let prepared = preparedCache.get(key);
  if (prepared === undefined) {
    const forcedMode = sloppy ? "cjs" : undefined;
    try {
      const source = canonicalSource(original, forcedMode);
      const mode = forcedMode || moduleMode(original);
      prepared = { source, executable: executableSource(original, forcedMode), meta: [mode === "esm" ? 0 : mode === "cjs" ? 1 : 2, ...positionRuns(original, source)], file_extension: sloppy ? ".cjs" : ".js" };
    } catch (e) {
      prepared = { source: original, executable: original, meta: null, file_extension: ".js" };
    }
    preparedCache.set(key, prepared);
  }
  return prepared;
}

// Roda o programa pelo bun como arquivo (transpilador incluído) e devolve `{ prepared, marked }`. O transpilador dobra
// constantes e recusa em tempo de parse o que o JSC puro só rejeita ao executar (`'abc'.length = 1` vira SyntaxError
// "Left side of assignment is not a reference"); nesse caso o resultado sairia `<undefined>` por artefato do oráculo, e o
// programa é rodado de novo como script puro (`vm.runInThisContext`, sem transpilador), como o harness do porte o roda.
function runPrepared(original, file, preload, cwd, timeout) {
  const { spawnSync } = require("child_process");
  const run = (prepared) => {
    fs.writeFileSync(file, prepared.executable);
    const out = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd, timeout });
    return { out, marked: (out.stdout || "").split("\n").find((line) => line.startsWith("\u0001")) };
  };
  let prepared = prepareProgram(original);
  let result = run(prepared);
  if (result.marked === "\u0001\"<undefined>\"" && /SyntaxError/.test(result.out.stderr || "")) {
    const script = prepareScript(original);
    const second = run({ ...script, file_extension: ".cjs" });
    if (second.marked) { prepared = script; result = second; }
  }
  return { prepared, marked: result.marked };
}

// Programa que o transpilador do bun nem parseia mas o JSC aceita (`function await() {}` e a referência a `await` em
// script sloppy): o bun recusa o ARQUIVO, não o motor. O arquivo vira um `.cjs` que entrega o texto original ao JSC por
// `vm.runInThisContext` (script sloppy no global, como o harness do porte o roda); o golden guarda o original, sem meta.
function prepareScript(original) {
  return {
    source: original,
    executable: `require("vm").runInThisContext(${JSON.stringify(original)});\n`,
    meta: null,
    file_extension: ".cjs",
  };
}

// Canonicalização com cache, para fonte que o bun não parseia devolve o próprio texto.
const canonicalCache = new Map();
function canonicalOrSelf(source) {
  let out = canonicalCache.get(source);
  if (out === undefined) {
    try { out = canonicalSource(source); } catch (e) { out = source; }
    canonicalCache.set(source, out);
  }
  return out;
}

// Conjunto dos programas dos goldens vizinhos que reconhece a forma crua e a canônica: `has(src)` vale se `src` ou a
// forma canônica dele, passados por `key`, batem com algum programa conhecido (cru ou canônico). `key` normaliza antes
// da comparação (padrão: identidade), por exemplo para comparar só o corpo do programa.
function knownProgramSet(own, filter, key = (source) => source) {
  if (typeof own !== "string" || !own.endsWith(".tsv")) throw new Error("knownProgramSet(own, filter): own é o nome do golden do chamador, ex. \"x_bun.tsv\"");
  const accepts = Array.isArray(filter) ? (name) => filter.includes(name) : filter;
  const names = fs.readdirSync(GOLDEN_DIR).filter((name) => name.endsWith(".tsv") && name !== own && accepts(name));
  // Canonizar 1,3 milhão de programas custa horas (duas transpilações cada); a forma canônica de cada golden vizinho fica
  // num cache em disco, por arquivo, e só os goldens que mudaram são refeitos (em paralelo, num subprocesso).
  ensureCanonicalCache(names);
  const set = new Set();
  for (const name of names) {
    const programs = sourcesOf(name, fs.readFileSync(path.join(GOLDEN_DIR, name), "utf8"));
    if (programs.length === 0) continue;
    // Um golden regravado por outro gerador depois do `ensure` muda de mtime e perde o cache: refaz só esse.
    if (!fs.existsSync(canonicalCachePath(name))) ensureCanonicalCache([name]);
    const canonical = readCanonicalCache(name, programs.length);
    for (let i = 0; i < programs.length; i++) {
      set.add(key(programs[i]));
      set.add(key(canonical[i]));
    }
  }
  return { has: (source) => set.has(key(source)) || set.has(key(canonicalOrSelf(source))) };
}

// Cache da forma canônica de cada golden: ~/.cache/zjsc-golden/canonical/<arquivo>.<tamanho>-<mtime>.json.gz, um array de
// textos alinhado a `sourcesOf`. A chave leva tamanho e mtime do golden e a versão do bun (a transpilação depende dela).
function canonicalCachePath(name) {
  const os = require("os");
  const stat = fs.statSync(path.join(GOLDEN_DIR, name));
  const dir = path.join(process.env.ZJSC_GOLDEN_CACHE || path.join(os.homedir(), ".cache", "zjsc-golden"), "canonical");
  return path.join(dir, `${name}.${stat.size}-${Math.floor(stat.mtimeMs)}-bun${Bun.version}.json.gz`);
}
function readCanonicalCache(name, expected) {
  const list = JSON.parse(require("zlib").gunzipSync(fs.readFileSync(canonicalCachePath(name))).toString("utf8"));
  if (list.length !== expected) throw new Error(`cache canônico de ${name} fora de alinhamento: ${list.length} contra ${expected}`);
  return list;
}
function buildCanonicalCache(name) {
  const programs = sourcesOf(name, fs.readFileSync(path.join(GOLDEN_DIR, name), "utf8"));
  if (programs.length === 0) return;
  const file = canonicalCachePath(name);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, require("zlib").gzipSync(JSON.stringify(programs.map(canonicalOrSelf))));
  fs.renameSync(tmp, file);
}
// Refaz, num subprocesso paralelo (golden-canonical-cache.js), os caches que faltam. O progresso vai para stderr.
function ensureCanonicalCache(names) {
  const missing = names.filter((name) => !fs.existsSync(canonicalCachePath(name)) && sourcesOf(name, fs.readFileSync(path.join(GOLDEN_DIR, name), "utf8")).length > 0);
  if (missing.length === 0) return;
  process.stderr.write(`cache canônico: ${missing.length} goldens a canonizar\n`);
  const run = require("child_process").spawnSync(process.execPath, [path.join(__dirname, "golden-canonical-cache.js"), ...missing], { stdio: ["ignore", "inherit", "inherit"] });
  if (run.status !== 0) throw new Error("golden-canonical-cache.js falhou (código " + run.status + ")");
}

// Preload que imprime a global `R` na saída do processo, marcada com U+0001. Tudo o que ele usa é capturado antes de o
// programa rodar: `process.stdout` é criado de forma preguiçosa pelo bun e, em um programa que apaga
// `Reflect.ownKeys`, troca o getter de `Object.prototype.__proto__` ou de `constructor`, a criação já lança e o
// resultado se perde. `fs.writeSync(1, ...)` não passa por isso. O valor lido é o mesmo de antes (`globalThis.R`).
const RESULT_PRELOAD = `{
  const writeSync = require("fs").writeSync;
  const stringify = JSON.stringify;
  const toText = String;
  const global = globalThis;
  process.on("exit", () => {
    const value = global.R;
    // O pipe pode aceitar só parte do texto (ou dar EAGAIN) quando o resultado é grande: repete até acabar.
    const bytes = Buffer.from("\\u0001" + stringify(value === undefined ? "<undefined>" : toText(value)) + "\\n");
    let offset = 0;
    while (offset < bytes.length) {
      try {
        offset += writeSync(1, bytes, offset, bytes.length - offset);
      } catch (error) {
        if (!error || error.code !== "EAGAIN") throw error;
      }
    }
  });
}
`;

// Para os geradores que rodam um bun filho por programa: grava o `RESULT_PRELOAD` num diretório temporário (removido na
// saída do gerador) e devolve o caminho, para o `--preload` do filho. O pai decodifica a saída com `decodeResult`.
function writeResultPreload() {
  const os = require("os");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "result-preload-"));
  const file = path.join(dir, "preload.js");
  fs.writeFileSync(file, RESULT_PRELOAD);
  process.on("exit", () => fs.rmSync(dir, { recursive: true, force: true }));
  return file;
}

// Lê o resultado emitido pelo `RESULT_PRELOAD`: a linha marcada com U+0001 traz o texto em JSON. `null` se faltou.
function decodeResult(stdout) {
  const marked = stdout.split("\n").find(line => line.startsWith("\u0001"));
  return marked === undefined ? null : JSON.parse(marked.slice(1));
}

module.exports = { RESULT_PRELOAD, writeResultPreload, decodeResult,prepareScript, runPrepared, canonicalSource, executableSource, prepareProgram, positionRuns, tokenize, cjsRuntimeBody, moduleMode,canonicalOrSelf, sampleByHash, stepSampler, knownPrograms, knownProgramSet, buildCanonicalCache, factorRows, factorMeta, normalizePrelude, joinMeta, emitFactored, emitFactoredLines, assertPublicResult, findLeak, emitRow, writePreludes, readRows, preludesPath, GOLDEN_DIR };
