//! `main` comum dos binários de candidato.
//!
//! Uso: `cand-<nome> <comando> [chave=valor ...]`. Cada comando imprime exatamente uma linha JSON no
//! stdout; diagnóstico vai pro stderr. Comandos:
//!
//! - `info`: nome e capacidades declaradas.
//! - `sort lines=N seed=S`, `sort-borrowed ...`: o sort de N linhas dentro de um pseudo-processo.
//! - `small ops=N seed=S`: alocações pequenas numa thread.
//! - `small-mt threads=T ops=N seed=S`: alocações pequenas em T pseudo-processos.
//! - `attribution`, `controlled`, `exit-residual`, `costs`: cenários de corretude e custo fixo.
//! - `overshoot limit=B chunk=B detect=self_poll|flag|watcher every=N period_us=U reps=R`.
//! - `kernel-acct reps=R pipe_mib=M file_mib=M`: custo da contabilidade explícita do kernel.
//!
//! Comandos próprios de um candidato (demonstrações que abortam) entram pelo `extra`.

use std::collections::BTreeMap;
use std::str::FromStr;

use serde_json::{Value, json};

use crate::accounting::Accounting;
use crate::scenarios::{self, Detect, OvershootCfg};
use crate::{kernel_acct, sys, workloads};

#[derive(Debug)]
pub struct Args {
    pub cmd: String,
    kv: BTreeMap<String, String>,
}

impl Args {
    pub fn parse() -> Args {
        let mut it = std::env::args().skip(1);
        let cmd = it.next().unwrap_or_else(|| "info".to_string());
        let kv = it
            .filter_map(|a| a.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
            .collect();
        Args { cmd, kv }
    }

    pub fn get<T: FromStr>(&self, key: &str, default: T) -> T {
        match self.kv.get(key) {
            None => default,
            Some(v) => v.parse().unwrap_or_else(|_| panic!("valor inválido pra {key}: {v}")),
        }
    }
}

fn to_json<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).expect("serialização")
}

/// Passo travado: gera a entrada uma vez, avisa que está pronto e roda um sort a cada linha "go" do
/// stdin, respondendo uma linha JSON por iteração. O orquestrador manda "go" pra todos os binários do
/// conjunto ao mesmo tempo, então a mesma iteração de cada um sofre a mesma carga dos vizinhos.
pub fn serve_sort<A: Accounting>(acct: &A, args: &Args) -> Value {
    use std::io::{BufRead, Write};
    let input = workloads::gen_lines(args.get("lines", 1_000_000), args.get("seed", 1));
    let stdout = std::io::stdout();
    writeln!(stdout.lock(), "{}", json!({"ready": true})).expect("stdout");
    let mut served = 0u64;
    for line in std::io::stdin().lock().lines() {
        if line.map(|l| l.trim() != "go").unwrap_or(true) {
            break;
        }
        let t = acct.run_process(None, |_| workloads::sort_owned(&input));
        writeln!(stdout.lock(), "{}", json!({"timed": t})).expect("stdout");
        served += 1;
    }
    json!({"served": served})
}

pub fn run<A: Accounting>(acct: &A, extra: impl Fn(&Args) -> Option<Value>) {
    let args = Args::parse();
    let out = match args.cmd.as_str() {
        "info" => json!({"name": acct.name(), "caps": acct.caps(), "overcommit": sys::overcommit_mode()}),
        "sort" | "sort-borrowed" => {
            // `iters` > 1 repete o sort no mesmo processo e reporta a iteração de menor CPU em `timed`
            // (as outras ficam em `all_*`): tira do número os soluços passageiros da máquina.
            let lines: usize = args.get("lines", 1_000_000);
            let iters: u32 = args.get("iters", 1).max(1);
            let input = workloads::gen_lines(lines, args.get("seed", 1));
            let owned = args.cmd == "sort";
            let runs: Vec<workloads::Timed> = (0..iters)
                .map(|_| {
                    acct.run_process(None, |_| {
                        if owned { workloads::sort_owned(&input) } else { workloads::sort_borrowed(&input) }
                    })
                })
                .collect();
            let best = *runs.iter().min_by_key(|t| t.cpu_ns).expect("ao menos uma iteração");
            json!({
                "timed": best,
                "all_cpu_ns": runs.iter().map(|t| t.cpu_ns).collect::<Vec<_>>(),
                "all_wall_ns": runs.iter().map(|t| t.elapsed_ns).collect::<Vec<_>>(),
                "input_bytes": input.len(),
                "peak_rss_kib": sys::peak_rss_kib(),
            })
        }
        "serve-sort" => serve_sort(acct, &args),
        "small" => {
            let t = acct.run_process(None, |_| workloads::small_ring(args.get("ops", 20_000_000), args.get("seed", 1)));
            json!({"timed": t, "peak_rss_kib": sys::peak_rss_kib()})
        }
        "small-mt" => {
            let t = workloads::small_ring_mt(acct, args.get("threads", 16), args.get("ops", 2_000_000), args.get("seed", 1));
            json!({"timed": t})
        }
        "attribution" => to_json(&scenarios::attribution(acct)),
        "controlled" => to_json(&scenarios::controlled(acct)),
        "exit-residual" => to_json(&scenarios::exit_residual(acct)),
        "costs" => to_json(&scenarios::costs(acct)),
        "overshoot" => {
            let cfg = OvershootCfg {
                limit: args.get("limit", 64 << 20),
                chunk: args.get("chunk", 65536),
                detect: args.get("detect", Detect::SelfPoll),
                every: args.get("every", 1).max(1),
                period_us: args.get("period_us", 1000),
            };
            let reps: u32 = args.get("reps", 5);
            let runs: Vec<_> = (0..reps).map(|_| scenarios::overshoot_once(acct, &cfg)).collect();
            json!({"cfg": cfg, "runs": runs})
        }
        "kernel-acct" => {
            let s = kernel_acct::suite(
                acct,
                args.get("reps", 5),
                args.get::<u64>("pipe_mib", 1024) << 20,
                args.get::<u64>("file_mib", 256) << 20,
            );
            to_json(&s)
        }
        other => match extra(&args) {
            Some(v) => v,
            None => {
                eprintln!("comando desconhecido: {other}");
                std::process::exit(2);
            }
        },
    };
    println!("{out}");
}
