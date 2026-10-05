//! `look` do util-linux 2.41 (pacote bsdextrautils do Debian 13): mostra as linhas de um arquivo
//! ordenado que começam com uma cadeia, por busca binária seguida de busca linear.
//!
//! O original mapeia o arquivo na memória com `mmap`; aqui o conteúdo é lido inteiro, e as falhas do
//! `mmap` que aparecem de fora são reproduzidas: arquivo vazio dá `Invalid argument` e o que não é
//! arquivo regular (diretório, dispositivo) dá `No such device`.
//!
//! O arquivo de palavras padrão é `/usr/share/dict/words` (ou `web2` com `-a`); a variável
//! `WORDLIST` o substitui quando legível. Com um só operando valem `-d` e `-f`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{AccessMode, AtFlags, Ctx, Errno, Fd, FileType, sys};

use crate::util::io::{self, File};
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const PATH_WORDS: &[u8] = b"/usr/share/dict/words";
const PATH_WORDS_ALT: &[u8] = b"/usr/share/dict/web2";

const LONGS: &[LongOpt] = &[
    LongOpt::new("alternative", HasArg::No, b'a' as i32),
    LongOpt::new("alphanum", HasArg::No, b'd' as i32),
    LongOpt::new("ignore-case", HasArg::No, b'f' as i32),
    LongOpt::new("terminate", HasArg::Required, b't' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 look [options] <string> [<file>...]

Display lines beginning with a specified string.

Options:
 -a, --alternative        use the alternative dictionary
 -d, --alphanum           compare only blanks and alphanumeric characters
 -f, --ignore-case        ignore case differences when comparing
 -t, --terminate <char>   define the string-termination character

 -h, --help               display this help
 -V, --version            display version

For more details see look(1).
";

#[derive(Copy, Clone, PartialEq, Eq)]
enum Ord3 {
    Equal,
    Greater,
    Less,
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn is_alnum(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// Estado da busca (as globais do original).
struct Look<'a> {
    data: &'a [u8],
    string: Vec<u8>,
    dflag: bool,
    fflag: bool,
}

impl Look<'_> {
    /// `compare`: `Less` quando a cadeia é menor que a linha em `s2`, `Greater` quando é maior, `Equal`
    /// quando a linha começa com ela (até `string.len()` caracteres úteis).
    fn compare(&self, mut s2: usize, s2end: usize) -> Ord3 {
        let stringlen = self.string.len();
        let mut buf: Vec<u8> = Vec::with_capacity(stringlen);
        let mut i = stringlen;
        while s2 < s2end && self.data[s2] != b'\n' && i > 0 {
            let c = self.data[s2];
            if !self.dflag || is_alnum(c) || is_blank(c) {
                buf.push(c);
                i -= 1;
            }
            s2 += 1;
        }
        // strncmp/strncasecmp sobre cadeias terminadas em NUL.
        let mut r = 0i32;
        for k in 0..stringlen {
            let mut x = buf.get(k).copied().unwrap_or(0);
            let mut y = self.string.get(k).copied().unwrap_or(0);
            if self.fflag {
                x = x.to_ascii_lowercase();
                y = y.to_ascii_lowercase();
            }
            if x != y {
                r = i32::from(x) - i32::from(y);
                break;
            }
            if x == 0 {
                break;
            }
        }
        if r > 0 {
            Ord3::Less
        } else if r < 0 {
            Ord3::Greater
        } else {
            Ord3::Equal
        }
    }

    /// `SKIP_PAST_NEWLINE`.
    fn skip_past_newline(&self, mut p: usize, back: usize) -> usize {
        while p < back {
            let c = self.data[p];
            p += 1;
            if c == b'\n' {
                break;
            }
        }
        p
    }

    fn binary_search(&self, mut front: usize, mut back: usize) -> usize {
        let mut p = front + (back - front) / 2;
        p = self.skip_past_newline(p, back);
        while p < back && back > front {
            if self.compare(p, back) == Ord3::Greater {
                front = p;
            } else {
                back = p;
            }
            p = front + (back - front) / 2;
            p = self.skip_past_newline(p, back);
        }
        front
    }

    fn linear_search(&self, mut front: usize, back: usize) -> Option<usize> {
        while front < back {
            match self.compare(front, back) {
                Ord3::Equal => return Some(front),
                Ord3::Less => return None,
                Ord3::Greater => {}
            }
            front = self.skip_past_newline(front, back);
        }
        None
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut file: Vec<u8> = match sys::getenv("WORDLIST") {
        Some(f)
            if sys::current()
                .faccessat(Fd::CWD, &f, AccessMode::R_OK, AtFlags::empty())
                .is_ok() =>
        {
            f
        }
        _ => PATH_WORDS.to_vec(),
    };
    let mut dflag = false;
    let mut fflag = false;
    let mut termchar: u8 = 0;

    let mut g = Getopt::from_env(&argv[1..], "adft:Vh", LONGS);
    while let Some(opt) = g.next_opt() {
        match opt {
            Ok(o) => match o.short() {
                Some('a') => file = PATH_WORDS_ALT.to_vec(),
                Some('d') => dflag = true,
                Some('f') => fflag = true,
                Some('t') => {
                    termchar = o
                        .arg
                        .as_deref()
                        .and_then(|a| a.first().copied())
                        .unwrap_or(0)
                }
                Some('V') => {
                    ul::print_version(&short);
                    return 0;
                }
                Some('h') => {
                    let mut out = io::stdout();
                    let _ = out.write_all(USAGE.as_bytes());
                    return 0;
                }
                _ => {
                    ul::errtryhelp(&short);
                    return 1;
                }
            },
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }
    let ops = g.operands();
    let mut string: Vec<u8> = match ops.len() {
        2 => {
            file = ops[1].clone();
            ops[0].clone()
        }
        1 => {
            // Sem arquivo, -d e -f valem por padrão.
            dflag = true;
            fflag = true;
            ops[0].clone()
        }
        _ => {
            ul::warnx(&short, "bad usage");
            ul::errtryhelp(&short);
            return 1;
        }
    };

    if termchar != 0
        && let Some(p) = string.iter().position(|&b| b == termchar)
    {
        string.truncate(p + 1);
    }

    let data = match load(&file) {
        Ok(d) => d,
        Err(e) => {
            ul::warn(&short, io::lossy(&file), e);
            return 1;
        }
    };

    if dflag {
        string.retain(|&c| is_alnum(c) || is_blank(c));
    }
    let look = Look {
        data: &data,
        string,
        dflag,
        fflag,
    };
    let back = data.len();
    let front = look.binary_search(0, back);
    let Some(mut front) = look.linear_search(front, back) else {
        return 1;
    };

    let mut out = io::stdout();
    while front < back && look.compare(front, back) == Ord3::Equal {
        let mut eol = false;
        while front < back && !eol {
            let c = data[front];
            let _ = out.write_all(&[c]);
            front += 1;
            if c == b'\n' {
                eol = true;
            }
        }
    }
    if out.flush().is_err() {
        io::eprint(format!("{short}: stdout: {}\n", Errno::EIO.message()));
        return 1;
    }
    0
}

/// Abre o arquivo e devolve o conteúdo, com os erros do `open`/`fstat`/`mmap` do original.
fn load(path: &[u8]) -> Result<Vec<u8>, Errno> {
    let mut f = File::open(path)?;
    let st = sys::current().fstat(f.fd())?;
    if st.file_type() != FileType::Regular {
        return Err(Errno::ENODEV);
    }
    if st.size == 0 {
        return Err(Errno::EINVAL);
    }
    f.read_to_end_sys()
}
