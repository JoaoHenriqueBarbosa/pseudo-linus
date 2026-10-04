//! `which` do debianutils 5.23 (Debian 13). O original é um script `/bin/sh` (o dash); aqui ele vira
//! código nativo com o mesmo comportamento observável:
//!
//! - `getopts as`: `-a` mostra todos os caminhos, `-s` não escreve nada (só o código de saída); a
//!   varredura para no primeiro operando e em `--`. Opção inválida: o dash escreve `Illegal option -x`
//!   no stderr e o script escreve `Usage: $0 [-as] args` no stdout (menos com `-s` já visto) e sai
//!   com 2;
//! - nome com `/` é testado direto; os outros, em cada elemento do `PATH` (elemento vazio é `.`), com
//!   `test -f` (arquivo regular, seguindo link) e `test -x`;
//! - saída 0 se todos foram achados, 1 se algum faltou ou se não houve operando.
//!
//! O `$0` do script é o caminho que o kernel passou ao interpretador: o `argv[0]` quando tem `/`,
//! senão o primeiro `which` executável no `PATH` (o que o shell achou).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{AccessMode, AtFlags, Ctx, Fd, FileType, sys};

use crate::util::io;

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn is_executable_file(path: &[u8]) -> bool {
    let Some(s) = sys::try_current() else { return false };
    match s.fstatat(Fd::CWD, path, AtFlags::empty()) {
        Ok(st) if st.file_type() == FileType::Regular => {
            s.faccessat(Fd::CWD, path, AccessMode::X_OK, AtFlags::empty()).is_ok()
        }
        _ => false,
    }
}

/// Elementos do `PATH` como o laço `for ELEMENT in $PATH` do script com `IFS=:` os vê: separador
/// final não cria campo vazio, mas o script acrescenta um `:` quando o `PATH` termina em um só, pra
/// que esse vazio final valha como `.`.
fn path_elements(path: &[u8]) -> Vec<Vec<u8>> {
    if path.is_empty() {
        return Vec::new();
    }
    let mut p = path.to_vec();
    if p.len() >= 2 && p[p.len() - 1] == b':' && p[p.len() - 2] != b':' {
        p.push(b':');
    }
    let mut fields: Vec<Vec<u8>> = p.split(|b| *b == b':').map(<[u8]>::to_vec).collect();
    if p.ends_with(b":") {
        fields.pop();
    }
    fields
}

fn script_name(argv0: &[u8], path: &[u8]) -> Vec<u8> {
    if argv0.contains(&b'/') {
        return argv0.to_vec();
    }
    for el in path_elements(path) {
        let dir = if el.is_empty() { b".".to_vec() } else { el };
        let mut cand = dir.clone();
        cand.push(b'/');
        cand.extend_from_slice(b"which");
        if is_executable_file(&cand) {
            return cand;
        }
    }
    b"/usr/bin/which".to_vec()
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let path = sys::getenv("PATH").unwrap_or_default();
    let mut all = false;
    let mut silent = false;
    let mut idx = 1;
    // getopts do dash: agrupamento permitido, para no primeiro operando, `--` encerra.
    while idx < argv.len() {
        let a = &argv[idx];
        if a == b"--" {
            idx += 1;
            break;
        }
        if a.len() < 2 || a[0] != b'-' {
            break;
        }
        for &c in &a[1..] {
            match c {
                b'a' => all = true,
                b's' => silent = true,
                other => {
                    io::eprint(format!("Illegal option -{}\n", char::from(other)));
                    if !silent {
                        let name = script_name(&argv[0], &path);
                        let mut out = io::stdout();
                        let _ = out.write_all(b"Usage: ");
                        let _ = out.write_all(&name);
                        let _ = out.write_all(b" [-as] args\n");
                    }
                    return 2;
                }
            }
        }
        idx += 1;
    }
    let programs = &argv[idx..];
    let mut allret = i32::from(programs.is_empty());
    let mut out = io::stdout();
    let mut puts = |line: &[u8]| {
        if !silent {
            let _ = out.write_all(line);
            let _ = out.write_all(b"\n");
        }
    };
    let elements = path_elements(&path);
    for prog in programs {
        let mut found = false;
        if prog.contains(&b'/') {
            if is_executable_file(prog) {
                puts(prog);
                found = true;
            }
        } else {
            for el in &elements {
                let dir: &[u8] = if el.is_empty() { b"." } else { el };
                let mut cand = dir.to_vec();
                cand.push(b'/');
                cand.extend_from_slice(prog);
                if is_executable_file(&cand) {
                    puts(&cand);
                    found = true;
                    if !all {
                        break;
                    }
                }
            }
        }
        if !found {
            allret = 1;
        }
    }
    allret
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysabi::Program;
    use sysabi::testkit::TestKit;

    #[test]
    fn path_splitting_matches_dash_ifs() {
        let f = |p: &str| -> Vec<String> { path_elements(p.as_bytes()).iter().map(|e| io::lossy(e)).collect() };
        assert_eq!(f("/a:/b"), vec!["/a", "/b"]);
        assert_eq!(f("/a:"), vec!["/a", ""]);
        assert_eq!(f("/a::"), vec!["/a", ""]);
        assert_eq!(f(":/a"), vec!["", "/a"]);
        assert_eq!(f("/a::/b"), vec!["/a", "", "/b"]);
        assert_eq!(f(":"), vec![""]);
        assert!(f("").is_empty());
    }

    fn noop(_: &mut Ctx, _: &[OsString]) -> i32 {
        0
    }

    #[test]
    fn finds_programs_like_debianutils() {
        let kit = TestKit::new()
            .programs([Program::bin("which", main), Program::bin("ls", noop)])
            .symlink("/usr/local/bin/ls", "/usr/bin/ls")
            .file("/work/x", "#!/bin/sh\n", 0o755)
            .file("/work/notexec", "x", 0o644)
            .env("PATH", "/usr/local/bin:/usr/bin:/bin");
        let r = kit.run(&["which", "ls"], b"");
        assert_eq!((r.stdout_str().as_str(), r.code()), ("/usr/local/bin/ls\n", 0));
        let r = kit.run(&["which", "-a", "ls", "nope"], b"");
        assert_eq!(r.stdout_str(), "/usr/local/bin/ls\n/usr/bin/ls\n/bin/ls\n");
        assert_eq!(r.code(), 1);
        let r = kit.run(&["which", "./x", "./notexec", "/usr/bin"], b"");
        assert_eq!((r.stdout_str().as_str(), r.code()), ("./x\n", 1));
        let r = kit.run(&["which", "-x", "ls"], b"");
        assert_eq!(r.stderr_str(), "Illegal option -x\n");
        assert_eq!(r.stdout_str(), "Usage: /usr/bin/which [-as] args\n");
        assert_eq!(r.code(), 2);
        let r = kit.run(&["which", "-s", "-x"], b"");
        assert_eq!((r.stdout_str().as_str(), r.code()), ("", 2));
        let r = kit.run(&["which"], b"");
        assert_eq!(r.code(), 1);
        let r = kit.run(&["which", "ls", "-a"], b"");
        assert_eq!((r.stdout_str().as_str(), r.code()), ("/usr/local/bin/ls\n", 1));
        let kit = kit.env("PATH", "/usr/bin:");
        let r = kit.run(&["/usr/bin/which", "-a", "x"], b"");
        assert_eq!(r.stdout_str(), "./x\n");
    }
}
