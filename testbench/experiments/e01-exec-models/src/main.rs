//! E01: roda todas as medições dos modelos de execução e grava `results/e01-exec-models.json`.
//!
//! Subcomandos internos (o binário chama a si mesmo em subprocesso):
//! - `scale <modelo> <n>`: escala de processos ociosos, roda dentro de um scope do systemd (H03/H04);
//! - `overflow <variante>`: stack overflow proposital (H08), pode derrubar o subprocesso.
//!
//! `quick` roda tudo com tamanhos reduzidos e não grava o JSON (pra desenvolvimento).

use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use e01_exec_models::experiments::{self, HypOut, h01, h02, h03_h04, h05, h06, h07, h08, h09, h10};
use e01_exec_models::{ModelSpec, stats::r3};
use harness::{CandidateResult, ExperimentResult, Fit};
use serde_json::{Value, json};

struct Profile {
    write: bool,
    h02: h02::Sizes,
    scale_counts: Vec<usize>,
    spawn_iters_a: usize,
    spawn_iters_fast: usize,
    vm_n: i64,
    vm_reps: usize,
    lat_n: i64,
    kill_reps: usize,
    h07_window: Duration,
    swap_iters: u64,
}

fn full() -> Profile {
    Profile {
        write: true,
        h02: h02::Sizes::FULL,
        scale_counts: h03_h04::COUNTS.to_vec(),
        spawn_iters_a: 2_000,
        spawn_iters_fast: 20_000,
        // 400 mil iterações = 6,4 milhões de instruções (~10 ms) por modo e rodada; 40 rodadas.
        vm_n: 400_000,
        vm_reps: 40,
        lat_n: 8_000_000,
        kill_reps: 100,
        h07_window: Duration::from_millis(400),
        swap_iters: 300_000,
    }
}

fn quick() -> Profile {
    Profile {
        write: false,
        h02: h02::Sizes {
            yes_lines: 4 << 20,
            bulk_bytes: 16 << 20,
            record_bytes: 1 << 20,
            record_size: 64,
            yield_iters_a: 3_000,
            yield_iters_fast: 50_000,
            pipe_iters_a: 3_000,
            pipe_iters_fast: 30_000,
            reps: 1,
        },
        scale_counts: vec![1_000],
        spawn_iters_a: 300,
        spawn_iters_fast: 3_000,
        vm_n: 100_000,
        vm_reps: 8,
        lat_n: 2_000_000,
        kill_reps: 10,
        h07_window: Duration::from_millis(200),
        swap_iters: 30_000,
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("scale") => {
            let spec = args.get(1).and_then(|s| ModelSpec::parse(s)).context("modelo")?;
            let n: usize = args.get(2).context("quantidade")?.parse()?;
            let rep = h03_h04::scale_child(spec, n);
            println!("{}", serde_json::to_string(&rep)?);
            Ok(())
        }
        Some("overflow") => {
            let variant = args.get(1).context("variante")?;
            let rep = h08::overflow_child(variant);
            println!("{}", serde_json::to_string(&rep)?);
            Ok(())
        }
        Some("quick") => run_all(quick()),
        Some("one") => {
            let id = args.get(1).context("hipótese (h01..h10)")?;
            let p = if args.get(2).map(String::as_str) == Some("quick") { quick() } else { full() };
            let outs = run_one(id, &p)?;
            for h in outs {
                println!("{}", serde_json::to_string_pretty(&h.evidence)?);
                println!("{} {:?}: {}", h.id, h.verdict, h.summary);
            }
            Ok(())
        }
        None | Some("all") => run_all(full()),
        Some(other) => bail!("subcomando desconhecido: {other}"),
    }
}

/// Roda uma hipótese só e devolve o resultado (pra depuração; não grava o JSON).
fn run_one(id: &str, p: &Profile) -> anyhow::Result<Vec<HypOut>> {
    Ok(match id {
        "h01" => vec![h01::run()],
        "h02" => vec![h02::verdict(&h02::measure(p.h02))],
        "h03" | "h04" => {
            let (a, b) = h03_h04::verdicts(&h03_h04::measure(&p.scale_counts, p.spawn_iters_a, p.spawn_iters_fast));
            vec![a, b]
        }
        "h05" => vec![h05::verdict(&h05::measure(p.vm_n, p.vm_reps, p.lat_n))],
        "h06" => vec![h06::verdict(&h06::measure(p.kill_reps))],
        "h07" => vec![h07::verdict(&h07::measure(p.h07_window))],
        "h08" => vec![h08::verdict(&h08::measure())],
        "h09" => vec![h09::verdict(&h09::measure())],
        "h10" => vec![h10::verdict(&h10::measure(p.swap_iters, p.vm_reps.min(3)))],
        other => bail!("hipótese desconhecida: {other}"),
    })
}

fn hyp(res: &mut ExperimentResult, h: HypOut) {
    println!("{} {:?}: {}", h.id, h.verdict, h.summary);
    res.hypothesis(h.id, h.verdict, h.summary, h.evidence);
}

fn run_all(p: Profile) -> anyhow::Result<()> {
    let t0 = Instant::now();
    let h01v = h01::run();

    let d02 = h02::measure(p.h02);
    let h02v = h02::verdict(&d02);

    let d34 = h03_h04::measure(&p.scale_counts, p.spawn_iters_a, p.spawn_iters_fast);
    let (h03v, h04v) = h03_h04::verdicts(&d34);

    let d05 = h05::measure(p.vm_n, p.vm_reps, p.lat_n);
    let h05v = h05::verdict(&d05);

    let d06 = h06::measure(p.kill_reps);
    let h06v = h06::verdict(&d06);

    let d07 = h07::measure(p.h07_window);
    let h07v = h07::verdict(&d07);

    let d08 = h08::measure();
    let h08v = h08::verdict(&d08);

    let d09 = h09::measure();
    let h09v = h09::verdict(&d09);

    let d10 = h10::measure(p.swap_iters, p.vm_reps.min(3));
    let h10v = h10::verdict(&d10);

    let rec = recommend(&h02v, &h03v, &h04v, &h05v, &h07v, &h08v, &h10v);
    experiments::progress(format!("tempo total: {:.1} s", t0.elapsed().as_secs_f64()));

    let mut res = ExperimentResult::new(
        "e01-exec-models",
        "Modelos de execução: thread por processo (A), corrotina presa ao worker (B), async (C)",
    );
    let candidates = candidates(&rec, &h01v, &h08v);
    for h in [h01v, h02v, h03v, h04v, h05v, h06v, h07v, h08v, h09v, h10v] {
        hyp(&mut res, h);
    }
    res.candidates = candidates;
    res.metrics = json!({
        "recommendation": rec.model,
        "recommendation_inputs": rec.inputs,
        "runtime_s": r3(t0.elapsed().as_secs_f64()),
        "host_load_at_end": std::fs::read_to_string("/proc/loadavg").unwrap_or_default().trim(),
    });
    res.notes = rec.notes.clone();
    println!("Recomendação: {}", rec.model);
    for n in &rec.notes {
        println!("- {n}");
    }
    if p.write {
        let path = res.write()?;
        println!("gravado em {}", path.display());
    }
    Ok(())
}

struct Recommendation {
    model: String,
    inputs: Value,
    notes: Vec<String>,
}

fn num(v: &Value, path: &[&str]) -> f64 {
    let mut cur = v;
    for p in path {
        cur = &cur[*p];
    }
    cur.as_f64().unwrap_or(f64::NAN)
}

/// Recomendação de modelo a partir dos números medidos. Regra (escrita também no README):
/// 1. B sai se tiver falha de segurança medida: alguma execução de stacker dentro de corrotina derrubando
///    o host (H08) ou thread-local de processo errado sem troca manual (H10).
/// 2. Entre A e C: A fica se sustentar o maior número de processos (H04), se o pior pipeline em bloco dele
///    ficar em pelo menos 25% do melhor modelo (H02) e se, com o watchdog, o vizinho de um laço sem
///    checkpoint ficar com mais de 90% da CPU que os dois dividem (H07); senão C.
fn recommend(h02: &HypOut, h03: &HypOut, h04: &HypOut, h05: &HypOut, h07: &HypOut, h08: &HypOut, h10: &HypOut) -> Recommendation {
    // Alguma execução de stacker dentro de corrotina B derrubou o host.
    let b_stacker_broken = h08.evidence["per_variant"]
        .as_array()
        .map(|rows| {
            rows.iter().any(|r| {
                r["variant"].as_str().is_some_and(|v| v.starts_with('B') && v.contains("-stacker"))
                    && r["host_survived"].as_u64() < r["runs"].as_u64()
            })
        })
        .unwrap_or(false);
    let b_tls_naive_bad = h10.evidence["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["case"] == "B_naive"))
        .map(|r| r["mismatches"].as_u64().unwrap_or(0))
        .unwrap_or(0);
    let worst_bulk = num(&h02.evidence, &["worst_a_over_best_bulk"]);
    let worst_records = num(&h02.evidence, &["worst_a_over_best_records"]);
    let a_sustains = h04.evidence["per_model"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["model"] == "A"))
        .map(|r| r["sustained_max_count"] == true)
        .unwrap_or(false);
    let a_spawn_us = h04.evidence["per_model"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["model"] == "A"))
        .map(|r| r["spawn_exit_p50_us"].as_f64().unwrap_or(f64::NAN))
        .unwrap_or(f64::NAN);
    let mitig = h07.evidence["mitigation_model_a"].as_array().cloned().unwrap_or_default();
    // Fração da CPU do núcleo compartilhado que o vizinho recebe contra o laço sem checkpoint.
    let share = |cfg: &str| {
        mitig
            .iter()
            .find(|m| m["config"] == cfg)
            .and_then(|m| m["neighbor_cpu_share_vs_spinner"].as_f64())
            .unwrap_or(0.0)
    };
    let mitigation_works = share("spinner_watchdog_nice19") > 0.9;
    let a_rss = h03.evidence["per_model"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["model"] == "A"))
        .map(|r| r["at_max_count"]["rss_per_proc_bytes"].as_f64().unwrap_or(f64::NAN))
        .unwrap_or(f64::NAN);
    let a_cg = h03.evidence["per_model"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["model"] == "A"))
        .map(|r| r["at_max_count"]["cgroup_per_proc_bytes"].as_f64().unwrap_or(f64::NAN))
        .unwrap_or(f64::NAN);
    let a_handoff_p99 = h05.evidence["latency"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["model"] == "A"))
        .map(|r| r["handoff_ns"]["p99"].as_f64().unwrap_or(f64::NAN))
        .unwrap_or(f64::NAN);

    // A contra os mesmos pipelines com processos reais do Linux (pior razão entre yes|head e 4 estágios).
    let native = |w: &str| {
        h02.evidence["native_linux_pipelines"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["workload"] == w))
            .and_then(|r| r["mb_per_s"].as_f64())
    };
    let a_pipe = |w: &str| {
        h02.evidence["pipelines"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter(|r| r["model"] == "A" && r["workload"] == w)
                    .filter_map(|r| r["mb_per_s"].as_f64())
                    .fold(f64::INFINITY, f64::min)
            })
            .unwrap_or(f64::NAN)
    };
    let a_vs_native = ["yes_head", "bulk4"]
        .iter()
        .filter_map(|w| native(w).map(|n| a_pipe(w) / n))
        .fold(f64::INFINITY, f64::min);
    let c_async_penalty = num(&h05.evidence, &["c_async_loop_vs_a_sync_loop_pct"]);
    let b_out = b_stacker_broken || b_tls_naive_bad > 0;
    let a_ok = a_sustains && worst_bulk >= 0.25 && mitigation_works;
    let model = if a_ok { "A" } else { "C" };

    let mut notes = vec![
        format!(
            "Recomendação: modelo {model}. Regra: B sai por falha de segurança medida; entre A e C, A fica se sustentar o \
             maior número de processos, se o pior pipeline em bloco dele ficar em pelo menos 25% do melhor modelo e se, \
             com o watchdog de nice 19, o vizinho de um laço sem checkpoint ficar com mais de 90% da CPU que os dois \
             dividem."
        ),
        format!(
            "B: alguma execução de stacker dentro de corrotina derrubou o host = {b_stacker_broken}; conferências de \
             thread-local erradas sem troca manual = {b_tls_naive_bad}. B {}.",
            if b_out { "descartado" } else { "não descartado por segurança" }
        ),
        format!(
            "A: pior pipeline em bloco = {:.0}% do melhor modelo e {:.0}% do mesmo pipeline com processos reais do \
             Linux; pior pipeline de registros de 64 bytes = {:.0}%; spawn+exit mediano {a_spawn_us:.1} µs; sustenta o \
             maior N = {a_sustains}; RSS por processo ocioso {:.1} KiB ({:.1} KiB no cgroup, contando pilha de kernel); \
             vizinho de um laço sem checkpoint no mesmo núcleo fica com {:.0}% da CPU dos dois sem mitigação e {:.0}% \
             com o watchdog; p99 do timer até o próximo processo rodar {:.1} µs.",
            worst_bulk * 100.0,
            a_vs_native * 100.0,
            worst_records * 100.0,
            a_rss / 1024.0,
            a_cg / 1024.0,
            share("spinner_nice0") * 100.0,
            share("spinner_watchdog_nice19") * 100.0,
            a_handoff_p99 / 1e3,
        ),
        format!(
            "C exige que todo builtin e toda crate de terceiros sejam async; código síncrono de terceiros (jaq, \
             uutils, gix, regex) bloqueia o worker inteiro enquanto roda, um laço sem checkpoint num worker do C não \
             tem mitigação por thread (H07) porque a thread é compartilhada; o mesmo interpretador escrito em async \
             custou {c_async_penalty:+.1}% em relação à versão síncrona na mesma thread (H05)."
        ),
        "Medições de tempo feitas com a máquina compartilhada com outros experimentos são provisórias; o runner \
         refaz tudo em sequência numa máquina quieta (ver host_load_at_end nas métricas)."
            .to_string(),
    ];
    if !a_ok {
        notes.push(format!(
            "A não passou na regra: sustenta = {a_sustains}, pior pipeline em bloco = {:.0}%, mitigação = {mitigation_works}.",
            worst_bulk * 100.0
        ));
    }
    Recommendation {
        model: model.to_string(),
        inputs: json!({
            "b_stacker_in_coroutine_crashed": b_stacker_broken,
            "b_tls_naive_mismatches": b_tls_naive_bad,
            "a_worst_bulk_pipeline_ratio": worst_bulk,
            "a_worst_records_pipeline_ratio": worst_records,
            "a_worst_pipeline_vs_native_linux": a_vs_native,
            "a_sustains_max_count": a_sustains,
            "a_spawn_exit_p50_us": a_spawn_us,
            "a_rss_per_idle_proc_bytes": a_rss,
            "a_cgroup_per_idle_proc_bytes": a_cg,
            "h07_neighbor_share_nice0": share("spinner_nice0"),
            "h07_neighbor_share_watchdog": share("spinner_watchdog_nice19"),
            "a_handoff_p99_ns": a_handoff_p99,
            "c_async_interpreter_slowdown_pct": c_async_penalty,
        }),
        notes,
    }
}

fn candidates(rec: &Recommendation, h01: &HypOut, h08: &HypOut) -> Vec<CandidateResult> {
    let fit_model = |m: &str| if rec.model == m { Fit::Fits } else { Fit::DoesNotFit };
    let b_crash = rec.inputs["b_stacker_in_coroutine_crashed"] == true;
    vec![
        CandidateResult {
            name: "modelo A (thread do SO por processo, tokens de CPU)".into(),
            version: "e01".into(),
            role: "exec-model".into(),
            category: None,
            conformance: None,
            fit: fit_model("A"),
            notes: "std::thread + park/unpark; variante A-spin com 20 µs de espera ativa antes do park.".into(),
            metrics: rec.inputs.clone(),
        },
        CandidateResult {
            name: "modelo B (corrotina corosensei presa ao worker)".into(),
            version: "corosensei 0.3.4".into(),
            role: "exec-model".into(),
            category: None,
            conformance: None,
            fit: if b_crash { Fit::DoesNotFit } else { fit_model("B") },
            notes: "Sem migração; thread-local compartilhado entre corrotinas do worker; stacker usa os limites da thread.".into(),
            metrics: json!({ "b_stacker_in_coroutine_crashed": b_crash }),
        },
        CandidateResult {
            name: "modelo C (future com executor próprio)".into(),
            version: "e01".into(),
            role: "exec-model".into(),
            category: None,
            conformance: None,
            fit: fit_model("C"),
            notes: "Fila global com slot LIFO por worker; exige builtins async.".into(),
            metrics: Value::Null,
        },
        CandidateResult {
            name: "corosensei".into(),
            version: "0.3.4".into(),
            role: "stackful-coroutine".into(),
            category: None,
            conformance: None,
            fit: if rec.model == "B" { Fit::Fits } else { Fit::DoesNotFit },
            notes: "API segura, mas Coroutine é !Send (E0277 ao migrar); só serve presa ao worker.".into(),
            metrics: h01.evidence["probes"].clone(),
        },
        CandidateResult {
            name: "may".into(),
            version: "0.3.51".into(),
            role: "stackful-coroutine".into(),
            category: None,
            conformance: None,
            fit: Fit::DoesNotFit,
            notes: "coroutine::spawn é unsafe fn (E0133); barrado pelo forbid(unsafe_code).".into(),
            metrics: Value::Null,
        },
        CandidateResult {
            name: "generator".into(),
            version: "0.8".into(),
            role: "stackful-coroutine".into(),
            category: None,
            conformance: None,
            fit: Fit::DoesNotFit,
            notes: "Migração compila graças a um unsafe impl Send próprio, unsound (thread-local da thread 1 usado na 2).".into(),
            metrics: h01.evidence["generator_migration"].clone(),
        },
        CandidateResult {
            name: "stacker".into(),
            version: "0.1.25".into(),
            role: "stack-growth".into(),
            category: None,
            conformance: None,
            fit: if rec.model == "B" { Fit::DoesNotFit } else { Fit::Fits },
            notes: "maybe_grow + limite de profundidade evita derrubar o host em thread do SO (A) e em worker (C).".into(),
            metrics: h08.evidence["rows"].clone(),
        },
    ]
}
