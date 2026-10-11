// Gera src/runtime/text_decoder_<nome>_data.rs: as tabelas de ponteiros das codificações de vários bytes do WHATWG
// Encoding, medidas no `TextDecoder` do bun 1.4.2. Para cada sequência (prefixo, lead, trail) do intervalo, ela é
// decodificada sozinha com `fatal`: o ponto de código que sai é a entrada da tabela, o erro é "sem mapeamento" (0).
// Uma sequência que dê mais de um ponto de código, ou um fora do BMP, aborta o gerador: a codificação precisa de
// tabela própria (ver PENDENTE). A tabela sai em decimal, uma linha por 24 entradas, para o arquivo ficar compacto.
//
// Uso (da raiz do crate, com o bun no PATH):
//   bun scripts/gen-text-decoder-multibyte-tables.js euc-kr > src/runtime/text_decoder_euc_kr_data.rs
//   bun scripts/gen-text-decoder-multibyte-tables.js jis    > src/runtime/text_decoder_jis_data.rs
//
// Portadas:
//   - euc-kr: ponteiro `(lead - 0x81) * 190 + (trail - 0x41)` (lead 0x81..0xFE, trail 0x41..0xFE).
//   - jis (`shift_jis` e `euc-jp`, uma tabela só): `JIS0208_INDEX` é indexada pelo ponteiro do shift_jis do WHATWG
//     (`lead_index * 188 + trail_index`; lead 0x81..0x9F e 0xE0..0xFC, trail 0x40..0x7E e 0x80..0xFC; 60 * 188 entradas,
//     com o bloco EUDC 8836..10715 em U+E000..). O euc-jp usa o MESMO índice com o ponteiro `(lead - 0xA1) * 94 +
//     (trail - 0xA1)` (0..8835): o gerador confere as 8836 entradas contra o `euc-jp` do bun e aborta se divergirem.
//     `JIS0212_INDEX` (euc-jp com prefixo 0x8F, mesmo ponteiro de 94 * 94) é medida só pelo euc-jp. Katakana de meia
//     largura (shift_jis 0xA1..0xDF, euc-jp 0x8E + 0xA1..0xDF) e o ASCII são conferidos aqui e viram fórmula no Rust.
//
//   - big5 (`wide`): ponteiro `(lead - 0x81) * 157 + posição do trail` (trail 0x40..0x7E e 0xA1..0xFE), tabela `u32`
//     (planos 2 e 3) e `BIG5_PAIRS` para os quatro pares que dão dois pontos de código (0x8862, 0x8864, 0x88A3, 0x88A5).
//
// PENDENTE (as outras multibyte do WHATWG; cada uma entra aqui como uma linha de CONFIG quando o decodificador for portado):
//   - iso-2022-jp: sem tabela de pares própria, usa a jis0208 do euc-jp com um autômato de escapes de estado.
const CONFIG = {
  "euc-kr": {
    // O nome canônico no bun e os rótulos do WHATWG (conferidos contra o bun: o que ele recusa cai fora).
    labels: "euc-kr cseuckr csksc56011987 iso-ir-149 korean ks_c_5601-1987 ks_c_5601-1989 ksc5601 ksc_5601 windows-949",
    leads: [[0x81, 0xfe]],
    trails: [[0x41, 0xfe]],
    constant: "EUC_KR_INDEX",
  },
  big5: {
    labels: "big5 big5-hkscs cn-big5 csbig5 x-x-big5",
    leads: [[0x81, 0xfe]],
    trails: [[0x40, 0x7e], [0xa1, 0xfe]],
    constant: "BIG5_INDEX",
    // `u32` (planos 2 e 3) e os pares que dão dois pontos de código viram a lista `BIG5_PAIRS`.
    wide: true,
  },
  // gb18030 e gbk: o WHATWG tem duas codificações (`gbk` e `gb18030`) com o MESMO decodificador, então um arquivo só.
  // Dois bytes: ponteiro `(lead - 0x81) * 190 + posição do trail` (trail 0x40..0x7E e 0x80..0xFE). Quatro bytes: a tabela
  // de intervalos medida (`generateGb18030`).
  gb18030: {
    labels: "gb18030",
    gbkLabels: "gbk chinese csgb2312 csiso58gb231280 gb2312 gb_2312 gb_2312-80 iso-ir-58 x-gbk",
    leads: [[0x81, 0xfe]],
    trails: [[0x40, 0x7e], [0x80, 0xfe]],
    constant: "GB18030_INDEX",
    fourByte: true,
  },
};
const JIS_LABELS = {
  shift_jis: "shift_jis csshiftjis ms932 ms_kanji shift-jis sjis windows-31j x-sjis",
  "euc-jp": "euc-jp cseucpkdfmtjapanese x-euc-jp",
};

const range = (ranges) => ranges.flatMap(([from, to]) => Array.from({ length: to - from + 1 }, (_, i) => from + i));
const acceptedLabels = (name, labels) =>
  labels.split(" ").filter((label) => {
    try {
      return new TextDecoder(label).encoding === name;
    } catch (error) {
      return false;
    }
  });

// Decodifica `bytes` sozinho com `fatal`: o ponto de código único (BMP), ou 0 quando dá erro.
const probe = (decoder, name, bytes, wide) => {
  let text;
  try {
    text = decoder.decode(new Uint8Array(bytes));
  } catch (error) {
    return wide ? [0] : 0;
  }
  const points = Array.from(text).map((c) => c.codePointAt(0));
  const limit = wide ? 0x10ffff : 0xfffe;
  if (points.length < 1 || points.length > (wide ? 2 : 1) || points.some((p) => p > limit || p === 0)) {
    throw new Error(`${name}: ${bytes.map((b) => b.toString(16))} deu ${JSON.stringify(text)}`);
  }
  return wide ? points : points[0];
};

// A tabela de `prefix + (lead, trail)` para todo lead e trail, em ordem de lead e depois de trail. Com `pairs` (um array),
// a codificação é larga: um par que dá dois pontos de código entra em `pairs` como `[ponteiro, primeiro, segundo]` e a
// tabela guarda o primeiro.
const probeTable = (name, prefix, leads, trails, pairs) => {
  const decoder = new TextDecoder(name, { fatal: true });
  const table = [];
  for (const lead of leads) {
    for (const trail of trails) {
      const result = probe(decoder, name, [...prefix, lead, trail], Boolean(pairs));
      if (!pairs) table.push(result);
      else {
        if (result.length === 2) pairs.push([table.length, result[0], result[1]]);
        table.push(result[0]);
      }
    }
  }
  return table;
};

const tableLines = (constant, table, doc, type = "u16") => {
  const out = [`/// ${doc || `${table.filter((x) => x).length} pares com mapeamento de ${table.length}.`}`, `pub(crate) static ${constant}: [${type}; ${table.length}] = [`];
  for (let i = 0; i < table.length; i += 24) out.push("    " + table.slice(i, i + 24).join(", ") + ",");
  out.push("];");
  return out;
};

const labelsLine = (constant, name, labels) =>
  `pub(crate) const ${constant}: &[&str] = &[${acceptedLabels(name, labels).map((label) => JSON.stringify(label)).join(", ")}];`;

const regenerate = (arg, file) => [
  "//! ARQUIVO GERADO, não edite à mão. Para regenerar (da raiz do crate, com o bun no PATH):",
  "//!",
  "//! ```sh",
  `//! bun scripts/gen-text-decoder-multibyte-tables.js ${arg} > src/runtime/${file}`,
  "//! ```",
  "",
];

const generateSimple = (name, config) => {
  const leads = range(config.leads);
  const trails = range(config.trails);
  const pairs = config.wide ? [] : undefined;
  const table = probeTable(name, [], leads, trails, pairs);
  const out = [];
  if (config.wide) {
    out.push(`//! A tabela de ponteiros da codificação \`${name}\` do WHATWG Encoding, medida no \`TextDecoder\` do bun 1.4.2: cada par`);
    out.push("//! (lead, trail) do intervalo decodificado sozinho com `fatal`. O ponteiro é `(lead - lead_min) * trail_count + posição do");
    out.push("//! trail` (trail 0x40..0x7E e depois 0xA1..0xFE, o `(lead - 0x81) * 157 + trail - offset` do WHATWG); a entrada é o");
    out.push("//! ponto de código (planos 2 e 3 incluídos), ou 0 quando o par não tem mapeamento. Os ponteiros que dão dois pontos de");
    out.push("//! código estão em `BIG5_PAIRS` (a tabela guarda o primeiro).");
    out.push("//!");
    out.push(...regenerate(name, `text_decoder_${name.replace(/-/g, "_")}_data.rs`));
    out.push(`/// Os rótulos que o bun aceita para \`${name}\`.`);
    out.push(labelsLine("LABELS", name, config.labels));
    out.push("");
    out.push(`/// O menor e o maior lead, e a quantidade de trails por lead.`);
    out.push(`pub(crate) const LEAD_MIN: u8 = ${config.leads[0][0]};`);
    out.push(`pub(crate) const LEAD_MAX: u8 = ${config.leads[0][1]};`);
    out.push(`pub(crate) const TRAIL_COUNT: usize = ${trails.length};`);
    out.push("");
    out.push("/// Os ponteiros que dão dois pontos de código: (ponteiro, primeiro, segundo).");
    out.push(`pub(crate) const BIG5_PAIRS: [(usize, u32, u32); ${pairs.length}] = [${pairs.map(([p, a, b]) => `(${p}, ${a}, ${b})`).join(", ")}];`);
    out.push("");
    out.push(...tableLines(config.constant, table, undefined, "u32"));
    return out;
  }
  out.push(`//! A tabela de ponteiros da codificação \`${name}\` do WHATWG Encoding, medida no \`TextDecoder\` do bun 1.4.2: cada par`);
  out.push("//! (lead, trail) do intervalo decodificado sozinho com `fatal`. O ponteiro é `(lead - lead_min) * trail_count + (trail -");
  out.push("//! trail_min)`; a entrada é o ponto de código (BMP), ou 0 quando o par não tem mapeamento.");
  out.push("//!");
  out.push(...regenerate(name, `text_decoder_${name.replace(/-/g, "_")}_data.rs`));
  out.push(`/// Os rótulos que o bun aceita para \`${name}\`.`);
  out.push(labelsLine("LABELS", name, config.labels));
  out.push("");
  out.push(`/// O menor lead, o intervalo de leads e o de trails (mínimo e quantidade).`);
  out.push(`pub(crate) const LEAD_MIN: u8 = ${config.leads[0][0]};`);
  out.push(`pub(crate) const LEAD_MAX: u8 = ${config.leads[0][1]};`);
  out.push(`pub(crate) const TRAIL_MIN: u8 = ${config.trails[0][0]};`);
  out.push(`pub(crate) const TRAIL_COUNT: usize = ${trails.length};`);
  out.push("");
  out.push(...tableLines(config.constant, table));
  return out;
};

const generateJis = () => {
  // jis0208 pelo shift_jis (ponteiro do shift_jis do WHATWG).
  const sjisLeads = range([[0x81, 0x9f], [0xe0, 0xfc]]);
  const sjisTrails = range([[0x40, 0x7e], [0x80, 0xfc]]);
  const jis0208 = probeTable("shift_jis", [], sjisLeads, sjisTrails);
  // O euc-jp: o mesmo índice nos 94 * 94 primeiros ponteiros.
  const eucLeads = range([[0xa1, 0xfe]]);
  const eucTrails = range([[0xa1, 0xfe]]);
  const eucPairs = probeTable("euc-jp", [], eucLeads, eucTrails);
  eucPairs.forEach((point, pointer) => {
    if (point !== jis0208[pointer]) throw new Error(`euc-jp e shift_jis divergem no ponteiro ${pointer}: ${point} contra ${jis0208[pointer]}`);
  });
  const jis0212 = probeTable("euc-jp", [0x8f], eucLeads, eucTrails);
  // As fórmulas que o Rust usa no lugar de tabela.
  const sjis = new TextDecoder("shift_jis", { fatal: true });
  const euc = new TextDecoder("euc-jp", { fatal: true });
  for (let byte = 1; byte < 0x80; byte++) {
    if (probe(sjis, "shift_jis", [byte]) !== byte) throw new Error(`shift_jis ASCII ${byte}`);
    if (probe(euc, "euc-jp", [byte]) !== byte) throw new Error(`euc-jp ASCII ${byte}`);
  }
  if (sjis.decode(new Uint8Array([0x00])) !== "\0" || euc.decode(new Uint8Array([0x00])) !== "\0") throw new Error("ASCII 0x00");
  if (sjis.decode(new Uint8Array([0x80])) !== "\u0080") throw new Error("shift_jis 0x80");
  for (let byte = 0xa1; byte <= 0xdf; byte++) {
    if (probe(sjis, "shift_jis", [byte]) !== 0xff61 + byte - 0xa1) throw new Error(`shift_jis katakana ${byte}`);
    if (probe(euc, "euc-jp", [0x8e, byte]) !== 0xff61 + byte - 0xa1) throw new Error(`euc-jp katakana ${byte}`);
  }
  const out = [];
  out.push("//! As tabelas de ponteiros das codificações `shift_jis` e `euc-jp` do WHATWG Encoding, medidas no `TextDecoder` do bun 1.4.2");
  out.push("//! (cada sequência decodificada sozinha com `fatal`; a entrada é o ponto de código BMP, ou 0 sem mapeamento).");
  out.push("//!");
  out.push("//! - `JIS0208_INDEX`: ponteiro do shift_jis, `lead_index * 188 + trail_index` (lead 0x81..0x9F e depois 0xE0..0xFC,");
  out.push("//!   trail 0x40..0x7E e depois 0x80..0xFC); o euc-jp usa o mesmo índice com `(lead - 0xA1) * 94 + (trail - 0xA1)`, conferido");
  out.push("//!   na geração. O bloco EUDC (ponteiros 8836..10715) já está em U+E000..;");
  out.push("//! - `JIS0212_INDEX`: o euc-jp com prefixo 0x8F, ponteiro `(lead - 0xA1) * 94 + (trail - 0xA1)`.");
  out.push("//!");
  out.push(...regenerate("jis", "text_decoder_jis_data.rs"));
  out.push("/// Os rótulos que o bun aceita para `shift_jis`.");
  out.push(labelsLine("SHIFT_JIS_LABELS", "shift_jis", JIS_LABELS.shift_jis));
  out.push("");
  out.push("/// Os rótulos que o bun aceita para `euc-jp`.");
  out.push(labelsLine("EUC_JP_LABELS", "euc-jp", JIS_LABELS["euc-jp"]));
  out.push("");
  out.push("/// A quantidade de trails por lead no ponteiro do shift_jis, e de leads/trails por linha no do euc-jp.");
  out.push(`pub(crate) const SJIS_TRAIL_COUNT: usize = ${sjisTrails.length};`);
  out.push("pub(crate) const EUC_TRAIL_COUNT: usize = 94;");
  out.push("");
  out.push(...tableLines("JIS0208_INDEX", jis0208));
  out.push("");
  out.push(...tableLines("JIS0212_INDEX", jis0212));
  return out;
};

// gb18030 (e gbk): a tabela de dois bytes e os intervalos de quatro bytes. O espaço de quatro bytes é varrido inteiro
// (ponteiro `((b1 - 0x81) * 10 + b2 - 0x30) * 126 * 10 + (b3 - 0x81) * 10 + b4 - 0x30`, 0..1587599; b1 0x81..0xFE, b2 e
// b4 0x30..0x39, b3 0x81..0xFE), cada sequência decodificada sozinha com `fatal`. `GB18030_RANGES` lista onde a função
// ponteiro -> ponto de código recomeça: `(ponteiro, ponto)`, o ponto sobe de 1 em 1 até o próximo par; 0 é um trecho
// sem mapeamento. Nenhuma fórmula fica no Rust: o que sobra além do último par (erro) também está medido.
const generateGb18030 = (name, config) => {
  const leads = range(config.leads);
  const trails = range(config.trails);
  const table = probeTable(name, [], leads, trails);
  const decoder = new TextDecoder(name, { fatal: true });
  const total = 126 * 10 * 126 * 10;
  const ranges = [];
  let previous = -1;
  for (let pointer = 0; pointer < total; pointer++) {
    const bytes = [0x81 + Math.floor(pointer / 12600), 0x30 + (Math.floor(pointer / 1260) % 10), 0x81 + (Math.floor(pointer / 10) % 126), 0x30 + (pointer % 10)];
    const points = probe(decoder, name, bytes, true);
    if (points.length !== 1) throw new Error(`${name}: quatro bytes ${bytes.map((b) => b.toString(16))} deu ${points}`);
    const point = points[0];
    const continues = point === 0 ? previous === 0 : previous > 0 && point === previous + 1;
    if (!continues) ranges.push([pointer, point]);
    previous = point;
  }
  const out = [];
  out.push("//! As tabelas do `gb18030` e do `gbk` do WHATWG Encoding (o mesmo decodificador), medidas no `TextDecoder` do bun 1.4.2:");
  out.push("//! cada sequência decodificada sozinha com `fatal`.");
  out.push("//!");
  out.push("//! - `GB18030_INDEX`: pares de dois bytes, ponteiro `(lead - 0x81) * 190 + posição do trail` (trail 0x40..0x7E e depois");
  out.push("//!   0x80..0xFE); a entrada é o ponto de código (BMP), ou 0 sem mapeamento;");
  out.push("//! - `GB18030_RANGES`: quatro bytes, ponteiro `((b1 - 0x81) * 10 + b2 - 0x30) * 1260 + (b3 - 0x81) * 10 + b4 - 0x30`");
  out.push("//!   (0..1587599). Cada par é `(ponteiro, ponto)`: a partir do ponteiro o ponto sobe de 1 em 1 até o próximo par, e o");
  out.push("//!   ponto 0 marca um trecho sem mapeamento (erro). O último trecho vai até o fim do espaço.");
  out.push("//!");
  out.push(...regenerate(name, "text_decoder_gb18030_data.rs"));
  out.push("/// Os rótulos que o bun aceita para `gb18030`.");
  out.push(labelsLine("GB18030_LABELS", name, config.labels));
  out.push("");
  out.push("/// Os rótulos que o bun aceita para `gbk`.");
  out.push(labelsLine("GBK_LABELS", "gbk", config.gbkLabels));
  out.push("");
  out.push("/// O menor e o maior lead, e a quantidade de trails por lead.");
  out.push(`pub(crate) const LEAD_MIN: u8 = ${config.leads[0][0]};`);
  out.push(`pub(crate) const LEAD_MAX: u8 = ${config.leads[0][1]};`);
  out.push(`pub(crate) const TRAIL_COUNT: usize = ${trails.length};`);
  out.push("");
  out.push(`/// Os ${ranges.length} pares (ponteiro, ponto de código) dos intervalos de quatro bytes; 0 é um trecho sem mapeamento.`);
  out.push(`pub(crate) static GB18030_RANGES: [(u32, u32); ${ranges.length}] = [`);
  for (let i = 0; i < ranges.length; i += 8) out.push("    " + ranges.slice(i, i + 8).map(([p, c]) => `(${p}, ${c})`).join(", ") + ",");
  out.push("];");
  out.push("");
  out.push(...tableLines(config.constant, table));
  return out;
};

const name = process.argv[2];
let out;
if (name === "jis") out = generateJis();
else if (CONFIG[name]) out = CONFIG[name].fourByte ? generateGb18030(name, CONFIG[name]) : generateSimple(name, CONFIG[name]);
else throw new Error(`uso: gen-text-decoder-multibyte-tables.js <${[...Object.keys(CONFIG), "jis"].join("|")}>`);
process.stdout.write(out.join("\n") + "\n");
