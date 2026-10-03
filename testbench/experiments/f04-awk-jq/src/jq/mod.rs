//! jq sobre o jaq com a camada nossa (CLI, JSON, erros, checkpoint).

pub mod cli;
pub mod engine;
pub mod errors;
pub mod input;
pub mod json;

use std::path::PathBuf;

use harness::{Bytes, Candidate, Entry, Invocation, Outcome};

use crate::exec::SubprocessCandidate;

/// Pilha das threads que rodam o jaq em processo (recursão funda não pode derrubar a bancada).
pub const JAQ_STACK: usize = 1 << 30;

/// `jaq-core` + `jaq-std` + `jaq-json` com a nossa CLI. Casos `argv` rodam em processo sobre a
/// fixture em memória; casos `script` (pipelines em bash) rodam o mesmo código como binário multicall.
pub struct JaqOurs {
    pub label: String,
    pub scripts: SubprocessCandidate,
}

impl JaqOurs {
    pub fn new(label: &str, scratch: &std::path::Path) -> anyhow::Result<JaqOurs> {
        let exe: PathBuf = std::env::current_exe()?;
        let scripts = SubprocessCandidate::new(&format!("{label}-multicall"), &exe, &[], &["jq"], scratch)?;
        Ok(JaqOurs { label: label.to_string(), scripts })
    }
}

/// Epoch de "AAAA-MM-DD HH:MM:SS" em UTC (o ambiente dos casos usa TZ=UTC).
pub fn parse_faketime(s: &str) -> Option<f64> {
    let (date, time) = s.trim().split_once(' ')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.split(':').map(|x| x.parse::<i64>());
    let (hh, mm, ss) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    // Dias desde 1970-01-01 (algoritmo de Howard Hinnant).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((days * 86400 + hh * 3600 + mm * 60 + ss) as f64)
}

/// Roda o jq nosso em processo sobre uma invocação `argv`.
pub fn run_inprocess(inv: &Invocation, opts: cli::RunOpts) -> cli::Output {
    let files = inv.files.clone();
    let host = cli::Host {
        stdin: inv.stdin.clone(),
        read_file: Box::new(move |path| match files.get(path) {
            Some(Entry::File { data: Some(d), .. }) => Ok(d.0.clone()),
            Some(Entry::Dir { .. }) => Err("Is a directory".into()),
            Some(_) => Err("Permission denied".into()),
            None => Err("No such file or directory".into()),
        }),
        env: inv.full_env().into_iter().collect(),
        now: inv.faketime.as_deref().and_then(parse_faketime),
    };
    cli::run(inv.args(), host, opts)
}

impl Candidate for JaqOurs {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        if inv.script.is_some() {
            return self.scripts.run(inv);
        }
        if inv.program() != Some("jq") {
            return Outcome::unsupported(format!("caso de outra ferramenta: {:?}", inv.program()));
        }
        let inv2 = inv.clone();
        let handle = std::thread::Builder::new()
            .stack_size(JAQ_STACK)
            .spawn(move || run_inprocess(&inv2, cli::RunOpts::default()));
        match handle.map(|h| h.join()) {
            Ok(Ok(out)) => Outcome {
                stdout: Bytes(out.stdout),
                stderr: Bytes(out.stderr),
                exit: Some(out.exit),
                files: inv.files.clone(),
                ..Outcome::default()
            },
            Ok(Err(payload)) => {
                let msg = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic sem mensagem".into());
                Outcome::unsupported(format!("panic no jaq: {msg}"))
            }
            Err(e) => Outcome::unsupported(format!("thread: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faketime_epoch() {
        assert_eq!(parse_faketime("2026-01-15 12:00:00"), Some(1_768_478_400.0));
        assert_eq!(parse_faketime("1970-01-01 00:00:00"), Some(0.0));
    }
}
