//! Corpus aleatório de aplicações de patch contra o GNU patch 2.8 do oráculo (precisa do docker:
//! `cargo test -p ul-diff --test random_patch -- --ignored --nocapture`).
//!
//! - `f08_corpus`: o mesmo gerador do F08 (semente 31, 500 aplicações; derivas Clean, Offset, Edited,
//!   Both, Applied), com o GNU fazendo o `diff -u` e aplicando num alvo derivado. Compara estrito
//!   (stdout, stderr, exit e a árvore final).
//! - `extended_corpus`: o mesmo esquema com mais variações: formato de contexto e normal, `-U0`/`-U1`,
//!   CRLF, arquivo sem newline final, `-R`, `-N`, `-t`, `-f`, `-F1`, `-l`, `--dry-run`, `-o`, `-b`,
//!   `--reject-format`, vários arquivos com `-p1` em subdiretórios e `-E`.
//!
//! `PATCH_DEBUG=1` mostra os casos que divergem.

use std::collections::BTreeMap;

use harness::case::{Case, FileSpec};
use harness::{Entry, Invocation, MemTree, Oracle, Outcome};
use pl_testing::TestkitCandidate;

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

const CODE_LINES: &[&str] = &[
    "}", "", "{", "    return 0;", "    }", "    if (x) {", "        x++;", "    } else {", "int f(void)", "    break;",
    "    return x;", "#include <stdio.h>", "        return 1;", "    for (;;) {", "// comment", "end",
];

const TEXT_LINES: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu", "nu",
    "xi", "omicron", "pi", "rho", "sigma", "tau", "upsilon", "phi", "chi", "psi", "omega",
];

fn render(lines: &[String], newline_at_end: bool, crlf: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        out.extend_from_slice(l.as_bytes());
        if i + 1 < lines.len() || newline_at_end {
            if crlf {
                out.push(b'\r');
            }
            out.push(b'\n');
        }
    }
    out
}

fn mutate(rng: &mut Lcg, a: &[String], pick: &mut dyn FnMut(&mut Lcg) -> String, max_edits: u64) -> Vec<String> {
    let mut b = a.to_vec();
    let edits = 1 + rng.below(max_edits);
    for _ in 0..edits {
        let len = b.len() as u64;
        match rng.below(4) {
            0 => {
                let at = rng.below(len + 1) as usize;
                let n = 1 + rng.below(3);
                for _ in 0..n {
                    let l = pick(rng);
                    b.insert(at, l);
                }
            }
            1 if len > 0 => {
                let at = rng.below(len) as usize;
                let n = (1 + rng.below(3)).min(len - at as u64) as usize;
                b.drain(at..at + n);
            }
            2 if len > 0 => {
                let at = rng.below(len) as usize;
                b[at] = pick(rng);
            }
            3 if len > 2 => {
                let at = rng.below(len - 1) as usize;
                let n = (1 + rng.below(3)).min(len - at as u64) as usize;
                let block: Vec<String> = b.drain(at..at + n).collect();
                let to = rng.below(b.len() as u64 + 1) as usize;
                for (k, l) in block.into_iter().enumerate() {
                    b.insert(to + k, l);
                }
            }
            _ => {}
        }
    }
    b
}

#[derive(Clone, Copy, Debug)]
enum Drift {
    Clean,
    Offset,
    Edited,
    Both,
    Applied,
}

fn text(d: &[u8]) -> FileSpec {
    FileSpec::Text(String::from_utf8(d.to_vec()).expect("corpus em ASCII"))
}

/// Um caso: o script do oráculo (que gera o patch com o GNU diff e aplica) e a nossa invocação
/// equivalente (o patch gerado pelo GNU, aplicado pelo nosso `patch`).
struct Trial {
    tag: String,
    case: Case,
    /// argv do nosso lado.
    argv: Vec<String>,
    /// Arquivos que ficam pro nosso lado (o alvo e o patch, que vem do oráculo).
    keep: Vec<String>,
    /// Arquivos que o oráculo cria só pra gerar o patch e apaga antes de aplicar.
    gnu: Option<Outcome>,
}

const OUT_FILES: [&str; 3] = [".out", ".err", ".status"];

fn script_for(diff_cmd: &str, patch_cmd: &str, cleanup: &str) -> String {
    format!("{diff_cmd} > fix.patch; {cleanup}; {patch_cmd} > .out 2> .err < /dev/null; echo -n $? > .status")
}

fn f08_trials(seed: u64, count: usize) -> Vec<Trial> {
    let mut rng = Lcg::new(seed);
    let mut trials = Vec::new();
    for i in 0..count {
        let code = i % 2 == 0;
        let len = 8 + rng.below(40);
        let mut pick = |r: &mut Lcg| -> String {
            if code {
                CODE_LINES[r.below(CODE_LINES.len() as u64) as usize].to_string()
            } else {
                format!("{} {}", TEXT_LINES[r.below(TEXT_LINES.len() as u64) as usize], r.below(30))
            }
        };
        let a: Vec<String> = (0..len).map(|_| pick(&mut rng)).collect();
        let b = mutate(&mut rng, &a, &mut pick, 3);
        let drift = match i % 5 {
            0 => Drift::Clean,
            1 => Drift::Offset,
            2 => Drift::Edited,
            3 => Drift::Both,
            _ => Drift::Applied,
        };
        let mut t = match drift {
            Drift::Applied => b.clone(),
            _ => a.clone(),
        };
        if matches!(drift, Drift::Offset | Drift::Both) {
            let at = rng.below(t.len() as u64 / 2 + 1) as usize;
            for _ in 0..1 + rng.below(4) {
                let l = pick(&mut rng);
                t.insert(at, l);
            }
        }
        if matches!(drift, Drift::Edited | Drift::Both) && !t.is_empty() {
            let at = rng.below(t.len() as u64) as usize;
            t[at] = format!("{} (local)", t[at]);
        }
        let mut files = BTreeMap::new();
        files.insert("a".to_string(), text(&render(&a, true, false)));
        files.insert("b".to_string(), text(&render(&b, true, false)));
        files.insert("t".to_string(), text(&render(&t, true, false)));
        let case = Case {
            id: format!("patch-fuzz-{i:04}"),
            argv: Vec::new(),
            script: Some(script_for("diff -u a b", "patch t fix.patch", "rm -f a b")),
            stdin: None,
            stdin_b64: None,
            files,
            env: Default::default(),
            tags: vec![format!("{drift:?}").to_lowercase()],
            faketime: None,
            timeout_ms: None,
        };
        trials.push(Trial {
            tag: format!("{drift:?}"),
            case,
            argv: vec!["patch".into(), "t".into(), "fix.patch".into()],
            keep: vec!["t".into()],
            gnu: None,
        });
    }
    trials
}

/// Variações do corpus estendido.
fn extended_trials(seed: u64, count: usize) -> Vec<Trial> {
    let mut rng = Lcg::new(seed);
    let mut trials = Vec::new();
    for i in 0..count {
        let code = rng.below(2) == 0;
        let len = 4 + rng.below(30);
        let mut pick = |r: &mut Lcg| -> String {
            if code {
                CODE_LINES[r.below(CODE_LINES.len() as u64) as usize].to_string()
            } else {
                format!("{} {}", TEXT_LINES[r.below(TEXT_LINES.len() as u64) as usize], r.below(20))
            }
        };
        let a: Vec<String> = (0..len).map(|_| pick(&mut rng)).collect();
        let b = mutate(&mut rng, &a, &mut pick, 4);
        let drift = match rng.below(5) {
            0 => Drift::Clean,
            1 => Drift::Offset,
            2 => Drift::Edited,
            3 => Drift::Both,
            _ => Drift::Applied,
        };
        let format = match rng.below(6) {
            0 => "-c",
            1 => "",
            2 => "-U0",
            3 => "-U1",
            _ => "-u",
        };
        let crlf = rng.below(8) == 0;
        let nl_a = rng.below(6) != 0;
        let nl_b = rng.below(6) != 0;
        let opt = match rng.below(14) {
            0 => "-R",
            1 => "-N",
            2 => "-t",
            3 => "-f",
            4 => "-F1",
            5 => "-l",
            6 => "--dry-run",
            7 => "-o out",
            8 => "-b",
            9 => "--reject-format=context",
            10 => "--reject-format=unified",
            11 => "-F3",
            _ => "",
        };
        let reverse = opt == "-R";
        let base = if reverse { &b } else { &a };
        let other = if reverse { &a } else { &b };
        let mut t = match drift {
            Drift::Applied => other.clone(),
            _ => base.clone(),
        };
        if matches!(drift, Drift::Offset | Drift::Both) {
            let at = rng.below(t.len() as u64 / 2 + 1) as usize;
            for _ in 0..1 + rng.below(4) {
                let l = pick(&mut rng);
                t.insert(at, l);
            }
        }
        if matches!(drift, Drift::Edited | Drift::Both) && !t.is_empty() {
            let at = rng.below(t.len() as u64) as usize;
            t[at] = format!("{} (local)", t[at]);
        }
        let nl_t = if matches!(drift, Drift::Applied) { if reverse { nl_a } else { nl_b } } else if reverse { nl_b } else { nl_a };
        let mut files = BTreeMap::new();
        let ra = render(&a, nl_a || a.is_empty(), crlf);
        let rb = render(&b, nl_b || b.is_empty(), crlf);
        let rt = render(&t, nl_t || t.is_empty(), crlf);
        // Normal não tem nomes: o alvo vai na linha de comando; os outros usam o nome do cabeçalho.
        let multi = format != "" && rng.below(4) == 0;
        let (diff_cmd, patch_args, keep): (String, Vec<String>, Vec<String>);
        if multi {
            let c: Vec<String> = (0..3 + rng.below(6)).map(|_| pick(&mut rng)).collect();
            let d = mutate(&mut rng, &c, &mut pick, 2);
            let remove = rng.below(3) == 0;
            files.insert("old/sub/t".to_string(), text(&ra));
            files.insert("new/sub/t".to_string(), text(&rb));
            files.insert("old/u".to_string(), text(&render(&c, true, false)));
            if !remove {
                files.insert("new/u".to_string(), text(&render(&d, true, false)));
            }
            files.insert("new/added".to_string(), text(&render(&d, true, false)));
            files.insert("work/sub/t".to_string(), text(&rt));
            files.insert("work/u".to_string(), text(&render(&c, true, false)));
            diff_cmd = format!("diff -ruN {format} old new");
            let mut args: Vec<String> = vec!["patch".into(), "-p1".into(), "-d".into(), "work".into(), "-i".into(), "../fix.patch".into()];
            if opt.contains(' ') {
                args.extend(opt.split(' ').map(str::to_string));
            } else if !opt.is_empty() {
                args.push(opt.into());
            }
            if rng.below(2) == 0 {
                args.push("-E".into());
            }
            patch_args = args;
            keep = vec!["work".into()];
        } else {
            files.insert("a".to_string(), text(&ra));
            files.insert("b".to_string(), text(&rb));
            files.insert("t".to_string(), text(&rt));
            diff_cmd = if format.is_empty() { "diff a b".to_string() } else { format!("diff {format} a b") };
            let mut args: Vec<String> = vec!["patch".into()];
            if opt.contains(' ') {
                args.extend(opt.split(' ').map(str::to_string));
            } else if !opt.is_empty() {
                args.push(opt.into());
            }
            args.push("t".into());
            args.push("fix.patch".into());
            patch_args = args;
            keep = vec!["t".into()];
        }
        let cleanup = if multi { "rm -rf old new" } else { "rm -f a b" };
        let patch_cmd = patch_args.join(" ");
        let case = Case {
            id: format!("patch-ext-{i:04}"),
            argv: Vec::new(),
            script: Some(script_for(&diff_cmd, &patch_cmd, cleanup)),
            stdin: None,
            stdin_b64: None,
            files,
            env: Default::default(),
            tags: vec![format!("{drift:?}").to_lowercase()],
            faketime: None,
            timeout_ms: None,
        };
        let tag = format!(
            "{}{}{}{}{}",
            if format.is_empty() { "normal" } else { format },
            if crlf { "+crlf" } else { "" },
            if !(nl_a && nl_b) { "+nonl" } else { "" },
            if multi { "+multi" } else { "" },
            if opt.is_empty() { String::new() } else { format!("+{opt}") }
        );
        trials.push(Trial { tag, case, argv: patch_args, keep, gnu: None });
    }
    trials
}

/// Roda os scripts no oráculo e guarda o resultado do GNU (saída do patch e árvore sem os arquivos
/// de captura).
fn run_oracle(trials: &mut [Trial]) {
    let oracle = Oracle::locate().expect("oráculo: rode `cargo run -p oracle -- build` em testbench/");
    let cases: Vec<Case> = trials.iter().map(|t| t.case.clone()).collect();
    let outs = oracle.run(&cases).expect("oráculo");
    for (t, out) in trials.iter_mut().zip(outs) {
        let mut files = out.files.clone();
        let mut take = |name: &str| -> Vec<u8> {
            files.entries.remove(name).and_then(|e| e.data().map(|d| d.to_vec())).unwrap_or_default()
        };
        let stdout = take(OUT_FILES[0]);
        let stderr = take(OUT_FILES[1]);
        let status: i32 = String::from_utf8_lossy(&take(OUT_FILES[2])).trim().parse().unwrap_or(-1);
        t.gnu = Some(Outcome::exited(stdout, stderr, status, files));
    }
}

/// Nossa invocação: a árvore que sobrou no oráculo antes do patch (alvo e patch gerado pelo GNU).
fn our_invocation(t: &Trial) -> Invocation {
    let gnu = t.gnu.as_ref().expect("resultado do oráculo");
    let mut files = MemTree::new();
    // O patch gerado pelo GNU diff está na árvore final do oráculo (o patch não o altera).
    if let Some(e) = gnu.files.get("fix.patch") {
        files.insert("fix.patch", e.clone());
    }
    for (path, spec) in &t.case.files {
        let keep = t.keep.iter().any(|k| path == k || path.starts_with(&format!("{k}/")));
        if keep {
            files.insert(path, spec.to_entry().expect("fixture"));
        }
    }
    Invocation {
        case_id: t.case.id.clone(),
        argv: t.argv.clone(),
        script: None,
        stdin: Vec::new(),
        files,
        env: Default::default(),
        faketime: None,
    }
}

fn score(name: &str, trials: &[Trial]) -> (usize, usize) {
    let cand = TestkitCandidate::new("ul-diff (testkit)", ul_diff::programs());
    let debug = std::env::var("PATCH_DEBUG").is_ok();
    let mut by_tag: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    let (mut strict, mut lenient) = (0, 0);
    for t in trials {
        let inv = our_invocation(t);
        let gnu = t.gnu.as_ref().expect("oráculo");
        let actual = harness::Candidate::run(&cand, &inv);
        let mut probe = t.case.clone();
        probe.files.clear();
        let cmp = harness::compare_outcome(&probe, gnu, &actual);
        strict += cmp.strict as usize;
        lenient += cmp.lenient as usize;
        let slot = by_tag.entry(t.tag.clone()).or_default();
        slot.0 += cmp.strict as usize;
        slot.1 += cmp.lenient as usize;
        slot.2 += 1;
        if !cmp.strict && debug {
            eprintln!("--- {} [{}] argv {:?}\n{}", t.case.id, t.tag, t.argv, cmp.detail.join("\n"));
            eprintln!("patch:\n{}", String::from_utf8_lossy(inv.files.read("fix.patch").unwrap_or_default()));
            for (p, e) in &inv.files.entries {
                if let Entry::File { data: Some(d), .. } = e
                    && p != "fix.patch"
                {
                    eprintln!("[{p}]\n{}", String::from_utf8_lossy(d.as_slice()));
                }
            }
            eprintln!("GNU stdout:\n{}\nnosso stdout:\n{}", String::from_utf8_lossy(gnu.stdout.as_slice()), String::from_utf8_lossy(actual.stdout.as_slice()));
            eprintln!("GNU stderr:\n{}\nnosso stderr:\n{}", String::from_utf8_lossy(gnu.stderr.as_slice()), String::from_utf8_lossy(actual.stderr.as_slice()));
            for d in gnu.files.diff(&actual.files) {
                let path = d.trim_start_matches(['~', '+', '-']).split(':').next().unwrap_or("").to_string();
                eprintln!(
                    "[GNU {path}]\n{:?}\n[nosso {path}]\n{:?}",
                    String::from_utf8_lossy(gnu.files.read(&path).unwrap_or_default()),
                    String::from_utf8_lossy(actual.files.read(&path).unwrap_or_default())
                );
            }
        }
    }
    eprintln!("{name}: {strict}/{} estrito, {lenient}/{} leniente", trials.len(), trials.len());
    for (tag, (s, l, n)) in &by_tag {
        eprintln!("  {tag}: {s}/{n} estrito, {l}/{n} leniente");
    }
    (strict, lenient)
}

#[test]
#[ignore = "precisa do oráculo (docker)"]
fn f08_corpus() {
    let mut trials = f08_trials(31, 500);
    run_oracle(&mut trials);
    let (strict, _) = score("patch F08 (semente 31)", &trials);
    assert_eq!(strict, trials.len());
}

#[test]
#[ignore = "precisa do oráculo (docker)"]
fn extended_corpus() {
    let mut trials = extended_trials(4242, 600);
    run_oracle(&mut trials);
    let (strict, _) = score("patch estendido (semente 4242)", &trials);
    assert!(strict * 100 >= trials.len() * 95, "estrito abaixo de 95%");
}
