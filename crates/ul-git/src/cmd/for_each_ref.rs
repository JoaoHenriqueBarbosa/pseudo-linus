//! `git for-each-ref`: lista as refs com `--format`, `--sort`, `--count`, padrões e os filtros
//! `--contains`, `--merged` e `--points-at` (o motor está em `reffmt`).

use super::Git;
use super::reffmt::{self, Ctx, Filter, Quote};
use crate::error::{Fail, R};
use crate::opts::{self, Spec};
use crate::os;

const SPECS: &[Spec] = &[
    opts::flag(Some(b's'), "shell", "shell"),
    opts::flag(Some(b'p'), "perl", "perl"),
    opts::flag(None, "python", "python"),
    opts::flag(None, "tcl", "tcl"),
    opts::flag(None, "omit-empty", "omit-empty"),
    opts::value(None, "count", "count"),
    opts::value(None, "format", "format"),
    opts::optional(None, "color", "color"),
    opts::value(None, "exclude", "exclude"),
    opts::value(None, "sort", "sort"),
    opts::value(None, "points-at", "points-at"),
    opts::noneg(opts::value(None, "merged", "merged")),
    opts::noneg(opts::value(None, "no-merged", "no-merged")),
    opts::noneg(opts::value(None, "contains", "contains")),
    opts::noneg(opts::value(None, "no-contains", "no-contains")),
    opts::flag(None, "ignore-case", "ignore-case"),
    opts::flag(None, "stdin", "stdin"),
    opts::flag(None, "include-root-refs", "include-root-refs"),
];

const DEFAULT_FORMAT: &str = "%(objectname) %(objecttype)\t%(refname)";

/// Modo de cor pedido: `--color` e `--color=always` ligam; `never` e `auto` (sem terminal) não.
pub fn color_enabled(p: &opts::Parsed) -> R<bool> {
    if !p.present("color") {
        return Ok(false);
    }
    match p.value("color") {
        None => Ok(!matches!(p.flag("color"), Some(false))),
        Some(v) => match v {
            b"always" | b"true" | b"yes" | b"on" | b"1" => Ok(true),
            b"never" | b"false" | b"no" | b"off" | b"0" | b"auto" | b"tty" => Ok(false),
            other => Err(Fail::Fatal(format!("bad boolean config value '{}' for '--color'", os::lossy(other)))),
        },
    }
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let args = reffmt::lastarg_default(args);
    let p = opts::parse(SPECS, &args, 0, usage)?;
    let repo = git.repo()?;
    let mut filter = Filter { match_as_path: true, ..Filter::default() };
    reffmt::apply_filter_opts(repo, &p, &mut filter)?;
    filter.include_root = p.has("include-root-refs");
    filter.patterns = p.args.iter().map(|a| os::lossy(a)).collect();
    if p.has("stdin") {
        let data = os::stdin_all();
        for line in data.split(|c| *c == b'\n').filter(|l| !l.is_empty()) {
            filter.patterns.push(os::lossy(line));
        }
    }
    filter.exclude = p.values("exclude").iter().map(|a| os::lossy(a)).collect();

    let quote = if p.has("shell") {
        Quote::Shell
    } else if p.has("perl") {
        Quote::Perl
    } else if p.has("python") {
        Quote::Python
    } else if p.has("tcl") {
        Quote::Tcl
    } else {
        Quote::None
    };
    let count: usize = match p.value("count") {
        None => 0,
        Some(v) => match os::lossy(v).parse::<i64>() {
            Ok(n) if n >= 0 => n as usize,
            _ => return Err(Fail::Fatal(format!("invalid --count argument: `{}'", os::lossy(v)))),
        },
    };
    let fmt_text = p.value("format").map(|v| v.to_vec()).unwrap_or_else(|| DEFAULT_FORMAT.as_bytes().to_vec());
    let nodes = reffmt::parse_format(&fmt_text, usage)?;
    let mut keys = Vec::new();
    for s in p.values("sort") {
        keys.push(reffmt::parse_sort_key(&os::lossy(&s))?);
    }

    let mut ctx = Ctx::new(repo)?;
    ctx.quote = quote;
    ctx.color = color_enabled(&p)?;
    let rows = reffmt::collect(&mut ctx, &filter)?;
    let rows = reffmt::sort_rows(&mut ctx, rows, &keys, filter.ignore_case)?;
    let omit_empty = p.has("omit-empty");
    let mut out: Vec<u8> = Vec::new();
    for (n, row) in rows.iter().enumerate() {
        if count != 0 && n >= count {
            break;
        }
        let mut line = Vec::new();
        ctx.render(&nodes, row, &mut line)?;
        if omit_empty && line.is_empty() {
            continue;
        }
        out.extend_from_slice(&line);
        out.push(b'\n');
    }
    os::out(&out);
    Ok(0)
}
