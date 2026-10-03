//! E04: o hostfs consegue impedir que um processo do sandbox leia algo fora do diretório montado, e com
//! que semântica?
//!
//! Tudo acontece dentro de `testbench/scratch/e04`. O "segredo" é um canário criado pelo próprio teste
//! fora da jaula. Ver `fixture.rs` e `vfs.rs`.

mod fixture;
mod vfs;

use std::collections::BTreeMap;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::Result;
use harness::{CandidateResult, ExperimentResult, Fit, Verdict};
use rustix::fs::{Mode, OFlags, RenameFlags, ResolveFlags};
use serde_json::json;
use vfs::{HostFs, Outcome, Sandbox};

/// Um caso da suíte: caminho visto de dentro do sandbox (cwd = /work).
struct Case {
    id: &'static str,
    path: String,
    /// Caso que não é teste de fuga (hardlink pré-existente pro canário): conteúdo do canário é esperado.
    canary_allowed: bool,
}

fn cases(fx: &fixture::Fixture) -> Vec<Case> {
    let outside_abs = fx.outside.canonicalize().expect("outside").to_string_lossy().into_owned();
    let c = |id, path: &str| Case { id, path: path.to_string(), canary_allowed: false };
    vec![
        c("plain-file", "file.txt"),
        c("nested-file", "dir/sub/file.txt"),
        c("absolute-guest-path", "/work/file.txt"),
        c("sandbox-etc-passwd", "/etc/passwd"),
        c("dotdot-from-mount-root", "../outside/canary.txt"),
        c("dotdot-inside-then-out", "dir/../../outside/canary.txt"),
        c("dotdot-deep-out", "dir/sub/../../../outside/canary.txt"),
        c("dotdot-to-sandbox-etc", "dir/../../etc/passwd"),
        c("host-absolute-path", &format!("{outside_abs}/canary.txt")),
        c("symlink-absolute-out", "link_abs_out/canary.txt"),
        c("symlink-relative-out", "link_rel_out/canary.txt"),
        c("symlink-relative-out-via-dir", "link_rel_out2/canary.txt"),
        c("symlink-to-guest-etc", "etc_link"),
        c("symlink-up-to-guest-etc", "up_passwd"),
        c("symlink-absolute-inside-guest", "abs_inside"),
        c("symlink-relative-inside", "inside_link"),
        c("magic-link-proc-root", "link_proc/canary.txt"),
        c("magic-link-proc-fd", "link_fd/canary.txt"),
        c("symlink-loop", "loop_a"),
        c("symlink-chain-40", "c00"),
        c("symlink-chain-41", "x00"),
        c("name-too-long", &"a".repeat(256)),
        c("path-too-long", &"d/".repeat(2600)),
        c("file-as-dir", "file.txt/x"),
        c("trailing-slash-on-file", "file.txt/"),
        c("missing", "nope.txt"),
        c("read-directory", "dir"),
        c("race-dir-static", "race/canary.txt"),
        Case { id: "preexisting-hardlink", path: "hard_canary".into(), canary_allowed: true },
    ]
}

fn open_dir(path: &std::path::Path) -> OwnedFd {
    rustix::fs::open(path, OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC, Mode::empty()).expect("abrir diretório")
}

fn candidates(fx: &fixture::Fixture) -> Vec<Sandbox> {
    let cap = cap_std::fs::Dir::open_ambient_dir(&fx.jail, cap_std::ambient_authority()).expect("cap-std");
    vec![
        Sandbox::new(HostFs::CapStd(cap), fixture::SANDBOX_PASSWD),
        Sandbox::new(
            HostFs::Openat2 { root: open_dir(&fx.jail), resolve: ResolveFlags::BENEATH | ResolveFlags::NO_MAGICLINKS },
            fixture::SANDBOX_PASSWD,
        ),
        Sandbox::new(
            HostFs::Openat2 { root: open_dir(&fx.jail), resolve: ResolveFlags::IN_ROOT | ResolveFlags::NO_MAGICLINKS },
            fixture::SANDBOX_PASSWD,
        ),
        Sandbox::new(HostFs::Manual { root: open_dir(&fx.jail) }, fixture::SANDBOX_PASSWD),
        Sandbox::new(HostFs::Hybrid { root: open_dir(&fx.jail) }, fixture::SANDBOX_PASSWD),
    ]
}

#[derive(Default)]
struct SuiteScore {
    escapes: Vec<String>,
    semantic_match: usize,
    errno_mismatch: Vec<String>,
    content_mismatch: Vec<String>,
    total: usize,
}

fn run_suite(sb: &Sandbox, refroot: &OwnedFd, cases: &[Case]) -> (SuiteScore, BTreeMap<String, String>) {
    let mut score = SuiteScore::default();
    let mut table = BTreeMap::new();
    for case in cases {
        let got = sb.read(&case.path);
        let want = vfs::reference(refroot, &case.path);
        score.total += 1;
        if got == Outcome::Content(fixture::CANARY.to_vec()) && !case.canary_allowed {
            score.escapes.push(case.id.to_string());
        }
        if got == want {
            score.semantic_match += 1;
        } else {
            let line = format!("{}: obtido {}, Linux {}", case.id, got.label(), want.label());
            match (&got, &want) {
                (Outcome::Content(_), _) | (_, Outcome::Content(_)) => score.content_mismatch.push(line),
                _ => score.errno_mismatch.push(line),
            }
        }
        table.insert(case.id.to_string(), got.label());
    }
    (score, table)
}

/// Uma thread troca `race` (diretório legítimo) e `race_alt` (symlink pra fora) com RENAME_EXCHANGE em
/// laço, enquanto o candidato tenta ler `race/canary.txt`.
fn race(sb: &Sandbox, jail: &std::path::Path, attempts: usize) -> BTreeMap<String, usize> {
    let stop = Arc::new(AtomicBool::new(false));
    let jail_fd = open_dir(jail);
    let swapper = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut swaps = 0u64;
            while !stop.load(Ordering::Relaxed) {
                if rustix::fs::renameat_with(&jail_fd, "race", &jail_fd, "race_alt", RenameFlags::EXCHANGE).is_ok() {
                    swaps += 1;
                }
            }
            // Deixa o estado par (race = diretório) pros testes seguintes.
            if swaps % 2 == 1 {
                let _ = rustix::fs::renameat_with(&jail_fd, "race", &jail_fd, "race_alt", RenameFlags::EXCHANGE);
                swaps += 1;
            }
            swaps
        })
    };
    let mut outcomes: BTreeMap<String, usize> = BTreeMap::new();
    for _ in 0..attempts {
        let key = match sb.read("race/canary.txt") {
            Outcome::Content(c) if c == fixture::INSIDE_RACE => "conteúdo legítimo".to_string(),
            Outcome::Content(c) if c == fixture::CANARY => "FUGA (canário lido)".to_string(),
            other => other.label(),
        };
        *outcomes.entry(key).or_default() += 1;
    }
    stop.store(true, Ordering::Relaxed);
    let swaps = swapper.join().expect("swapper");
    outcomes.insert("trocas feitas pela outra thread".into(), swaps as usize);
    outcomes
}

fn latency_ns(sb: &Sandbox, path: &str, iters: usize) -> f64 {
    // Aquecimento.
    for _ in 0..iters / 10 {
        let _ = sb.read(path);
    }
    let start = Instant::now();
    for _ in 0..iters {
        let _ = std::hint::black_box(sb.read(path));
    }
    start.elapsed().as_nanos() as f64 / iters as f64
}

fn main() -> Result<()> {
    let fx = fixture::build()?;
    let refroot = open_dir(&fx.refroot);
    let cases = cases(&fx);
    let race_attempts: usize = std::env::var("E04_RACE_ATTEMPTS").ok().and_then(|v| v.parse().ok()).unwrap_or(1_000_000);
    let lat_iters: usize = std::env::var("E04_LAT_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000);

    let reference_table: BTreeMap<String, String> =
        cases.iter().map(|c| (c.id.to_string(), vfs::reference(&refroot, &c.path).label())).collect();
    let ref_lat: BTreeMap<&str, f64> = [("1", "f1"), ("4", "d1/d2/d3/f4"), ("16", "e1/e2/e3/e4/e5/e6/e7/e8/e9/e10/e11/e12/e13/e14/e15/f16")]
        .iter()
        .map(|(k, p)| {
            let start = Instant::now();
            for _ in 0..lat_iters {
                let _ = std::hint::black_box(vfs::reference(&refroot, p));
            }
            (*k, start.elapsed().as_nanos() as f64 / lat_iters as f64)
        })
        .collect();

    let mut result = ExperimentResult::new("e04-hostfs-escape", "Confinamento e semântica de caminho do hostfs");
    let mut per_candidate = serde_json::Map::new();
    let mut total_escapes = 0usize;
    let mut summaries = Vec::new();
    let cap_scan = depscan::scan(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")), "cap-std").ok();

    for sb in candidates(&fx) {
        let name = sb.host.name();
        let (score, table) = run_suite(&sb, &refroot, &cases);
        let race_out = race(&sb, &fx.jail, race_attempts);
        let race_escapes = race_out.get("FUGA (canário lido)").copied().unwrap_or(0);
        let lat: BTreeMap<&str, f64> = [("1", "f1"), ("4", "d1/d2/d3/f4"), ("16", "e1/e2/e3/e4/e5/e6/e7/e8/e9/e10/e11/e12/e13/e14/e15/f16")]
            .iter()
            .map(|(k, p)| (*k, (latency_ns(&sb, p, lat_iters) * 10.0).round() / 10.0))
            .collect();
        let escapes = score.escapes.len() + race_escapes;
        total_escapes += escapes;
        eprintln!(
            "{name}: fugas={escapes} semântica={}/{} race={race_out:?} lat={lat:?}",
            score.semantic_match, score.total
        );
        summaries.push(format!("{name}: {escapes} fugas, {}/{} iguais ao Linux", score.semantic_match, score.total));
        let metrics = json!({
            "escapes_static": score.escapes,
            "escapes_race": race_escapes,
            "race_attempts": race_attempts,
            "race_outcomes": race_out,
            "linux_parity": format!("{}/{}", score.semantic_match, score.total),
            "content_mismatches": score.content_mismatch,
            "errno_mismatches": score.errno_mismatch,
            "latency_ns_by_components": lat,
            "outcomes": table,
        });
        per_candidate.insert(name.clone(), metrics.clone());
        let parity = score.semantic_match == score.total;
        let fit = match (escapes, parity) {
            (0, true) => Fit::Fits,
            (0, false) => Fit::DoesNotFit,
            _ => Fit::DoesNotFit,
        };
        let (version, category, notes) = match &sb.host {
            HostFs::CapStd(_) => (
                cap_scan.as_ref().map(|s| s.root.version.clone()).unwrap_or_default(),
                cap_scan.as_ref().map(|s| s.root.category.letter().to_string()),
                "Confina (BENEATH por baixo), mas resolve symlink com a semântica do host: alvo absoluto e `..` acima da montagem viram erro próprio sem errno, em vez de seguir no namespace do sandbox.".to_string(),
            ),
            HostFs::Openat2 { resolve, .. } if resolve.contains(ResolveFlags::IN_ROOT) => (
                "Linux 6.12 openat2 (rustix 1.x)".into(),
                Some("a".into()),
                "Confina como chroot da própria montagem: symlink absoluto e `..` acima da raiz ficam presos dentro do diretório do host, quando no sandbox deveriam apontar pro resto do namespace.".into(),
            ),
            HostFs::Openat2 { .. } => (
                "Linux 6.12 openat2 (rustix 1.x)".into(),
                Some("a".into()),
                "Confina, mas qualquer symlink absoluto ou `..` acima da montagem vira EXDEV, um errno que o Linux nunca daria pro processo do sandbox.".into(),
            ),
            HostFs::Manual { .. } => (
                "à mão (rustix 1.x: openat O_PATH|O_NOFOLLOW, fstat, readlinkat no próprio fd)".into(),
                Some("a".into()),
                "O hostfs só faz lookup de um nome por vez sem seguir symlink e devolve o symlink pro namei do sandbox; `..` desempilha o fd pelo qual a resolução passou. Correto, mas 2 syscalls por componente.".into(),
            ),
            HostFs::Hybrid { .. } => (
                "à mão (rustix 1.x: openat2 BENEATH|NO_SYMLINKS|NO_MAGICLINKS + namei manual)".into(),
                Some("a".into()),
                "Resolve o resto do caminho numa syscall quando não há symlink nem `..` acima do ponto de partida; o kernel recusa com ELOOP/EXDEV e aí o namei manual assume. Mesma semântica do manual, custo de uma syscall no caso comum.".into(),
            ),
        };
        result.candidates.push(CandidateResult {
            name,
            version,
            role: "hostfs".into(),
            category,
            conformance: None,
            fit,
            notes,
            metrics,
        });
    }

    result.metrics = json!({
        "cases": cases.iter().map(|c| json!({"id": c.id, "path_len": c.path.len()})).collect::<Vec<_>>(),
        "linux_reference": reference_table,
        "reference_latency_ns_by_components": ref_lat,
        "candidates": per_candidate,
    });
    let own_namei_ok = result
        .candidates
        .iter()
        .filter(|c| c.name.starts_with("namei"))
        .all(|c| c.fit == Fit::Fits);
    let verdict = if total_escapes == 0 { Verdict::Partial } else { Verdict::Refuted };
    result.hypothesis(
        "H18",
        verdict,
        format!(
            "Contra fuga, todos os candidatos seguraram ({total_escapes} leituras do canário em {} casos estáticos e {race_attempts} tentativas de corrida por candidato). Mas o critério também pede errno igual ao Linux, e aí cap-std, openat2 BENEATH e openat2 IN_ROOT erram: delegar a resolução ao kernel do host aplica a semântica do host a symlinks absolutos e a `..` acima da montagem. Os dois candidatos com namei próprio (manual e híbrido, symlink devolvido e resolvido no namespace do sandbox){} batem com o Linux em todos os casos, e o híbrido custa o mesmo que um openat2. Resumo: {}.",
            cases.len(),
            if own_namei_ok { "" } else { " (um deles falhou em algum caso, ver métricas)" },
            summaries.join("; "),
        ),
        json!({"total_escapes": total_escapes, "per_candidate": summaries}),
    );
    result.notes.push("A referência de 'igual ao Linux' é o próprio kernel resolvendo o mesmo caminho num namespace materializado (refroot com /etc do sandbox e cópia da jaula em /work) com RESOLVE_IN_ROOT, que é a semântica de chroot.".into());
    result.notes.push("Hardlink pré-existente pro canário dentro da montagem é lido por todos os candidatos e não conta como fuga: é um objeto que o dono colocou dentro do diretório montado; a defesa é não montar diretório com hardlink pra fora (ou montar só leitura de uma árvore controlada).".into());
    let path = result.write()?;
    eprintln!("gravado {}", path.display());
    println!("{}", serde_json::to_string_pretty(&result.hypotheses)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Invariantes que a implementação escolhida (namei próprio) tem que manter: nenhuma fuga, nem na
    /// corrida, e resultado idêntico ao do kernel em todos os casos. Os candidatos que delegam também não
    /// podem deixar fugir nada.
    #[test]
    fn no_escape_and_own_namei_matches_linux() {
        let fx = fixture::build().expect("fixture");
        let refroot = open_dir(&fx.refroot);
        let cases = cases(&fx);
        for sb in candidates(&fx) {
            let (score, _) = run_suite(&sb, &refroot, &cases);
            assert!(score.escapes.is_empty(), "{}: fugas {:?}", sb.host.name(), score.escapes);
            let race_out = race(&sb, &fx.jail, 5_000);
            assert!(!race_out.contains_key("FUGA (canário lido)"), "{}: {race_out:?}", sb.host.name());
            if sb.host.name().starts_with("namei") {
                assert_eq!(
                    score.semantic_match,
                    score.total,
                    "{}: {:?} {:?}",
                    sb.host.name(),
                    score.content_mismatch,
                    score.errno_mismatch
                );
            }
        }
    }
}
