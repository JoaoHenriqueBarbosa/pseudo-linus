//! `colrm` do util-linux 2.41 (pacote bsdextrautils do Debian 13): remove colunas da entrada padrão.
//!
//! `colrm [início [fim]]`, colunas contadas de 1 e medidas em largura de exibição (tabulação vai
//! pro próximo múltiplo de 8, backspace volta uma coluna). Se o corte começa no meio de um caractere
//! largo, ou termina nele, a parte que sobra sai como espaços. Sequência UTF-8 inválida termina com
//! `fgetwc() failed: Invalid or incomplete multibyte or wide character`; uma sequência incompleta
//! no fim da entrada conta como fim de arquivo.
//!
//! Como o original, os operandos saem do `argv` já permutado pelo `getopt_long` (por isso um `--`
//! no meio vira o primeiro argumento e é recusado como número).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno};

use crate::getopt_cmd::engine::{Engine, LongDef};
use crate::util::io;
use crate::util::ul::{self, Wide, peek_wide, wcwidth};

const USAGE: &str = "
Usage:
 colrm [startcol [endcol]]

Filter out the specified columns from standard input.

Options:
 -h, --help     display this help
 -V, --version  display version

For more details see colrm(1).
";

/// A entrada inteira, lida como o `fgetwc` da glibc em C.UTF-8.
struct Reader {
    data: Vec<u8>,
    pos: usize,
    /// Erro de leitura do stdin, devolvido na primeira leitura.
    read_err: Option<Errno>,
}

impl Reader {
    /// `fgetwc_or_err`: `Ok(None)` no fim, `Err` com o errno que o `err()` imprimiria.
    fn getwc(&mut self) -> Result<Option<char>, Errno> {
        if let Some(e) = self.read_err {
            return Err(e);
        }
        match peek_wide(&self.data, self.pos) {
            Wide::Char(c) => {
                self.pos += c.len_utf8();
                Ok(Some(c))
            }
            // Sequência incompleta no fim da entrada conta como fim de arquivo, sem erro.
            Wide::Eof | Wide::Truncated => Ok(None),
            Wide::Invalid(_) => Err(Errno::EILSEQ),
        }
    }
}

fn putwc(out: &mut impl Write, c: char) {
    let mut b = [0u8; 4];
    let _ = out.write_all(c.encode_utf8(&mut b).as_bytes());
}

/// Processa uma linha (ou o resto do arquivo). `Ok(true)` quando terminou numa quebra de linha e há
/// mais a ler, `Ok(false)` no fim da entrada.
fn process_input(rd: &mut Reader, out: &mut impl Write, first: u64, last: u64) -> Result<bool, Errno> {
    let mut ct: u64 = 0;
    let mut w: i64;
    loop {
        let Some(c) = rd.getwc()? else { return Ok(false) };
        if c == '\t' {
            w = (((ct + 8) & !7) - ct) as i64;
        } else if c == '\u{8}' {
            w = if ct != 0 { -1 } else { 0 };
        } else {
            w = i64::from(wcwidth(c)).max(0);
        }
        ct = ct.wrapping_add(w as u64);
        if c == '\n' {
            putwc(out, c);
            ct = 0;
            continue;
        }
        if first == 0 || ct < first {
            putwc(out, c);
            continue;
        }
        break;
    }

    let mut i = ct.wrapping_sub(w as u64).wrapping_add(1);
    while i < first {
        putwc(out, ' ');
        i += 1;
    }

    // Loop getting rid of characters
    while last == 0 || ct < last {
        let Some(c) = rd.getwc()? else { return Ok(false) };
        if c == '\n' {
            putwc(out, c);
            return Ok(true);
        }
        if c == '\t' {
            ct = (ct + 8) & !7;
        } else if c == '\u{8}' {
            ct = if ct != 0 { ct - 1 } else { 0 };
        } else {
            ct += u64::from(wcwidth(c).max(0) as u32);
        }
    }

    let mut padding = false;

    // Output last of the line
    loop {
        let Some(c) = rd.getwc()? else { break };
        if c == '\n' {
            putwc(out, c);
            return Ok(true);
        }
        if !padding && last < ct {
            for _ in last..ct {
                putwc(out, ' ');
            }
            padding = true;
        }
        putwc(out, c);
    }
    Ok(false)
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);

    let longs = vec![
        LongDef { name: b"version".to_vec(), has_arg: 0, val: i32::from(b'V'), flag: false },
        LongDef { name: b"help".to_vec(), has_arg: 0, val: i32::from(b'h'), flag: false },
    ];
    let posix = sysabi::sys::getenv("POSIXLY_CORRECT").is_some();
    let mut eng = Engine::new(argv.clone(), 1, posix);
    loop {
        let opt = eng.getopt(b"Vh", &longs, false);
        if opt == -1 {
            break;
        }
        match u8::try_from(opt).unwrap_or(b'?') {
            b'V' => {
                ul::print_version(&short);
                return 0;
            }
            b'h' => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    // Os operandos vêm do argv permutado, sem olhar o optind.
    let av = &eng.argv;
    let mut first = 0u64;
    let mut last = 0u64;
    if av.len() > 1 {
        match ul::strtou64_or_err(&av[1], "first argument") {
            Ok(n) => first = n,
            Err(m) => {
                ul::warnx(&short, m);
                return 1;
            }
        }
    }
    if av.len() > 2 {
        match ul::strtou64_or_err(&av[2], "second argument") {
            Ok(n) => last = n,
            Err(m) => {
                ul::warnx(&short, m);
                return 1;
            }
        }
    }

    let (data, read_err) = match io::read_stdin() {
        Ok(d) => (d, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let mut rd = Reader { data, pos: 0, read_err };
    let mut out = io::stdout();
    loop {
        match process_input(&mut rd, &mut out, first, last) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                ul::warn(&short, "fgetwc() failed", e);
                return 1;
            }
        }
        sysabi::sys::checkpoint();
    }
    if out.flush().is_err() {
        return 1;
    }
    0
}
