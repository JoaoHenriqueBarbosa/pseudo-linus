//! `iconv` da glibc 2.41 (pacote libc-bin do Debian 13), portado de `iconv/iconv_prog.c`: converte
//! arquivos (ou o stdin) de uma codificação para outra.
//!
//! Comportamento:
//!
//! - `-f`/`--from-code` e `-t`/`--to-code` sem valor (ou com nome vazio antes do `//`) usam o
//!   codeset do locale: `UTF-8` no `C.UTF-8`, `ANSI_X3.4-1968` no `C`.
//! - Nomes sem diferenciar caixa, com os apelidos do `gconv-modules`; sufixos depois de `//`,
//!   separados por `,` ou `/`: `TRANSLIT` (tabela do locale, ver [`translit`]) e `IGNORE`. Os
//!   sufixos da origem não valem nada, como na glibc.
//! - Codificação desconhecida: `conversion from `X' is not supported`, `conversion to `Y' is not supported`
//!   ou `conversions from `X' and to `Y' are not supported`, a dica do argp e código 1.
//! - Sequência inválida na origem ou caractere sem representação no destino:
//!   `illegal input sequence at position N` (N é o deslocamento no arquivo), código 1; a saída
//!   produzida até ali sai antes, e os arquivos seguintes não são lidos.
//! - Caractere incompleto no fim: `incomplete character or shift sequence at end of buffer`.
//! - `-c` (ou `//IGNORE`) pula o que não converte; termina com 1, sem mensagem com `-c`, e com o
//!   aviso de posição no fim do arquivo com `//IGNORE` sozinho (o comportamento antigo da glibc).
//! - `-l` lista os nomes conhecidos, um por linha com o `//` (fora de terminal) ou em colunas.
//! - Erro de opção: mensagem do getopt, dica do argp e código 64.

mod charsets;
mod translit;

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, OFlags, sys};

use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

use charsets::{CHARSETS, Charset, Decoded, Decoder, Encoder};

const OPT_USAGE: i32 = 256;
const OPT_VERBOSE: i32 = 300;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("from-code", HasArg::Required, 'f' as i32),
    LongOpt::new("to-code", HasArg::Required, 't' as i32),
    LongOpt::new("list", HasArg::No, 'l' as i32),
    LongOpt::new("output", HasArg::Required, 'o' as i32),
    LongOpt::new("silent", HasArg::No, 's' as i32),
    LongOpt::new("verbose", HasArg::No, OPT_VERBOSE),
    LongOpt::new("help", HasArg::No, '?' as i32),
    LongOpt::new("usage", HasArg::No, OPT_USAGE),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

const HELP: &str = "Usage: iconv [OPTION...] [FILE...]
Convert encoding of given files from one encoding to another.

 Input/Output format specification:
  -f, --from-code=NAME       encoding of original text
  -t, --to-code=NAME         encoding for output

 Information:
  -l, --list                 list all known coded character sets

 Output control:
  -c                         omit invalid characters from output
  -o, --output=FILE          output file
  -s, --silent               suppress warnings
      --verbose              print progress information

  -?, --help                 Give this help list
      --usage                Give a short usage message
  -V, --version              Print program version

Mandatory or optional arguments to long options are also mandatory or optional
for any corresponding short options.

For bug reporting instructions, please see:
<http://www.debian.org/Bugs/>.
";

const USAGE: &str = "Usage: iconv [-lcs?V] [-f NAME] [-t NAME] [-o FILE] [--from-code=NAME]
            [--to-code=NAME] [--list] [--output=FILE] [--silent] [--verbose]
            [--help] [--usage] [--version] [FILE...]
";

const VERSION: &str = "iconv (Debian GLIBC 2.41-12+deb13u4) 2.41
Copyright (C) 2024 Free Software Foundation, Inc.
This is free software; see the source for copying conditions.  There is NO
warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
Written by Ulrich Drepper.
";

const TRY: &str = "Try `iconv --help' or `iconv --usage' for more information.\n";

const LIST_HEADER: &str = "The following list contains all the coded character sets known.  This does
not necessarily mean that all combinations of these names can be used for
the FROM and TO command line parameters.  One coded character set can be
listed with several different names (aliases).

  ";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// `true` se o `LC_CTYPE` do ambiente é o `C.UTF-8` (o único locale com UTF-8 no sandbox).
fn utf8_locale() -> bool {
    let mut name: Vec<u8> = b"C".to_vec();
    for var in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Some(v) = sys::getenv(var)
            && !v.is_empty()
        {
            name = v;
            break;
        }
    }
    let Some(dot) = name.iter().position(|b| *b == b'.') else {
        return false;
    };
    let lang = &name[..dot];
    let codeset: Vec<u8> = name[dot + 1..]
        .iter()
        .take_while(|b| **b != b'@')
        .filter(|b| b.is_ascii_alphanumeric())
        .map(|b| b.to_ascii_lowercase())
        .collect();
    (lang == b"C" || lang.starts_with(b"C_")) && codeset == b"utf8"
}

/// Uma especificação `NOME//SUFIXOS` já separada.
struct Spec {
    name: Vec<u8>,
    translit: bool,
    ignore: bool,
}

fn parse_spec(code: &[u8], locale_codeset: &str) -> Spec {
    let (name, suffixes) = match code.windows(2).position(|w| w == b"//") {
        Some(p) => (&code[..p], &code[p + 2..]),
        None => (code, &b""[..]),
    };
    let mut translit = false;
    let mut ignore = false;
    for s in suffixes.split(|b| *b == b',' || *b == b'/') {
        if s.eq_ignore_ascii_case(b"TRANSLIT") {
            translit = true;
        } else if s.eq_ignore_ascii_case(b"IGNORE") {
            ignore = true;
        }
    }
    let name = if name.is_empty() {
        locale_codeset.as_bytes().to_vec()
    } else {
        name.to_vec()
    };
    Spec {
        name,
        translit,
        ignore,
    }
}

/// O que interrompeu a conversão de um arquivo.
enum Failure {
    Illegal(usize),
    Incomplete,
}

/// O conversor aberto (o `iconv_t`): o estado do BOM vale para todos os arquivos.
struct Converter {
    dec: Decoder,
    enc: Encoder,
    translit: bool,
    utf8_locale: bool,
    ignore: bool,
}

impl Converter {
    /// Codifica `c`, recorrendo à transliteração quando pedida.
    fn put(&self, c: u32, out: &mut Vec<u8>) -> bool {
        if self.enc.encode(c, out) {
            return true;
        }
        if !self.translit {
            return false;
        }
        if let Some(rep) = translit::lookup(c, self.utf8_locale)
            && self.put_all(rep, out)
        {
            return true;
        }
        self.put_all(translit::DEFAULT_MISSING, out)
    }

    fn put_all(&self, rep: &str, out: &mut Vec<u8>) -> bool {
        let mut tmp = Vec::new();
        for ch in rep.chars() {
            if !self.enc.encode(u32::from(ch), &mut tmp) {
                return false;
            }
        }
        out.extend_from_slice(&tmp);
        true
    }

    /// Converte um arquivo inteiro. Devolve a falha (se houve) e se algo foi pulado pelo
    /// `IGNORE`.
    fn convert(&mut self, data: &[u8], out: &mut Vec<u8>) -> (Option<Failure>, bool) {
        if data.is_empty() {
            return (None, false);
        }
        self.enc.start(out);
        let mut skipped = false;
        let mut pos = 0;
        let mut steps = 0u32;
        while pos < data.len() {
            steps = steps.wrapping_add(1);
            if steps.is_multiple_of(4096) {
                sys::checkpoint();
            }
            match self.dec.decode(&data[pos..]) {
                Decoded::Char(c, n) => {
                    if self.put(c, out) {
                        pos += n;
                    } else if self.ignore {
                        skipped = true;
                        pos += n;
                    } else {
                        return (Some(Failure::Illegal(pos)), skipped);
                    }
                }
                Decoded::Skip(n) => pos += n,
                Decoded::Invalid(n) => {
                    if self.ignore {
                        skipped = true;
                        pos += n.max(1);
                    } else {
                        return (Some(Failure::Illegal(pos)), skipped);
                    }
                }
                Decoded::Incomplete => return (Some(Failure::Incomplete), skipped),
            }
        }
        (None, skipped)
    }
}

/// Destino da saída: stdout ou o arquivo do `-o`, aberto na primeira escrita.
struct Output {
    path: Option<Vec<u8>>,
    file: Option<io::File>,
}

impl Output {
    /// `Err` quando o arquivo de saída não abre (a mensagem já saiu).
    fn write(&mut self, data: &[u8]) -> Result<(), ()> {
        if data.is_empty() {
            return Ok(());
        }
        match &self.path {
            None => {
                let _ = io::stdout().write_all(data);
                Ok(())
            }
            Some(path) => {
                if self.file.is_none() {
                    match io::File::open_with(
                        path,
                        OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC,
                        0o666,
                    ) {
                        Ok(f) => self.file = Some(f),
                        Err(e) => {
                            let _ = io::flush_stdout();
                            io::eprint(format!(
                                "iconv: cannot open output file: {}\n",
                                e.message()
                            ));
                            return Err(());
                        }
                    }
                }
                if let Some(f) = self.file.as_mut() {
                    let _ = f.write_all(data);
                }
                Ok(())
            }
        }
    }
}

/// Mensagem de erro no formato do `error (0, 0, ...)`, com a saída descarregada antes.
fn error(msg: &str) {
    let _ = io::flush_stdout();
    io::eprint(format!("iconv: {msg}\n"));
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut from_code: Vec<u8> = Vec::new();
    let mut to_code: Vec<u8> = Vec::new();
    let mut omit_invalid = false;
    let mut output_file: Option<Vec<u8>> = None;
    let mut verbose = false;

    let mut getopt = Getopt::from_env(&argv[1..], "f:t:lco:s?V", LONGOPTS);
    while let Some(r) = getopt.next_opt() {
        let opt = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n{TRY}", e.message(&argv0)));
                return 64;
            }
        };
        let arg = opt.arg.clone().unwrap_or_default();
        match opt.id {
            id if id == 'f' as i32 => from_code = arg,
            id if id == 't' as i32 => to_code = arg,
            id if id == 'l' as i32 => {
                print_known_names();
                return 0;
            }
            id if id == 'c' as i32 => omit_invalid = true,
            id if id == 'o' as i32 => output_file = Some(arg),
            id if id == 's' as i32 => {}
            OPT_VERBOSE => verbose = true,
            id if id == '?' as i32 => {
                let _ = io::stdout().write_all(HELP.as_bytes());
                return 0;
            }
            OPT_USAGE => {
                let _ = io::stdout().write_all(USAGE.as_bytes());
                return 0;
            }
            id if id == 'V' as i32 => {
                let _ = io::stdout().write_all(VERSION.as_bytes());
                return 0;
            }
            _ => {}
        }
    }
    let files = getopt.operands();

    let utf8 = utf8_locale();
    let codeset = if utf8 { "UTF-8" } else { "ANSI_X3.4-1968" };
    let from = parse_spec(&from_code, codeset);
    let to = parse_spec(&to_code, codeset);
    let from_cs = charsets::lookup(&from.name);
    let to_cs = charsets::lookup(&to.name);
    let pretty = |code: &[u8]| -> String {
        if code.is_empty() {
            codeset.to_string()
        } else {
            io::lossy(code)
        }
    };
    let (from_cs, to_cs): (&Charset, &Charset) = match (from_cs, to_cs) {
        (Some(f), Some(t)) => (f, t),
        (None, None) => {
            error(&format!(
                "conversions from `{}' and to `{}' are not supported",
                pretty(&from_code),
                pretty(&to_code)
            ));
            io::eprint(TRY);
            return 1;
        }
        (None, Some(_)) => {
            error(&format!(
                "conversion from `{}' is not supported",
                pretty(&from_code)
            ));
            io::eprint(TRY);
            return 1;
        }
        (Some(_), None) => {
            error(&format!("conversion to `{}' is not supported", pretty(&to_code)));
            io::eprint(TRY);
            return 1;
        }
    };
    let mut conv = Converter {
        dec: Decoder::new(from_cs.kind),
        enc: Encoder::new(to_cs.kind),
        translit: to.translit,
        utf8_locale: utf8,
        ignore: omit_invalid || to.ignore,
    };
    let mut output = Output {
        path: output_file,
        file: None,
    };

    let files = if files.is_empty() {
        vec![b"-".to_vec()]
    } else {
        files
    };
    let mut status = 0;
    for name in &files {
        if verbose {
            io::eprint(format!("{}:\n", io::lossy(name)));
        }
        let data = if name == b"-" {
            io::read_stdin()
        } else {
            match io::File::open(name) {
                Ok(mut f) => f.read_to_end_sys(),
                Err(e) => {
                    error(&format!(
                        "cannot open input file `{}': {}",
                        io::lossy(name),
                        e.message()
                    ));
                    status = 1;
                    continue;
                }
            }
        };
        let data = match data {
            Ok(d) => d,
            Err(e) => {
                error(&format!("error while reading the input: {}", e.message()));
                status = 1;
                break;
            }
        };
        let mut out = Vec::new();
        let (failure, skipped) = conv.convert(&data, &mut out);
        if output.write(&out).is_err() {
            return 1;
        }
        match failure {
            Some(Failure::Illegal(pos)) => {
                error(&format!("illegal input sequence at position {pos}"));
                status = 1;
                break;
            }
            Some(Failure::Incomplete) => {
                error("incomplete character or shift sequence at end of buffer");
                status = 1;
                break;
            }
            None => {}
        }
        if skipped {
            status = 1;
            if !omit_invalid {
                // Só o `//IGNORE`: o `iconv()` devolve EILSEQ no fim do bloco, e o programa
                // avisa na posição em que parou.
                error(&format!("illegal input sequence at position {}", data.len()));
                break;
            }
        }
    }
    if io::flush_stdout().is_err() {
        return 1;
    }
    status
}

/// `print_known_names`: os nomes ordenados com o `strverscmp`.
fn print_known_names() {
    let mut names: Vec<&'static str> = CHARSETS
        .iter()
        .flat_map(|cs| cs.names.iter().copied())
        .filter(|n| *n != "INTERNAL")
        .collect();
    names.sort_by(|a, b| strverscmp(a.as_bytes(), b.as_bytes()));
    names.dedup();
    let mut out = io::stdout();
    if io::stdout_is_tty() {
        let _ = out.write_all(LIST_HEADER.as_bytes());
        let mut column = 2usize;
        let mut first = true;
        for s in names {
            let trimmed = s.trim_end_matches('/');
            if !trimmed.bytes().any(|b| b.is_ascii_alphanumeric()) {
                continue;
            }
            if !first {
                if column + trimmed.len() > 77 {
                    let _ = out.write_all(b",\n  ");
                    column = 2;
                } else {
                    let _ = out.write_all(b", ");
                    column += 2;
                }
            }
            first = false;
            let _ = out.write_all(trimmed.as_bytes());
            column += trimmed.len();
        }
        let _ = out.write_all(b"\n");
    } else {
        for s in names {
            let _ = out.write_all(s.as_bytes());
            let _ = out.write_all(b"\n");
        }
    }
}

/// `strverscmp` da glibc: compara trechos de dígitos como números (com a regra dos zeros à
/// esquerda como parte fracionária).
fn strverscmp(s1: &[u8], s2: &[u8]) -> std::cmp::Ordering {
    const S_N: usize = 0;
    const S_I: usize = 3;
    const S_F: usize = 6;
    const S_Z: usize = 9;
    const CMP: i8 = 2;
    const LEN: i8 = 3;
    const NEXT_STATE: [usize; 12] = [S_N, S_I, S_Z, S_N, S_I, S_I, S_N, S_F, S_F, S_N, S_F, S_Z];
    const RESULT_TYPE: [i8; 36] = [
        CMP, CMP, CMP, CMP, LEN, CMP, CMP, CMP, CMP, //
        CMP, -1, -1, 1, LEN, LEN, 1, LEN, LEN, //
        CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP, //
        CMP, 1, 1, -1, CMP, CMP, -1, CMP, CMP,
    ];
    let at = |s: &[u8], i: usize| -> u8 { s.get(i).copied().unwrap_or(0) };
    let class = |c: u8| -> usize { usize::from(c == b'0') + usize::from(c.is_ascii_digit()) };
    let (mut i1, mut i2) = (0usize, 0usize);
    let mut c1 = at(s1, i1);
    let mut c2 = at(s2, i2);
    i1 += 1;
    i2 += 1;
    let mut state = S_N + class(c1);
    let mut diff = i32::from(c1) - i32::from(c2);
    while diff == 0 {
        if c1 == 0 {
            return std::cmp::Ordering::Equal;
        }
        state = NEXT_STATE[state];
        c1 = at(s1, i1);
        c2 = at(s2, i2);
        i1 += 1;
        i2 += 1;
        state += class(c1);
        diff = i32::from(c1) - i32::from(c2);
    }
    let result = RESULT_TYPE[state * 3 + class(c2)];
    let value = match result {
        CMP => diff,
        LEN => {
            loop {
                let a = at(s1, i1);
                i1 += 1;
                if !a.is_ascii_digit() {
                    break;
                }
                let b = at(s2, i2);
                i2 += 1;
                if !b.is_ascii_digit() {
                    return std::cmp::Ordering::Greater;
                }
            }
            if at(s2, i2).is_ascii_digit() {
                -1
            } else {
                diff
            }
        }
        other => i32::from(other),
    };
    value.cmp(&0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn version_sort() {
        assert_eq!(strverscmp(b"437//", b"850//"), Ordering::Less);
        assert_eq!(strverscmp(b"ISO-8859-2//", b"ISO-8859-10//"), Ordering::Less);
        assert_eq!(strverscmp(b"CP1250//", b"CP437//"), Ordering::Greater);
        assert_eq!(strverscmp(b"abc", b"abc"), Ordering::Equal);
        assert_eq!(strverscmp(b"a", b"b"), Ordering::Less);
    }

    #[test]
    fn spec_suffixes() {
        let s = parse_spec(b"ascii//TRANSLIT,ignore", "UTF-8");
        assert_eq!(s.name, b"ascii");
        assert!(s.translit && s.ignore);
        let s = parse_spec(b"//IGNORE", "UTF-8");
        assert_eq!(s.name, b"UTF-8");
        assert!(!s.translit && s.ignore);
    }

    fn conv(from: &str, to: &str, translit: bool, ignore: bool) -> Converter {
        Converter {
            dec: Decoder::new(charsets::lookup(from.as_bytes()).unwrap().kind),
            enc: Encoder::new(charsets::lookup(to.as_bytes()).unwrap().kind),
            translit,
            utf8_locale: true,
            ignore,
        }
    }

    #[test]
    fn conversions() {
        let mut out = Vec::new();
        let (f, _) = conv("UTF-8", "ISO-8859-1", false, false).convert("é\n".as_bytes(), &mut out);
        assert!(f.is_none());
        assert_eq!(out, b"\xe9\n");

        let mut out = Vec::new();
        let (f, _) = conv("UTF-8", "UTF-16", false, false).convert(b"a", &mut out);
        assert!(f.is_none());
        assert_eq!(out, b"\xff\xfea\x00");

        let mut out = Vec::new();
        let (f, _) = conv("UTF-8", "ASCII", false, false).convert("aé".as_bytes(), &mut out);
        assert!(matches!(f, Some(Failure::Illegal(1))));
        assert_eq!(out, b"a");

        let mut out = Vec::new();
        let (f, _) = conv("UTF-8", "ASCII", true, false).convert("é€".as_bytes(), &mut out);
        assert!(f.is_none());
        assert_eq!(out, b"eEUR");

        let mut out = Vec::new();
        let (f, skipped) = conv("UTF-8", "ASCII", false, true).convert("aéb".as_bytes(), &mut out);
        assert!(f.is_none() && skipped);
        assert_eq!(out, b"ab");

        let mut out = Vec::new();
        let (f, _) = conv("UTF-16", "UTF-8", false, false).convert(b"\xff\xfeA\x00", &mut out);
        assert!(f.is_none());
        assert_eq!(out, b"A");

        let mut out = Vec::new();
        let (f, _) = conv("UTF-8", "UTF-8", false, false).convert(b"a\xc3", &mut out);
        assert!(matches!(f, Some(Failure::Incomplete)));
        assert_eq!(out, b"a");
    }
}
