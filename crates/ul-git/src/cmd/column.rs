//! `git column`: lê linhas do stdin e as imprime em colunas (porta do `builtin/column.c`, em cima do
//! `column.rs` que `branch`, `tag` e `status` já usam).

use super::Git;
use crate::column::{self, Options};
use crate::error::{Fail, R};
use crate::opts::{self, Spec};
use crate::os;

const SPECS: &[Spec] = &[
    opts::value(None, "command", "command"),
    opts::optional(None, "mode", "mode"),
    opts::value(None, "raw-mode", "raw-mode"),
    opts::value(None, "width", "width"),
    opts::value(None, "indent", "indent"),
    opts::value(None, "nl", "nl"),
    opts::value(None, "padding", "padding"),
];

/// O valor de uma opção inteira (`OPT_INTEGER`): só o `error:` e exit 129 quando não é número.
fn integer(name: &str, v: &[u8]) -> R<i64> {
    match std::str::from_utf8(v).ok().and_then(|s| s.trim().parse::<i64>().ok()) {
        Some(n) => Ok(n),
        None => Err(opts::error_only(&format!("option `{name}' expects a numerical value"))),
    }
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    // `--command=<nome>` só vale como primeiro argumento: é ele que escolhe o `column.<nome>`.
    let command: Option<String> = args.first().and_then(|a| a.strip_prefix(b"--command=")).map(os::lossy);
    let mut colopts = column::from_config(git.config(), command.as_deref().unwrap_or(""))?;

    let p = opts::parse(SPECS, args, 0, usage)?;
    let mut width: i64 = 0;
    let mut padding: i64 = 1;
    for h in &p.hits {
        let val = h.value.as_deref();
        match h.id {
            "mode" => column::parse_option(&mut colopts, h.negated, val)?,
            "raw-mode" => {
                let n = if h.negated { 0 } else { integer("raw-mode", val.unwrap_or(b""))? };
                colopts = n as u32;
            }
            "width" => width = if h.negated { 0 } else { integer("width", val.unwrap_or(b""))? },
            "padding" => padding = if h.negated { 0 } else { integer("padding", val.unwrap_or(b""))? },
            _ => {}
        }
    }
    if padding < 0 {
        return Err(Fail::Fatal("--padding must be non-negative".into()));
    }
    if !p.args.is_empty() {
        opts::usage_to_stderr(usage);
        return Err(Fail::Exit(129));
    }
    let real_command = p.value_str("command");
    if real_command.is_some() || command.is_some() {
        match (&real_command, &command) {
            (Some(a), Some(b)) if a == b => {}
            _ => return Err(Fail::Fatal("--command must be the first argument".into())),
        }
    }
    column::finalize(&mut colopts);

    // `strbuf_getline`: tira o `\n` e o `\r` antes dele.
    let data = os::stdin_all();
    let mut list: Vec<Vec<u8>> = Vec::new();
    let mut rest: &[u8] = &data;
    while !rest.is_empty() {
        let (line, next) = match rest.iter().position(|c| *c == b'\n') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, &rest[rest.len()..]),
        };
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        list.push(line.to_vec());
        rest = next;
    }

    let indent = p.value_str("indent").unwrap_or_default();
    let nl: Vec<u8> = match p.value("nl") {
        Some(v) => v.to_vec(),
        None => b"\n".to_vec(),
    };
    let o = Options { width: width.max(0) as usize, padding: padding as usize, indent: &indent };
    os::out(&column::print_columns_nl(&list, colopts, &o, &nl));
    Ok(0)
}
