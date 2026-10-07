//! `diff3` (GNU diffutils 3.10): compara MEU, VELHO e SEU.
//!
//! Como o GNU, calcula duas comparações de duas vias contra o arquivo comum (MEU contra VELHO e SEU
//! contra VELHO, com `--horizon-lines=100`), aqui com o nosso motor em processo, e junta os blocos que
//! se sobrepõem ou encostam no arquivo comum em blocos de três vias: só MEU mudou (`====1`), só SEU
//! (`====3`), os dois mudaram igual (`====2`) ou diferente (`====`). Saídas: o formato legível padrão,
//! os scripts de ed (`-e`, `-E`, `-x`, `-X`, `-3`, `-A`, com `-i`) e o arquivo fundido (`-m`), com os
//! marcadores de conflito, o tratamento de linhas que começam com ponto, a falta de newline final, os
//! rótulos (`-L`), as mensagens e os códigos de saída observados no oráculo.

use std::ffi::OsString;

use sysabi::{Ctx, Errno, Fd};
use ul_common::getopt::{Getopt, HasArg, Item, LongOpt};

use crate::diff::engine;
use crate::diff::format::build_script;
use crate::diff::text::{self, Normalize};
use crate::sysutil::{self, Output};

const HELP: &str = r#"Usage: diff3 [OPTION]... MYFILE OLDFILE YOURFILE
Compare three files line by line.

Mandatory arguments to long options are mandatory for short options too.
  -A, --show-all              output all changes, bracketing conflicts

  -e, --ed                    output ed script incorporating changes
                                from OLDFILE to YOURFILE into MYFILE
  -E, --show-overlap          like -e, but bracket conflicts
  -3, --easy-only             like -e, but incorporate only nonoverlapping changes
  -x, --overlap-only          like -e, but incorporate only overlapping changes
  -X                          like -x, but bracket conflicts
  -i                          append 'w' and 'q' commands to ed scripts

  -m, --merge                 output actual merged file, according to
                                -A if no other options are given

  -a, --text                  treat all files as text
      --strip-trailing-cr     strip trailing carriage return on input
  -T, --initial-tab           make tabs line up by prepending a tab
      --diff-program=PROGRAM  use PROGRAM to compare files
  -L, --label=LABEL           use LABEL instead of file name
                                (can be repeated up to three times)

      --help                  display this help and exit
  -v, --version               output version information and exit

The default output format is a somewhat human-readable representation of
the changes.

The -e, -E, -x, -X (and corresponding long) options cause an ed script
to be output instead of the default.

Finally, the -m (--merge) option causes diff3 to do the merge internally
and output the actual merged file.  For unusual input, this is more
robust than using ed.

If a FILE is '-', read standard input.
Exit status is 0 if successful, 1 if conflicts, 2 if trouble.

Report bugs to: bug-diffutils@gnu.org
GNU diffutils home page: <https://www.gnu.org/software/diffutils/>
General help using GNU software: <https://www.gnu.org/gethelp/>
"#;

const VERSION: &str = "diff3 (GNU diffutils) 3.10
Copyright (C) 2023 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>.
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

Written by Randy Smith.
";

const DIFF_PROGRAM: i32 = 1000;
const HELP_ID: i32 = 1001;
const STRIP_TRAILING_CR: i32 = 1002;

const LONGS: &[LongOpt] = &[
    LongOpt::new("diff-program", HasArg::Required, DIFF_PROGRAM),
    LongOpt::new("easy-only", HasArg::No, b'3' as i32),
    LongOpt::new("ed", HasArg::No, b'e' as i32),
    LongOpt::new("help", HasArg::No, HELP_ID),
    LongOpt::new("initial-tab", HasArg::No, b'T' as i32),
    LongOpt::new("label", HasArg::Required, b'L' as i32),
    LongOpt::new("merge", HasArg::No, b'm' as i32),
    LongOpt::new("overlap-only", HasArg::No, b'x' as i32),
    LongOpt::new("show-all", HasArg::No, b'A' as i32),
    LongOpt::new("show-overlap", HasArg::No, b'E' as i32),
    LongOpt::new("strip-trailing-cr", HasArg::No, STRIP_TRAILING_CR),
    LongOpt::new("text", HasArg::No, b'a' as i32),
    LongOpt::new("version", HasArg::No, b'v' as i32),
];

/// Quais blocos o script de ed (ou a fusão) inclui e como.
#[derive(Clone, Copy, Debug, Default)]
struct Selection {
    /// Inclui os blocos em que os dois mudaram igual (`-A`), como conflito.
    show_2nd: bool,
    /// Só os blocos em conflito (`-x`, `-X`).
    overlap_only: bool,
    /// Só os blocos em que só o SEU mudou (`-3`).
    simple_only: bool,
    /// Marca os conflitos (`-E`, `-A`).
    flagging: bool,
}

/// Bloco de duas vias de uma comparação OUTRO contra COMUM, com faixas 1-based inclusivas (faixa
/// vazia: `lo == hi + 1`).
#[derive(Clone, Copy, Debug)]
struct Block2 {
    lo_c: i64,
    hi_c: i64,
    lo_o: i64,
    hi_o: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Os três diferem.
    All,
    /// Só o MEU mudou.
    Mine,
    /// Os dois mudaram igual: o VELHO é o diferente.
    Old,
    /// Só o SEU mudou.
    Yours,
}

/// Bloco de três vias: faixas no MEU, no VELHO e no SEU.
#[derive(Clone, Copy, Debug)]
struct Block3 {
    kind: Kind,
    /// (lo, hi) por arquivo, na ordem MEU, VELHO, SEU.
    range: [(i64, i64); 3],
}

impl Block3 {
    fn count(&self, f: usize) -> usize {
        (self.range[f].1 - self.range[f].0 + 1).max(0) as usize
    }
}

struct Input {
    lines: Vec<Vec<u8>>,
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv = sysutil::args_bytes(args);
    let argv0 = sysutil::argv0(&argv);
    let try_help = |msg: &str| -> i32 {
        sysutil::eprint(format!("{argv0}: {msg}\n{argv0}: Try '{argv0} --help' for more information.\n"));
        2
    };
    let mut text_mode = false;
    let mut strip_cr = false;
    let mut initial_tab = false;
    let mut merge = false;
    let mut finalwrite = false;
    let mut labels: Vec<Vec<u8>> = Vec::new();
    let mut kinds: Vec<u8> = Vec::new();
    let mut diff_program: Option<Vec<u8>> = None;
    let mut operands = Vec::new();
    for item in Getopt::from_env(&argv, "aeimvx3AEL:TX", LONGS).after_argv0() {
        let opt = match item {
            Ok(Item::Operand(v)) => {
                operands.push(v);
                continue;
            }
            Ok(Item::Opt(o)) => o,
            Err(e) => {
                sysutil::eprint(e.message_line(&argv0));
                sysutil::eprint(format!("{argv0}: Try '{argv0} --help' for more information.\n"));
                return 2;
            }
        };
        match opt.id {
            x if x == b'a' as i32 => text_mode = true,
            x if x == b'i' as i32 => finalwrite = true,
            x if x == b'm' as i32 => merge = true,
            x if x == b'T' as i32 => initial_tab = true,
            x if b"eExX3A".iter().any(|c| *c as i32 == x) => {
                let k = x as u8;
                if !kinds.contains(&k) {
                    kinds.push(k);
                }
            }
            x if x == b'L' as i32 => {
                if labels.len() >= 3 {
                    return try_help("too many file label options");
                }
                labels.push(opt.arg.clone().unwrap_or_default());
            }
            x if x == b'v' as i32 => {
                let mut out = Output::stdout();
                out.write_str(VERSION);
                return if out.finish().is_ok() { 0 } else { 2 };
            }
            HELP_ID => {
                let mut out = Output::stdout();
                out.write_str(HELP);
                return if out.finish().is_ok() { 0 } else { 2 };
            }
            DIFF_PROGRAM => diff_program = opt.arg.clone(),
            STRIP_TRAILING_CR => strip_cr = true,
            _ => {}
        }
    }
    if kinds.len() > 1 || (merge && finalwrite) {
        return try_help("incompatible options");
    }
    if operands.len() < 3 {
        let last = operands.last().map(|o| String::from_utf8_lossy(o).into_owned()).unwrap_or_else(|| argv0.clone());
        return try_help(&format!("missing operand after '{last}'"));
    }
    if operands.len() > 3 {
        return try_help(&format!("extra operand '{}'", String::from_utf8_lossy(&operands[3])));
    }
    let edscript = !kinds.is_empty();
    let kind = kinds.first().copied();
    let sel = match (kind, merge) {
        (Some(b'A'), _) | (None, true) => Selection { show_2nd: true, flagging: true, ..Selection::default() },
        (Some(b'E'), _) => Selection { flagging: true, ..Selection::default() },
        (Some(b'x'), _) | (Some(b'X'), _) => Selection { overlap_only: true, ..Selection::default() },
        (Some(b'3'), _) => Selection { simple_only: true, ..Selection::default() },
        _ => Selection::default(),
    };
    let _ = diff_program;
    let names = [operands[0].clone(), operands[1].clone(), operands[2].clone()];
    let label = |k: usize| labels.get(k).cloned().unwrap_or_else(|| names[k].clone());
    let marks = [label(0), label(1), label(2)];
    let mut out = Output::stdout();

    // Lê os três arquivos (o `-` uma vez só).
    let mut stdin_data: Option<Vec<u8>> = None;
    let mut raw: Vec<Option<Vec<u8>>> = vec![None, None, None];
    let mut errs: Vec<Option<Errno>> = vec![None, None, None];
    for k in 0..3 {
        let r = if names[k] == b"-" {
            if stdin_data.is_none() {
                stdin_data = Some(sysutil::read_fd(Fd::STDIN).unwrap_or_default());
            }
            Ok(stdin_data.clone().unwrap_or_default())
        } else {
            sysutil::read_path(&names[k])
        };
        match r {
            Ok(mut d) => {
                if strip_cr {
                    d = text::strip_trailing_cr(&d);
                }
                raw[k] = Some(d);
            }
            Err(e) => errs[k] = Some(e),
        }
    }
    // Como o GNU: primeiro `diff VELHO SEU`, depois `diff MEU VELHO` (a direção muda o desempate do
    // alinhamento e a ordem das mensagens de erro).
    let pairs = [(1usize, 2usize), (0usize, 1usize)];
    let mut threads: [Vec<Block2>; 2] = [Vec::new(), Vec::new()];
    let mut incomplete_markers = 0usize;
    for &(first, second) in &pairs {
        let mut failed = false;
        for k in [first, second] {
            if let Some(e) = errs[k] {
                let mut m = b"diff: ".to_vec();
                m.extend_from_slice(&names[k]);
                m.extend_from_slice(b": ");
                m.extend_from_slice(e.message().as_bytes());
                m.push(b'\n');
                sysutil::eprint(m);
                failed = true;
            }
        }
        if failed {
            sysutil::eprint(format!("{argv0}: subsidiary program 'diff' failed (exit status 2)\n"));
            return 2;
        }
        let a = raw[first].as_deref().unwrap_or_default();
        let b = raw[second].as_deref().unwrap_or_default();
        let blk = |d: &[u8]| d[..d.len().min(4096)].contains(&0);
        if !text_mode && a != b && (blk(a) || blk(b)) {
            sysutil::eprint(
                [
                    format!("{argv0}: diff failed: Binary files ").as_bytes(),
                    &names[first],
                    b" and ",
                    &names[second],
                    b" differ\n",
                ]
                .concat(),
            );
            return 2;
        }
        // O comum é o VELHO (índice 1): o primeiro na comparação com o SEU, o segundo na com o MEU.
        let common_first = first == 1;
        let (blocks, markers) = two_way(a, b, common_first);
        incomplete_markers += markers;
        threads[if common_first { 1 } else { 0 }] = blocks;
    }
    let files: Vec<Input> = (0..3)
        .map(|k| Input {
            lines: text::split_lines(raw[k].as_deref().unwrap_or_default()).into_iter().map(|l| l.to_vec()).collect(),
        })
        .collect();
    let blocks = three_way(&threads[0], &threads[1], &files);

    let mut conflicts = false;
    if merge {
        conflicts = output_merge(&mut out, &blocks, &files, sel, &marks);
    } else if edscript {
        for _ in 0..incomplete_markers {
            out.flush();
            sysutil::eprint(format!("{argv0}: No newline at end of file\n"));
        }
        conflicts = output_edscript(&mut out, &blocks, &files, sel, &marks);
        if finalwrite {
            out.write(b"w\nq\n");
        }
    } else {
        output_normal(&mut out, &blocks, &files, initial_tab);
    }
    match out.finish() {
        Ok(()) => conflicts as i32,
        Err(e) => {
            sysutil::error(&argv0, format!("write failed: {}", e.message()));
            2
        }
    }
}

/// Comparação `diff --horizon-lines=100 a b` com o motor do diff; `common_first` diz se o arquivo comum
/// é o `a`. Devolve os blocos (faixas no comum e no outro) e quantos avisos "\ No newline at end of
/// file" a saída normal teria.
fn two_way(a: &[u8], b: &[u8], common_first: bool) -> (Vec<Block2>, usize) {
    let la = text::split_lines(a);
    let lb = text::split_lines(b);
    let norm = Normalize::default();
    let it = text::intern(&la, &lb, &norm);
    if it.a == it.b {
        return (Vec::new(), 0);
    }
    let opts = engine::Options { minimal: false, speed_large_files: false, horizon: 100 };
    let al = engine::compare(&la, &lb, &it.a, &it.b, it.classes, opts);
    let mut markers = 0;
    if la.last().is_some_and(|l| !l.ends_with(b"\n")) && al.changed_a.last() == Some(&true) {
        markers += 1;
    }
    if lb.last().is_some_and(|l| !l.ends_with(b"\n")) && al.changed_b.last() == Some(&true) {
        markers += 1;
    }
    let blocks = build_script(&al.changed_a, &al.changed_b)
        .into_iter()
        .map(|c| {
            let (lo_a, hi_a) = (c.line0 as i64 + 1, (c.line0 + c.deleted) as i64);
            let (lo_b, hi_b) = (c.line1 as i64 + 1, (c.line1 + c.inserted) as i64);
            if common_first {
                Block2 { lo_c: lo_a, hi_c: hi_a, lo_o: lo_b, hi_o: hi_b }
            } else {
                Block2 { lo_o: lo_a, hi_o: hi_a, lo_c: lo_b, hi_c: hi_b }
            }
        })
        .collect();
    (blocks, markers)
}

/// Junta os blocos das duas comparações (`mine` = MEU contra VELHO, `yours` = SEU contra VELHO) em
/// blocos de três vias. Blocos que se sobrepõem ou encostam no VELHO vão pro mesmo bloco.
fn three_way(mine: &[Block2], yours: &[Block2], files: &[Input]) -> Vec<Block3> {
    let threads = [mine, yours];
    let mut next = [0usize, 0usize];
    let mut out = Vec::new();
    // Último bloco de três vias, pra mapear as faixas de quem não tem bloco: (hi do VELHO, hi do MEU,
    // hi do SEU).
    let mut last = (0i64, 0i64, 0i64);
    loop {
        let has = [next[0] < threads[0].len(), next[1] < threads[1].len()];
        if !has[0] && !has[1] {
            break;
        }
        let base = if !has[0] {
            1
        } else if !has[1] {
            0
        } else {
            (threads[0][next[0]].lo_c > threads[1][next[1]].lo_c) as usize
        };
        let mut using: [Vec<Block2>; 2] = [Vec::new(), Vec::new()];
        let mut high_thread = base;
        let first = threads[base][next[base]];
        let mut high_mark = first.hi_c;
        using[base].push(first);
        next[base] += 1;
        let mut other = high_thread ^ 1;
        while next[other] < threads[other].len() && threads[other][next[other]].lo_c <= high_mark + 1 {
            let b = threads[other][next[other]];
            using[other].push(b);
            next[other] += 1;
            if high_mark < b.hi_c {
                high_thread ^= 1;
                high_mark = b.hi_c;
            }
            other = high_thread ^ 1;
        }
        let lowc = using[base][0].lo_c;
        let highc = using[high_thread].last().expect("bloco").hi_c;
        let mut ranges = [(0i64, 0i64); 3];
        ranges[1] = (lowc, highc);
        for (t, file_index) in [(0usize, 0usize), (1, 2)] {
            ranges[file_index] = match (using[t].first(), using[t].last()) {
                (Some(f), Some(l)) => (lowc - f.lo_c + f.lo_o, highc - l.hi_c + l.hi_o),
                _ => {
                    let prev_hi = if t == 0 { last.1 } else { last.2 };
                    (lowc - last.0 + prev_hi, highc - last.0 + prev_hi)
                }
            };
        }
        let kind = if using[0].is_empty() {
            Kind::Yours
        } else if using[1].is_empty() {
            Kind::Mine
        } else {
            let m = slice(&files[0], ranges[0]);
            let y = slice(&files[2], ranges[2]);
            if m == y { Kind::Old } else { Kind::All }
        };
        last = (ranges[1].1, ranges[0].1, ranges[2].1);
        out.push(Block3 { kind, range: ranges });
        sysabi::sys::checkpoint();
    }
    out
}

fn slice(f: &Input, (lo, hi): (i64, i64)) -> &[Vec<u8>] {
    if hi < lo {
        return &[];
    }
    let lo = (lo - 1).max(0) as usize;
    let hi = (hi.max(0) as usize).min(f.lines.len());
    &f.lines[lo.min(hi)..hi]
}

/// Formato padrão: `====X`, `N:faixa(a|c)` e as linhas com dois espaços (ou tab com `-T`).
fn output_normal(out: &mut Output, blocks: &[Block3], files: &[Input], initial_tab: bool) {
    let prefix: &[u8] = if initial_tab { b"\t" } else { b"  " };
    for b in blocks {
        // Arquivo cujo texto não se imprime (é igual ao de outro) e a ordem de impressão.
        let (tag, skip, order): (&str, Option<usize>, [usize; 3]) = match b.kind {
            Kind::All => ("", None, [0, 1, 2]),
            Kind::Mine => ("1", Some(1), [0, 1, 2]),
            Kind::Old => ("2", Some(0), [0, 2, 1]),
            Kind::Yours => ("3", Some(0), [0, 1, 2]),
        };
        out.write_str(&format!("===={tag}\n"));
        for f in order {
            let (lo, hi) = b.range[f];
            let head = match lo - hi {
                1 => format!("{}:{}a\n", f + 1, lo - 1),
                0 => format!("{}:{}c\n", f + 1, lo),
                _ => format!("{}:{},{}c\n", f + 1, lo, hi),
            };
            out.write_str(&head);
            if skip == Some(f) {
                continue;
            }
            let lines = slice(&files[f], b.range[f]);
            for l in lines {
                out.write(prefix);
                out.write(l);
            }
            if lines.last().is_some_and(|l| !l.ends_with(b"\n")) {
                out.write(b"\n\\ No newline at end of file\n");
            }
        }
    }
}

/// Decide se o bloco entra na saída e se é conflito.
fn selected(b: &Block3, sel: Selection) -> Option<bool> {
    match b.kind {
        Kind::Old => sel.show_2nd.then_some(true),
        Kind::Yours => (!sel.overlap_only).then_some(false),
        Kind::All => (!sel.simple_only).then_some(sel.flagging),
        Kind::Mine => None,
    }
}

/// Linhas de um bloco num script de ed: com newline sempre, e com um ponto a mais nas que começam com
/// ponto. Devolve se houve alguma assim.
fn dot_lines(out: &mut Output, lines: &[Vec<u8>]) -> bool {
    let mut leading = false;
    for l in lines {
        if l.first() == Some(&b'.') {
            leading = true;
            out.write(b".");
        }
        out.write(l);
        if !l.ends_with(b"\n") {
            out.write(b"\n");
        }
    }
    leading
}

/// Fecha a entrada do ed e, se houve linha com ponto extra, tira o ponto nas linhas `start..`.
fn undot(out: &mut Output, leading: bool, start: i64, num: i64) {
    out.write(b".\n");
    if leading {
        if num == 1 {
            out.write_str(&format!("{start}s/^\\.//\n"));
        } else {
            out.write_str(&format!("{start},{}s/^\\.//\n", start + num - 1));
        }
    }
}

fn output_edscript(out: &mut Output, blocks: &[Block3], files: &[Input], sel: Selection, marks: &[Vec<u8>; 3]) -> bool {
    let mut conflicts = false;
    for b in blocks.iter().rev() {
        let Some(conflict) = selected(b, sel) else { continue };
        let (low0, high0) = b.range[0];
        let old = slice(&files[1], b.range[1]);
        let yours = slice(&files[2], b.range[2]);
        if conflict {
            conflicts = true;
            out.write_str(&format!("{high0}a\n"));
            let mut leading = false;
            if b.kind == Kind::All {
                if sel.show_2nd {
                    marker(out, b"||||||| ", &marks[1]);
                    leading = dot_lines(out, old);
                }
                out.write(b"=======\n");
                leading |= dot_lines(out, yours);
            }
            marker(out, b">>>>>>> ", &marks[2]);
            undot(out, leading, high0 + 2, b.count(1) as i64 + b.count(2) as i64 + 1);
            let who = if b.kind == Kind::All { &marks[0] } else { &marks[1] };
            out.write_str(&format!("{}a\n", low0 - 1));
            marker(out, b"<<<<<<< ", who);
            let mut leading = false;
            if b.kind == Kind::Old {
                leading = dot_lines(out, old);
                out.write(b"=======\n");
            }
            undot(out, leading, low0 + 1, b.count(1) as i64);
        } else if yours.is_empty() {
            if low0 == high0 {
                out.write_str(&format!("{low0}d\n"));
            } else {
                out.write_str(&format!("{low0},{high0}d\n"));
            }
        } else {
            match high0 - low0 {
                -1 => out.write_str(&format!("{high0}a\n")),
                0 => out.write_str(&format!("{high0}c\n")),
                _ => out.write_str(&format!("{low0},{high0}c\n")),
            }
            let leading = dot_lines(out, yours);
            undot(out, leading, low0, yours.len() as i64);
        }
    }
    conflicts
}

/// A linha de marcador de um conflito: `<<<<<<< nome`, `||||||| nome` ou `>>>>>>> nome`.
fn marker(out: &mut Output, head: &[u8], mark: &[u8]) {
    out.write(head);
    out.write(mark);
    out.write(b"\n");
}

fn write_lines(out: &mut Output, lines: &[Vec<u8>]) {
    for l in lines {
        out.write(l);
    }
}

fn output_merge(out: &mut Output, blocks: &[Block3], files: &[Input], sel: Selection, marks: &[Vec<u8>; 3]) -> bool {
    let mut conflicts = false;
    let mine = &files[0].lines;
    let mut read = 0usize;
    for b in blocks {
        let Some(conflict) = selected(b, sel) else { continue };
        let (low0, _) = b.range[0];
        let upto = ((low0 - 1).max(0) as usize).min(mine.len());
        write_lines(out, &mine[read.min(upto)..upto]);
        read = read.max(upto);
        if conflict {
            conflicts = true;
            if b.kind == Kind::All {
                marker(out, b"<<<<<<< ", &marks[0]);
                write_lines(out, slice(&files[0], b.range[0]));
            }
            if sel.show_2nd {
                let head: &[u8] = if b.kind == Kind::All { b"||||||| " } else { b"<<<<<<< " };
                marker(out, head, &marks[1]);
                write_lines(out, slice(&files[1], b.range[1]));
            }
            out.write(b"=======\n");
        }
        write_lines(out, slice(&files[2], b.range[2]));
        if conflict {
            marker(out, b">>>>>>> ", &marks[2]);
        }
        read += b.count(0);
    }
    write_lines(out, &mine[read.min(mine.len())..]);
    conflicts
}
