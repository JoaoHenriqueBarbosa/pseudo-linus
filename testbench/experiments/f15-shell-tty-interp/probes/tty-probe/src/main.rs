//! Sonda do H38: chama `reedline` e `rustyline` do jeito documentado, sem nenhum tty do host.
//!
//! O orquestrador roda este binário com `setsid` (sem terminal de controle), stdin num pipe com uma
//! linha digitada e sob `strace`. Se a biblioteca aceitasse I/O próprio, não haveria como ela tocar
//! o host; o que aparece no trace (abrir `/dev/tty`, `ioctl` de termios no fd 0 ou 1, `read` direto
//! do fd 0) é a prova por teste de que ela fala com o tty do host.
//!
//! Uso: `f15-tty-probe reedline|rustyline|rustyline-preferterm`. Imprime uma linha JSON no stderr
//! (o stdout fica livre pro que a biblioteca quiser escrever).

use std::io::Write;

/// Marcadores pro strace (stat em caminho inexistente), como no protocolo dos interpretadores.
fn marker(path: &str) {
    let _ = std::fs::metadata(path);
}

fn report(mode: &str, outcome: &str, detail: &str) {
    let line = format!(
        "{{\"mode\":{},\"outcome\":{},\"detail\":{}}}\n",
        json_str(mode),
        json_str(outcome),
        json_str(detail)
    );
    let _ = std::io::stderr().write_all(line.as_bytes());
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn probe_reedline() {
    let mut editor = reedline::Reedline::create();
    let prompt = reedline::DefaultPrompt::default();
    marker("/f15-strace-marker-begin");
    let r = editor.read_line(&prompt);
    marker("/f15-strace-marker-end");
    match r {
        Ok(sig) => report("reedline", "ok", &format!("{sig:?}")),
        Err(e) => report("reedline", "error", &e.to_string()),
    }
}

fn probe_rustyline(prefer_term: bool) {
    let mode = if prefer_term { "rustyline-preferterm" } else { "rustyline" };
    let config = if prefer_term {
        rustyline::Config::builder().behavior(rustyline::Behavior::PreferTerm).build()
    } else {
        rustyline::Config::default()
    };
    marker("/f15-strace-marker-begin");
    let editor = rustyline::DefaultEditor::with_config(config);
    let r = editor.and_then(|mut ed| ed.readline("$ "));
    marker("/f15-strace-marker-end");
    match r {
        Ok(line) => report(mode, "ok", &line),
        Err(e) => report(mode, "error", &e.to_string()),
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("reedline") => probe_reedline(),
        Some("rustyline") => probe_rustyline(false),
        Some("rustyline-preferterm") => probe_rustyline(true),
        other => {
            report("?", "usage", &format!("modo desconhecido: {other:?}"));
            std::process::exit(2);
        }
    }
}
