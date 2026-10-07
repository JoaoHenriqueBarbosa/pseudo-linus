//! `hexdump` e `hd` do util-linux 2.41.5 (pacote bsdextrautils do Debian 13).
//!
//! Porte em Rust do `text-utils/hexdump.c`, `hexdump-parse.c`, `hexdump-display.c` e
//! `hexdump-conv.c` do util-linux (repositório <https://github.com/util-linux/util-linux>, tag
//! `v2.41`, idêntica à 2.41.3 nesses arquivos), que são o hexdump do BSD sob a licença abaixo, e do
//! `parse_size` de `lib/strutils.c` (domínio público). A tabela de cores e o tratamento de
//! `--color` seguem o `lib/colors.c` (domínio público), conferidos no oráculo.
//!
//! ```text
//! Copyright (c) 1989 The Regents of the University of California.
//! All rights reserved.
//!
//! Redistribution and use in source and binary forms, with or without
//! modification, are permitted provided that the following conditions
//! are met:
//! 1. Redistributions of source code must retain the above copyright
//!    notice, this list of conditions and the following disclaimer.
//! 2. Redistributions in binary form must reproduce the above copyright
//!    notice, this list of conditions and the following disclaimer in the
//!    documentation and/or other materials provided with the distribution.
//! 3. All advertising materials mentioning features or use of this software
//!    must display the following acknowledgement:
//!     This product includes software developed by the University of
//!     California, Berkeley and its contributors.
//! 4. Neither the name of the University nor the names of its contributors
//!    may be used to endorse or promote products derived from this software
//!    without specific prior written permission.
//!
//! THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
//! ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
//! IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
//! ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
//! FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
//! DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
//! OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
//! HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
//! LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
//! OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
//! SUCH DAMAGE.
//! ```
//!
//! Como o original: a entrada é lida em blocos do tamanho do maior formato; cada formato (`-e`,
//! `-f`, ou os prontos de `-b -c -C -d -o -x -X`) imprime o bloco inteiro; blocos repetidos viram
//! `*` (sem `-v`); o último bloco parcial é completado com zeros e as conversões depois do fim saem
//! como espaços; `%_A` imprime o endereço final. Os arquivos são lidos em sequência como um fluxo só
//! (o `freopen` do stdin do C), `-s` pula por arquivo inteiro quando o salto passa do tamanho.
//! O `%s` sem NUL lê até o fim do bloco (no C ele continuaria pela memória depois do bloco; o
//! resultado só coincide quando a sobra do `calloc` é zero, que é o caso comum).

mod cfmt;
mod display;
mod parse;
mod size;

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, Fd};
use ul_common::fsutil::after_last_slash;

use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

use cfmt::Arg;

/// Tipo de uma unidade de impressão (os `F_*` do C).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Address,
    Bpad,
    C,
    Char,
    Dbl,
    Int,
    P,
    Str,
    Text,
    U,
    Uint,
}

/// Unidade de cor (`struct hexdump_clr`).
#[derive(Clone, Debug, Default)]
struct Clr {
    /// Sequência de escape; vazia numa unidade sem cor (lista `_L[]`).
    fmt: &'static str,
    offt: i64,
    range: i32,
    val: i32,
    str: Option<Vec<u8>>,
    invert: bool,
}

/// Unidade de impressão (`struct hexdump_pr`).
#[derive(Clone, Debug)]
struct Pr {
    kind: Kind,
    bcnt: i32,
    /// O formato printf, como o C o monta (`texto%02llx`).
    fmt: Vec<u8>,
    /// Posição do caractere de conversão em `fmt` (o `cchar`).
    cchar: usize,
    colorlist: Option<Vec<Clr>>,
    /// Posição do espaço final que some na última repetição.
    nospace: Option<usize>,
}

impl Pr {
    fn text(fmt: Vec<u8>) -> Pr {
        Pr {
            kind: Kind::Text,
            bcnt: 0,
            fmt,
            cchar: 0,
            colorlist: None,
            nospace: None,
        }
    }
}

/// Unidade de formato (`struct hexdump_fu`).
#[derive(Clone, Debug)]
struct Fu {
    reps: i32,
    bcnt: i32,
    /// Contagem de repetição explícita (`F_SETREP`).
    setrep: bool,
    /// Unidade com `%_A` (`F_IGNORE`): só sai no fim.
    ignore: bool,
    fmt: Vec<u8>,
    prs: Vec<Pr>,
}

/// Um formato (`struct hexdump_fs`).
#[derive(Clone, Debug)]
struct Fs {
    fus: Vec<Fu>,
    bcnt: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VFlag {
    All,
    Dup,
    First,
    Wait,
}

/// O "stdin" do C, que o `freopen` troca a cada arquivo.
enum Input {
    Stdin,
    File(io::File),
    /// `freopen` falhou: o fluxo ficou fechado.
    Closed,
}

impl Input {
    fn fd(&self) -> Option<Fd> {
        match self {
            Input::Stdin => Some(Fd::STDIN),
            Input::File(f) => Some(f.fd()),
            Input::Closed => None,
        }
    }
}

/// Estado do programa (`struct hexdump` mais os estáticos do C).
struct Hexdump {
    prog: String,
    fss: Vec<Fs>,
    blocksize: usize,
    exitval: i32,
    /// `-n`; -1 é sem limite.
    length: i64,
    skip: i64,
    endfu: Option<(usize, usize)>,
    colors: bool,
    vflag: VFlag,
    address: i64,
    eaddress: i64,
    curp: Vec<u8>,
    savp: Vec<u8>,
    started: bool,
    ateof: bool,
    files: Vec<Vec<u8>>,
    argi: usize,
    /// O `_argv[-1]` do C: nome usado na mensagem de erro de leitura.
    last_name: Vec<u8>,
    done: bool,
    input: Input,
    /// Saída pendente, escrita no stdout do processo a cada bloco e antes de qualquer erro.
    out: Vec<u8>,
}

const OPT_HELP: i32 = 'h' as i32;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("one-byte-octal", HasArg::No, 'b' as i32),
    LongOpt::new("one-byte-hex", HasArg::No, 'X' as i32),
    LongOpt::new("one-byte-char", HasArg::No, 'c' as i32),
    LongOpt::new("canonical", HasArg::No, 'C' as i32),
    LongOpt::new("two-bytes-decimal", HasArg::No, 'd' as i32),
    LongOpt::new("two-bytes-octal", HasArg::No, 'o' as i32),
    LongOpt::new("two-bytes-hex", HasArg::No, 'x' as i32),
    LongOpt::new("format", HasArg::Required, 'e' as i32),
    LongOpt::new("format-file", HasArg::Required, 'f' as i32),
    LongOpt::new("color", HasArg::Optional, 'L' as i32),
    LongOpt::new("length", HasArg::Required, 'n' as i32),
    LongOpt::new("skip", HasArg::Required, 's' as i32),
    LongOpt::new("no-squeezing", HasArg::No, 'v' as i32),
    LongOpt::new("help", HasArg::No, OPT_HELP),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

const HEX_OFFT: &[u8] = b"\"%07.7_Ax\n\"";

/// Entrada do `hexdump` e do `hd` (o `hd` é o mesmo programa; sem formato, usa o canônico).
pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ColorMode {
    Undef,
    Auto,
    Never,
    Always,
}

fn usage(prog: &str) -> String {
    format!(
        "\nUsage:\n {prog} [options] <file>...\n\n\
         Display file contents in hexadecimal, decimal, octal, or ascii.\n\n\
         Options:\n \
         -b, --one-byte-octal      one-byte octal display\n \
         -X, --one-byte-hex        one-byte hexadecimal display\n \
         -c, --one-byte-char       one-byte character display\n \
         -C, --canonical           canonical hex+ASCII display\n \
         -d, --two-bytes-decimal   two-byte decimal display\n \
         -o, --two-bytes-octal     two-byte octal display\n \
         -x, --two-bytes-hex       two-byte hexadecimal display\n \
         -L, --color[=<mode>]      interpret color formatting specifiers\n                             \
         colors are enabled by default\n \
         -e, --format <format>     format string to be used for displaying data\n \
         -f, --format-file <file>  file that contains format strings\n \
         -n, --length <length>     interpret only length bytes of input\n \
         -s, --skip <offset>       skip offset bytes from the beginning\n \
         -v, --no-squeezing        output identical lines\n\n \
         -h, --help                display this help\n \
         -V, --version             display version\n\n\
         Arguments:\n \
         Values for <length> and <offset> may be followed by a suffix: KiB, MiB,\n \
         GiB, TiB, PiB, EiB, ZiB, or YiB (where the \"iB\" is optional).\n\n\
         For more details see hexdump(1).\n"
    )
}

/// `errx`: mensagem e saída 1 (o stdout pendente sai antes, como no `exit(3)`).
fn fatal(hex: &mut Hexdump, msg: &str) -> ! {
    hex.flush_out();
    io::eprint(format!("{}: {msg}\n", hex.prog));
    sysabi::sys::exit(1)
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let prog = {
        let a = argv.first().cloned().unwrap_or_default();
        io::lossy(after_last_slash(&a))
    };
    let mut hex = Hexdump {
        prog: prog.clone(),
        fss: Vec::new(),
        blocksize: 0,
        exitval: 0,
        length: -1,
        skip: 0,
        endfu: None,
        colors: false,
        vflag: VFlag::First,
        address: 0,
        eaddress: 0,
        curp: Vec::new(),
        savp: Vec::new(),
        started: false,
        ateof: true,
        files: Vec::new(),
        argi: 0,
        last_name: argv.first().cloned().unwrap_or_default(),
        done: false,
        input: Input::Stdin,
        out: Vec::new(),
    };

    let rest = argv.get(1..).unwrap_or(&[]);
    let mut g = Getopt::from_env(rest, "bXcCde:f:L::n:os:vxhV", LONGOPTS);
    let mut colormode = ColorMode::Undef;
    let mut consumed: Vec<Vec<u8>> = Vec::new();
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!(
                    "{}\nTry '{prog} --help' for more information.\n",
                    e.message(&argv0)
                ));
                return 1;
            }
        };
        consumed.push(opt.spelled().into_bytes());
        if let Some(a) = &opt.arg {
            consumed.push(a.clone());
        }
        let res = match u8::try_from(opt.id).map(char::from).unwrap_or('\0') {
            'b' => hex
                .add_fmt(HEX_OFFT)
                .and_then(|_| hex.add_fmt(b"\"%07.7_ax \" 16/1 \"%03o \" \"\\n\"")),
            'X' => hex
                .add_fmt(b"\"%07.7_Ax\n\"")
                .and_then(|_| hex.add_fmt(b"\"%07.7_ax \" 16/1 \" %02x \" \"\\n\"")),
            'c' => hex
                .add_fmt(HEX_OFFT)
                .and_then(|_| hex.add_fmt(b"\"%07.7_ax \" 16/1 \"%3_c \" \"\\n\"")),
            'C' => add_canonical(&mut hex),
            'd' => hex
                .add_fmt(HEX_OFFT)
                .and_then(|_| hex.add_fmt(b"\"%07.7_ax \" 8/2 \"  %05u \" \"\\n\"")),
            'e' => hex.add_fmt(opt.arg.as_deref().unwrap_or_default()),
            'f' => {
                let name = opt.arg.clone().unwrap_or_default();
                match io::read_path(&name) {
                    Ok(data) => parse::parse_format_file(&data, &mut hex),
                    // Diretório abre no `fopen` e o `getline` falha calado.
                    Err(Errno::EISDIR) => Ok(()),
                    Err(e) => Err(format!("can't read {}: {}", io::lossy(&name), e.message())),
                }
            }
            'L' => {
                colormode = ColorMode::Auto;
                if let Some(a) = &opt.arg {
                    let p = a.strip_prefix(b"=").unwrap_or(a);
                    let s = String::from_utf8_lossy(p).to_ascii_lowercase();
                    colormode = match s.as_str() {
                        "auto" => ColorMode::Auto,
                        "never" => ColorMode::Never,
                        "always" => ColorMode::Always,
                        _ => {
                            return fatal_ret(
                                &prog,
                                &format!("unsupported color mode: '{}'", io::lossy(p)),
                            );
                        }
                    };
                }
                Ok(())
            }
            'n' => match size::parse_size(opt.arg.as_deref().unwrap_or_default()) {
                Ok(v) => {
                    hex.length = v as i64;
                    Ok(())
                }
                Err(e) => Err(format!(
                    "failed to parse length: '{}': {}",
                    opt.arg_str(),
                    e.message()
                )),
            },
            'o' => hex
                .add_fmt(HEX_OFFT)
                .and_then(|_| hex.add_fmt(b"\"%07.7_ax \" 8/2 \" %06o \" \"\\n\"")),
            's' => match size::parse_size(opt.arg.as_deref().unwrap_or_default()) {
                Ok(v) => {
                    hex.skip = v as i64;
                    Ok(())
                }
                Err(e) => Err(format!(
                    "failed to parse offset: '{}': {}",
                    opt.arg_str(),
                    e.message()
                )),
            },
            'v' => {
                hex.vflag = VFlag::All;
                Ok(())
            }
            'x' => hex
                .add_fmt(HEX_OFFT)
                .and_then(|_| hex.add_fmt(b"\"%07.7_ax \" 8/2 \"   %04x \" \"\\n\"")),
            'h' => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&prog).as_bytes());
                return 0;
            }
            'V' => {
                let mut out = io::stdout();
                let _ = out.write_all(format!("{prog} from util-linux 2.41.5\n").as_bytes());
                return 0;
            }
            _ => Ok(()),
        };
        if let Err(msg) = res {
            return fatal_ret(&prog, &msg);
        }
    }
    hex.files = g.operands();
    // `_argv[-1]` antes do primeiro operando: o último argumento de opção (o getopt da glibc põe os
    // operandos no fim do argv) ou o próprio argv[0].
    if let Some(last) = consumed.last() {
        hex.last_name = last.clone();
    }

    if hex.fss.is_empty() {
        let r = if prog == "hd" {
            add_canonical(&mut hex)
        } else {
            hex.add_fmt(HEX_OFFT)
                .and_then(|_| hex.add_fmt(b"\"%07.7_ax \" 8/2 \"%04x \" \"\\n\""))
        };
        if let Err(msg) = r {
            return fatal_ret(&prog, &msg);
        }
    }
    hex.colors = colors_wanted(colormode);

    let mut blocksize: i64 = 0;
    for i in 0..hex.fss.len() {
        let b = match Hexdump::block_size(&hex.fss[i]) {
            Ok(b) => b,
            Err(msg) => return fatal_ret(&prog, &msg),
        };
        hex.fss[i].bcnt = b;
        blocksize = blocksize.max(b);
    }
    hex.blocksize = usize::try_from(blocksize).unwrap_or(0);
    for i in 0..hex.fss.len() {
        if let Err(msg) = hex.rewrite_rules(i) {
            return fatal_ret(&prog, &msg);
        }
    }

    hex.display();
    hex.flush_out();
    hex.exitval
}

fn fatal_ret(prog: &str, msg: &str) -> i32 {
    io::eprint(format!("{prog}: {msg}\n"));
    1
}

fn add_canonical(hex: &mut Hexdump) -> Result<(), String> {
    hex.add_fmt(b"\"%08.8_Ax\n\"")?;
    hex.add_fmt(b"\"%08.8_ax  \" 8/1 \"%02x \" \"  \" 8/1 \"%02x \" ")?;
    hex.add_fmt(b"\"  |\" 16/1 \"%_p\" \"|\\n\"")
}

/// `colors_init` do util-linux: `always` sempre colore; `auto` (`-L`) e o padrão só num terminal
/// que tenha cores, e o padrão também respeita `NO_COLOR`.
fn colors_wanted(mode: ColorMode) -> bool {
    let tty = io::stdout_is_tty();
    let term_ready = || sysabi::sys::getenv("TERM").is_some_and(|t| !t.is_empty() && t != b"dumb");
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => tty && term_ready(),
        ColorMode::Undef => tty && sysabi::sys::getenv("NO_COLOR").is_none() && term_ready(),
    }
}

impl Hexdump {
    /// Passa a saída pendente pro stdout do processo (que tem o buffer do stdio).
    fn flush_out(&mut self) {
        if !self.out.is_empty() {
            let mut so = io::stdout();
            let _ = so.write_all(&self.out);
            self.out.clear();
        }
    }

    /// `warn`/`warnx`: `prog: msg` no stderr, depois da saída que o C já teria posto no buffer.
    fn warn(&mut self, msg: &str) {
        self.flush_out();
        io::eprint(format!("{}: {msg}\n", self.prog));
    }

    fn printf(&mut self, fmt: &[u8], arg: Arg<'_>) {
        cfmt::cprintf(&mut self.out, fmt, arg);
    }
}

#[cfg(test)]
mod tests;
