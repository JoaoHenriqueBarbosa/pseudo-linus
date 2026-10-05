//! Peças comuns das ferramentas do util-linux 2.41 e do debianutils 5.23: nome curto do programa
//! (`program_invocation_short_name`), as mensagens padrão de `err`/`warnx`/`errtryhelp` e as de
//! `--version`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Errno;

use super::io;

/// `program_invocation_short_name`: o `argv[0]` sem o diretório.
pub fn short_name(args: &[OsString]) -> String {
    let a = io::argv0(args);
    match a.rfind('/') {
        Some(p) => a[p + 1..].to_string(),
        None => a,
    }
}

/// `warnx(3)`: `<prog>: <msg>` no stderr.
pub fn warnx(short: &str, msg: impl AsRef<str>) {
    io::eprint(format!("{short}: {}\n", msg.as_ref()));
}

/// `warn(3)` com errno: `<prog>: <msg>: <strerror>`.
pub fn warn(short: &str, msg: impl AsRef<str>, e: Errno) {
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

/// Lê um inteiro sem sinal como o `strtoul_or_err`/`strtou32_or_err` do util-linux (base 10, rejeita
/// lixo e vazio). `None` quando inválido ou fora de faixa.
pub fn parse_u64(s: &[u8]) -> Option<u64> {
    let t = std::str::from_utf8(s).ok()?;
    let t = t.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse::<u64>().ok()
}

/// `strtou32_or_err` do util-linux: o número decimal sem sinal, ou o texto do erro (sem o prefixo do
/// programa): `<what>: '<arg>'` pra lixo e `<what>: '<arg>': Numerical result out of range` pra
/// negativo ou maior que 32 bits.
pub fn strtou32_or_err(arg: &[u8], what: &str) -> Result<u32, String> {
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
    match body.parse::<u64>() {
        Ok(n) if n <= u64::from(u32::MAX) => Ok(n as u32),
        _ => Err(range()),
    }
}

/// `isspace` do locale C.
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}
