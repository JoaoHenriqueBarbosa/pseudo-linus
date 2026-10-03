//! Laço comum das linhas de base do H40 (bashkit, rust-bash, kaish, wasmsh).
//!
//! Protocolo com o orquestrador (`src/baseline.rs` do crate principal): uma linha JSON por caso no
//! stdin (um [`harness::Case`] serializado), uma linha JSON por resultado no stdout (um
//! [`harness::Outcome`]). O orquestrador espera cada resposta com timeout; se o candidato travar ou
//! estourar memória, ele mata o processo, registra o caso como timeout e sobe outro processo pro caso
//! seguinte. Por isso o candidato roda sempre em processo separado, nunca dentro da bancada.
//!
//! O que cada linha de base precisa fazer em [`serve`]:
//!
//! - montar o FS virtual dela com a fixture do caso em [`harness::CASE_DIR`] (`/work/case`), com modo
//!   e mtime ([`harness::FIXTURE_MTIME`]) quando a API permitir;
//! - rodar o script com cwd `/work/case`, o ambiente de [`harness::Invocation::full_env`] e o stdin do
//!   caso;
//! - devolver stdout, stderr, exit e o retrato de `/work/case` depois da execução.
//!
//! O que a API do candidato não oferece vira `Outcome::unsupported` com o motivo.

use std::io::{BufRead, Write};

use harness::{Case, Invocation, Outcome};

/// Lê casos do stdin até EOF e responde um por linha.
pub fn serve(mut run: impl FnMut(&Invocation) -> Outcome) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.expect("ler caso do stdin");
        if line.trim().is_empty() {
            continue;
        }
        let outcome = match serde_json::from_str::<Case>(&line) {
            Ok(case) => match case.invocation() {
                Ok(inv) => run_guarded(&mut run, &inv),
                Err(e) => Outcome::unsupported(format!("caso inválido: {e}")),
            },
            Err(e) => Outcome::unsupported(format!("JSON de caso inválido: {e}")),
        };
        serde_json::to_writer(&mut out, &outcome).expect("escrever resultado");
        out.write_all(b"\n").expect("escrever resultado");
        out.flush().expect("flush");
    }
}

/// Um panic do candidato vira `unsupported` daquele caso, sem derrubar o processo.
fn run_guarded(run: &mut impl FnMut(&Invocation) -> Outcome, inv: &Invocation) -> Outcome {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(inv))) {
        Ok(o) => o,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic sem mensagem".into());
            Outcome::unsupported(format!("panic: {msg}"))
        }
    }
}

/// Caminho absoluto de uma entrada da fixture dentro do diretório do caso.
pub fn case_path(rel: &str) -> String {
    format!("{}/{}", harness::CASE_DIR, rel)
}

/// Converte o `faketime` de um caso ("AAAA-MM-DD HH:MM:SS", sempre UTC na bancada) em epoch.
pub fn faketime_epoch(spec: &str) -> Option<i64> {
    let spec = spec.trim();
    if let Some(rest) = spec.strip_prefix('@') {
        return rest.parse().ok();
    }
    let (date, time) = spec.split_once(' ').unwrap_or((spec, "00:00:00"));
    let d: Vec<i64> = date.split('-').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let t: Vec<i64> = time.split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    if d.len() != 3 || t.is_empty() || t.len() > 3 {
        return None;
    }
    let days = days_from_civil(d[0], d[1], d[2]);
    let secs = t[0] * 3600 + t.get(1).copied().unwrap_or(0) * 60 + t.get(2).copied().unwrap_or(0);
    Some(days * 86_400 + secs)
}

/// Dias desde 1970-01-01 no calendário gregoriano proléptico (algoritmo de Howard Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Aspas simples no estilo do shell, só quando precisa: `grep` fica `grep`, `it's` vira `'it'\''s'`.
/// Palavra simples fica sem aspas porque o kaish (que não é bash) não aceita nome de comando citado.
pub fn shell_quote(s: &str) -> String {
    let plain = !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_./=:,+@%-".contains(&b))
        && !s.starts_with('=');
    if plain { s.to_string() } else { format!("'{}'", s.replace('\'', r"'\''")) }
}

/// Script equivalente a um caso `argv` (cada argumento entre aspas simples), pra linhas de base que
/// só sabem rodar texto de shell.
pub fn argv_script(argv: &[String]) -> String {
    argv.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" ")
}

/// Texto do caso como script: o `script` quando existe, senão o argv citado.
pub fn script_of(inv: &Invocation) -> String {
    match &inv.script {
        Some(s) => s.clone(),
        None => argv_script(&inv.argv),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faketime_epoch_matches_fixture_mtime() {
        assert_eq!(faketime_epoch("2026-01-15 12:00:00"), Some(harness::FIXTURE_MTIME as i64));
        assert_eq!(faketime_epoch("1970-01-01 00:00:00"), Some(0));
        assert_eq!(faketime_epoch("@42"), Some(42));
        assert_eq!(faketime_epoch("lixo"), None);
    }

    #[test]
    fn argv_script_quotes_every_argument() {
        let argv = vec!["grep".to_string(), "it's".to_string(), "a b".to_string()];
        assert_eq!(argv_script(&argv), r"grep 'it'\''s' 'a b'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("-n"), "-n");
        assert_eq!(shell_quote("$x"), "'$x'");
    }
}
