//! Peças comuns das ferramentas do util-linux 2.41 e do debianutils 5.23: nome curto do programa
//! (`program_invocation_short_name`), as mensagens padrão de `err`/`warnx`/`errtryhelp` e as de
//! `--version`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Errno;

use super::display_width;
use super::io;

/// `program_invocation_short_name`: o `argv[0]` sem o diretório.
pub fn short_name(args: &[OsString]) -> String {
    let a = io::argv0(args);
    match a.rfind('/') {
        Some(p) => a[p + 1..].to_string(),
        None => a,
    }
}

/// `warnx(3)`: `<prog>: <msg>` no stderr. A glibc descarrega o stdout antes de escrever.
pub fn warnx(short: &str, msg: impl AsRef<str>) {
    let _ = io::flush_stdout();
    io::eprint(format!("{short}: {}\n", msg.as_ref()));
}

/// `warn(3)` com errno: `<prog>: <msg>: <strerror>`. A glibc descarrega o stdout antes de escrever.
pub fn warn(short: &str, msg: impl AsRef<str>, e: Errno) {
    let _ = io::flush_stdout();
    io::eprint(format!("{short}: {}: {}\n", msg.as_ref(), e.message()));
}

/// A segunda linha de `errtryhelp`: `Try '<prog> --help' for more information.`
pub fn errtryhelp(short: &str) {
    io::eprint(format!("Try '{short} --help' for more information.\n"));
}

/// `print_version`: `<prog> from util-linux 2.41.5`.
pub fn print_version(short: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(format!("{short} from util-linux 2.41.5\n").as_bytes());
}

/// `strtou64_or_err` do util-linux: o número decimal sem sinal, ou o texto do erro (sem o prefixo do
/// programa): `<what>: '<arg>'` pra lixo e `<what>: '<arg>': Numerical result out of range` pra
/// negativo ou maior que 64 bits. Aceita espaço à frente e `+`; `-0` vale zero.
pub fn strtou64_or_err(arg: &[u8], what: &str) -> Result<u64, String> {
    let text = io::lossy(arg);
    let range = || format!("{what}: '{text}': {}", Errno::ERANGE.message());
    let invalid = || format!("{what}: '{text}'");
    let digits = text.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let body = digits.strip_prefix('+').unwrap_or(digits);
    if let Some(neg) = body.strip_prefix('-') {
        if !neg.is_empty() && neg.bytes().all(|b| b.is_ascii_digit()) {
            // "-0" vale zero: strtoimax não dá negativo.
            return if neg.bytes().all(|b| b == b'0') { Ok(0) } else { Err(range()) };
        }
        return Err(invalid());
    }
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    body.parse::<u64>().map_err(|_| range())
}

/// `strtou32_or_err`: como [`strtou64_or_err`], com o teto de 32 bits (acima dele é fora de faixa).
pub fn strtou32_or_err(arg: &[u8], what: &str) -> Result<u32, String> {
    let n = strtou64_or_err(arg, what)?;
    u32::try_from(n).map_err(|_| format!("{what}: '{}': {}", io::lossy(arg), Errno::ERANGE.message()))
}

/// `isspace` do locale C.
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `wcwidth` do C.UTF-8: -1 pra controle, 0 pra combinante, 1 ou 2 pro resto.
pub fn wcwidth(c: char) -> i32 {
    let u = c as u32;
    if u < 0x20 || (0x7f..0xa0).contains(&u) {
        return -1;
    }
    display_width(c.encode_utf8(&mut [0; 4])) as i32
}

/// `iswspace` da glibc em C.UTF-8: os espaços Unicode, exceto os sem quebra.
pub fn is_wspace(c: char) -> bool {
    matches!(c as u32, 0x09..=0x0d | 0x20 | 0x1680 | 0x2000..=0x2006 | 0x2008..=0x200a | 0x2028 | 0x2029 | 0x205f | 0x3000)
}

/// Decodificação de UTF-8 como o `fgetwc` da glibc em C.UTF-8.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Wide {
    Char(char),
    /// Byte inicial de sequência inválida (EILSEQ).
    Invalid(u8),
    /// Fim da entrada, ou sequência incompleta que a entrada interrompe.
    Eof,
    /// Sequência incompleta no fim da entrada: pro `fgetwc` é EILSEQ, pro `getwchar` do `col` é fim.
    Truncated,
}

/// O próximo caractere de `data` a partir de `pos`, sem consumir.
pub fn peek_wide(data: &[u8], pos: usize) -> Wide {
    let rest = &data[pos.min(data.len())..];
    let Some(&first) = rest.first() else { return Wide::Eof };
    if first < 0x80 {
        return Wide::Char(char::from(first));
    }
    let need = match first {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return Wide::Invalid(first),
    };
    let take = need.min(rest.len());
    match std::str::from_utf8(&rest[..take]) {
        Ok(s) => match s.chars().next() {
            Some(c) => Wide::Char(c),
            None => Wide::Eof,
        },
        Err(e) => {
            if e.error_len().is_none() && take == rest.len() && take < need {
                Wide::Truncated
            } else {
                Wide::Invalid(first)
            }
        }
    }
}
