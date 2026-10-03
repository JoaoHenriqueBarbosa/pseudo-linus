//! Sondagens de checkpoint: dá pra interromper a avaliação por fora, sem matar o processo?
//!
//! Cada sondagem roda num subprocesso (`f04-awk-jq probe NOME`) com teto de memória (`prlimit`) e
//! tempo; o processo pai mede se a sondagem voltou sozinha (interrompida pelo checkpoint) ou teve de
//! ser morta. Dentro do subprocesso, uma thread de timer liga a flag de interrupção depois de
//! `ARM_AFTER`; quem respeita a flag desenrola a pilha com `resume_unwind` (o mesmo mecanismo do kill
//! no design) e a sondagem mede a latência entre ligar a flag e o desenrolar.

use std::io::{BufRead, Read};
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::jq::cli::{self, RunOpts};
use crate::jq::engine::{EngineOpts, Interrupted};

pub const ARM_AFTER: Duration = Duration::from_millis(100);
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// Teto de memória virtual das sondagens (evita que `[range(1e18)]` coma a RAM do host).
pub const PROBE_AS_LIMIT: &str = "--as=3000000000";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeResult {
    pub name: String,
    pub subject: String,
    pub program: String,
    /// A sondagem voltou sozinha antes do timeout do pai.
    pub returned: bool,
    /// Interrompida pelo checkpoint (desenrolou com `Interrupted` ou o candidato devolveu erro de cancelamento).
    pub interrupted: bool,
    /// Morta pelo pai no timeout, ou por sinal (ex.: falta de memória sob o teto).
    pub killed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_us: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoints: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_gap_us: Option<f64>,
    pub elapsed_ms: f64,
    pub note: String,
}

pub struct ProbeSpec {
    pub name: &'static str,
    pub subject: &'static str,
    pub program: &'static str,
    pub what: &'static str,
}

pub const PROBES: &[ProbeSpec] = &[
    ProbeSpec {
        name: "jaq-nohook-last-range",
        subject: "jaq-core 3.1.1 (iterador de saída embrulhado, sem gancho)",
        program: "last(range(1e18))",
        what: "o único controle é o next() do iterador de saída, que não volta enquanto não houver saída",
    },
    ProbeSpec {
        name: "jaq-hook-last-range",
        subject: "jaq-core 3.1.1 + DataT com checkpoint em HasLut::lut()",
        program: "last(range(1e18))",
        what: "checkpoint a cada nó avaliado",
    },
    ProbeSpec {
        name: "jaq-hook-limit-repeat",
        subject: "jaq-core 3.1.1 + DataT com checkpoint em HasLut::lut()",
        program: "[limit(1e18; repeat(1))] | length",
        what: "recursão de definição jq (repeat)",
    },
    ProbeSpec {
        name: "jaq-hook-tailrec",
        subject: "jaq-core 3.1.1 + DataT com checkpoint em HasLut::lut()",
        program: "def f: f; f",
        what: "recursão de cauda infinita",
    },
    ProbeSpec {
        name: "jaq-hook-collect-range",
        subject: "jaq-core 3.1.1 + DataT com checkpoint em HasLut::lut()",
        program: "[range(1e18)] | length",
        what: "gerador nativo consumido por construtor nativo de array: nenhum nó avaliado por elemento",
    },
    ProbeSpec {
        name: "jaq-hook-range-override-collect",
        subject: "jaq-core 3.1.1 + checkpoint em lut() + range/3 nativa nossa com checkpoint",
        program: "[range(1e18)] | length",
        what: "a nativa range/3 sobrescrita passa pelo checkpoint a cada elemento",
    },
    ProbeSpec {
        name: "qj-core-small",
        subject: "qj 0.2.1, núcleo jq em processo (qj::jq::lang::execute::driver)",
        program: "[range(5)] | add",
        what: "controle: o núcleo do qj roda em processo sobre bytes em memória",
    },
    ProbeSpec {
        name: "qj-core-last-range",
        subject: "qj 0.2.1, núcleo jq em processo (qj::jq::lang::execute::driver)",
        program: "last(range(1e18))",
        what: "a VM do qj (jq_next) é um laço de despacho sem gancho",
    },
    ProbeSpec {
        name: "bashkit-jq-cancel",
        subject: "bashkit 0.18.2 jq (cancellation_token ligado e timeout de 1 s)",
        program: "jq -n 'last(range(1e18))'",
        what: "o bashkit só checa o prazo a cada valor emitido",
    },
    ProbeSpec {
        name: "bashkit-awk-cancel",
        subject: "bashkit 0.18.2 awk (cancellation_token ligado, limites largos)",
        program: "awk 'BEGIN { while (1) x++ }'",
        what: "orçamento de trabalho consumido a cada ação do awk",
    },
    ProbeSpec {
        name: "bashkit-awk-empty-loop",
        subject: "bashkit 0.18.2 awk (cancellation_token ligado, limites largos)",
        program: "awk 'BEGIN { while (1) {} }'",
        what: "laço com corpo vazio: nenhuma ação, nenhum orçamento consumido",
    },
    ProbeSpec {
        name: "bashkit-awk-default-limits",
        subject: "bashkit 0.18.2 awk (limites padrão)",
        program: "awk 'BEGIN { while (1) x++; print \"fim\", x }'",
        what: "o limite de 10 mil iterações por laço do bashkit",
    },
    ProbeSpec {
        name: "rawk-core-while",
        subject: "rawk-core 0.6.0",
        program: "BEGIN { while (1) x++ }",
        what: "API recebe as linhas e devolve a saída; nenhum ponto de controle",
    },
    ProbeSpec {
        name: "awk-rs-lib-while",
        subject: "awk-rs 0.2.0 (biblioteca, Read/Write nossos)",
        program: "BEGIN { while (1) x++ }",
        what: "Read/Write nossos só são chamados em E/S",
    },
    ProbeSpec {
        name: "awk-rs-lib-records",
        subject: "awk-rs 0.2.0 (biblioteca, Read nosso checando a flag)",
        program: "{ n++ }",
        what: "registro a registro, o leitor nosso devolve erro quando a flag liga",
    },
];

/// Pai: roda cada sondagem num subprocesso.
pub fn run_all() -> Vec<ProbeResult> {
    let exe = std::env::current_exe().expect("executável atual");
    let prlimit = Command::new("prlimit").arg("--version").output().is_ok_and(|o| o.status.success());
    PROBES
        .iter()
        .map(|spec| {
            let mut cmd = if prlimit {
                let mut c = Command::new("prlimit");
                c.arg(PROBE_AS_LIMIT).arg("--").arg(&exe);
                c
            } else {
                Command::new(&exe)
            };
            cmd.arg("probe")
                .arg(spec.name)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let start = Instant::now();
            let out = crate::exec::spawn_and_wait(cmd, &[], PROBE_TIMEOUT, std::path::Path::new("."));
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            let base = ProbeResult {
                name: spec.name.into(),
                subject: spec.subject.into(),
                program: spec.program.into(),
                returned: false,
                interrupted: false,
                killed: true,
                signal: None,
                latency_us: None,
                checkpoints: None,
                max_gap_us: None,
                elapsed_ms,
                note: spec.what.into(),
            };
            match out {
                Ok(o) if !o.timed_out && o.exit == Some(0) => {
                    match serde_json::from_slice::<ProbeResult>(o.stdout.as_slice()) {
                        Ok(mut r) => {
                            r.elapsed_ms = elapsed_ms;
                            r
                        }
                        Err(e) => ProbeResult { note: format!("{} (saída inválida: {e})", spec.what), ..base },
                    }
                }
                Ok(o) if o.timed_out => ProbeResult {
                    note: format!("{}; não voltou em {:?}, morta pelo pai", spec.what, PROBE_TIMEOUT),
                    ..base
                },
                Ok(o) => ProbeResult {
                    signal: o.signal,
                    note: format!(
                        "{}; terminou sem interrupção (exit {:?}, sinal {:?}): {}",
                        spec.what,
                        o.exit,
                        o.signal,
                        o.stderr.preview(160)
                    ),
                    ..base
                },
                Err(e) => ProbeResult { note: format!("falha ao rodar a sondagem: {e}"), ..base },
            }
        })
        .collect()
}

/// Liga a flag depois de `ARM_AFTER` e guarda o instante.
fn arm(flag: Arc<AtomicBool>) -> Arc<Mutex<Option<Instant>>> {
    let armed_at = Arc::new(Mutex::new(None));
    let a2 = armed_at.clone();
    std::thread::spawn(move || {
        std::thread::sleep(ARM_AFTER);
        *a2.lock().expect("lock") = Some(Instant::now());
        flag.store(true, Ordering::SeqCst);
    });
    armed_at
}

fn emit(v: serde_json::Value) -> ExitCode {
    println!("{v}");
    ExitCode::SUCCESS
}

fn base(spec: &ProbeSpec) -> serde_json::Value {
    json!({
        "name": spec.name, "subject": spec.subject, "program": spec.program,
        "returned": true, "interrupted": false, "killed": false, "elapsed_ms": 0.0, "note": spec.what,
    })
}

fn jaq_probe(spec: &ProbeSpec, hook: bool, range_override: bool) -> ExitCode {
    let flag = Arc::new(AtomicBool::new(false));
    let armed_at = arm(flag.clone());
    let program = spec.program.to_string();
    let stats = Arc::new(crate::jq::engine::HookStats::default());
    let opts = RunOpts {
        engine: EngineOpts { checkpoint_range: range_override },
        interrupt: hook.then(|| flag.clone()),
        timing: true,
        stats: Some(stats.clone()),
    };
    let host = cli::Host {
        stdin: Vec::new(),
        read_file: Box::new(|_| Err("No such file or directory".into())),
        env: Vec::new(),
        now: None,
    };
    let start = Instant::now();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cli::run(&["-n".to_string(), program], host, opts)
    }));
    let end = Instant::now();
    let mut v = base(spec);
    v["elapsed_ms"] = json!(start.elapsed().as_secs_f64() * 1000.0);
    match r {
        Ok(out) => {
            v["note"] = json!(format!("{}; terminou sem interrupção (exit {})", spec.what, out.exit));
            v["checkpoints"] = json!(out.checkpoints);
        }
        Err(payload) => {
            let interrupted = payload.downcast_ref::<Interrupted>().is_some();
            v["interrupted"] = json!(interrupted);
            if let Some(at) = *armed_at.lock().expect("lock") {
                v["latency_us"] = json!(end.duration_since(at).as_secs_f64() * 1e6);
            }
        }
    }
    v["checkpoints"] = json!(stats.calls.load(Ordering::Relaxed));
    v["max_gap_us"] = json!(stats.max_gap_ns.load(Ordering::Relaxed) as f64 / 1000.0);
    emit(v)
}

/// O núcleo do qj (porte do jq 1.8.1) em processo: `driver::run` recebe programa e bytes e só volta
/// no fim; a VM (`jq_next`) não tem gancho.
fn qj_probe(spec: &ProbeSpec) -> ExitCode {
    let opts = qj::jq::lang::execute::driver::Options { null_input: true, ..Default::default() };
    let start = Instant::now();
    let out = qj::jq::lang::execute::driver::run(spec.program, b"", &opts);
    let mut v = base(spec);
    v["elapsed_ms"] = json!(start.elapsed().as_secs_f64() * 1000.0);
    v["note"] = json!(format!("{}; terminou com exit {}", spec.what, out.exit));
    emit(v)
}

fn bashkit_probe(spec: &ProbeSpec, cmd: &str, wide: bool, cancel: bool) -> ExitCode {
    let mut limits = if wide { crate::bashkit_cand::wide_limits() } else { bashkit::ExecutionLimits::default() };
    if spec.name == "bashkit-jq-cancel" {
        limits.timeout = Duration::from_secs(1);
    }
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().expect("tokio");
    let mut bash = bashkit::Bash::builder().limits(limits).build();
    let token = bash.cancellation_token();
    let armed_at = if cancel { Some(arm(token)) } else { None };
    let start = Instant::now();
    let r = rt.block_on(bash.exec(cmd));
    let end = Instant::now();
    let mut v = base(spec);
    v["elapsed_ms"] = json!(start.elapsed().as_secs_f64() * 1000.0);
    let text = match &r {
        Ok(res) => format!(
            "exit {} stdout {:?} stderr {:?}",
            res.exit_code,
            String::from_utf8_lossy(res.stdout.as_bytes()),
            String::from_utf8_lossy(res.stderr.as_bytes())
        ),
        Err(e) => format!("erro {e}"),
    };
    let cancelled = matches!(&r, Err(e) if e.to_string().to_lowercase().contains("cancel"))
        || matches!(&r, Ok(res) if res.exit_code != 0);
    if let Some(a) = armed_at
        && let Some(at) = *a.lock().expect("lock")
    {
        v["latency_us"] = json!(end.duration_since(at).as_secs_f64() * 1e6);
        v["interrupted"] = json!(cancelled);
    }
    v["note"] = json!(format!("{}; resultado: {text}", spec.what));
    emit(v)
}

/// Leitor infinito de registros que devolve erro quando a flag liga.
struct FlagReader {
    flag: Arc<AtomicBool>,
    pending: &'static [u8],
}

impl Read for FlagReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let data = self.fill_buf()?;
        let n = data.len().min(buf.len());
        buf[..n].copy_from_slice(&data[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for FlagReader {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.flag.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("checkpoint: interrompido"));
        }
        if self.pending.is_empty() {
            self.pending = b"registro\n";
        }
        Ok(self.pending)
    }

    fn consume(&mut self, n: usize) {
        self.pending = &self.pending[n..];
    }
}

fn awk_rs_probe(spec: &ProbeSpec, records: bool) -> ExitCode {
    let flag = Arc::new(AtomicBool::new(false));
    let armed_at = arm(flag.clone());
    let mut lexer = awk_rs::Lexer::new(spec.program);
    let tokens = lexer.tokenize().expect("tokens");
    let mut parser = awk_rs::Parser::new(tokens);
    let program = parser.parse().expect("programa");
    let mut interp = awk_rs::Interpreter::new(&program);
    let mut out = Vec::new();
    let start = Instant::now();
    let r = if records {
        let reader = FlagReader { flag: flag.clone(), pending: b"" };
        interp.run(vec![reader], &mut out)
    } else {
        let inputs: Vec<std::io::BufReader<&[u8]>> = Vec::new();
        interp.run(inputs, &mut out)
    };
    let end = Instant::now();
    let mut v = base(spec);
    v["elapsed_ms"] = json!(start.elapsed().as_secs_f64() * 1000.0);
    v["interrupted"] = json!(r.is_err());
    if let Some(at) = *armed_at.lock().expect("lock") {
        v["latency_us"] = json!(end.duration_since(at).as_secs_f64() * 1e6);
    }
    v["note"] = json!(format!("{}; resultado: {:?}", spec.what, r.map_err(|e| e.to_string())));
    emit(v)
}

fn rawk_probe(spec: &ProbeSpec) -> ExitCode {
    let awk = rawk_core::awk::Awk::new(spec.program).expect("programa");
    let start = Instant::now();
    let (out, err) = awk.run(Vec::new(), None, None);
    let mut v = base(spec);
    v["elapsed_ms"] = json!(start.elapsed().as_secs_f64() * 1000.0);
    v["note"] = json!(format!("{}; terminou: {} linhas, erro {err:?}", spec.what, out.len()));
    emit(v)
}

/// Filho: executa uma sondagem e imprime o resultado em JSON.
pub fn probe_main(args: &[String]) -> ExitCode {
    let Some(name) = args.first() else {
        eprintln!("uso: probe NOME");
        return ExitCode::FAILURE;
    };
    let Some(spec) = PROBES.iter().find(|p| p.name == name.as_str()) else {
        eprintln!("sondagem desconhecida: {name}");
        return ExitCode::FAILURE;
    };
    match spec.name {
        "jaq-nohook-last-range" => jaq_probe(spec, false, false),
        "jaq-hook-last-range" | "jaq-hook-limit-repeat" | "jaq-hook-tailrec" | "jaq-hook-collect-range" => {
            jaq_probe(spec, true, false)
        }
        "jaq-hook-range-override-collect" => jaq_probe(spec, true, true),
        "bashkit-jq-cancel" => bashkit_probe(spec, "jq -n 'last(range(1e18))'", true, true),
        "bashkit-awk-cancel" => bashkit_probe(spec, "awk 'BEGIN { while (1) x++ }'", true, true),
        "bashkit-awk-empty-loop" => bashkit_probe(spec, "awk 'BEGIN { while (1) {} }'", true, true),
        "bashkit-awk-default-limits" => {
            bashkit_probe(spec, "awk 'BEGIN { while (1) x++; print \"fim\", x }'", false, false)
        }
        "qj-core-last-range" | "qj-core-small" => qj_probe(spec),
        "rawk-core-while" => rawk_probe(spec),
        "awk-rs-lib-while" => awk_rs_probe(spec, false),
        "awk-rs-lib-records" => awk_rs_probe(spec, true),
        _ => ExitCode::FAILURE,
    }
}

/// Custo do checkpoint no jaq: o mesmo programa no jaq puro (`JustLut`, sem gancho) e no jaq com o
/// nosso `DataT` (contador + leitura atômica da flag a cada nó), em processo, 9 rodadas alternadas,
/// mediana. A máquina é compartilhada com outros experimentos, então a dispersão também é registrada.
pub fn hook_overhead() -> serde_json::Value {
    let program = "reduce range(1000000) as $x (0; . + ($x % 7))";
    let ours_with = |interrupt: bool| {
        let flag = Arc::new(AtomicBool::new(false));
        let opts = RunOpts {
            engine: EngineOpts::default(),
            interrupt: interrupt.then_some(flag),
            timing: false,
            stats: None,
        };
        let host = cli::Host {
            stdin: Vec::new(),
            read_file: Box::new(|_| Err("No such file or directory".into())),
            env: Vec::new(),
            now: None,
        };
        let start = Instant::now();
        let out = cli::run(&["-n".to_string(), program.to_string()], host, opts);
        (start.elapsed().as_secs_f64(), out.checkpoints)
    };
    let mut base = Vec::new();
    let mut counter = Vec::new();
    let mut hooked = Vec::new();
    let mut calls = 0;
    for i in 0..9 {
        // Gira a ordem pra não favorecer um lado com cache quente ou frequência de CPU.
        for k in 0..3 {
            match (i + k) % 3 {
                0 => base.push(crate::jq::engine::baseline_eval(program).as_secs_f64()),
                1 => counter.push(ours_with(false).0),
                _ => {
                    let (t, c) = ours_with(true);
                    hooked.push(t);
                    calls = c;
                }
            }
        }
    }
    for v in [&mut base, &mut counter, &mut hooked] {
        v.sort_by(|a, b| a.total_cmp(b));
    }
    let (m_base, m_counter, m_hook) = (base[4], counter[4], hooked[4]);
    json!({
        "program": program,
        "runs": 9,
        "median_ms_jaq_plain": m_base * 1000.0,
        "median_ms_layer_counter_only": m_counter * 1000.0,
        "median_ms_layer_with_flag": m_hook * 1000.0,
        "min_max_ms_jaq_plain": [base[0] * 1000.0, base[8] * 1000.0],
        "min_max_ms_layer_with_flag": [hooked[0] * 1000.0, hooked[8] * 1000.0],
        "checkpoints_per_run": calls,
        "flag_extra_ns_per_checkpoint": if calls > 0 { (m_hook - m_counter) * 1e9 / calls as f64 } else { 0.0 },
        "flag_slowdown_pct": (m_hook / m_counter - 1.0) * 100.0,
        "layer_vs_plain_pct": (m_hook / m_base - 1.0) * 100.0,
        // O mínimo de 9 é mais robusto à interferência de outros processos que a mediana.
        "best_ms_jaq_plain": base[0] * 1000.0,
        "best_ms_layer_counter_only": counter[0] * 1000.0,
        "best_ms_layer_with_flag": hooked[0] * 1000.0,
        "best_layer_with_flag_vs_plain_pct": (hooked[0] / base[0] - 1.0) * 100.0,
        "best_flag_vs_counter_pct": (hooked[0] / counter[0] - 1.0) * 100.0,
    })
}
