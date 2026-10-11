const { emitRow } = require("./golden-prelude.js");
// Gera tests/golden/collator_bun.tsv: ordenação do Intl.Collator por locale, medida no bun.
// Colunas: programa de uma linha (ASCII, letras fora do ASCII entram como \uXXXX) e o resultado
// (JSON ASCII). tests/collator_bun_golden.rs roda cada programa na engine e compara com a segunda coluna.
// Uso: bun scripts/gen-collator-golden.js > tests/golden/collator_bun.tsv
function q(text) {
  return JSON.stringify(text).replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
}

const common = "a b c d e f g h i j k l m n o p q r s t u v w x y z".split(" ");
const lists = {
  sv: "zebra år äpple öl ånga ägg ört apa bil ö z a ä å y x vår väg över åtta ärlig zon yta xylofon sol sjö själ skog strand stad tal träd tåg tät ö ål älv ödla ordna orm zink zoo zäta åk ark".split(" "),
  da: "zebra år æble øl ånd ægte ørn apa bil æ ø å z a aa aarhus ål ærlig øst vest zink zoo yngel xylofon sol sø skov strand stad tal træ tog tæt ørred ål ælv øde ordna orm ark".split(" "),
  nb: "zebra år ære øl ånd egg ørn apa bil æ ø å z a ål ærlig øst vest zink zoo yngel xylofon sol sjø skog strand stad tall tre tog tett ørret ålen elv øde orden orm ark".split(" "),
  fi: "zebra äiti öljy åland yö vesi wiki vika väki xylofon ylä ääni örkki sauna sisu kala koira kissa talo tuli tyyli tähti tie ulos uusi vaara zoo ärsyttää öinen åbo ankka".split(" "),
  tr: "çay cam ağaç agac gül göl ıslak ilik iğne isim ışık ocak ördek oda okul şeker sen süt sütun uzak üzüm ulus zaman yıl yol ayı abla bağ cadde ceviz çocuk dağ eşek fırın gece hava ısı iş jilet kuş ömür".split(" "),
  pl: "zebra żaba źrebię ząb ćma cały czas łódź lato las ławka ń nos noga ósmy ogon ołów ślad sól siła ćwiek ęś ęza edukacja ąka ala ale ból bór chata dom dąb gęś goście hałas jabłko kość mąka mleko nić ryż".split(" "),
  cs: "zebra žába čaj cesta chata chléb hora hrad ch h c č d ď e ě i í r ř s š sůl ten t ť z ž zima žena čáp říp říše rok rys šála škola stůl tráva čokoláda cukr dům".split(" "),
  es: "ñu nube nada noche nuevo ñandú año ano caña cana llave lluvia luz lado casa perro pero cama chico cielo ch ll n ñ o torre tarde mañana manzana ñoño árbol arbol zapato zorro".split(" "),
  de: "Äpfel Apfel Ärger Arzt Öl Ofen Ölbaum über Übel Uhr Straße Strasse Strauß Stress ß ss s ae ä oe ö ue ü zebra Zoo Maß Mass Fuß Fuss Bär Bar Müller Mueller Mutter schön schon Köln Koeln Löwe Lowe".split(" "),
  fr: "cote côte coté côté coté cote pêche péché pêcher pécher été etre être élève eleve œuf oeuf œil ça ca ceci coeur cœur noël noel naïf naif hôtel hotel façade facade fêter fêtes".split(" "),
  it: "città citta perché perche è e é caffè caffe più piu già gia lì li là la qui quì sì si ancòra ancora".split(" "),
  pt: "ação acao ámbar ambar são sao são sau coração coracao côco coco avó avô avo ás as à a pé pe pêra pera ça cá ca irmã irma maçã maca lição licao".split(" "),
  ru: "ёж еж ель жук зебра яблоко Яблоко арбуз авто ёлка еда ё е и й к л м н о п р с т у ф х ц ч ш щ ъ ы ь э ю я а б в г д".split(" "),
  uk: "ґанок гора їжак іній ї і є е ж з и ь я ю г ґ д а б в ґуля гуля їсти ірис єнот ще ящір яблуко юшка".split(" "),
  el: "άλφα αλφα ά α έψιλον εψιλον ή η ί ι ό ο ύ υ ώ ω σίγμα ς σ τ ϊ ι ΐ γάτα γατα δέντρο δεντρο βήτα ωμέγα".split(" "),
};

const specs = [
  ["sv", "sv"], ["da", "da"], ["nb", "nb"], ["fi", "fi"], ["tr", "tr"], ["pl", "pl"], ["cs", "cs"], ["es", "es"],
  ["de", "de"], ["de-u-co-phonebk", "de"], ["es-u-co-trad", "es"], ["fr", "fr"], ["it", "it"], ["pt", "pt"],
  ["ru", "ru"], ["uk", "uk"], ["el", "el"],
];

// As letras específicas de cada locale entram misturadas em maiúscula e minúscula.
function build(key) {
  const base = lists[key];
  const extra = base.filter((w) => w.length > 0).map((w) => w[0].toUpperCase() + w.slice(1));
  return [...new Set([...base, ...extra.slice(0, 8)])].slice(0, 60);
}

const programs = [];
const add = (source) => programs.push(source);
const sortProgram = (locale, words, options) =>
  `JSON.stringify(${JSON.stringify(words).replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"))}.sort(new Intl.Collator(${q(locale)}${options ? ", " + options : ""}).compare))`;

for (const [locale, key] of specs) {
  const words = build(key);
  add(sortProgram(locale, words, ""));
  add(sortProgram(locale, [...words].reverse(), ""));
}

// Opções: sensitivity, numeric, caseFirst, em alguns locales.
const optionWords = ["a", "A", "á", "Á", "ä", "Ä", "å", "z", "Z", "ç", "c", "C", "ö", "ş", "s", "ı", "i", "I", "İ", "ñ", "n", "ß", "ss", "file2", "file10", "file1", "File2", "x10y", "x9y", "item 05", "item 5", "ch", "cz", "dz", "ll", "lz"];
for (const locale of ["en", "sv", "tr", "de", "es", "cs", "pt", "fr"]) {
  for (const sensitivity of ["base", "accent", "case", "variant"]) {
    add(sortProgram(locale, optionWords, `{ sensitivity: ${q(sensitivity)} }`));
  }
  add(sortProgram(locale, optionWords, "{ numeric: true }"));
  add(sortProgram(locale, optionWords, '{ caseFirst: "upper" }'));
  add(sortProgram(locale, optionWords, '{ caseFirst: "lower" }'));
  add(sortProgram(locale, optionWords, '{ numeric: true, caseFirst: "upper", sensitivity: "variant" }'));
}
for (const [x, y] of [["a", "å"], ["z", "å"], ["z", "ä"], ["ä", "ö"], ["ch", "i"], ["ch", "d"], ["n", "ñ"], ["ñ", "o"]]) {
  for (const locale of ["en", "sv", "cs", "es", "de"]) {
    add(`${q(x)}.localeCompare(${q(y)}, ${q(locale)})`);
  }
}

// Resolução do locale do Collator.
for (const tag of ["en-GB", "en-US", "en-ZZ", "pt-PT", "pt-AO", "pt-BR", "fr-CA", "fr-FR", "de-AT", "de-CH", "de-DE", "es-MX", "es-ES", "sv-SE", "sv-FI", "da-DK", "nb-NO", "no", "nn", "fi-FI", "tr-TR", "pl-PL", "cs-CZ", "sk", "lt", "ru-RU", "uk-UA", "el-GR", "zh", "ja", "ko", "xx", "und", "en-u-co-emoji", "de-u-co-phonebk", "de-u-co-phonebk-kn", "es-u-co-trad", "sv-u-kf-upper", "en-u-kn", "fr-u-kn-false", "pt-PT-u-kf-lower", "sr-Latn", "zh-Hant-TW"]) {
  add(`new Intl.Collator(${q(tag)}).resolvedOptions().locale`);
}

for (const source of programs) {
  let result;
  try {
    const value = (0, eval)(source);
    result = JSON.stringify(value);
  } catch (error) {
    result = "throw";
  }
  const ascii = result.replace(/[^\x20-\x7e]/g, (c) => "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0"));
  emitRow(source + "\t" + ascii);
}
