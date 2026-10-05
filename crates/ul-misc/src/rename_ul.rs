//! `rename.ul` do util-linux 2.41 (o `rename` do util-linux no Debian 13): troca a primeira (ou todas,
//! ou a última) ocorrência de uma cadeia no nome de cada arquivo, ou no alvo de links simbólicos com
//! `-s`.
//!
//! Porte direto do `misc-utils/rename.c`, inclusive os detalhes que saem na tela: a busca só olha o
//! último componente do caminho quando nem a expressão nem o substituto têm `/`, o `ul_basename` corta
//! as barras finais do nome (e o `rename(2)` usa o nome já cortado), `-n -s` sem ocorrência escreve
//! `(null)` como o `printf` da glibc, e a saída é 0 (alguma troca), 1 (erros), 2 (troca parcial), 4
//! (nada a fazer) ou 64.
//!
//! Com `-i` a pergunta lê o stdin; sem terminal (o único caso aqui) só a primeira letra da linha
//! conta, e `y` ou `Y` confirma.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{AtFlags, Ctx, Errno, Fd, FileType, RenameFlags, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const RENAME_EXIT_SOMEOK: i32 = 2;
const RENAME_EXIT_NOTHING: i32 = 4;
const RENAME_EXIT_UNEXPLAINED: i32 = 64;

const LONGS: &[LongOpt] = &[
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("all", HasArg::No, b'a' as i32),
    LongOpt::new("last", HasArg::No, b'l' as i32),
    LongOpt::new("no-act", HasArg::No, b'n' as i32),
    LongOpt::new("no-overwrite", HasArg::No, b'o' as i32),
    LongOpt::new("interactive", HasArg::No, b'i' as i32),
    LongOpt::new("symlink", HasArg::No, b's' as i32),
];

fn usage_text(short: &str) -> String {
    format!(
        "\nUsage:\n {short} [options] <expression> <replacement> <file>...\n\nRename files.\n\nOptions:\n \
-v, --verbose       explain what is being done\n \
-s, --symlink       act on the target of symlinks\n \
-n, --no-act        do not make any changes\n \
-a, --all           replace all occurrences\n \
-l, --last          replace only the last occurrence\n \
-o, --no-overwrite  don't overwrite existing files\n \
-i, --interactive   prompt before overwrite\n\
\n \
-h, --help          display this help\n \
-V, --version       display version\n\nFor more details see rename(1).\n"
    )
}

/// As opções que mudam o comportamento.
#[derive(Copy, Clone)]
struct Opts {
    all: bool,
    last: bool,
    verbose: bool,
    noact: bool,
    nooverwrite: bool,
    interactive: bool,
}

/// `strstr(&hay[start..], needle)`: o índice absoluto da primeira ocorrência, se houver.
fn find_from(hay: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if start > hay.len() {
        return None;
    }
    if needle.is_empty() {
        return Some(start);
    }
    hay[start..].windows(needle.len()).position(|w| w == needle).map(|p| p + start)
}

/// `ul_basename`: o índice onde começa o último componente; corta as barras finais de `path`.
fn ul_basename(path: &mut Vec<u8>) -> usize {
    if path.is_empty() {
        return 0;
    }
    let Some(mut p) = path.iter().rposition(|&b| b == b'/') else { return 0 };
    if p + 1 < path.len() {
        return p + 1;
    }
    while p > 0 && path[p - 1] == b'/' {
        p -= 1;
    }
    if p > 0 {
        path.truncate(p);
        p -= 1;
        while p > 0 && path[p - 1] != b'/' {
            p -= 1;
        }
    } else {
        while p + 1 < path.len() {
            p += 1;
        }
    }
    p
}

/// `string_replace`: o novo nome, ou `None` quando não há o que trocar. `orig` pode perder as barras
/// finais (efeito colateral do `ul_basename`).
fn string_replace(o: &Opts, from: &[u8], to: &[u8], orig: &mut Vec<u8>) -> Option<Vec<u8>> {
    let fromlen = from.len();
    let mut search_start = 0;
    if !from.contains(&b'/') && !to.contains(&b'/') {
        // Só o último componente; o '/' final não entra (from vazio casa depois dele).
        search_start = ul_basename(orig);
    }
    let first = find_from(orig, from, search_start)?;
    let mut where_ = first;
    let mut count = 1usize;
    let mut p = Some(first);
    while (o.all || o.last) && p.is_some_and(|i| i < orig.len()) {
        let cur = p.unwrap_or(0);
        let step = if o.last { 1 } else { fromlen.max(1) };
        p = find_from(orig, from, cur + step);
        if let Some(i) = p {
            if o.all {
                count += 1;
            }
            if o.last {
                where_ = i;
            }
        }
    }

    let mut out: Vec<u8> = Vec::with_capacity(orig.len() + to.len());
    let mut p = 0usize;
    let mut w = Some(where_);
    for _ in 0..count {
        let at = w.unwrap_or(orig.len());
        while p < at {
            out.push(orig[p]);
            p += 1;
        }
        out.extend_from_slice(to);
        if fromlen > 0 {
            p = at + fromlen;
            w = find_from(orig, from, p);
        } else {
            p = at;
            w = Some(at + 1);
        }
    }
    out.extend_from_slice(&orig[p.min(orig.len())..]);
    Some(out)
}

/// `ask`: a pergunta de `-i`. `true` quando a resposta é sim.
fn ask(short: &str, name: &[u8]) -> bool {
    let mut out = io::stdout();
    let _ = out.write_all(format!("{short}: overwrite `{}'? ", io::lossy(name)).as_bytes());
    let _ = out.flush();
    let mut byte = [0u8; 1];
    let first = read_byte(&mut byte);
    let answer = match first {
        None => {
            let _ = out.write_all(b"n\n");
            b'n'
        }
        Some(c) => {
            if c != b'\n' {
                // descarta o resto da linha
                while let Some(x) = read_byte(&mut byte) {
                    if x == b'\n' {
                        break;
                    }
                }
            }
            c
        }
    };
    answer == b'y' || answer == b'Y'
}

fn read_byte(buf: &mut [u8; 1]) -> Option<u8> {
    loop {
        match sys::read(Fd::STDIN, buf) {
            Ok(0) => return None,
            Ok(_) => return Some(buf[0]),
            Err(Errno::EINTR) => continue,
            Err(_) => return None,
        }
    }
}

fn not_accessible(short: &str, s: &[u8], e: Errno) {
    ul::warn(short, format!("{}: not accessible", io::lossy(s)), e);
}

fn do_symlink(o: &Opts, short: &str, from: &[u8], to: &[u8], s: &[u8]) -> i32 {
    let mut nooverwrite = o.nooverwrite;
    let mut interactive = o.interactive;
    let st = match sys::lstat(s) {
        Ok(st) => st,
        Err(e) => {
            // faccessat(F_OK, AT_SYMLINK_NOFOLLOW) falha antes do lstat com o mesmo errno
            not_accessible(short, s, e);
            return 2;
        }
    };
    if st.file_type() != FileType::Symlink {
        ul::warnx(short, format!("{}: not a symbolic link", io::lossy(s)));
        return 2;
    }
    let mut target = match sys::current().readlinkat(Fd::CWD, s) {
        Ok(t) => t,
        Err(e) => {
            ul::warn(short, format!("{}: readlink failed", io::lossy(s)), e);
            return 2;
        }
    };

    let newname = string_replace(o, from, to, &mut target);
    let mut ret = if newname.is_some() { 1 } else { 0 };

    if ret == 1
        && (nooverwrite || interactive)
        && let Some(n) = &newname
        && sys::lstat(n).is_err()
    {
        nooverwrite = false;
        interactive = false;
    }

    if ret == 1 && (nooverwrite || (interactive && (o.noact || !ask(short, newname.as_deref().unwrap_or(b""))))) {
        if o.verbose {
            let mut out = io::stdout();
            let _ = out.write_all(format!("Skipping existing link: `{}' -> `{}'\n", io::lossy(s), io::lossy(&target)).as_bytes());
        }
        ret = 0;
    }

    if ret == 1 {
        let n = newname.clone().unwrap_or_default();
        if !o.noact {
            if let Err(e) = sys::current().unlinkat(Fd::CWD, s, AtFlags::empty()) {
                ul::warn(short, format!("{}: unlink failed", io::lossy(s)), e);
                ret = 2;
            } else if let Err(e) = sys::current().symlinkat(&n, Fd::CWD, s) {
                ul::warn(short, format!("{}: symlinking to {} failed", io::lossy(s), io::lossy(&n)), e);
                ret = 2;
            }
        }
    }
    if o.verbose && (o.noact || ret == 1) {
        let n = newname.as_deref().map_or_else(|| "(null)".to_string(), io::lossy);
        let mut out = io::stdout();
        let _ = out.write_all(format!("{}: `{}' -> `{}'\n", io::lossy(s), io::lossy(&target), n).as_bytes());
    }
    ret
}

fn do_file(o: &Opts, short: &str, from: &[u8], to: &[u8], s_in: &[u8]) -> i32 {
    let mut nooverwrite = o.nooverwrite;
    let mut interactive = o.interactive;
    let mut ret = 1;
    if let Err(e) = sys::lstat(s_in) {
        not_accessible(short, s_in, e);
        return 2;
    }
    let mut s = s_in.to_vec();
    let Some(newname) = string_replace(o, from, to, &mut s) else { return 0 };

    if (nooverwrite || interactive) && sys::stat(&newname).is_err() {
        nooverwrite = false;
        interactive = false;
    }

    if nooverwrite || (interactive && (o.noact || !ask(short, &newname))) {
        if o.verbose {
            let mut out = io::stdout();
            let _ = out.write_all(format!("Skipping existing file: `{}'\n", io::lossy(&newname)).as_bytes());
        }
        ret = 0;
    } else if !o.noact
        && let Err(e) = sys::current().renameat2(Fd::CWD, &s, Fd::CWD, &newname, RenameFlags::empty())
    {
        ul::warn(short, format!("{}: rename to {} failed", io::lossy(&s), io::lossy(&newname)), e);
        ret = 2;
    }
    if o.verbose && (o.noact || ret == 1) {
        let mut out = io::stdout();
        let _ = out.write_all(format!("`{}' -> `{}'\n", io::lossy(&s), io::lossy(&newname)).as_bytes());
    }
    ret
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut o = Opts { all: false, last: false, verbose: false, noact: false, nooverwrite: false, interactive: false };
    let mut symlink = false;
    // err_exclusive_options: {a,l} e {i,o}.
    let mut excl_al: Option<char> = None;
    let mut excl_io: Option<char> = None;

    let mut g = Getopt::from_env(&argv[1..], "vsVhnaloi", LONGS);
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(x) => x,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let c = opt.short().unwrap_or('?');
        let (slot, names) = match c {
            'a' | 'l' => (Some(&mut excl_al), "--all --last"),
            'i' | 'o' => (Some(&mut excl_io), "--interactive --no-overwrite"),
            _ => (None, ""),
        };
        if let Some(slot) = slot {
            match *slot {
                None => *slot = Some(c),
                Some(prev) if prev != c => {
                    io::eprint(format!("{short}: mutually exclusive arguments: {names}\n"));
                    return 1;
                }
                _ => {}
            }
        }
        match c {
            'n' => o.noact = true,
            'a' => o.all = true,
            'l' => o.last = true,
            'v' => o.verbose = true,
            'o' => o.nooverwrite = true,
            'i' => o.interactive = true,
            's' => symlink = true,
            'V' => {
                ul::print_version(&short);
                return 0;
            }
            'h' => {
                let mut out = io::stdout();
                let _ = out.write_all(usage_text(&short).as_bytes());
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    let ops = g.operands();
    if ops.len() < 3 {
        ul::warnx(&short, "not enough arguments");
        ul::errtryhelp(&short);
        return 1;
    }
    let from = &ops[0];
    let to = &ops[1];
    if from == to {
        return RENAME_EXIT_NOTHING;
    }

    let mut ret = 0;
    for s in &ops[2..] {
        ret |= if symlink { do_symlink(&o, &short, from, to, s) } else { do_file(&o, &short, from, to, s) };
        sys::checkpoint();
    }
    let _ = io::flush_stdout();
    match ret {
        0 => RENAME_EXIT_NOTHING,
        1 => 0,
        2 => 1,
        3 => RENAME_EXIT_SOMEOK,
        _ => RENAME_EXIT_UNEXPLAINED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(all: bool, last: bool) -> Opts {
        Opts { all, last, verbose: false, noact: false, nooverwrite: false, interactive: false }
    }

    fn rep(o: &Opts, from: &str, to: &str, orig: &str) -> Option<String> {
        let mut v = orig.as_bytes().to_vec();
        string_replace(o, from.as_bytes(), to.as_bytes(), &mut v).map(|r| String::from_utf8_lossy(&r).into_owned())
    }

    #[test]
    fn first_all_and_last_occurrence() {
        assert_eq!(rep(&opts(false, false), "a", "X", "banana").as_deref(), Some("bXnana"));
        assert_eq!(rep(&opts(true, false), "a", "X", "banana").as_deref(), Some("bXnXnX"));
        assert_eq!(rep(&opts(false, true), "a", "X", "banana").as_deref(), Some("bananX"));
        assert_eq!(rep(&opts(false, false), "z", "X", "banana"), None);
    }

    #[test]
    fn only_last_component_without_slashes() {
        assert_eq!(rep(&opts(false, false), "a", "X", "dir-a/a-file").as_deref(), Some("dir-a/X-file"));
        assert_eq!(rep(&opts(false, false), "dir", "X", "dir/dir").as_deref(), Some("dir/X"));
        assert_eq!(rep(&opts(false, false), "d/", "X", "dir/dir"), None);
        assert_eq!(rep(&opts(false, false), "", "pre-", "a/b").as_deref(), Some("a/pre-b"));
    }
}
