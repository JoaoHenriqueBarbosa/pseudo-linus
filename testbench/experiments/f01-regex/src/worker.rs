//! Isolamento: cada motor roda em subprocessos (o próprio binário com `--worker`) que devolvem uma
//! comparação por linha. Motor que trava num padrão (backtracking exponencial, laço) é morto depois
//! de [`STALL`] sem progresso; o caso vira "timeout" e o worker recomeça do caso seguinte. Um motor
//! que derruba o processo (stack overflow) perde só o caso em que caiu.
//!
//! A comparação com o golden acontece no worker, por `harness::score` caso a caso, pra que o pai não
//! precise guardar a saída de dezenas de milhares de casos.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use harness::{Case, CaseComparison, Conformance, Outcome};
use serde::{Deserialize, Serialize};

use crate::engines::engine_by_name;
use crate::probe::EngineCandidate;

pub const STALL: Duration = Duration::from_secs(3);

#[derive(Serialize, Deserialize)]
struct Line {
    cmp: CaseComparison,
    micros: u64,
}

/// Lado do worker: `bin --worker ENGINE SHARD FROM`.
pub fn worker_main(engine: &str, shard: &Path, from: usize) -> Result<()> {
    let engine = engine_by_name(engine).with_context(|| format!("motor {engine}"))?;
    let cases: Vec<(Case, Outcome)> = serde_json::from_slice(&std::fs::read(shard)?)?;
    let cand = EngineCandidate { engine: engine.as_ref() };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for pair in cases.iter().skip(from) {
        let started = Instant::now();
        let (_, mut cmps) = harness::score(&cand, std::slice::from_ref(pair));
        let line = Line { cmp: cmps.remove(0), micros: started.elapsed().as_micros() as u64 };
        serde_json::to_writer(&mut out, &line)?;
        out.write_all(b"\n")?;
        out.flush()?;
    }
    Ok(())
}

/// Resultado de um motor num conjunto de casos.
#[derive(Default)]
pub struct EngineRun {
    pub comparisons: BTreeMap<String, CaseComparison>,
    pub timeouts: Vec<String>,
    pub crashes: Vec<String>,
    pub busy: Duration,
}

fn run_shard(engine: &str, shard: &Path, cases: &[(Case, Outcome)]) -> Result<EngineRun> {
    let exe = std::env::current_exe()?;
    let mut run = EngineRun::default();
    let mut next = 0usize;
    while next < cases.len() {
        let mut child = Command::new(&exe)
            .arg("--worker")
            .arg(engine)
            .arg(shard)
            .arg(next.to_string())
            .env("LC_ALL", "C.UTF-8")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("subindo worker")?;
        let stdout = child.stdout.take().expect("stdout");
        let (tx, rx) = mpsc::channel::<String>();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        // O primeiro caso inclui o tempo de carregar o shard.
        let mut wait = STALL + Duration::from_secs(10);
        loop {
            match rx.recv_timeout(wait) {
                Ok(text) => {
                    let line: Line = serde_json::from_str(&text)?;
                    run.busy += Duration::from_micros(line.micros);
                    run.comparisons.insert(line.cmp.id.clone(), line.cmp);
                    next += 1;
                    wait = STALL;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = child.wait();
                    if next < cases.len() {
                        let (case, golden) = &cases[next];
                        run.crashes.push(case.id.clone());
                        let cmp = harness::compare_outcome(case, golden, &Outcome::unsupported("worker morreu (crash do motor)"));
                        run.comparisons.insert(case.id.clone(), cmp);
                        next += 1;
                    }
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let (case, golden) = &cases[next];
                    run.timeouts.push(case.id.clone());
                    run.busy += STALL;
                    let why = format!("timeout: {}s sem resposta", STALL.as_secs());
                    run.comparisons.insert(case.id.clone(), harness::compare_outcome(case, golden, &Outcome::unsupported(why)));
                    next += 1;
                    break;
                }
            }
        }
        let _ = reader.join();
    }
    Ok(run)
}

/// Roda todos os motores em todos os shards, com no máximo `parallel` workers ao mesmo tempo.
pub fn run_all(
    engines: &[&str],
    cases: &[(Case, Outcome)],
    shards: usize,
    parallel: usize,
    dir: &Path,
) -> Result<BTreeMap<String, EngineRun>> {
    let chunk = cases.len().div_ceil(shards.max(1)).max(1);
    let mut shard_files: Vec<(PathBuf, &[(Case, Outcome)])> = Vec::new();
    for (i, part) in cases.chunks(chunk).enumerate() {
        let path = dir.join(format!("shard-{i}.json"));
        std::fs::write(&path, serde_json::to_vec(part)?)?;
        shard_files.push((path, part));
    }
    let tasks: Vec<(&str, usize)> =
        engines.iter().flat_map(|e| (0..shard_files.len()).map(move |s| (*e, s))).collect();
    let queue = Mutex::new(tasks.into_iter());
    let results: Mutex<BTreeMap<String, EngineRun>> = Mutex::new(BTreeMap::new());
    let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..parallel.max(1) {
            s.spawn(|| {
                loop {
                    let task = queue.lock().unwrap_or_else(|e| e.into_inner()).next();
                    let Some((engine, shard)) = task else { break };
                    let (path, part) = &shard_files[shard];
                    match run_shard(engine, path, part) {
                        Ok(run) => {
                            let mut all = results.lock().unwrap_or_else(|e| e.into_inner());
                            let slot = all.entry(engine.to_string()).or_default();
                            slot.comparisons.extend(run.comparisons);
                            slot.timeouts.extend(run.timeouts);
                            slot.crashes.extend(run.crashes);
                            slot.busy += run.busy;
                        }
                        Err(e) => errors.lock().unwrap_or_else(|e| e.into_inner()).push(format!("{engine}: {e:#}")),
                    }
                }
            });
        }
    });
    let errors = errors.into_inner().unwrap_or_default();
    anyhow::ensure!(errors.is_empty(), "workers falharam: {errors:?}");
    for (path, _) in &shard_files {
        let _ = std::fs::remove_file(path);
    }
    Ok(results.into_inner().unwrap_or_default())
}

/// Placar no formato do `harness::score`, a partir das comparações caso a caso.
pub fn tally(name: &str, cmps: &[&CaseComparison]) -> Conformance {
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
