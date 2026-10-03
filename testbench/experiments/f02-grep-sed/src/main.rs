//! F02 (H24, H25): grep com as crates do ripgrep e sed em Rust, contra o GNU.
//!
//! `cargo run --release` roda os candidatos de grep e de sed contra `golden/{grep,sed}`, mede
//! acoplamento (depscan) e esforço de porte (pontos de I/O do host e de regex no código-fonte), e
//! grava `results/f02-grep-sed.json`. Modo interno: `--exec IMPL ARGS...` (candidatos em subprocesso).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Result;
use f02_grep_sed::bashkit_run::Bashkit;
use f02_grep_sed::effort;
use f02_grep_sed::exec::{Subprocess, ensure, scratch};
use f02_grep_sed::grep::matcher::MatcherKind;
use f02_grep_sed::grep::{GrepImpl, PrinterKind};
use harness::{Candidate, CandidateResult, CaseComparison, Conformance, ExperimentResult, Fit, Invocation, Outcome, Verdict};
use serde_json::{Value, json};

fn sort_lines(b: &[u8]) -> Vec<u8> {
    let mut lines: Vec<&[u8]> = b.split_inclusive(|&c| c == b'\n').collect();
    lines.sort();
    lines.concat()
}

/// Casos `order-insensitive` (saída de `-r`, cuja ordem segue o readdir do GNU) comparam as linhas
/// ordenadas dos dois lados.
struct Ordered<'a> {
    inner: &'a (dyn Candidate + Sync),
    sort: bool,
}

impl Candidate for Ordered<'_> {
    fn name(&self) -> String {
        self.inner.name()
    }
    fn run(&self, inv: &Invocation) -> Outcome {
        let mut o = self.inner.run(inv);
        if self.sort {
            o.stdout = sort_lines(o.stdout.as_slice()).into();
        }
        o
    }
}

fn score(cand: &(dyn Candidate + Sync), cases: &[(harness::Case, Outcome)]) -> (Conformance, Vec<CaseComparison>, f64) {
    let started = Instant::now();
    let mut all = Vec::new();
    for (case, golden) in cases {
        let sort = case.tags.iter().any(|t| t == "order-insensitive");
        let mut g = golden.clone();
        if sort {
            g.stdout = sort_lines(g.stdout.as_slice()).into();
        }
        let wrapped = Ordered { inner: cand, sort };
        let (_, mut cmps) = harness::score(&wrapped, &[(case.clone(), g)]);
        all.push(cmps.remove(0));
    }
    let refs: Vec<&CaseComparison> = all.iter().collect();
    (tally(&cand.name(), &refs), all, started.elapsed().as_secs_f64())
}

/// Placar no formato do `harness::score`, somado caso a caso.
fn tally(name: &str, cmps: &[&CaseComparison]) -> Conformance {
    let mut conf = Conformance { candidate: name.to_string(), ..Conformance::default() };
    for cmp in cmps {
        conf.total += 1;
        conf.strict_pass += cmp.strict as usize;
        conf.lenient_pass += cmp.lenient as usize;
        conf.unsupported += cmp.unsupported.is_some() as usize;
        let tags: Vec<String> = if cmp.tags.is_empty() { vec!["untagged".into()] } else { cmp.tags.clone() };
        for tag in tags {
            let slot = conf.by_tag.entry(tag).or_default();
            slot.0 += cmp.strict as usize;
            slot.1 += cmp.lenient as usize;
            slot.2 += 1;
        }
    }
    conf.sample_failures = cmps.iter().filter(|c| !c.strict).take(15).map(|c| (*c).clone()).collect();
    conf
}

fn pct(a: usize, b: usize) -> String {
    if b == 0 { "-".into() } else { format!("{:.1}% ({a}/{b})", 100.0 * a as f64 / b as f64) }
}

fn by_source(all: &[CaseComparison]) -> BTreeMap<String, Value> {
    let mut m: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    for c in all {
        let src = c.tags.iter().find_map(|t| t.strip_prefix("src:")).unwrap_or("agent-style").to_string();
        let e = m.entry(src).or_default();
        e.0 += c.strict as usize;
        e.1 += c.lenient as usize;
        e.2 += 1;
    }
    m.into_iter().map(|(k, (s, l, t))| (k, json!({ "strict": pct(s, t), "lenient": pct(l, t) }))).collect()
}

/// Taxa leniente por tag (as que não estão em 100%), pra mostrar onde cada candidato erra.
fn weak_tags(conf: &Conformance) -> BTreeMap<String, String> {
    conf.by_tag
        .iter()
        .filter(|(t, (_, l, n))| l < n && !t.starts_with("src:"))
        .map(|(t, (_, l, n))| (t.clone(), pct(*l, *n)))
        .collect()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--exec") {
        // Mesmo ambiente do oráculo: umask 022 (arquivos criados por `w` e `-i` saem 644).
        rustix::process::umask(rustix::fs::Mode::from_raw_mode(0o022));
        let code = f02_grep_sed::execmode::dispatch(&args[2], &args[3..]);
        std::process::exit(code);
    }
    run(args.get(1).cloned())
}

/// Um candidato e os casos que ele roda.
type Job<'a> = (&'a (dyn Candidate + Sync), &'a [(harness::Case, Outcome)]);

struct Meta {
    role: &'static str,
    version: &'static str,
    /// Crates (no grafo deste experimento) cujo acoplamento importa.
    crates: &'static [&'static str],
    /// Onde medir o esforço de porte: (crate, subdiretório).
    effort: Option<(&'static str, &'static str)>,
}

fn meta(name: &str) -> Meta {
    match name {
        "rg-regex+rg-printer" => Meta {
            role: "grep",
            version: "grep-searcher 0.1.17, grep-regex 0.1.14, grep-printer 0.3.1",
            crates: &["grep-searcher", "grep-matcher", "grep-regex", "grep-printer"],
            effort: None,
        },
        "f01+rg-printer" => Meta {
            role: "grep",
            version: "grep-searcher 0.1.17, grep-printer 0.3.1, motor F01",
            crates: &["grep-searcher", "grep-matcher", "grep-printer"],
            effort: None,
        },
        "f01+gnu-printer" => Meta {
            role: "grep",
            version: "grep-searcher 0.1.17, grep-matcher 0.1.9, motor F01",
            crates: &["grep-searcher", "grep-matcher"],
            effort: None,
        },
        "uu_grep" => Meta { role: "grep", version: "0.2.0", crates: &["uu_grep"], effort: Some(("uu_grep", "src")) },
        "bashkit-grep" => Meta { role: "grep", version: "0.18.2", crates: &["bashkit"], effort: Some(("bashkit", "src/builtins/grep.rs")) },
        "uutils-sed" => Meta { role: "sed", version: "0.2.0", crates: &["sed"], effort: Some(("sed", "src")) },
        "sed-rs" => Meta { role: "sed", version: "2.0.0", crates: &["sed-rs"], effort: Some(("sed-rs", "src")) },
        "red" => Meta { role: "sed", version: "1.0.2", crates: &["red-sed"], effort: Some(("red-sed", "src")) },
        _ => Meta { role: "sed", version: "0.18.2", crates: &["bashkit"], effort: Some(("bashkit", "src/builtins/sed")) },
    }
}

/// Categoria efetiva: o depscan marca (c) por build-dependency de C declarada mesmo quando opcional;
/// aqui (c) só fica se o grafo resolvido tiver `cc`/`cmake`/`bindgen`/`pkg-config` ou houver `links`.
fn effective_category(manifest: &Path, package: &str, letter: &str, links: bool, touches_host: bool) -> String {
    if letter != "c" || links {
        return letter.to_string();
    }
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
    if resolved_c {
        "c".into()
    } else if touches_host {
        "b".into()
    } else {
        "a".into()
    }
}

fn depscan_all(names: &[&str]) -> (BTreeMap<String, Value>, BTreeMap<String, PathBuf>) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let mut out = BTreeMap::new();
    let mut dirs = BTreeMap::new();
    for name in names {
        let v = match depscan::scan(&manifest, name) {
            Ok(scan) => {
                dirs.insert(name.to_string(), scan.root.manifest_dir.clone());
                json!({
                    "version": scan.root.version,
                    "own": effective_category(&manifest, name, scan.root.category.letter(), scan.root.links.is_some(), scan.root.counts.host_touch() > 0),
                    // Na árvore, `c_deps` inclui crates só de outras plataformas (haiku, wasm); vale o
                    // grafo resolvido pro host.
                    "tree": effective_category(&manifest, name, scan.tree_category.letter(), false, scan.totals.host_touch() + scan.root.counts.host_touch() > 0),
                    "depscan_own": scan.root.category.letter(),
                    "depscan_tree": scan.tree_category.letter(),
                    "own_host_touch": scan.root.counts.host_touch(),
                    "own_unsafe": scan.root.counts.unsafe_total(),
                    "tree_deps": scan.deps.len(),
                    "tree_unsafe": scan.totals.unsafe_total(),
                    "c_deps": scan.c_deps,
                    "host_touching_deps": scan.host_touching_deps,
                })
            }
            Err(e) => json!({ "error": format!("{e:#}") }),
        };
        out.insert(name.to_string(), v);
    }
    (out, dirs)
}

fn run(only: Option<String>) -> Result<()> {
    let started = Instant::now();
    let (grep_cases, missing_g) = harness::paths::load_tool("grep")?;
    let (sed_cases, missing_s) = harness::paths::load_tool("sed")?;
    anyhow::ensure!(missing_g + missing_s == 0, "casos sem golden; rode `cargo run -p oracle -- gen --tool grep` e `--tool sed`");
    let dir = scratch();
    ensure(&dir)?;
    let engine = || MatcherKind::Gnu(Box::new(f01_regex::engines::AutomataFerroni));
    let grep_cands: Vec<Box<dyn Candidate + Sync>> = vec![
        Box::new(GrepImpl { name: "rg-regex+rg-printer".into(), matcher: MatcherKind::RipgrepRegex, printer: PrinterKind::Ripgrep }),
        Box::new(GrepImpl { name: "f01+rg-printer".into(), matcher: engine(), printer: PrinterKind::Ripgrep }),
        Box::new(GrepImpl { name: "f01+gnu-printer".into(), matcher: engine(), printer: PrinterKind::Gnu }),
        Box::new(Subprocess { name: "uu_grep".into(), implementation: "uu-grep", argv0: "grep", scratch: dir.clone() }),
        Box::new(Bashkit { name: "bashkit-grep".into() }),
    ];
    let sed_cands: Vec<Box<dyn Candidate + Sync>> = vec![
        Box::new(Subprocess { name: "uutils-sed".into(), implementation: "uutils-sed", argv0: "sed", scratch: dir.clone() }),
        Box::new(Subprocess { name: "sed-rs".into(), implementation: "sed-rs", argv0: "sed", scratch: dir.clone() }),
        Box::new(Subprocess { name: "red".into(), implementation: "red", argv0: "sed", scratch: dir.clone() }),
        Box::new(Bashkit { name: "bashkit-sed".into() }),
    ];
    let jobs: Vec<Job<'_>> = grep_cands
        .iter()
        .map(|c| (c.as_ref(), grep_cases.as_slice()))
        .chain(sed_cands.iter().map(|c| (c.as_ref(), sed_cases.as_slice())))
        .filter(|(c, _)| only.as_deref().is_none_or(|o| o == c.name()))
        .collect();
    let results: Vec<(String, Conformance, Vec<CaseComparison>, f64)> = std::thread::scope(|s| {
        let hs: Vec<_> = jobs
            .iter()
            .map(|(c, cases)| {
                s.spawn(move || {
                    let (conf, all, secs) = score(*c, cases);
                    (c.name(), conf, all, secs)
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().expect("thread")).collect()
    });
    if only.is_some() {
        for (name, conf, all, _) in &results {
            println!("{name}: estrito {}/{} leniente {}", conf.strict_pass, conf.total, conf.lenient_pass);
            for c in all.iter().filter(|c| !c.lenient) {
                let d: Vec<String> = c.detail.iter().map(|x| x.chars().take(220).collect()).collect();
                println!("  {} {:?}", c.id, d);
            }
        }
        return Ok(());
    }

    let crates = ["grep-searcher", "grep-matcher", "grep-regex", "grep-printer", "uu_grep", "sed", "sed-rs", "red-sed", "bashkit"];
    let (deps, dirs) = depscan_all(&crates);
    let mut result = ExperimentResult::new("f02-grep-sed", "F02: grep com as crates do ripgrep e sed em Rust contra o GNU");
    let mut rows = Vec::new();
    let mut details = BTreeMap::new();
    let mut rates: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    for (name, conf, all, secs) in &results {
        let m = meta(name);
        let strict = conf.strict_pass as f64 / conf.total.max(1) as f64;
        let lenient = conf.lenient_pass as f64 / conf.total.max(1) as f64;
        rates.insert(name.clone(), (strict, lenient));
        rows.push(format!(
            "{:22} {:4} estrito {:6} leniente {:6} sem suporte {}",
            name,
            m.role,
            pct(conf.strict_pass, conf.total),
            pct(conf.lenient_pass, conf.total),
            conf.unsupported
        ));
        let effort = m.effort.and_then(|(krate, sub)| {
            let base = dirs.get(krate)?;
            let e = effort::scan(&base.join(sub), &["tests", "benches", "bin/"]);
            Some(serde_json::to_value(e).unwrap_or_default())
        });
        let depscan: BTreeMap<&str, Value> = m.crates.iter().map(|c| (*c, deps.get(*c).cloned().unwrap_or(Value::Null))).collect();
        let category = m.crates.first().and_then(|c| deps.get(*c)).and_then(|v| v.get("tree")).and_then(Value::as_str).map(str::to_string);
        let (fit, notes) = assess(name, conf);
        let failing: Vec<Value> = all
            .iter()
            .filter(|c| !c.lenient)
            .map(|c| json!({ "id": c.id, "detail": c.detail.iter().map(|d| d.chars().take(300).collect::<String>()).collect::<Vec<_>>() }))
            .collect();
        details.insert(name.clone(), failing);
        result.candidates.push(CandidateResult {
            name: name.clone(),
            version: m.version.to_string(),
            role: m.role.to_string(),
            category,
            conformance: Some(conf.clone()),
            fit,
            notes,
            metrics: json!({
                "by_source": by_source(all),
                "weak_tags": weak_tags(conf),
                "seconds": secs,
                "depscan": depscan,
                "porting_effort": effort,
            }),
        });
    }

    // Veredito H24.
    let g = |n: &str| rates.get(n).copied().unwrap_or_default();
    let (ours_s, ours_l) = g("f01+gnu-printer");
    let (rgp_s, rgp_l) = g("f01+rg-printer");
    let (rg_s, rg_l) = g("rg-regex+rg-printer");
    let (uu_s, uu_l) = g("uu_grep");
    let h24 = if rg_l >= 0.99 {
        Verdict::Confirmed
    } else if ours_l >= 0.99 {
        Verdict::Partial
    } else {
        Verdict::Refuted
    };
    result.hypothesis(
        "H24",
        h24,
        format!(
            "grep-searcher e grep-matcher servem como camada de varredura: com o front-end de flags nosso, o motor do F01 \
             e um printer nosso no formato do GNU, o grep montado iguala o GNU em {:.1}% estrito ({:.1}% leniente) de {} casos \
             (agente, foad1 e yesno do GNU grep). Com o grep-printer do ripgrep cai pra {:.1}% e com grep-regex + grep-printer \
             (o ripgrep como é) pra {:.1}%: o printer (contexto com -o, -m com contexto, separadores entre arquivos, -T, \
             casadas vazias do -o) e o motor (leftmost-first, sem backref) não são do GNU. Referência: uu_grep {:.1}%.",
            100.0 * ours_s,
            100.0 * ours_l,
            grep_cases.len(),
            100.0 * rgp_l,
            100.0 * rg_l,
            100.0 * uu_l
        ),
        json!({
            "cases": grep_cases.len(),
            "rates_strict_lenient": rates.iter().filter(|(k, _)| meta(k).role == "grep").collect::<BTreeMap<_, _>>(),
            "ripgrep_as_is_strict": rg_s,
            "ripgrep_printer_strict": rgp_s,
            "uu_grep_strict": uu_s,
        }),
    );

    // Veredito H25.
    let sed_names = ["uutils-sed", "sed-rs", "red", "bashkit-sed"];
    let (best_sed, (best_s, best_l)) = sed_names
        .iter()
        .map(|n| (n.to_string(), g(n)))
        .max_by(|a, b| a.1.1.total_cmp(&b.1.1))
        .unwrap_or_default();
    let h25 = if best_l >= 0.99 {
        Verdict::Confirmed
    } else if best_l >= 0.85 {
        Verdict::Partial
    } else {
        Verdict::Refuted
    };
    let sed_rates: BTreeMap<&str, String> =
        sed_names.iter().map(|n| (*n, format!("{:.1}%/{:.1}%", 100.0 * g(n).0, 100.0 * g(n).1))).collect();
    result.hypothesis(
        "H25",
        h25,
        format!(
            "Nenhum sed em Rust é adotável como está: o melhor ({best_sed}) iguala o GNU sed 4.9 em {:.1}% estrito ({:.1}% \
             leniente) de {} casos (agente e misc.pl do GNU sed); estrito/leniente por candidato: {}. O mais perto é o sed do \
             bashkit, que já roda sobre VFS em memória; vira base de fork trocando o motor de regex pelo do F01 e corrigindo \
             escapes, -z e mensagens. uutils/sed e red usam std::fs e stdout direto; sed-rs fica abaixo de 75%.",
            100.0 * best_s,
            100.0 * best_l,
            sed_cases.len(),
            sed_rates.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")
        ),
        json!({ "cases": sed_cases.len(), "best": best_sed, "rates_strict_lenient": sed_rates }),
    );

    result.metrics = json!({
        "grep_cases": grep_cases.len(),
        "sed_cases": sed_cases.len(),
        "summary": rows,
        "total_seconds": started.elapsed().as_secs_f64(),
    });
    result.notes = notes();
    let path = result.write()?;
    std::fs::write(dir.join("details.json"), serde_json::to_string_pretty(&details)?)?;
    for r in result.metrics["summary"].as_array().into_iter().flatten() {
        println!("{}", r.as_str().unwrap_or_default());
    }
    println!("H24: {h24:?}; H25: {h25:?}; {}", path.display());
    Ok(())
}

fn assess(name: &str, conf: &Conformance) -> (Fit, String) {
    let rate = format!(
        "{}/{} estrito, {}/{} leniente.",
        conf.strict_pass, conf.total, conf.lenient_pass, conf.total
    );
    let (fit, why) = match name {
        "rg-regex+rg-printer" => (
            Fit::DoesNotFit,
            "O ripgrep como biblioteca (grep-regex + grep-printer) com o nosso front-end: motor leftmost-first sem backref e printer com formato próprio.",
        ),
        "f01+rg-printer" => (
            Fit::DoesNotFit,
            "grep-printer não reproduz o GNU: -o com contexto imprime as linhas de contexto, -o com -v imprime linhas, sem separador -- entre arquivos, -T ignorado, casadas vazias do -o viram linha vazia.",
        ),
        "f01+gnu-printer" => (
            Fit::FitsWithWork,
            "grep-searcher (varredura, contexto, -v) e grep-matcher (trait do casador) servem; o resto é nosso: front-end de flags do getopt, casador sobre o motor do F01 com o -w do GNU, printer no formato do GNU (-m com contexto, -o com contexto e -v, separadores), binário, -r sobre o VFS e exit codes.",
        ),
        "uu_grep" => (
            Fit::Reference,
            "Referência: uutils/grep usa onig (C) e std::fs; erra o separador -- com -o e entre arquivos, --group-separator/--no-group-separator e -v -o com contexto (yesno).",
        ),
        "bashkit-grep" => (
            Fit::Reference,
            "Linha de base: grep do bashkit sobre VFS; caminhos absolutos no -r, erros no stdout, sem backref, -NUM e --label.",
        ),
        "uutils-sed" => (
            Fit::DoesNotFit,
            "std::fs/stdout via uucore; -i consome o argumento seguinte como sufixo, sem \\U\\L\\u, sem F, -z e \\b; erros e exit codes diferentes.",
        ),
        "sed-rs" => (
            Fit::DoesNotFit,
            "Motor regex do Rust (sem backref), sem endereço I, addr,+N, 0,/re/, -, sem preservar a falta de \\n final e o \\r; exit codes errados.",
        ),
        "red" => (
            Fit::DoesNotFit,
            "Motor de regex próprio (o red-sed-regex do F01), mas std::fs e stdout direto; erra I em endereço, i/a com texto começando por //, bloco { } dividido em vários -e, \\s e \\S, -z, \\x00 em y, s///N e para de processar quando falta um arquivo.",
        ),
        _ => (
            Fit::FitsWithWork,
            "Sed do bashkit sobre VFS em memória: o mais perto do GNU; usa regex/fancy-regex (leftmost-first) e erra escapes \\t em colchete e \\xHH, -z e scripts longos. Base de fork, extraindo o módulo e trocando o motor pelo do F01.",
        ),
    };
    (fit, format!("{rate} {why}"))
}

fn notes() -> Vec<String> {
    vec![
        "Oráculo: GNU grep 3.11 e GNU sed 4.9 do Debian 13 (LC_ALL=C.UTF-8, umask 022). Casos: corpus/cases/grep (agent-style à mão, foad1 e yesno importados do GNU grep) e corpus/cases/sed (agent-style à mão, misc.pl e testes de script do GNU sed 4.9).".into(),
        "Saída de -r: a ordem do GNU segue o readdir do sistema de arquivos do container (não ordenada); casos com a tag order-insensitive comparam as linhas ordenadas dos dois lados.".into(),
        "grep montado: o front-end de flags (getopt_long com permutação, -NUM, prefixo de opção longa, mensagens do getopt), a leitura da árvore do caso em memória (FsView), a detecção de binário (NUL ou UTF-8 inválido), -r/-R com --include/--exclude/--exclude-dir e os exit codes são nossos; grep-searcher varre, grep-matcher é a interface do casador, o motor é o regex-automata+ferroni do F01 com o nosso parser GNU.".into(),
        "Candidatos que usam std::fs (uu_grep, uutils/sed, sed-rs, red) rodam em subprocesso (o próprio binário com --exec, argv[0] igual ao da ferramenta) com cwd numa cópia da fixture em scratch/f02-grep-sed e o retrato do diretório comparado depois; o acoplamento ao host deles está no depscan e no porting_effort. O CLI do red é do binário dele (não da lib): o parser lexopt dele foi portado pra receber o argv. O bashkit roda o comando no bash virtual dele com a fixture num InMemoryFs.".into(),
        "porting_effort conta, no código-fonte do candidato, linhas com I/O do host (std::fs, File, stdout/stdin, std::process, std::env, libc, rustix, memmap2, tempfile) e linhas acopladas ao motor de regex; é o tamanho do trabalho de pôr a ferramenta sobre bytes em memória e trocar o motor pelo do F01.".into(),
        "Erros de execução do bashkit (padrão inválido, sem padrão) voltam como erro da API, não como exit 2; contam como falha sem suporte.".into(),
    ]
}
