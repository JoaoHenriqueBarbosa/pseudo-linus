//! Banco Unicode do `unicodedata`, lido dos arquivos oficiais do Unicode Character Database
//! (`crates/ul-python/data`, ver o README de lá): o 15.1.0 do `unicodedata` do CPython 3.13 e o
//! 3.2.0 de `unicodedata.ucd_3_2_0`. Cada banco é montado na primeira consulta.
//!
//! As regras de leitura seguem o que o CPython expõe: nomes algorítmicos só para Hangul e os
//! ideogramas CJK unificados, largura do Leste Asiático com o padrão `N`, valores numéricos do
//! Unihan (`kAccountingNumeric`, `kOtherNumeric`, `kPrimaryNumeric`) e normalização pelo UAX #15.

use std::collections::HashMap;
use std::sync::OnceLock;

const DATA_15_1: Sources = Sources {
    version: "15.1.0",
    unicode_data: include_str!("../../data/unicode-15.1.0/UnicodeData.txt"),
    east_asian_width: include_str!("../../data/unicode-15.1.0/EastAsianWidth.txt"),
    exclusions: include_str!("../../data/unicode-15.1.0/CompositionExclusions.txt"),
    aliases: include_str!("../../data/unicode-15.1.0/NameAliases.txt"),
    sequences: include_str!("../../data/unicode-15.1.0/NamedSequences.txt"),
};

const DATA_3_2: Sources = Sources {
    version: "3.2.0",
    unicode_data: include_str!("../../data/unicode-3.2.0/UnicodeData-3.2.0.txt"),
    east_asian_width: include_str!("../../data/unicode-3.2.0/EastAsianWidth-3.2.0.txt"),
    exclusions: include_str!("../../data/unicode-3.2.0/CompositionExclusions-3.2.0.txt"),
    aliases: "",
    sequences: "",
};

const UNIHAN_NUMERIC: &str = include_str!("../../data/unicode-15.1.0/Unihan_NumericValues.txt");

struct Sources {
    version: &'static str,
    unicode_data: &'static str,
    east_asian_width: &'static str,
    exclusions: &'static str,
    aliases: &'static str,
    sequences: &'static str,
}

/// Uma linha do `UnicodeData.txt`, com os campos como estão no arquivo.
#[derive(Clone, Copy)]
pub struct Rec {
    pub name: &'static str,
    pub category: &'static str,
    pub combining: u8,
    pub bidi: &'static str,
    pub decomposition: &'static str,
    pub decimal: &'static str,
    pub digit: &'static str,
    pub numeric: &'static str,
    pub mirrored: bool,
}

/// Intervalo `<..., First>`/`<..., Last>` do arquivo (ideogramas, Hangul, uso privado...).
struct Range {
    first: u32,
    last: u32,
    rec: Rec,
    ideograph: bool,
}

pub struct Db {
    pub version: &'static str,
    by_cp: HashMap<u32, Rec>,
    ranges: Vec<Range>,
    /// Larguras do `EastAsianWidth.txt` em intervalos ordenados; o resto é `N`.
    widths: Vec<(u32, u32, &'static str)>,
    names: HashMap<&'static str, u32>,
    aliases: HashMap<String, u32>,
    sequences: HashMap<String, Vec<u32>>,
    composition: HashMap<(u32, u32), u32>,
    /// `numeric()` dos ideogramas, do Unihan.
    unihan: HashMap<u32, &'static str>,
}

// Hangul (Unicode 3.12, "Conjoining Jamo Behavior").
const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;

const JAMO_L: [&str; 19] = ["G", "GG", "N", "D", "DD", "R", "M", "B", "BB", "S", "SS", "", "J", "JJ", "C", "K", "T", "P", "H"];
const JAMO_V: [&str; 21] = [
    "A", "AE", "YA", "YAE", "EO", "E", "YEO", "YE", "O", "WA", "WAE", "OE", "YO", "U", "WEO", "WE", "WI", "YU", "EU", "YI", "I",
];
const JAMO_T: [&str; 28] = [
    "", "G", "GG", "GS", "N", "NJ", "NH", "D", "L", "LG", "LM", "LB", "LS", "LT", "LP", "LH", "M", "B", "BS", "S", "SS", "NG",
    "J", "C", "K", "T", "P", "H",
];

pub fn current() -> &'static Db {
    static DB: OnceLock<Db> = OnceLock::new();
    DB.get_or_init(|| Db::parse(&DATA_15_1))
}

/// `unicodedata.ucd_3_2_0`: o banco atual visto pelo 3.2.0 (ver [`Legacy`]).
pub fn v3_2() -> &'static Legacy {
    static OLD: OnceLock<Db> = OnceLock::new();
    static VIEW: OnceLock<Legacy> = OnceLock::new();
    VIEW.get_or_init(|| Legacy { cur: current(), old: OLD.get_or_init(|| Db::parse(&DATA_3_2)) })
}

/// As consultas do `unicodedata`, comuns ao banco atual e à visão 3.2.0.
pub trait Props: Sync {
    fn version(&self) -> &'static str;
    fn category(&self, cp: u32) -> &'static str;
    fn bidirectional(&self, cp: u32) -> &'static str;
    fn combining(&self, cp: u32) -> u8;
    fn mirrored(&self, cp: u32) -> bool;
    fn decomposition(&self, cp: u32) -> &'static str;
    fn east_asian_width(&self, cp: u32) -> &'static str;
    fn decimal(&self, cp: u32) -> Option<i64>;
    fn digit(&self, cp: u32) -> Option<i64>;
    fn numeric(&self, cp: u32) -> Option<f64>;
    fn name(&self, cp: u32) -> Option<String>;
    fn lookup(&self, name: &str) -> Option<Vec<u32>>;
    /// A decomposição usada pela normalização (vazia: o caractere fica como está).
    fn norm_decomposition(&self, cp: u32) -> &'static str;
    /// Se `cp` pode ser decomposto ou composto algoritmicamente como Hangul.
    fn hangul(&self, cp: u32) -> bool;
    /// O composto canônico de `a` + `b`, se existe e é permitido.
    fn composite(&self, a: u32, b: u32) -> Option<u32>;

    /// `normalize(forma, texto)`; `None` se a forma não existe.
    fn normalize(&self, form: &str, text: &str) -> Option<String> {
        let cps: Vec<u32> = text.chars().map(u32::from).collect();
        let out = match form {
            "NFD" => decompose(self, &cps, false),
            "NFKD" => decompose(self, &cps, true),
            "NFC" => compose(self, decompose(self, &cps, false)),
            "NFKC" => compose(self, decompose(self, &cps, true)),
            _ => return None,
        };
        Some(out.into_iter().filter_map(char::from_u32).collect())
    }
}

fn decompose_into<P: Props + ?Sized>(p: &P, cp: u32, compat: bool, out: &mut Vec<u32>) {
    if (S_BASE..S_BASE + S_COUNT).contains(&cp) && p.hangul(cp) {
        let s = cp - S_BASE;
        out.push(L_BASE + s / N_COUNT);
        out.push(V_BASE + (s % N_COUNT) / T_COUNT);
        if s % T_COUNT != 0 {
            out.push(T_BASE + s % T_COUNT);
        }
        return;
    }
    let d = p.norm_decomposition(cp);
    let (tagged, body) = match d.strip_prefix('<') {
        Some(rest) => (true, rest.split_once('>').map_or("", |(_, b)| b)),
        None => (false, d),
    };
    if d.is_empty() || (tagged && !compat) {
        out.push(cp);
        return;
    }
    for part in body.split_whitespace().filter_map(hex) {
        decompose_into(p, part, compat, out);
    }
}

fn decompose<P: Props + ?Sized>(p: &P, text: &[u32], compat: bool) -> Vec<u32> {
    let mut out = Vec::with_capacity(text.len());
    for &cp in text {
        decompose_into(p, cp, compat, &mut out);
    }
    // Ordenação canônica: corridas de marcas com classe não nula, ordenadas de forma estável.
    // A classe vem sempre do banco atual, também no 3.2.0, como no CPython.
    let ccc = |c: u32| current().combining(c);
    let mut i = 0;
    while i < out.len() {
        if ccc(out[i]) == 0 {
            i += 1;
            continue;
        }
        let start = i;
        while i < out.len() && ccc(out[i]) != 0 {
            i += 1;
        }
        out[start..i].sort_by_key(|&c| ccc(c));
    }
    out
}

/// Composição canônica, como o exemplo de referência do UAX #15: um caractere compõe com o
/// último inicial se nada entre os dois o bloqueia.
fn compose<P: Props + ?Sized>(p: &P, text: Vec<u32>) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(text.len());
    let mut starter: Option<usize> = None;
    // Classe do último caractere guardado; 256 marca uma marca sem inicial antes.
    let mut last_class: u16 = 256;
    for cp in text {
        let class = u16::from(current().combining(cp));
        if let Some(si) = starter {
            if last_class < class || last_class == 0 {
                if let Some(c) = compose_pair(p, out[si], cp) {
                    out[si] = c;
                    continue;
                }
            }
        }
        if class == 0 {
            starter = Some(out.len());
        }
        last_class = class;
        out.push(cp);
    }
    out
}

fn compose_pair<P: Props + ?Sized>(p: &P, a: u32, b: u32) -> Option<u32> {
    if (L_BASE..L_BASE + L_COUNT).contains(&a) && (V_BASE..V_BASE + V_COUNT).contains(&b) {
        return Some(S_BASE + ((a - L_BASE) * V_COUNT + (b - V_BASE)) * T_COUNT);
    }
    if (S_BASE..S_BASE + S_COUNT).contains(&a) && (a - S_BASE) % T_COUNT == 0 && (T_BASE + 1..T_BASE + T_COUNT).contains(&b) {
        return Some(a + (b - T_BASE));
    }
    p.composite(a, b)
}

/// O banco atual visto como o 3.2.0, como o `ucd_3_2_0` do CPython: os caracteres que só
/// existem depois do 3.2.0 aparecem como não atribuídos; nos que existem nas duas versões,
/// categoria, bidi, largura, espelhamento e valor decimal são os do 3.2.0, e o resto é o atual.
/// A normalização usa os dados atuais, sem decompor nem compor caracteres novos e com os
/// mapeamentos do 3.2.0 onde eles mudaram depois (os corrigendos).
pub struct Legacy {
    cur: &'static Db,
    old: &'static Db,
}

impl Legacy {
    /// Caractere que o banco atual tem e o 3.2.0 não.
    fn is_new(&self, cp: u32) -> bool {
        self.cur.record(cp).is_some() && self.old.record(cp).is_none()
    }

    fn in_both(&self, cp: u32) -> bool {
        self.old.record(cp).is_some() && self.cur.record(cp).is_some()
    }
}

impl Props for Legacy {
    fn version(&self) -> &'static str {
        self.old.version
    }

    fn category(&self, cp: u32) -> &'static str {
        if self.is_new(cp) {
            "Cn"
        } else if self.in_both(cp) {
            self.old.category(cp)
        } else {
            self.cur.category(cp)
        }
    }

    fn bidirectional(&self, cp: u32) -> &'static str {
        if self.is_new(cp) {
            ""
        } else if self.in_both(cp) {
            self.old.bidirectional(cp)
        } else {
            self.cur.bidirectional(cp)
        }
    }

    fn combining(&self, cp: u32) -> u8 {
        if self.is_new(cp) { 0 } else { self.cur.combining(cp) }
    }

    fn mirrored(&self, cp: u32) -> bool {
        if self.is_new(cp) {
            false
        } else if self.in_both(cp) {
            self.old.mirrored(cp)
        } else {
            self.cur.mirrored(cp)
        }
    }

    fn decomposition(&self, cp: u32) -> &'static str {
        if self.is_new(cp) { "" } else { self.cur.decomposition(cp) }
    }

    fn east_asian_width(&self, cp: u32) -> &'static str {
        if self.is_new(cp) {
            "N"
        } else if self.in_both(cp) {
            self.old.east_asian_width(cp)
        } else {
            self.cur.east_asian_width(cp)
        }
    }

    fn decimal(&self, cp: u32) -> Option<i64> {
        if self.is_new(cp) {
            None
        } else if self.in_both(cp) {
            self.old.decimal(cp)
        } else {
            self.cur.decimal(cp)
        }
    }

    fn digit(&self, cp: u32) -> Option<i64> {
        self.cur.digit(cp)
    }

    fn numeric(&self, cp: u32) -> Option<f64> {
        if self.is_new(cp) { None } else { self.cur.numeric(cp) }
    }

    fn name(&self, cp: u32) -> Option<String> {
        if self.is_new(cp) { None } else { self.cur.name(cp) }
    }

    fn lookup(&self, name: &str) -> Option<Vec<u32>> {
        // Apelidos formais e sequências nomeadas não existiam no 3.2.0.
        let upper = name.to_ascii_uppercase();
        if self.cur.aliases.contains_key(upper.as_str()) || self.cur.sequences.contains_key(upper.as_str()) {
            return None;
        }
        self.cur.lookup(name).filter(|cps| cps.iter().all(|&c| !self.is_new(c)))
    }

    fn norm_decomposition(&self, cp: u32) -> &'static str {
        if self.is_new(cp) {
            return "";
        }
        let (old, cur) = (self.old.decomposition(cp), self.cur.decomposition(cp));
        if self.in_both(cp) && old != cur { old } else { cur }
    }

    fn hangul(&self, cp: u32) -> bool {
        self.old.record(cp).is_some()
    }

    fn composite(&self, a: u32, b: u32) -> Option<u32> {
        self.cur.composite(a, b).filter(|&c| !self.is_new(c))
    }
}

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim(), 16).ok()
}

/// Linhas úteis de um arquivo do UCD: sem comentário, sem espaços nas pontas, não vazias.
fn data_lines(text: &'static str) -> impl Iterator<Item = &'static str> {
    text.lines().map(|l| l.split('#').next().unwrap_or("").trim()).filter(|l| !l.is_empty())
}

/// `0041` ou `0041..005A`.
fn code_range(s: &str) -> Option<(u32, u32)> {
    match s.trim().split_once("..") {
        Some((a, b)) => Some((hex(a)?, hex(b)?)),
        None => hex(s).map(|c| (c, c)),
    }
}

impl Db {
    fn parse(src: &Sources) -> Db {
        let mut by_cp = HashMap::new();
        let mut ranges = Vec::new();
        let mut names = HashMap::new();
        let mut open: Option<(u32, Rec, bool)> = None;
        for line in src.unicode_data.lines().filter(|l| !l.is_empty()) {
            let f: Vec<&'static str> = line.split(';').collect();
            if f.len() < 15 {
                continue;
            }
            let Some(cp) = hex(f[0]) else { continue };
            let rec = Rec {
                name: f[1],
                category: f[2],
                combining: f[3].parse().unwrap_or(0),
                bidi: f[4],
                decomposition: f[5],
                decimal: f[6],
                digit: f[7],
                numeric: f[8],
                mirrored: f[9] == "Y",
            };
            if f[1].ends_with(", First>") {
                open = Some((cp, rec, f[1].starts_with("<CJK Ideograph")));
                continue;
            }
            if f[1].ends_with(", Last>") {
                if let Some((first, rec, ideograph)) = open.take() {
                    ranges.push(Range { first, last: cp, rec, ideograph });
                }
                continue;
            }
            if !f[1].starts_with('<') {
                names.insert(f[1], cp);
            }
            by_cp.insert(cp, rec);
        }
        ranges.sort_by_key(|r| r.first);

        let mut widths = Vec::new();
        for line in data_lines(src.east_asian_width) {
            let Some((range, w)) = line.split_once(';') else { continue };
            let Some((a, b)) = code_range(range) else { continue };
            widths.push((a, b, w.trim()));
        }
        widths.sort_by_key(|w| w.0);

        let mut aliases = HashMap::new();
        for line in data_lines(src.aliases) {
            let f: Vec<&str> = line.split(';').collect();
            if let (Some(cp), Some(name)) = (f.first().and_then(|c| hex(c)), f.get(1)) {
                aliases.insert(name.trim().to_string(), cp);
            }
        }
        let mut sequences = HashMap::new();
        for line in data_lines(src.sequences) {
            if let Some((name, seq)) = line.split_once(';') {
                let cps: Vec<u32> = seq.split_whitespace().filter_map(hex).collect();
                sequences.insert(name.trim().to_string(), cps);
            }
        }

        let excluded: std::collections::HashSet<u32> = data_lines(src.exclusions).filter_map(hex).collect();
        let mut db = Db {
            version: src.version,
            by_cp,
            ranges,
            widths,
            names,
            aliases,
            sequences,
            composition: HashMap::new(),
            unihan: HashMap::new(),
        };
        // Pares de composição canônica: decomposições canônicas de dois caracteres, fora das
        // exclusões do arquivo e das decomposições de não-iniciais (UAX #15, "Full Composition Exclusion").
        let mut composition = HashMap::new();
        for (&cp, rec) in &db.by_cp {
            if rec.decomposition.is_empty() || rec.decomposition.starts_with('<') || excluded.contains(&cp) || rec.combining != 0 {
                continue;
            }
            let parts: Vec<u32> = rec.decomposition.split_whitespace().filter_map(hex).collect();
            if let [a, b] = parts[..] {
                if db.combining(a) == 0 {
                    composition.insert((a, b), cp);
                }
            }
        }
        db.composition = composition;
        for line in UNIHAN_NUMERIC.lines().filter(|l| l.starts_with("U+")) {
            let f: Vec<&'static str> = line.split('\t').collect();
            if let [cp, field, value] = f[..] {
                if matches!(field, "kAccountingNumeric" | "kOtherNumeric" | "kPrimaryNumeric") {
                    if let Some(cp) = hex(&cp[2..]) {
                        db.unihan.insert(cp, value);
                    }
                }
            }
        }
        db
    }

    /// O registro de `cp`, se o caractere existe nesta versão.
    pub fn record(&self, cp: u32) -> Option<Rec> {
        if let Some(r) = self.by_cp.get(&cp) {
            return Some(*r);
        }
        self.range_of(cp).map(|r| r.rec)
    }

    fn range_of(&self, cp: u32) -> Option<&Range> {
        let i = self.ranges.partition_point(|r| r.first <= cp);
        let r = self.ranges.get(i.checked_sub(1)?)?;
        (cp <= r.last).then_some(r)
    }

    pub fn category(&self, cp: u32) -> &'static str {
        self.record(cp).map_or("Cn", |r| r.category)
    }

    pub fn bidirectional(&self, cp: u32) -> &'static str {
        self.record(cp).map_or("", |r| r.bidi)
    }

    pub fn combining(&self, cp: u32) -> u8 {
        self.record(cp).map_or(0, |r| r.combining)
    }

    pub fn mirrored(&self, cp: u32) -> bool {
        self.record(cp).is_some_and(|r| r.mirrored)
    }

    /// A decomposição como no campo 5 (Hangul não aparece, como no CPython).
    pub fn decomposition(&self, cp: u32) -> &'static str {
        self.record(cp).map_or("", |r| r.decomposition)
    }

    pub fn east_asian_width(&self, cp: u32) -> &'static str {
        let i = self.widths.partition_point(|w| w.0 <= cp);
        match i.checked_sub(1).and_then(|i| self.widths.get(i)) {
            Some(&(_, last, w)) if cp <= last => w,
            _ => "N",
        }
    }

    pub fn decimal(&self, cp: u32) -> Option<i64> {
        self.record(cp).and_then(|r| r.decimal.parse().ok())
    }

    pub fn digit(&self, cp: u32) -> Option<i64> {
        self.record(cp).and_then(|r| r.digit.parse().ok())
    }

    pub fn numeric(&self, cp: u32) -> Option<f64> {
        let rec = self.record(cp)?;
        // O Unihan pode listar vários valores; o CPython fica com o primeiro.
        let text = if rec.numeric.is_empty() { self.unihan.get(&cp)?.split_whitespace().next()? } else { rec.numeric };
        match text.split_once('/') {
            Some((n, d)) => Some(n.parse::<f64>().ok()? / d.parse::<f64>().ok()?),
            None => text.parse().ok(),
        }
    }

    pub fn name(&self, cp: u32) -> Option<String> {
        if (S_BASE..S_BASE + S_COUNT).contains(&cp) {
            if self.record(cp).is_none() {
                return None;
            }
            let s = cp - S_BASE;
            let (l, v, t) = (s / N_COUNT, (s % N_COUNT) / T_COUNT, s % T_COUNT);
            return Some(format!("HANGUL SYLLABLE {}{}{}", JAMO_L[l as usize], JAMO_V[v as usize], JAMO_T[t as usize]));
        }
        if let Some(r) = self.by_cp.get(&cp) {
            return (!r.name.starts_with('<')).then(|| r.name.to_string());
        }
        match self.range_of(cp) {
            Some(r) if r.ideograph => Some(format!("CJK UNIFIED IDEOGRAPH-{cp:X}")),
            _ => None,
        }
    }

    /// `lookup(nome)`: nome, apelido formal ou sequência nomeada (as duas últimas só no banco
    /// atual, como no CPython). Maiúsculas e minúsculas não importam.
    pub fn lookup(&self, name: &str) -> Option<Vec<u32>> {
        let upper = name.to_ascii_uppercase();
        if let Some(&cp) = self.names.get(upper.as_str()) {
            return Some(vec![cp]);
        }
        // Os nomes algorítmicos só casam em maiúsculas, como no `_getcode` do CPython.
        if let Some(rest) = name.strip_prefix("HANGUL SYLLABLE ") {
            return self.hangul_from_name(rest).map(|c| vec![c]);
        }
        if let Some(h) = name.strip_prefix("CJK UNIFIED IDEOGRAPH-") {
            if (4..=5).contains(&h.len()) && h.chars().all(|c| c.is_ascii_digit() || ('A'..='F').contains(&c)) {
                let cp = hex(h)?;
                return self.range_of(cp).filter(|r| r.ideograph).map(|_| vec![cp]);
            }
            return None;
        }
        if let Some(&cp) = self.aliases.get(upper.as_str()) {
            return Some(vec![cp]);
        }
        self.sequences.get(upper.as_str()).cloned()
    }

    fn hangul_from_name(&self, rest: &str) -> Option<u32> {
        // Como no CPython: o prefixo mais longo de cada tabela que casar.
        let take = |s: &str, table: &[&str]| -> Option<(usize, usize)> {
            table.iter().enumerate().filter(|(_, j)| s.starts_with(**j)).max_by_key(|(_, j)| j.len()).map(|(i, j)| (i, j.len()))
        };
        let (l, n1) = take(rest, &JAMO_L)?;
        let (v, n2) = take(&rest[n1..], &JAMO_V)?;
        let (t, n3) = take(&rest[n1 + n2..], &JAMO_T)?;
        if n1 + n2 + n3 != rest.len() {
            return None;
        }
        Some(S_BASE + (l as u32 * V_COUNT + v as u32) * T_COUNT + t as u32)
    }

    fn composite(&self, a: u32, b: u32) -> Option<u32> {
        self.composition.get(&(a, b)).copied()
    }
}

impl Props for Db {
    fn version(&self) -> &'static str {
        self.version
    }
    fn category(&self, cp: u32) -> &'static str {
        Db::category(self, cp)
    }
    fn bidirectional(&self, cp: u32) -> &'static str {
        Db::bidirectional(self, cp)
    }
    fn combining(&self, cp: u32) -> u8 {
        Db::combining(self, cp)
    }
    fn mirrored(&self, cp: u32) -> bool {
        Db::mirrored(self, cp)
    }
    fn decomposition(&self, cp: u32) -> &'static str {
        Db::decomposition(self, cp)
    }
    fn east_asian_width(&self, cp: u32) -> &'static str {
        Db::east_asian_width(self, cp)
    }
    fn decimal(&self, cp: u32) -> Option<i64> {
        Db::decimal(self, cp)
    }
    fn digit(&self, cp: u32) -> Option<i64> {
        Db::digit(self, cp)
    }
    fn numeric(&self, cp: u32) -> Option<f64> {
        Db::numeric(self, cp)
    }
    fn name(&self, cp: u32) -> Option<String> {
        Db::name(self, cp)
    }
    fn lookup(&self, name: &str) -> Option<Vec<u32>> {
        Db::lookup(self, name)
    }
    fn norm_decomposition(&self, cp: u32) -> &'static str {
        Db::decomposition(self, cp)
    }
    fn hangul(&self, cp: u32) -> bool {
        self.record(cp).is_some()
    }
    fn composite(&self, a: u32, b: u32) -> Option<u32> {
        Db::composite(self, a, b)
    }
}
