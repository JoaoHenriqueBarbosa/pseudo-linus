//! H32: date (parsing de `-d` e `+FORMAT`) contra o GNU coreutils 9.7 com relógio congelado.
//!
//! O front-end do `date` ([`cli`]) é nosso e igual pra todos os candidatos; só mudam a gramática do
//! `-d` ([`cli::DateParser`]) e o strftime com banco de fusos ([`cli::DateFormatter`]). Candidatos:
//!
//! 1. `parse_datetime` 0.16 + `jiff` 0.2 com o tzdb embutido (o prior do design);
//! 2. `interim` 0.2 + `chrono` 0.4 + `chrono-tz` 0.10 (o lado chrono pedido);
//! 3. `parse_datetime` 0.11 + `chrono`/`chrono-tz` (alternativa do lado chrono: a última versão do
//!    parser do uutils antes da migração pro jiff);
//! 4. `parse_datetime` 0.16 + strftime do chrono (combinação de atribuição: separa o formatador do
//!    chrono do parser).

pub mod cli;
pub mod engines;

use std::collections::BTreeMap;

use anyhow::Result;
use harness::{Candidate, CandidateResult, Case, CaseComparison, Conformance, Fit, HypothesisVerdict, Outcome, Verdict};
use serde_json::{Value, json};

use crate::common::{self, DepSummary, Part};
use cli::DateCandidate;
use engines::{ChronoFormatter, InterimChrono, JiffFormatter, ParseDatetimeChrono, ParseDatetimeJiff};

/// Tags que resumem a conformidade por tema.
const SUMMARY_TAGS: &[&str] = &[
    "format", "flags", "width", "nanos", "locale", "week", "offset", "options", "cli", "parse", "relative", "weekday",
    "time", "iso", "text", "rfc2822", "epoch", "zone-name", "error", "tz", "dst", "posix", "tz-in-string", "edge",
];

struct Spec {
    candidate: DateCandidate,
    key: &'static str,
    /// Componentes, pra atribuir a divergência por troca (mesmo parser com outro formatador e vice-versa).
    parser_key: &'static str,
    formatter_key: &'static str,
    /// Crates cujo depscan entra no candidato.
    deps: &'static [&'static str],
    reference: bool,
}

/// Placar de um candidato com as taxas por tema que decidem o veredito.
struct Scored {
    name: String,
    conf: Conformance,
    reference: bool,
    /// +FORMAT: casos com tag "format" ou "flags" (sem -d, o relógio é congelado).
    format: (usize, usize),
    /// Só diretivas, sem flags nem largura.
    directives: (usize, usize),
    parse: (usize, usize),
    tz: (usize, usize),
    cli: (usize, usize),
}

pub fn run() -> Result<Part> {
    let cases = common::load_cases("date")?;
    let specs = [
        Spec {
            candidate: DateCandidate { parser: Box::new(ParseDatetimeJiff), formatter: Box::new(JiffFormatter) },
            key: "parse_datetime-jiff",
            parser_key: "parse_datetime-0.16",
            formatter_key: "jiff",
            deps: &["parse_datetime", "jiff"],
            reference: false,
        },
        Spec {
            candidate: DateCandidate { parser: Box::new(InterimChrono), formatter: Box::new(ChronoFormatter) },
            key: "interim-chrono",
            parser_key: "interim",
            formatter_key: "chrono",
            deps: &["interim", "chrono", "chrono-tz"],
            reference: false,
        },
        Spec {
            candidate: DateCandidate { parser: Box::new(ParseDatetimeChrono), formatter: Box::new(ChronoFormatter) },
            key: "parse_datetime011-chrono",
            parser_key: "parse_datetime-0.11",
            formatter_key: "chrono",
            deps: &["chrono", "chrono-tz"],
            reference: false,
        },
        Spec {
            candidate: DateCandidate { parser: Box::new(ParseDatetimeJiff), formatter: Box::new(ChronoFormatter) },
            key: "parse_datetime-chrono-format",
            parser_key: "parse_datetime-0.16",
            formatter_key: "chrono",
            deps: &["parse_datetime", "chrono", "chrono-tz"],
            reference: true,
        },
    ];

    let mut scans: BTreeMap<&str, DepSummary> = BTreeMap::new();
    let mut linux_c_deps: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for name in ["parse_datetime", "jiff", "chrono", "chrono-tz", "interim"] {
        let scan = common::dep_scan(name)?;
        linux_c_deps.insert(name, c_deps_on_linux(&scan)?);
        scans.insert(name, scan);
    }

    // Primeiro pontua todos; a classificação usa os outros candidatos pra atribuir a causa.
    let scored_all: Vec<(Conformance, Vec<CaseComparison>)> = specs
        .iter()
        .map(|spec| {
            let (conf, all) = harness::score(&spec.candidate, &cases);
            dump_failures(&all, &format!("h32-{}", spec.key));
            (conf, all)
        })
        .collect();
    let passes: Vec<BTreeMap<&str, bool>> =
        scored_all.iter().map(|(_, all)| all.iter().map(|c| (c.id.as_str(), c.strict)).collect()).collect();
    let probes = root_cause_probes();
    let golden: BTreeMap<&str, (&Case, &Outcome)> = cases.iter().map(|(c, g)| (c.id.as_str(), (c, g))).collect();

    let mut part = Part { key: "h32_date".into(), ..Part::default() };
    let mut failing_by_candidate: Vec<Vec<String>> = Vec::new();
    let mut results: Vec<Scored> = Vec::new();
    for (idx, spec) in specs.iter().enumerate() {
        let (conf, all) = &scored_all[idx];
        let mut divergences: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut failing = Vec::new();
        for cmp in all.iter().filter(|c| !c.strict) {
            let (case, g) = golden[cmp.id.as_str()];
            let same = |pick: &dyn Fn(&Spec) -> bool| {
                specs.iter().enumerate().any(|(j, s)| j != idx && pick(s) && passes[j].get(cmp.id.as_str()) == Some(&true))
            };
            let same_parser_passes = same(&|s: &Spec| s.parser_key == spec.parser_key);
            let same_formatter_passes = same(&|s: &Spec| s.formatter_key == spec.formatter_key);
            let class = known_cause(&probes, spec.formatter_key, &cmp.id).unwrap_or_else(|| {
                match (same_parser_passes, same_formatter_passes) {
                    (true, false) => "formatador (atribuído por troca: mesmo parser passa com outro formatador)".to_string(),
                    (false, true) => "parser (atribuído por troca: mesmo formatador passa com outro parser)".to_string(),
                    _ => classify(case, g, cmp).to_string(),
                }
            });
            let detail = cmp.detail.first().map(|d| truncate(d, 200)).unwrap_or_default();
            divergences.entry(class).or_default().push(format!("{}: {detail}", cmp.id));
            failing.push(cmp.id.clone());
        }
        failing_by_candidate.push(failing);
        let deps: Vec<&DepSummary> = spec.deps.iter().map(|d| &scans[d]).collect();
        let raw_category = deps.iter().map(|d| d.tree_category.clone()).max().unwrap_or_default();
        let mut c_on_linux: Vec<String> = deps.iter().flat_map(|d| linux_c_deps[d.package.as_str()].clone()).collect();
        c_on_linux.sort();
        c_on_linux.dedup();
        // O depscan não filtra por alvo: defmt (`links`), iana-time-zone-haiku e wasm-bindgen-shared não
        // entram no build de Linux. Sem C no alvo real, a pior categoria passa a ser a do acoplamento ao host.
        let category = if raw_category == "c" && c_on_linux.is_empty() {
            if deps.iter().any(|d| d.own_host_touch > 0 || !d.host_touching_deps.is_empty()) { "b" } else { "a" }.to_string()
        } else {
            raw_category.clone()
        };
        let s = Scored {
            name: spec.candidate.name(),
            conf: conf.clone(),
            reference: spec.reference,
            format: subset(&cases, all, |c| has_tag(c, "format") || has_tag(c, "flags")),
            directives: subset(&cases, all, |c| has_tag(c, "format") && !has_tag(c, "flags") && !has_tag(c, "edge")),
            parse: subset(&cases, all, |c| has_tag(c, "parse")),
            tz: subset(&cases, all, |c| has_tag(c, "tz")),
            cli: subset(&cases, all, |c| has_tag(c, "options")),
        };
        let fit = if spec.reference { Fit::Reference } else { fit_for(&s) };
        let class_counts: BTreeMap<&str, usize> = divergences.iter().map(|(k, v)| (k.as_str(), v.len())).collect();
        part.candidates.push(CandidateResult {
            name: spec.candidate.name(),
            version: versions(spec.key),
            role: "date".into(),
            category: Some(category),
            conformance: Some(conf.clone()),
            fit,
            notes: candidate_notes(spec.key, &s),
            metrics: json!({
                "strict_rate": round3(conf.strict_rate()),
                "lenient_rate": round3(conf.lenient_rate()),
                "format_rate": pair_json(s.format),
                "directives_rate": pair_json(s.directives),
                "parse_rate": pair_json(s.parse),
                "tz_rate": pair_json(s.tz),
                "options_rate": pair_json(s.cli),
                "by_tag_strict_rate": tag_rates(conf),
                "divergence_counts": class_counts,
                "divergences": divergences,
                "category_depscan_all_targets": raw_category,
                "c_deps_built_on_linux": c_on_linux,
                "depscan": deps,
            }),
        });
        results.push(s);
    }

    // Casos que falham em todos os candidatos: ou é front-end nosso, ou é comportamento do GNU que
    // nenhuma das bibliotecas reproduz.
    let mut common_failures: Vec<String> = failing_by_candidate.first().cloned().unwrap_or_default();
    for f in &failing_by_candidate[1..] {
        common_failures.retain(|id| f.contains(id));
    }

    let host = host_coupling_probe();
    let tzdb = json!({
        "oracle_tzdata": oracle_tzdata(),
        "jiff_tzdb_bundled": jiff_tzdb::VERSION,
        "chrono_tz": chrono_tz::IANA_TZDB_VERSION,
    });

    part.hypotheses.push(verdict(&results, &host, &tzdb));
    part.metrics = json!({
        "cases": cases.len(),
        "common_failures": common_failures,
        "root_cause_probes": probes,
        "tzdb_versions": tzdb,
        "host_coupling": host,
        "clock": "relógio congelado via FAKETIME + LD_PRELOAD da libfaketime (ver corpus/cases/date/format.toml)",
    });
    part.notes.push(
        "H32: os casos com relógio usam FAKETIME absoluto + LD_PRELOAD da libfaketime em vez do campo `faketime` do harness: \
         o wrapper `faketime` só fixa os segundos e herda a fração de segundo real (medido: 12:00:00.79, 12:00:00.92...), \
         então %S vira o segundo seguinte de vez em quando. Com -u a libfaketime reinterpreta o FAKETIME absoluto no TZ \
         novo, então os casos com -u usam o mtime da fixture (-r)."
            .into(),
    );
    part.notes.push(
        "H32: o tzdata do oráculo (Debian 13) não traz os nomes legados (US/Eastern, Universal ficam no pacote \
         tzdata-legacy); o tzdb embutido do jiff traz, então esses casos divergem por dado de fuso, não por código."
            .into(),
    );
    Ok(part)
}

/// Das dependências com C que o depscan achou, quais entram de fato na árvore pro alvo Linux x86_64
/// (`cargo tree --target`), já que o depscan varre a árvore de todos os alvos.
fn c_deps_on_linux(scan: &DepSummary) -> Result<Vec<String>> {
    if scan.c_deps.is_empty() {
        return Ok(Vec::new());
    }
    let out = std::process::Command::new("cargo")
        .args(["tree", "-e", "normal", "--target", "x86_64-unknown-linux-gnu", "--prefix", "none", "--format", "{p}"])
        .arg("--manifest-path")
        .arg(common::manifest_path())
        .arg("-p")
        .arg(format!("{}@{}", scan.package, scan.version))
        .output()?;
    anyhow::ensure!(out.status.success(), "cargo tree: {}", String::from_utf8_lossy(&out.stderr));
    let built: std::collections::BTreeSet<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next()?;
            let version = it.next()?.trim_start_matches('v');
            Some(format!("{name} {version}"))
        })
        .collect();
    Ok(scan.c_deps.iter().filter(|c| built.contains(*c)).cloned().collect())
}

fn dump_failures(all: &[CaseComparison], name: &str) {
    let failures: Vec<&CaseComparison> = all.iter().filter(|c| !c.strict).collect();
    if let Ok(text) = serde_json::to_string_pretty(&failures) {
        let _ = std::fs::write(common::scratch().join(format!("{name}.json")), text);
    }
}

/// Classifica a divergência de um caso: biblioteca (parser, formatador, fuso) ou front-end.
fn classify(case: &Case, golden: &Outcome, cmp: &CaseComparison) -> &'static str {
    let has = |t: &str| case.tags.iter().any(|x| x == t);
    if cmp.unsupported.is_some() {
        return "front-end: não suportado";
    }
    if has("cli") {
        return "front-end: linha de comando";
    }
    let tz_resolution = has("tz") && (has("edge") || has("tzdb-content") || has("posix"));
    if tz_resolution && cmp.exit_ok && !cmp.stdout_ok {
        return "fuso: resolução do TZ ou dado do tzdb";
    }
    if has("ambiguous") && cmp.exit_ok && !cmp.stdout_ok {
        return "parser: hora local ambígua (o GNU herda o isdst do agora)";
    }
    if has("dst") && has("relative") && cmp.exit_ok && !cmp.stdout_ok {
        return "parser: relativo atravessando horário de verão";
    }
    if has("parse") {
        if !cmp.exit_ok {
            return if golden.exit == Some(0) {
                "parser: rejeita entrada que o GNU aceita"
            } else {
                "parser: aceita entrada que o GNU rejeita"
            };
        }
        if !cmp.stdout_ok {
            return if has("tz") { "parser ou fuso: instante diferente" } else { "parser: instante diferente" };
        }
        return "mensagem de erro";
    }
    if !cmp.exit_ok {
        return "formatador: erro de formatação";
    }
    if !cmp.stdout_ok {
        return if has("tz") { "fuso: resolução do TZ ou dado do tzdb" } else { "formatador: strftime" };
    }
    "mensagem de erro"
}

fn tag_rates(conf: &Conformance) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for tag in SUMMARY_TAGS {
        if let Some((strict, _lenient, total)) = conf.by_tag.get(*tag) {
            out.insert(tag.to_string(), json!({"strict": strict, "total": total, "rate": round3(ratio(*strict, *total))}));
        }
    }
    out
}

fn ratio(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { a as f64 / b as f64 }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { s.chars().take(max).collect::<String>() + "..." }
}

fn has_tag(case: &Case, tag: &str) -> bool {
    case.tags.iter().any(|t| t == tag)
}

/// (passaram, total) no subconjunto de casos escolhido.
fn subset(cases: &[(Case, Outcome)], all: &[CaseComparison], keep: impl Fn(&Case) -> bool) -> (usize, usize) {
    let by_id: BTreeMap<&str, &CaseComparison> = all.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut pass = 0;
    let mut total = 0;
    for (case, _) in cases.iter().filter(|(c, _)| keep(c)) {
        total += 1;
        pass += by_id.get(case.id.as_str()).is_some_and(|c| c.strict) as usize;
    }
    (pass, total)
}

fn pct(p: (usize, usize)) -> f64 {
    ratio(p.0, p.1)
}

fn pair_json(p: (usize, usize)) -> Value {
    json!({"strict": p.0, "total": p.1, "rate": round3(pct(p))})
}

/// Encaixa se passa >= 95% no total; encaixa com trabalho se as diretivas do +FORMAT e o -d passam
/// >= 85% (as lacunas cabem numa camada nossa); senão não encaixa.
fn fit_for(s: &Scored) -> Fit {
    if s.conf.strict_rate() >= 0.95 {
        Fit::Fits
    } else if pct(s.directives) >= 0.85 && pct(s.parse) >= 0.85 {
        Fit::FitsWithWork
    } else {
        Fit::DoesNotFit
    }
}

/// Sondas que confirmam em tempo de execução a causa de divergências que a troca de componente não
/// separa (o formatador alternativo também falha no caso por outro motivo).
fn root_cause_probes() -> Value {
    let neg = jiff::Timestamp::from_nanosecond(-1_500_000_000).expect("ts").to_zoned(jiff::tz::TimeZone::UTC);
    let jiff_neg_s = neg.strftime("%s").to_string();
    let ny = engines::jiff_zone("America/New_York");
    let base = jiff::Timestamp::from_second(1_768_478_400).expect("ts").to_zoned(ny.clone());
    let gap = parse_datetime::parse_datetime_at_date(base.clone(), "2026-03-08 02:30")
        .ok()
        .and_then(|p| p.into_zoned())
        .map(|z| z.strftime("%F %T %Z").to_string());
    let plus = parse_datetime::parse_datetime_at_date(base, "2026-01-15 12:00 +3 hours")
        .ok()
        .and_then(|p| p.into_zoned())
        .map(|z| z.with_time_zone(jiff::tz::TimeZone::UTC).strftime("%F %T").to_string());
    json!({
        "jiff_strftime_s_negative_fraction": {"instant": "-1.5 s", "jiff": jiff_neg_s, "gnu": "-2"},
        "parse_datetime_accepts_dst_gap": {"input": "2026-03-08 02:30 America/New_York", "result": gap, "gnu": "invalid date"},
        "parse_datetime_time_then_signed_number": {
            "input": "2026-01-15 12:00 +3 hours (UTC)",
            "result": plus,
            "gnu": "2026-01-15 10:00:00 (o GNU lê '+3' depois da hora como fuso UTC+3 e 'hours' como +1 hora)"
        },
    })
}

/// Causa confirmada por sonda pra um caso específico.
fn known_cause(probes: &Value, formatter_key: &str, case_id: &str) -> Option<String> {
    let jiff_truncates = probes["jiff_strftime_s_negative_fraction"]["jiff"] == "-1";
    (formatter_key == "jiff" && case_id == "date-parse-epoch-negative-fraction" && jiff_truncates)
        .then(|| "formatador: %s do jiff trunca pra zero em instante negativo fracionário (sonda)".to_string())
}

fn versions(key: &str) -> String {
    match key {
        "parse_datetime-jiff" => "parse_datetime 0.16.0, jiff 0.2.37".into(),
        "interim-chrono" => "interim 0.2.1, chrono 0.4.45, chrono-tz 0.10.4".into(),
        "parse_datetime011-chrono" => "parse_datetime 0.11.0, chrono 0.4.45, chrono-tz 0.10.4".into(),
        _ => "parse_datetime 0.16.0, chrono 0.4.45, chrono-tz 0.10.4".into(),
    }
}

fn candidate_notes(key: &str, s: &Scored) -> String {
    let base = format!(
        "strict {}/{} (+FORMAT {}/{}, só diretivas {}/{}, -d {}/{}, fusos {}/{}, opções {}/{}).",
        s.conf.strict_pass,
        s.conf.total,
        s.format.0,
        s.format.1,
        s.directives.0,
        s.directives.1,
        s.parse.0,
        s.parse.1,
        s.tz.0,
        s.tz.1,
        s.cli.0,
        s.cli.1
    );
    let extra = match key {
        "parse_datetime-jiff" => {
            "Front-end nosso injeta o agora (parse_datetime_at_date) e o fuso (TimeZoneDatabase::bundled); o prefixo TZ=\"...\" \
             do -d é separado pelo front-end porque o parse_datetime o resolve pelo banco global do jiff, que lê /usr/share/zoneinfo."
        }
        "interim-chrono" => {
            "interim não tem a gramática do GNU (sem @epoch, ISO com offset, fuso por nome, combinações data + relativo); \
             chrono não tem %N, %q, flags ^ e # nem largura; chrono-tz não entende TZ POSIX."
        }
        "parse_datetime011-chrono" => {
            "parse_datetime 0.11 tem a gramática do GNU mas trabalha com offset fixo (DST errado em datas de outra estação) \
             e a API exige DateTime<Local>; strftime do chrono com as mesmas lacunas do candidato interim."
        }
        _ => "Combinação de atribuição: o mesmo parser do candidato principal com o strftime do chrono.",
    };
    format!("{base} {extra}")
}

/// O parse_datetime liga as features `tz-system` e `tzdb-zoneinfo` do jiff, e resolve TZ="..." com
/// `TimeZone::get`, que usa o banco global. Mede qual banco global está em uso neste processo e o que
/// o parse_datetime faz com o prefixo sem o nosso front-end.
fn host_coupling_probe() -> Value {
    let global = format!("{:?}", jiff::tz::db());
    let now = jiff::Timestamp::from_second(1_768_478_400).expect("ts").to_zoned(jiff::tz::TimeZone::UTC);
    let raw = parse_datetime::parse_datetime_at_date(now, r#"TZ="Asia/Tokyo" 2026-01-15 09:00"#)
        .ok()
        .and_then(|p| p.into_zoned())
        .map(|z| z.timestamp().as_second());
    let reads_host = global.contains("/usr/share/zoneinfo") || global.contains("ZoneInfo");
    json!({
        "jiff_global_db": global,
        "global_db_reads_host_zoneinfo": reads_host,
        "parse_datetime_tz_prefix_resolves_via": "jiff::tz::TimeZone::get (banco global), parse_datetime-0.16.0/src/items/timezone.rs:93",
        "parse_datetime_jiff_features": ["tz-system", "tzdb-bundle-platform", "tzdb-zoneinfo"],
        "raw_tz_prefix_tokyo_epoch": raw,
        "expected_tokyo_epoch": 1_768_435_200,
        "mitigation": "front-end separa TZ=\"...\" e resolve o fuso pelo TimeZoneDatabase::bundled(); now e fuso sempre injetados",
        "zoned_now_used_only_without_base": "parse_datetime chama Zoned::now() só quando não recebe base; usamos parse_datetime_at_date",
    })
}

fn oracle_tzdata() -> String {
    let path = harness::paths::root().join("golden/oracle-versions.txt");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| t.lines().find(|l| l.starts_with("tzdata\t")).map(|l| l.trim_start_matches("tzdata\t").to_string()))
        .unwrap_or_default()
}

fn verdict(all: &[Scored], host: &Value, tzdb: &Value) -> HypothesisVerdict {
    let best = all
        .iter()
        .filter(|s| !s.reference)
        .max_by_key(|s| s.conf.strict_pass)
        .expect("ao menos um candidato");
    let format_rate = pct(best.format);
    let parse_rate = pct(best.parse);
    // Regra: confirmada se o melhor candidato passa >= 95% no +FORMAT (diretivas e flags) e no -d;
    // parcial se passa >= 75% nos dois (as lacunas cabem numa camada nossa); refutada abaixo disso.
    let min_rate = format_rate.min(parse_rate);
    let verdict = if min_rate >= 0.95 {
        Verdict::Confirmed
    } else if min_rate >= 0.75 {
        Verdict::Partial
    } else {
        Verdict::Refuted
    };
    let others: Vec<String> = all
        .iter()
        .filter(|s| s.name != best.name || s.reference)
        .map(|s| format!("{}{}: {}/{}", s.name, if s.reference { " (atribuição)" } else { "" }, s.conf.strict_pass, s.conf.total))
        .collect();
    let bundled = tzdb["jiff_tzdb_bundled"].as_str().unwrap_or("?");
    let oracle = tzdb["oracle_tzdata"].as_str().unwrap_or("?");
    let summary = format!(
        "{}: {}/{} casos byte a byte ({:.1}%); +FORMAT {}/{} (só diretivas {}/{}), -d {}/{}, fusos {}/{}, opções {}/{}. \
         Outros: {}. O agora e o fuso entram por API (parse_datetime_at_date; tzdb embutido do jiff {bundled}, oráculo {oracle}), \
         mas TZ=\"...\" dentro do -d é resolvido pelo banco global do jiff, que lê /usr/share/zoneinfo do host: o front-end separa o prefixo.",
        best.name,
        best.conf.strict_pass,
        best.conf.total,
        best.conf.strict_rate() * 100.0,
        best.format.0,
        best.format.1,
        best.directives.0,
        best.directives.1,
        best.parse.0,
        best.parse.1,
        best.tz.0,
        best.tz.1,
        best.cli.0,
        best.cli.1,
        others.join("; ")
    );
    HypothesisVerdict {
        id: "H32".into(),
        verdict,
        summary,
        evidence: json!({
            "best": best.name,
            "strict": best.conf.strict_pass,
            "total": best.conf.total,
            "format": pair_json(best.format),
            "directives": pair_json(best.directives),
            "parse": pair_json(best.parse),
            "tz": pair_json(best.tz),
            "options": pair_json(best.cli),
            "others": others,
            "tzdb_versions": tzdb,
            "host_coupling": host,
            "rule": "confirmada se +FORMAT (diretivas e flags) e -d >= 95% no melhor candidato; parcial se ambos >= 75%; refutada abaixo",
        }),
    }
}
