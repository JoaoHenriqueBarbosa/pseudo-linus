//! `envsubst` do gettext-base 0.23.1 (Debian 13), escrito a partir do manual e do comportamento
//! observado (o gettext é GPL e o código dele não foi usado).
//!
//! - Copia o stdin pro stdout trocando `$NOME` e `${NOME}` (nome = letra ou `_`, depois letras,
//!   dígitos e `_`) pelo valor no ambiente; variável indefinida vira vazio. Formas que não são
//!   referência (`${X:-y}`, `$1`, `$$`, `${` sem fecho) passam intactas.
//! - Com SHELL-FORMAT, só as variáveis citadas nele são trocadas.
//! - `-v`/`--variables` lista as variáveis do SHELL-FORMAT, uma por linha, na ordem e com repetição.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("variables", HasArg::No, 'v' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

const HELP: &str = "Usage: envsubst [OPTION] [SHELL-FORMAT]

Substitutes the values of environment variables.

Operation mode:
  -v, --variables             output the variables occurring in SHELL-FORMAT

Informative output:
  -h, --help                  display this help and exit
  -V, --version               output version information and exit

In normal operation mode, standard input is copied to standard output,
with references to environment variables of the form $VARIABLE or ${VARIABLE}
being replaced with the corresponding values.  If a SHELL-FORMAT is given,
only those environment variables that are referenced in SHELL-FORMAT are
substituted; otherwise all environment variables references occurring in
standard input are substituted.

When --variables is used, standard input is ignored, and the output consists
of the environment variables that are referenced in SHELL-FORMAT, one per line.

Report bugs in the bug tracker at <https://savannah.gnu.org/projects/gettext>
or by email to <bug-gettext@gnu.org>.
";

const VERSION: &str = "envsubst (GNU gettext-runtime) 0.23.1
Copyright (C) 2003-2024 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.
Written by Bruno Haible.
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// Uma referência encontrada no texto: posição, tamanho e nome.
struct Reference {
    start: usize,
    end: usize,
    name: Vec<u8>,
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Todas as referências `$NOME`/`${NOME}` do texto, na ordem.
fn references(text: &[u8]) -> Vec<Reference> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text[i] != b'$' {
            i += 1;
            continue;
        }
        let braced = text.get(i + 1) == Some(&b'{');
        let s = if braced { i + 2 } else { i + 1 };
        if s < text.len() && is_name_start(text[s]) {
            let mut e = s + 1;
            while e < text.len() && is_name_char(text[e]) {
                e += 1;
            }
            if !braced {
                out.push(Reference {
                    start: i,
                    end: e,
                    name: text[s..e].to_vec(),
                });
                i = e;
                continue;
            }
            if text.get(e) == Some(&b'}') {
                out.push(Reference {
                    start: i,
                    end: e + 1,
                    name: text[s..e].to_vec(),
                });
                i = e + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut list_vars = false;
    let mut getopt = Getopt::from_env(&argv[1..], "hvV", LONGOPTS);
    while let Some(r) = getopt.next_opt() {
        match r {
            Ok(opt) => match opt.short() {
                Some('v') => list_vars = true,
                Some('h') => {
                    let _ = io::stdout().write_all(HELP.as_bytes());
                    return 0;
                }
                Some('V') => {
                    let _ = io::stdout().write_all(VERSION.as_bytes());
                    return 0;
                }
                _ => {}
            },
            Err(e) => {
                io::eprint(format!(
                    "{}\nTry 'envsubst --help' for more information.\n",
                    e.message(&argv0)
                ));
                return 1;
            }
        }
    }
    let operands = getopt.operands();
    if operands.len() > 1 {
        io::eprint("envsubst: too many arguments\n");
        return 1;
    }
    let format = operands.first();
    let mut out = io::stdout();
    if list_vars {
        let Some(format) = format else {
            io::eprint("envsubst: missing arguments\n");
            return 1;
        };
        for r in references(format) {
            let _ = out.write_all(&r.name);
            let _ = out.write_all(b"\n");
        }
        return 0;
    }
    let allowed: Option<Vec<Vec<u8>>> =
        format.map(|f| references(f).into_iter().map(|r| r.name).collect());
    let input = match io::read_stdin() {
        Ok(d) => d,
        Err(e) => {
            io::eprint(format!(
                "envsubst: error while reading \"standard input\": {}\n",
                e.message()
            ));
            return 1;
        }
    };
    let mut result: Vec<u8> = Vec::new();
    if result.try_reserve(input.len()).is_err() {
        io::eprint("envsubst: memory exhausted\n");
        return 1;
    }
    let mut last = 0;
    for (n, r) in references(&input).into_iter().enumerate() {
        if n % 4096 == 0 {
            sys::checkpoint();
        }
        if allowed.as_ref().is_some_and(|a| !a.contains(&r.name)) {
            continue;
        }
        result.extend_from_slice(&input[last..r.start]);
        if let Some(v) = sys::try_current().and_then(|s| s.getenv(&r.name)) {
            result.extend_from_slice(&v);
        }
        last = r.end;
    }
    result.extend_from_slice(&input[last..]);
    let _ = out.write_all(&result);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysabi::Program;
    use sysabi::testkit::TestKit;

    fn kit() -> TestKit {
        TestKit::new()
            .programs([Program::bin("envsubst", main)])
            .env("FOO", "foo")
            .env("BAR", "b a r")
            .env("EMPTY", "")
    }

    #[test]
    fn substitutes_like_gettext() {
        let input = "a $FOO b ${BAR} c $UNDEF d ${FOO}x $FOOx $ ${ ${FOO $1 $$ \\$FOO ${FOO:-def} $EMPTY. ${9a} $_x\n";
        let r = kit().run(&["envsubst"], input.as_bytes());
        assert_eq!(
            r.stdout_str(),
            "a foo b b a r c  d foox  $ ${ ${FOO $1 $$ \\foo ${FOO:-def} . ${9a} \n"
        );
        let r = kit().run(&["envsubst", "$BAR"], b"a $FOO ${BAR} $UNDEF\n");
        assert_eq!(r.stdout_str(), "a $FOO b a r $UNDEF\n");
        let r = kit().run(&["envsubst", "-v", "$FOO ${BAR} $FOO x$Y"], b"");
        assert_eq!(r.stdout_str(), "FOO\nBAR\nFOO\nY\n");
    }

    #[test]
    fn argument_errors() {
        let r = kit().run(&["envsubst", "-v"], b"");
        assert_eq!(
            (r.stderr_str().as_str(), r.status.shell_status()),
            ("envsubst: missing arguments\n", 1)
        );
        let r = kit().run(&["envsubst", "a", "b"], b"");
        assert_eq!(
            (r.stderr_str().as_str(), r.status.shell_status()),
            ("envsubst: too many arguments\n", 1)
        );
        let r = kit().run(&["envsubst", "-Z"], b"");
        assert_eq!(
            r.stderr_str(),
            "envsubst: invalid option -- 'Z'\nTry 'envsubst --help' for more information.\n"
        );
    }
}
