//! F01 (H23): existe motor de regex em Rust puro com a semântica POSIX do GNU?
//!
//! `cargo run --release` roda tudo e grava `results/f01-regex.json`:
//! 1. corpus commitado (`corpus/cases/regex`, golden em `golden/regex`): suítes do GNU grep 3.11 e
//!    regexes no estilo de agente;
//! 2. corpus minerado (padrões reais dos transcripts), com o oráculo rodado agora (cache em
//!    `scratch/f01-regex`), sem nada commitado;
//! 3. cada motor candidato em subprocesso isolado, com timeout por caso;
//! 4. métricas, combinações, depscan e veredito.
//!
//! Modos auxiliares: `--worker` (interno) e `--try MOTOR DIALETO PADRÃO TEXTO` (depuração).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use f01_regex::corpus::{ProbeSet, probes_for, spec_path, spec_subjects};
use f01_regex::engines::{Engine, all_engines};
use f01_regex::mined::{self, MinedStats};
use f01_regex::report::{CaseInfo, Rates, by_feature, case_info, combos, rates};
use f01_regex::sample::seed_of;
use f01_regex::worker::{EngineRun, run_all, tally, worker_main};
use harness::{Case, CandidateResult, ExperimentResult, Fit, Outcome, Verdict};
use serde_json::json;

const MINED_TOP: usize = 2000;
const MINED_RANDOM: usize = 1000;
const MINED_SEED: u64 = 0xF01;
/// Motores lentos demais pro corpus minerado inteiro: (nome, 1 a cada k regexes mineradas).
const SLOW_ENGINES: &[(&str, u64)] = &[("regast", 10)];

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--worker") => worker_main(&args[2], Path::new(&args[3]), args[4].parse()?),
        Some("--try") => try_one(&args[2..]),
        _ => run(),
    }
}

fn try_one(a: &[String]) -> Result<()> {
    let engine = f01_regex::engines::engine_by_name(&a[0]).context("motor")?;
    let dialect: f01_regex::parse::Dialect = serde_json::from_str(&format!("\"{}\"", a[1]))?;
    let re = f01_regex::parse::parse(&a[2], dialect)?;
    println!("ast: {:?}", re.root);
    for f in [f01_regex::emit::Flavor::Rust, f01_regex::emit::Flavor::Onig, f01_regex::emit::Flavor::PosixEre] {
        println!("{f:?}: {:?}", f01_regex::emit::emit(&re, f, Default::default()));
    }
    let m = engine.compile(&re, &a[2], dialect, false).map_err(|e| anyhow::anyhow!("{e}"))?;
    let hay = a[3].as_bytes();
    println!("first: {:?}", m.captures_at(hay, 0));
    println!("grep -o: {:?}", f01_regex::probe::grep_o_spans(m.as_ref(), hay));
    Ok(())
}

struct Mined {
    pairs: Vec<(Case, Outcome)>,
    weights: HashMap<String, u64>,
    stats: MinedStats,
    oracle_secs: f64,
    cached: bool,
}

fn mined_corpus(scratch: &Path) -> Result<Option<Mined>> {
    let dir = mined::agent_dir();
    if !dir.join("patterns.jsonl").exists() {
        return Ok(None);
    }
    let (entries, mut stats) = mined::load(&dir)?;
    let mut sampled = mined::sample(entries, MINED_TOP, MINED_RANDOM, MINED_SEED, &mut stats);
    let hay = mined::haystack(&spec_subjects(&spec_path())?);
    mined::attach_subjects(&mut sampled, &hay);
    let mut cases = Vec::new();
    let mut weights = HashMap::new();
    for m in &sampled {
        weights.insert(m.entry.id.clone(), m.weight);
        for (id, probe, tags) in probes_for(&m.entry, ProbeSet::Full) {
            cases.push(probe.to_case(id, tags));
        }
    }
    let key = harness::memtree::sha256_hex(&serde_json::to_vec(&cases)?);
    let cache = scratch.join(format!("mined-golden-{}.json", &key[..16]));
    let started = Instant::now();
    let (goldens, cached): (Vec<Outcome>, bool) = if cache.exists() {
        (serde_json::from_slice(&std::fs::read(&cache)?)?, true)
    } else {
        let oracle = harness::Oracle::locate()?;
        let chunk = cases.len().div_ceil(8).max(1);
        let parts: Vec<Result<Vec<Outcome>>> = std::thread::scope(|s| {
            let hs: Vec<_> = cases.chunks(chunk).map(|c| s.spawn(|| oracle.run(c))).collect();
            hs.into_iter().map(|h| h.join().expect("thread do oráculo")).collect()
        });
        let mut all = Vec::with_capacity(cases.len());
        for p in parts {
            all.extend(p?);
        }
        std::fs::write(&cache, serde_json::to_vec(&all)?)?;
        (all, false)
    };
    anyhow::ensure!(goldens.len() == cases.len(), "golden minerado incompleto");
    let pairs = cases.into_iter().zip(goldens).filter(|(_, g)| g.unsupported.is_none() && !g.timed_out).collect();
    Ok(Some(Mined { pairs, weights, stats, oracle_secs: started.elapsed().as_secs_f64(), cached }))
}

fn pct(r: &f01_regex::report::Ratio) -> String {
    format!("{:.2}% ({}/{})", 100.0 * r.rate(), r.pass, r.total)
}

fn rates_json(r: &Rates) -> serde_json::Value {
    json!({
        "match": pct(&r.r#match),
        "span": pct(&r.span),
        "submatch": pct(&r.submatch),
        "syntax_errors": pct(&r.syntax_errors),
        "unsupported_probes": pct(&r.unsupported),
        "regex_all_probes_equal": pct(&r.regex),
        "raw": r,
    })
}

fn run() -> Result<()> {
    let started = Instant::now();
    let scratch = harness::paths::scratch_dir("f01-regex");
    let mut result = ExperimentResult::new("f01-regex", "F01: motor de regex com a semântica POSIX do GNU em Rust puro");

    // 1. Corpus commitado.
    let (committed, missing) = harness::paths::load_tool("regex")?;
    anyhow::ensure!(missing == 0, "{missing} casos de regex sem golden; rode `cargo run -p oracle -- gen --tool regex`");
    eprintln!("corpus commitado: {} sondas", committed.len());

    // 2. Corpus minerado.
    let mined = mined_corpus(&scratch)?;
    if let Some(m) = &mined {
        eprintln!("corpus minerado: {} sondas (oráculo {:.1}s{})", m.pairs.len(), m.oracle_secs, if m.cached { ", cache" } else { "" });
    }
    let empty = HashMap::new();
    let weights = mined.as_ref().map(|m| &m.weights).unwrap_or(&empty);
    let mut all_pairs: Vec<(Case, Outcome)> = committed.clone();
    if let Some(m) = &mined {
        all_pairs.extend(m.pairs.iter().cloned());
    }
    let infos: Vec<CaseInfo> = all_pairs.iter().map(|(c, g)| case_info(c, g, weights)).collect();

    // Sanidade do oráculo: o exit do GNU grep 3.11 bate com a expectativa das suítes upstream?
    let mut upstream_expect = (0usize, 0usize, Vec::new());
    for (case, golden) in &committed {
        if let Some(exp) = case.tags.iter().find_map(|t| t.strip_prefix("expect:"))
            && case.tags.iter().any(|t| t == "probe:grep-o")
            && !case.tags.iter().any(|t| t == "upstream-todo")
        {
            upstream_expect.1 += 1;
            if golden.exit.map(|e| e.to_string()).as_deref() == Some(exp) {
                upstream_expect.0 += 1;
            } else {
                upstream_expect.2.push(case.id.clone());
            }
        }
    }

    // 3. Motores.
    let engines = all_engines();
    let names: Vec<&str> = engines.iter().map(|e| e.name()).collect();
    let run_dir = scratch.join("run");
    std::fs::create_dir_all(&run_dir)?;
    let t_engines = Instant::now();
    // O regast acha a casada tentando todo par (início, fim) com casamento completo, O(n³) por
    // linha; no corpus minerado ele roda numa fração das regexes (registrado nas métricas).
    let (slow, fast): (Vec<&str>, Vec<&str>) = names.iter().partition(|n| SLOW_ENGINES.iter().any(|(s, _)| s == *n));
    let mut runs: BTreeMap<String, EngineRun> = run_all(&fast, &all_pairs, 4, 16, &run_dir)?;
    for name in slow {
        let stride = SLOW_ENGINES.iter().find(|(s, _)| *s == name).map(|(_, k)| *k).unwrap_or(1);
        let subset: Vec<(Case, Outcome)> = all_pairs
            .iter()
            .zip(&infos)
            .filter(|(_, info)| info.committed() || seed_of(&info.regex_id).is_multiple_of(stride))
            .map(|(p, _)| p.clone())
            .collect();
        runs.extend(run_all(&[name], &subset, 16, 16, &run_dir)?);
    }
    let engines_secs = t_engines.elapsed().as_secs_f64();
    eprintln!("motores: {engines_secs:.1}s");

    // 4. Métricas por motor.
    let committed_infos: Vec<&CaseInfo> = infos.iter().filter(|i| i.committed()).collect();
    let mined_infos: Vec<&CaseInfo> = infos.iter().filter(|i| !i.committed()).collect();
    let committed_vec: Vec<CaseInfo> = committed_infos.iter().map(|i| (*i).clone()).collect();
    let mined_vec: Vec<CaseInfo> = mined_infos.iter().map(|i| (*i).clone()).collect();
    let deps = depscan_all(&engines);
    let mut summary_rows = Vec::new();
    let mut details = BTreeMap::new();
    let mut per_engine_regex: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    for engine in &engines {
        let run = &runs[engine.name()];
        let get = |i: &CaseInfo| run.comparisons.get(&i.id);
        let committed_cmps: Vec<&harness::CaseComparison> =
            committed_infos.iter().filter_map(|i| run.comparisons.get(&i.id)).collect();
        let conformance = tally(&format!("{} {}", engine.name(), engine.version()), &committed_cmps);
        let rc = rates(committed_infos.iter().copied(), false, &get);
        let by_source: BTreeMap<String, Rates> = ["upstream-spencer1", "upstream-bre", "upstream-ere", "agent-style"]
            .iter()
            .map(|s| (s.to_string(), rates(committed_infos.iter().copied().filter(|i| i.source == *s), false, &get)))
            .collect();
        let rm = rates(mined_infos.iter().copied(), false, &get);
        let rmw = rates(mined_infos.iter().copied(), true, &get);
        let feats_committed = by_feature(&committed_vec, &get);
        let feats_mined = by_feature(&mined_vec, &get);
        per_engine_regex.insert(engine.name().to_string(), (rc.regex.rate(), rmw.regex.rate()));
        summary_rows.push(format!(
            "{:28} commitado regex {:7} | minerado regex {:7} ponderado {:7} | match {:7} span {:7} grupos {:7}",
            engine.name(),
            format!("{:.2}%", 100.0 * rc.regex.rate()),
            format!("{:.2}%", 100.0 * rm.regex.rate()),
            format!("{:.2}%", 100.0 * rmw.regex.rate()),
            format!("{:.2}%", 100.0 * rmw.r#match.rate()),
            format!("{:.2}%", 100.0 * rmw.span.rate()),
            format!("{:.2}%", 100.0 * rmw.submatch.rate()),
        ));
        let failing: Vec<&String> = committed_cmps.iter().filter(|c| !c.lenient).map(|c| &c.id).collect();
        // Detalhe do minerado só no scratch (gitignored): id, argv e a divergência.
        let mined_failing: Vec<serde_json::Value> = all_pairs
            .iter()
            .zip(&infos)
            .filter(|(_, i)| !i.committed())
            .filter_map(|((case, _), i)| run.comparisons.get(&i.id).filter(|c| !c.lenient).map(|c| (case, c)))
            .map(|(case, c)| json!({ "id": case.id, "argv": case.argv, "detail": c.detail }))
            .collect();
        details.insert(
            engine.name().to_string(),
            json!({ "committed_failures": failing, "mined_failures": mined_failing, "timeouts": run.timeouts, "crashes": run.crashes }),
        );
        let dep = deps.get(engine.crate_name()).cloned().unwrap_or(json!(null));
        let category = dep.get("own").and_then(|v| v.as_str()).map(str::to_string);
        let (fit, notes) = assess(engine.as_ref(), &rc, &rmw);
        result.candidates.push(CandidateResult {
            name: engine.name().to_string(),
            version: engine.version().to_string(),
            role: "regex".into(),
            category,
            conformance: Some(conformance),
            fit,
            notes,
            metrics: json!({
                "crate": engine.crate_name(),
                "semantics": engine.semantics(),
                "committed": rates_json(&rc),
                "committed_by_source": by_source.iter().map(|(k, v)| (k.clone(), rates_json(v))).collect::<BTreeMap<_, _>>(),
                "mined_unique": rates_json(&rm),
                "mined_weighted_by_frequency": rates_json(&rmw),
                "by_feature_committed": feats_committed.iter().map(|(k, v)| (k.clone(), pct(v))).collect::<BTreeMap<_, _>>(),
                "by_feature_mined": feats_mined.iter().map(|(k, v)| (k.clone(), pct(v))).collect::<BTreeMap<_, _>>(),
                "mined_probes_run": mined_infos.iter().filter(|i| run.comparisons.contains_key(&i.id)).count(),
                "timeouts": run.timeouts.len(),
                "crashes": run.crashes.len(),
                "busy_seconds": run.busy.as_secs_f64(),
                "depscan": dep,
            }),
        });
    }

    // 5. Combinações.
    let mut combo_rows = Vec::new();
    let mut best_combo: Option<(String, f64, f64)> = None;
    for combo in combos() {
        let get = |i: &CaseInfo| runs.get((combo.route)(i)).and_then(|r| r.comparisons.get(&i.id));
        let rc = rates(committed_infos.iter().copied(), false, &get);
        let rmw = rates(mined_infos.iter().copied(), true, &get);
        let rm = rates(mined_infos.iter().copied(), false, &get);
        let cmps: Vec<&harness::CaseComparison> = committed_infos.iter().filter_map(|i| get(i)).collect();
        combo_rows.push(format!(
            "{:60} commitado regex {:.2}% | minerado ponderado {:.2}%",
            combo.name,
            100.0 * rc.regex.rate(),
            100.0 * rmw.regex.rate()
        ));
        if best_combo.as_ref().is_none_or(|b| rmw.regex.rate() > b.2) {
            best_combo = Some((combo.name.to_string(), rc.regex.rate(), rmw.regex.rate()));
        }
        result.candidates.push(CandidateResult {
            name: combo.name.to_string(),
            version: "-".into(),
            role: "regex".into(),
            category: None,
            conformance: Some(tally(combo.name, &cmps)),
            fit: Fit::FitsWithWork,
            notes: format!("{}. Roteamento estático pelas características da regex, medido sobre as mesmas sondas.", combo.description),
            metrics: json!({
                "committed": rates_json(&rc),
                "mined_unique": rates_json(&rm),
                "mined_weighted_by_frequency": rates_json(&rmw),
                "by_feature_mined": by_feature(&mined_vec, &get).iter().map(|(k, v)| (k.clone(), pct(v))).collect::<BTreeMap<_, _>>(),
            }),
        });
    }

    // 6. Veredito.
    let (best_name, (best_c, best_m)) = per_engine_regex
        .iter()
        .max_by(|a, b| (a.1.1 + a.1.0).total_cmp(&(b.1.1 + b.1.0)))
        .map(|(k, v)| (k.clone(), *v))
        .unwrap_or_default();
    // Onde o melhor motor isolado ainda erra (classes das regexes que falham no corpus commitado).
    let mut fail_features: BTreeMap<String, usize> = BTreeMap::new();
    if let Some(run) = runs.get(&best_name) {
        let mut failing_regex: BTreeMap<&str, &CaseInfo> = BTreeMap::new();
        for i in &committed_infos {
            if run.comparisons.get(&i.id).is_some_and(|c| !c.lenient) {
                failing_regex.insert(i.regex_id.as_str(), i);
            }
        }
        for i in failing_regex.values() {
            for t in i.tags.iter().filter_map(|t| t.strip_prefix("feat:")) {
                *fail_features.entry(t.to_string()).or_default() += 1;
            }
            *fail_features.entry(format!("aspect-{}", i.kind.aspect())).or_default() += 1;
        }
    }
    let perfect = best_c >= 0.9999 && best_m >= 0.9999;
    let combo = best_combo.clone().unwrap_or_default();
    let verdict = if perfect {
        Verdict::Confirmed
    } else if best_m >= 0.99 || combo.2 >= 0.99 {
        Verdict::Partial
    } else {
        Verdict::Refuted
    };
    result.hypothesis(
        "H23",
        verdict,
        format!(
            "Nenhuma crate reproduz o GNU sozinha: as que leem o dialeto GNU direto (red-sed-regex, ferroni com as sintaxes \
             do Oniguruma) igualam só {:.2}% e {:.2}% das regexes do corpus de borda, e as melhores precisam do nosso parser \
             BRE/ERE (grep e sed) e de montagem pra leftmost-longest. Com o tradutor, o melhor motor isolado ({best_name}) \
             iguala o GNU em {:.2}% das regexes do corpus commitado (suítes do grep e casos de borda) e em {:.2}% do uso real \
             ponderado; a melhor combinação ({}) chega a {:.2}%/{:.2}%. O que sobra é a regra de subexpressão do glibc em \
             repetição de grupo ((a*)*, (a*)+, (^)*), que só um motor nosso reproduz.",
            100.0 * per_engine_regex.get("red-sed-regex").map(|v| v.0).unwrap_or_default(),
            100.0 * per_engine_regex.get("ferroni-native-syntax").map(|v| v.0).unwrap_or_default(),
            100.0 * best_c,
            100.0 * best_m,
            combo.0,
            100.0 * combo.1,
            100.0 * combo.2
        ),
        json!({
            "best_single": { "engine": best_name, "committed_regex": best_c, "mined_weighted_regex": best_m },
            "best_combo": { "name": combo.0, "committed_regex": combo.1, "mined_weighted_regex": combo.2 },
            "best_single_failing_regex_features": fail_features,
            "per_engine_regex_rate": per_engine_regex,
        }),
    );

    // 7. Métricas gerais e notas.
    result.metrics = json!({
        "committed_probes": committed.len(),
        "committed_regexes": committed_vec.iter().map(|i| &i.regex_id).collect::<std::collections::BTreeSet<_>>().len(),
        "mined_probes": mined_vec.len(),
        "mined_regexes": mined_vec.iter().map(|i| &i.regex_id).collect::<std::collections::BTreeSet<_>>().len(),
        "mined_stats": mined.as_ref().map(|m| serde_json::to_value(&m.stats).unwrap_or_default()),
        "oracle_upstream_expectation_agreement": format!("{}/{}", upstream_expect.0, upstream_expect.1),
        "oracle_upstream_expectation_mismatches": upstream_expect.2,
        "oracle_mined_seconds": mined.as_ref().map(|m| m.oracle_secs),
        "engines_wall_seconds": engines_secs,
        "total_seconds": started.elapsed().as_secs_f64(),
        "summary": summary_rows,
        "combos": combo_rows,
    });
    result.notes = notes();
    let path = result.write()?;
    std::fs::write(scratch.join("details.json"), serde_json::to_string_pretty(&details)?)?;
    for row in result.metrics["summary"].as_array().into_iter().flatten().chain(result.metrics["combos"].as_array().into_iter().flatten()) {
        println!("{}", row.as_str().unwrap_or_default());
    }
    println!("H23: {:?}; {}", verdict, path.display());
    Ok(())
}

/// Encaixe de cada motor, com o motivo medido.
fn assess(engine: &dyn Engine, rc: &Rates, rmw: &Rates) -> (Fit, String) {
    let base = format!(
        "{}. Regexes iguais ao GNU: {:.2}% no corpus commitado, {:.2}% no uso real ponderado; sondas sem suporte {:.2}%.",
        engine.semantics(),
        100.0 * rc.regex.rate(),
        100.0 * rmw.regex.rate(),
        100.0 * rmw.unsupported.rate()
    );
    let (fit, why) = match engine.name() {
        "regex" => (Fit::DoesNotFit, "Leftmost-first: erra spans de alternação e repetição em que o GNU pega a casada mais longa; sem backref."),
        "regex-automata-longest" => (
            Fit::FitsWithWork,
            "Peça: DFA de tempo linear com o fim mais longo (MatchKind::All) acerta spans e casa/não casa sem backref; grupos e backref ficam pra outro motor ou pra código nosso.",
        ),
        "fancy-regex" => (Fit::DoesNotFit, "Leftmost-first com backtracking: mesmos erros de span do regex; backref funciona mas os grupos seguem a regra Perl."),
        "revera" => (
            Fit::DoesNotFit,
            "ERE POSIX correta, mas a regra de subexpressão POSIX não é a do glibc (diverge em (a|ab)(c|bcd)), sem BRE, backref, \\b, \\< e com classes só ASCII no locale POSIX.",
        ),
        "posix-regex" => (Fit::DoesNotFit, "Só ASCII, iteração e escolha da casada mais à esquerda erradas, trava em padrões com repetição aninhada."),
        "regast" => (Fit::DoesNotFit, "Desambiguação POSIX correta mas diferente do glibc; sem backref, sem \\b, sem classes POSIX, sem flag de caixa; lento."),
        "rusty_expressions-longest" => (Fit::DoesNotFit, "Porte do Oniguruma com defeitos de busca (ex.: \\[[a-z]+\\] não casa com [db]); o ferroni cobre o mesmo papel."),
        "ferroni-longest" => (
            Fit::FitsWithWork,
            "Peça: com o nosso tradutor e FIND_LONGEST ancorado no início leftmost, acerta spans e cobre backref; grupos seguem o backtracking, não o glibc.",
        ),
        "ferroni-native-syntax" => (Fit::DoesNotFit, "As sintaxes GREP/POSIX_EXTENDED do Oniguruma não são o dialeto do GNU (\\w, \\b, intervalos, contexto de * e ^); precisa do nosso parser."),
        "resharp" => (Fit::DoesNotFit, "Leftmost-longest e lookaround, mas sem backref, com grupos experimentais que recusam repetição de grupo e erros de span em `.` sobre UTF-8."),
        "red-sed-regex" => (
            Fit::DoesNotFit,
            "Parser próprio do dialeto GNU, mas diverge em \\s, \\S, \\W, {,n}, \\`, \\', operadores em posição inicial e mensagens de erro; depende do locale do processo via setlocale/mbrtowc (libc) e puxa signal-hook.",
        ),
        _ => (Fit::DoesNotFit, ""),
    };
    (fit, format!("{base} {why}"))
}

fn depscan_all(engines: &[Box<dyn Engine>]) -> BTreeMap<String, serde_json::Value> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let mut out = BTreeMap::new();
    for e in engines {
        let name = e.crate_name();
        if out.contains_key(name) {
            continue;
        }
        let value = match depscan::scan(&manifest, name) {
            Ok(scan) => json!({
                "own": effective_category(&manifest, name, scan.root.category.letter(), scan.root.links.is_some(), scan.root.counts.host_touch() > 0),
                "tree": effective_category(
                    &manifest,
                    name,
                    scan.tree_category.letter(),
                    // `c_deps` inclui crates só de outras plataformas; vale o grafo resolvido pro host.
                    false,
                    scan.totals.host_touch() > 0 || scan.root.counts.host_touch() > 0,
                ),
                "depscan_own": scan.root.category.letter(),
                "depscan_tree": scan.tree_category.letter(),
                "own_unsafe": scan.root.counts.unsafe_total(),
                "own_host_touch": scan.root.counts.host_touch(),
                "tree_deps": scan.deps.len(),
                "tree_unsafe": scan.totals.unsafe_total(),
                "c_deps": scan.c_deps,
                "host_touching_deps": scan.host_touching_deps,
            }),
            Err(err) => json!({ "error": format!("{err:#}") }),
        };
        out.insert(name.to_string(), value);
    }
    out
}

/// O depscan marca (c) quando o manifesto declara build-dependency de C, mesmo opcional e desligada
/// (é o caso do `cc` atrás da feature `ffi` do ferroni). Aqui a categoria (c) só fica se o grafo
/// resolvido (`cargo tree -e build,normal`) tiver de fato `cc`/`cmake`/`bindgen`/`pkg-config`, ou se
/// houver `links`.
fn effective_category(manifest: &Path, package: &str, letter: &str, links: bool, touches_host: bool) -> String {
    if letter != "c" || links {
        return letter.to_string();
    }
    let without_c = if touches_host { "b" } else { "a" };
    let out = std::process::Command::new("cargo")
        .args(["tree", "-e", "build,normal", "--prefix", "none", "-p", package, "--manifest-path"])
        .arg(manifest)
        .output();
    let resolved_c = match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .any(|l| ["cc ", "cmake ", "bindgen ", "pkg-config "].iter().any(|p| l.starts_with(p))),
        Err(_) => true,
    };
    if resolved_c { "c".into() } else { without_c.into() }
}

fn notes() -> Vec<String> {
    vec![
        "Oráculo: grep 3.11, sed 4.9 e gawk 5.2.1 do Debian 13, LC_ALL=C.UTF-8. Sondas: grep -n (casa/não casa), grep -ob (spans leftmost-longest), sed s///g (spans na iteração do sed), sed s//\\1/ (conteúdo dos grupos, BRE e ERE) e gawk match(s, re, arr) (posição e participação dos grupos, só ERE sem backref).".into(),
        "Tradutor nosso (src/parse.rs e src/emit.rs): parser dos dialetos BRE/ERE do grep e do sed seguindo o regcomp do glibc, mais a visão do dfa.c do grep (operador de repetição em posição inicial no -E), e emissão pra sintaxe de cada motor. Todos os motores recebem o mesmo AST; nenhuma falha do corpus commitado é comum a todos os motores, então o tradutor não é o gargalo.".into(),
        "spencer2.tests não existe na tag v3.11: foi dividida em bre.tests e ere.tests em 2009 (ChangeLog-2009). Linhas com 4 campos (TO CORRECT) entram com a tag upstream-todo; as de 5 campos ficam de fora, como no próprio grep.".into(),
        "O GNU não segue a regra de subexpressão do POSIX: em (a|ab)(c|bcd)(d*) sobre abcd o glibc dá a/bcd/vazio, e os motores POSIX corretos (revera, regast) dão ab/c/d. Por isso nem o motor POSIX mais correto serve de referência pros grupos.".into(),
        "Corpus minerado: corpus/agent/patterns.jsonl (E08), só tool=grep (BRE/ERE do GNU) e tool=sed; rg fica de fora. Flags (-E/-F/-P/-i, sed -E) recuperadas parseando commands.jsonl, nunca executando. Amostra: os 2000 padrões de grep e 1000 de sed mais frequentes mais 1000 e 500 sorteados (semente 0xF01). Linhas de teste: o palheiro fixo das linhas do corpus escrito à mão mais 6 amostras geradas pela própria regex. Só agregados entram neste JSON; casos e golden minerados ficam em scratch/f01-regex.".into(),
        "Motores examinados e não rodados: eregex 0.1.5 (POSIX matching só planejado, leftmost-first como o fancy-regex), regex-lite (mesma semântica do regex), derivre (casa linguagem inteira, sem busca com spans), ere/ere-core (macro de compilação, sem padrão em tempo de execução); pcre2, onig, tre-regex, minrx, gnurx-sys e regex-rs são C ou FFI pra libc e ficam só como referência.".into(),
        "Defeito encontrado no rusty_expressions 0.2.2: \\[[a-z]+\\] não casa com [db] (teste em src/engines.rs).".into(),
    ]
}
