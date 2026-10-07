//! `less` (668) e `more` (util-linux 2.41.5) quando a saída não é terminal, que é o único modo que
//! o pseudo-linus usa hoje: os dois viram cópia, como os originais fazem nesse caso.
//!
//! - `less`: copia os arquivos (ou o stdin, sem operandos) em sequência; arquivo inexistente e
//!   diretório viram aviso no stdout (`x: No such file or directory`, `d is a directory`) e a cópia
//!   segue; opção desconhecida ou sem valor avisa no stderr e é ignorada; sai sempre com 0. O
//!   comportamento veio do manual e do Debian 13 em caixa preta (o less é GPLv3 ou licença própria e
//!   o código dele não foi usado).
//! - `more`: se o stdin não é terminal, mostra ele primeiro; depois cada arquivo com o cabeçalho
//!   `::::::::::::::` / nome / `::::::::::::::`; diretório vira `*** d: directory ***`; arquivo que
//!   não abre avisa no stderr; as opções de paginação não têm efeito; sai com 0.
//!
//! Pendente: modo interativo (precisa de tty no kernel) e o texto do `less --help`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{
    Ctx, Errno, Fd, FdAction, FileType, OFlags, ProcAttrs, SpawnSpec, WaitOptions, WaitStatus,
    WaitTarget, sys,
};

use crate::util::io::{self, File};
use crate::util::{Getopt, HasArg, LongOpt};

// ------------------------------------------------------------------------------------------------
// less

const LESS_VERSION: &str = "less 668 (GNU regular expressions)
Copyright (C) 1984-2024  Mark Nudelman

less comes with NO WARRANTY, to the extent permitted by law.
For information about the terms of redistribution,
see the file named README in the less distribution.
Home page: https://greenwoodsoftware.com/less
";

/// Opções curtas que levam valor e o nome longo delas (pra mensagem de valor faltando).
const LESS_VALUED: &[(u8, &str)] = &[
    (b'b', "buffers"),
    (b'D', "color"),
    (b'h', "max-back-scroll"),
    (b'j', "jump-target"),
    (b'k', "lesskey-file"),
    (b'o', "log-file"),
    (b'O', "LOG-FILE"),
    (b'p', "pattern"),
    (b'P', "prompt"),
    (b't', "tag"),
    (b'T', "tag-file"),
    (b'x', "tabs"),
    (b'y', "max-forw-scroll"),
    (b'z', "window"),
    (b'#', "shift"),
];

/// Opções curtas sem valor.
const LESS_FLAGS: &[u8] = b"aABcCdeEfFgGiIJKLmMnNqQrRsSuUwWX~";

/// Opções longas: nome e se leva valor.
const LESS_LONG: &[(&str, bool)] = &[
    ("search-skip-screen", false),
    ("SEARCH-SKIP-SCREEN", false),
    ("buffers", true),
    ("auto-buffers", false),
    ("clear-screen", false),
    ("dumb", false),
    ("color", true),
    ("quit-at-eof", false),
    ("QUIT-AT-EOF", false),
    ("force", false),
    ("quit-if-one-screen", false),
    ("hilite-search", false),
    ("HILITE-SEARCH", false),
    ("max-back-scroll", true),
    ("ignore-case", false),
    ("IGNORE-CASE", false),
    ("jump-target", true),
    ("status-column", false),
    ("lesskey-file", true),
    ("lesskey-src", true),
    ("lesskey-context", true),
    ("quit-on-intr", false),
    ("no-lessopen", false),
    ("long-prompt", false),
    ("LONG-PROMPT", false),
    ("line-numbers", false),
    ("LINE-NUMBERS", false),
    ("log-file", true),
    ("LOG-FILE", true),
    ("pattern", true),
    ("prompt", true),
    ("quiet", false),
    ("QUIET", false),
    ("silent", false),
    ("SILENT", false),
    ("raw-control-chars", false),
    ("RAW-CONTROL-CHARS", false),
    ("squeeze-blank-lines", false),
    ("chop-long-lines", false),
    ("tag", true),
    ("tag-file", true),
    ("underline-special", false),
    ("UNDERLINE-SPECIAL", false),
    ("version", false),
    ("hilite-unread", false),
    ("HILITE-UNREAD", false),
    ("tabs", true),
    ("no-init", false),
    ("max-forw-scroll", true),
    ("window", true),
    ("shift", true),
    ("exit-follow-on-close", false),
    ("file-size", false),
    ("follow-name", false),
    ("header", true),
    ("help", false),
    ("incsearch", false),
    ("intr", true),
    ("line-num-width", true),
    ("match-shift", true),
    ("modelines", true),
    ("mouse", false),
    ("no-histdups", false),
    ("no-keypad", false),
    ("no-number-headers", false),
    ("no-search-header-columns", false),
    ("no-search-header-lines", false),
    ("no-search-headers", false),
    ("no-vbell", false),
    ("proc-backspace", false),
    ("PROC-BACKSPACE", false),
    ("proc-return", false),
    ("PROC-RETURN", false),
    ("proc-tab", false),
    ("PROC-TAB", false),
    ("quotes", true),
    ("redraw-on-quit", false),
    ("rscroll", true),
    ("save-marks", false),
    ("search-options", true),
    ("show-preproc-errors", false),
    ("status-col-width", true),
    ("status-line", false),
    ("tilde", false),
    ("use-backslash", false),
    ("use-color", false),
    ("wheel-lines", true),
    ("wordwrap", false),
];

pub fn less_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| less_run(args))
}

fn less_run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let mut files: Vec<Vec<u8>> = Vec::new();
    let mut i = 1;
    let mut only_files = false;
    // Opções do ambiente `LESS` vêm antes das da linha de comando; sem tty elas não mudam a cópia,
    // mas os avisos de opção inválida aparecem do mesmo jeito.
    let mut words: Vec<Vec<u8>> = Vec::new();
    if let Some(env) = sys::getenv("LESS") {
        for w in env.split(|b| *b == b' ').filter(|w| !w.is_empty()) {
            let mut w = w.to_vec();
            if w[0] != b'-' && w[0] != b'+' {
                w.insert(0, b'-');
            }
            words.push(w);
        }
    }
    let env_words = words.len();
    words.extend(argv[1..].iter().cloned());
    // `i` aponta pro próximo argumento a examinar, contando a partir de 1.
    while i <= words.len() {
        let idx = i - 1;
        let arg = words[idx].clone();
        i += 1;
        let from_cmdline = idx >= env_words;
        if only_files || !(arg.starts_with(b"-") || arg.starts_with(b"+")) || arg == b"-" {
            if from_cmdline {
                files.push(arg);
            }
            continue;
        }
        if arg == b"--" {
            only_files = true;
            continue;
        }
        if arg[0] == b'+' {
            continue;
        }
        if let Some(body) = arg.strip_prefix(b"--") {
            let text = String::from_utf8_lossy(body).into_owned();
            let (name, inline) = match text.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (text.clone(), None),
            };
            let exact = LESS_LONG.iter().find(|(n, _)| *n == name);
            let found = exact.or_else(|| {
                let cands: Vec<&(&str, bool)> = LESS_LONG
                    .iter()
                    .filter(|(n, _)| n.starts_with(name.as_str()))
                    .collect();
                (cands.len() == 1).then(|| cands[0])
            });
            match found {
                Some((n, valued)) => {
                    if *n == "version" {
                        let _ = io::stdout().write_all(LESS_VERSION.as_bytes());
                        return 0;
                    }
                    if *valued && inline.is_none() {
                        if i <= words.len() {
                            i += 1;
                        } else {
                            io::eprint(format!("Value is required after --{n}\n"));
                        }
                    }
                }
                None => io::eprint(format!(
                    "There is no {name} option (\"less --help\" for help)\n"
                )),
            }
            continue;
        }
        let mut j = 1;
        while j < arg.len() {
            let c = arg[j];
            j += 1;
            if c == b'V' {
                let _ = io::stdout().write_all(LESS_VERSION.as_bytes());
                return 0;
            }
            if let Some((_, long)) = LESS_VALUED.iter().find(|(s, _)| *s == c) {
                if j < arg.len() {
                    j = arg.len();
                } else if i <= words.len() {
                    i += 1;
                } else {
                    io::eprint(format!(
                        "Value is required after -{} (--{long})\n",
                        char::from(c)
                    ));
                }
                continue;
            }
            if LESS_FLAGS.contains(&c) || c.is_ascii_digit() {
                continue;
            }
            let shown = if c.is_ascii() {
                char::from(c).to_string()
            } else {
                format!("\\x{c:02x}")
            };
            io::eprint(format!(
                "There is no -{shown} option (\"less --help\" for help)\n"
            ));
        }
    }
    let mut out = io::stdout();
    if files.is_empty() {
        copy_fd(&mut File::stdin(), &mut out);
        return 0;
    }
    for f in &files {
        if f == b"-" {
            copy_fd(&mut File::stdin(), &mut out);
            continue;
        }
        match sys::stat(f) {
            Ok(st) if st.file_type() == FileType::Directory => {
                let _ = out.write_all(f);
                let _ = out.write_all(b" is a directory\n");
                continue;
            }
            Err(e) => {
                let _ = out.write_all(f);
                let _ = out.write_all(format!(": {}\n", e.message()).as_bytes());
                continue;
            }
            Ok(_) => {}
        }
        if let Some(text) = lessopen(f) {
            let _ = out.write_all(&text);
            continue;
        }
        match File::open(f) {
            Ok(mut file) => {
                copy_fd(&mut file, &mut out);
            }
            Err(e) => {
                let _ = out.write_all(f);
                let _ = out.write_all(format!(": {}\n", e.message()).as_bytes());
            }
        }
    }
    0
}

/// Preprocessador de entrada do `LESSOPEN` no modo pipe (`|cmd %s`, ou `||cmd %s`, ou `|-cmd %s`):
/// roda o comando pelo `sh` com o nome do arquivo citado no lugar do `%s` e devolve o que ele
/// escreveu. Saída vazia volta como `None` (o less mostra o arquivo original), exceto no `||`, em que
/// vazio é conteúdo válido. O modo de arquivo temporário (sem `|`) não é suportado e cai na cópia.
fn lessopen(file: &[u8]) -> Option<Vec<u8>> {
    let spec = sys::getenv("LESSOPEN")?;
    let mut cmd = spec.strip_prefix(b"|")?;
    let mut empty_ok = false;
    if let Some(rest) = cmd.strip_prefix(b"|") {
        cmd = rest;
        empty_ok = true;
    }
    if let Some(rest) = cmd.strip_prefix(b"-") {
        cmd = rest;
    }
    let mut quoted = b"'".to_vec();
    for &b in file {
        if b == b'\'' {
            quoted.extend_from_slice(b"'\\''");
        } else {
            quoted.push(b);
        }
    }
    quoted.push(b'\'');
    let mut script = Vec::new();
    let mut k = 0;
    while k < cmd.len() {
        if cmd[k] == b'%' && cmd.get(k + 1) == Some(&b's') {
            script.extend_from_slice(&quoted);
            k += 2;
        } else {
            script.push(cmd[k]);
            k += 1;
        }
    }
    let s = sys::current();
    let (r, w) = s.pipe2(OFlags::CLOEXEC).ok()?;
    let attrs = ProcAttrs {
        fd_actions: vec![
            FdAction::Dup2 { from: w, to: Fd::STDOUT },
            FdAction::Close(r),
            FdAction::Close(w),
        ],
        ..ProcAttrs::default()
    };
    let spawned = s.spawn(SpawnSpec {
        path: b"/bin/sh".to_vec(),
        argv: vec![b"sh".to_vec(), b"-c".to_vec(), script],
        attrs,
    });
    let _ = s.close(w);
    let pid = match spawned {
        Ok(p) => p,
        Err(_) => {
            let _ = s.close(r);
            return None;
        }
    };
    let mut text = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match sys::read(r, &mut buf) {
            Ok(0) => break,
            Ok(n) => text.extend_from_slice(&buf[..n]),
            Err(Errno::EINTR) => continue,
            Err(_) => break,
        }
    }
    let _ = s.close(r);
    loop {
        match s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()) {
            Err(Errno::EINTR) | Ok(Some((_, WaitStatus::Stopped { .. }))) => continue,
            _ => break,
        }
    }
    if text.is_empty() && !empty_ok {
        return None;
    }
    Some(text)
}

/// Copia um fd inteiro pro stdout em blocos.
fn copy_fd(src: &mut File, out: &mut impl Write) -> Option<Errno> {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match sys::read(src.fd(), &mut buf) {
            Ok(0) => return None,
            Ok(n) => {
                if out.write_all(&buf[..n]).is_err() {
                    return Some(Errno::EIO);
                }
            }
            Err(Errno::EINTR) => {}
            Err(e) => return Some(e),
        }
        sys::checkpoint();
    }
}

// ------------------------------------------------------------------------------------------------
// more

const MORE_USAGE: &str = "
Usage:
 more [options] <file>...

Display the contents of a file in a terminal.

Options:
 -d, --silent          display help instead of ringing bell
 -f, --logical         count logical rather than screen lines
 -l, --no-pause        suppress pause after form feed
 -c, --print-over      do not scroll, display text and clean line ends
 -p, --clean-print     do not scroll, clean screen and display text
 -e, --exit-on-eof     exit on end-of-file
 -s, --squeeze         squeeze multiple blank lines into one
 -u, --plain           suppress underlining and bold
 -n, --lines <number>  the number of lines per screenful
 -<number>             same as --lines
 +<number>             display file beginning from line number
 +/<pattern>           display file beginning from pattern match

 -h, --help            display this help
 -V, --version         display version

For more details see more(1).
";

const MORE_LONG: &[LongOpt] = &[
    LongOpt::new("silent", HasArg::No, 'd' as i32),
    LongOpt::new("logical", HasArg::No, 'f' as i32),
    LongOpt::new("no-pause", HasArg::No, 'l' as i32),
    LongOpt::new("print-over", HasArg::No, 'c' as i32),
    LongOpt::new("clean-print", HasArg::No, 'p' as i32),
    LongOpt::new("exit-on-eof", HasArg::No, 'e' as i32),
    LongOpt::new("squeeze", HasArg::No, 's' as i32),
    LongOpt::new("plain", HasArg::No, 'u' as i32),
    LongOpt::new("lines", HasArg::Required, 'n' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
    LongOpt::new("help", HasArg::No, 'h' as i32),
];

pub fn more_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| more_run(args))
}

fn more_run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    // `+N`, `+/padrão` e `-N` saem antes do getopt, como no original.
    let rest: Vec<Vec<u8>> = argv[1..]
        .iter()
        .filter(|a| {
            let plus = a.first() == Some(&b'+');
            let dash_num = a.len() > 1 && a[0] == b'-' && a[1..].iter().all(u8::is_ascii_digit);
            !(plus || dash_num)
        })
        .cloned()
        .collect();
    let mut getopt = Getopt::from_env(&rest, "dflcpsun:eVh", MORE_LONG);
    while let Some(r) = getopt.next_opt() {
        match r {
            Ok(opt) => match opt.short() {
                Some('V') => {
                    let _ = io::stdout().write_all(b"more from util-linux 2.41.5\n");
                    return 0;
                }
                Some('h') => {
                    let _ = io::stdout().write_all(MORE_USAGE.as_bytes());
                    return 0;
                }
                Some('n') => {
                    let a = opt.arg_str();
                    if a.is_empty() || !a.bytes().all(|b| b.is_ascii_digit()) {
                        io::eprint(format!("more: failed to parse number: '{a}'\n"));
                        return 1;
                    }
                }
                _ => {}
            },
            Err(e) => {
                io::eprint(format!(
                    "{}\nTry 'more --help' for more information.\n",
                    e.message(&argv0)
                ));
                return 1;
            }
        }
    }
    let files = getopt.operands();
    let mut out = io::stdout();
    let stdin_tty = sys::try_current().is_some_and(|s| s.isatty(Fd::STDIN));
    if !stdin_tty {
        copy_fd(&mut File::stdin(), &mut out);
    }
    for f in &files {
        match sys::stat(f) {
            Ok(st) if st.file_type() == FileType::Directory => {
                let _ = out.write_all(b"\n*** ");
                let _ = out.write_all(f);
                let _ = out.write_all(b": directory ***\n\n");
                continue;
            }
            _ => {}
        }
        match File::open(f) {
            Ok(mut file) => {
                let _ = out.write_all(b"::::::::::::::\n");
                let _ = out.write_all(f);
                let _ = out.write_all(b"\n::::::::::::::\n");
                copy_fd(&mut file, &mut out);
            }
            Err(e) => {
                let _ = out.flush();
                io::eprint(format!(
                    "more: cannot open {}: {}\n",
                    io::lossy(f),
                    e.message()
                ));
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysabi::Program;
    use sysabi::testkit::TestKit;

    fn kit() -> TestKit {
        TestKit::new()
            .programs([
                Program::bin("less", less_main),
                Program::bin("more", more_main),
            ])
            .file("/work/a", "l1\nl2\n", 0o644)
            .file("/work/b", "x\n", 0o644)
            .file("/work/c", "nonl", 0o644)
            .dir("/work/d", 0o755)
    }

    #[test]
    fn less_copies_like_the_real_one_without_tty() {
        let r = kit().run(&["less", "a", "nope", "d", "b"], b"");
        assert_eq!(
            r.stdout_str(),
            "l1\nl2\nnope: No such file or directory\nd is a directory\nx\n"
        );
        assert_eq!(r.status.shell_status(), 0);
        let r = kit().run(&["less", "-Z", "-N", "--bogus", "a"], b"");
        assert_eq!(r.stdout_str(), "l1\nl2\n");
        assert_eq!(
            r.stderr_str(),
            "There is no -Z option (\"less --help\" for help)\nThere is no bogus option (\"less --help\" for help)\n"
        );
        let r = kit().run(&["less"], b"from stdin\n");
        assert_eq!(r.stdout_str(), "from stdin\n");
        let r = kit().run(&["less", "-b"], b"");
        assert_eq!(r.stderr_str(), "Value is required after -b (--buffers)\n");
    }

    #[test]
    fn more_prints_headers_without_tty() {
        let r = kit().run(&["more", "c", "b", "nope", "d"], b"");
        assert_eq!(
            r.stdout_str(),
            "::::::::::::::\nc\n::::::::::::::\nnonl::::::::::::::\nb\n::::::::::::::\nx\n\n*** d: directory ***\n\n"
        );
        assert_eq!(
            r.stderr_str(),
            "more: cannot open nope: No such file or directory\n"
        );
        let r = kit().run(&["more", "a"], b"in\n");
        assert_eq!(
            r.stdout_str(),
            "in\n::::::::::::::\na\n::::::::::::::\nl1\nl2\n"
        );
        let r = kit().run(&["more", "-Z"], b"");
        assert_eq!(
            r.stderr_str(),
            "more: invalid option -- 'Z'\nTry 'more --help' for more information.\n"
        );
        assert_eq!(r.status.shell_status(), 1);
    }
}
