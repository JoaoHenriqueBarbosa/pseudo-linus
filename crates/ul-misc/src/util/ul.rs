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

/// `strtoumax(s, &end, 0)`: o valor e o índice onde a leitura parou (0 quando não leu nada).
/// Overflow é ERANGE.
fn strtoumax0(s: &[u8]) -> Result<(u64, usize), Errno> {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut i = 0;
    while is_space(at(i)) {
        i += 1;
    }
    if at(i) == b'+' {
        i += 1;
    }
    let (base, mut j): (u64, usize) = if at(i) == b'0' && matches!(at(i + 1), b'x' | b'X') && at(i + 2).is_ascii_hexdigit() {
        (16, i + 2)
    } else if at(i) == b'0' {
        (8, i)
    } else {
        (10, i)
    };
    let digit = |b: u8| -> Option<u64> { char::from(b).to_digit(base as u32).map(u64::from) };
    let start = j;
    let mut value: u64 = 0;
    let mut overflow = false;
    while let Some(d) = digit(at(j)) {
        match value.checked_mul(base).and_then(|v| v.checked_add(d)) {
            Some(v) => value = v,
            None => overflow = true,
        }
        j += 1;
    }
    if j == start {
        return Ok((0, 0));
    }
    if overflow {
        return Err(Errno::ERANGE);
    }
    Ok((value, j))
}

/// `do_scale_by_power`: multiplica `x` por `base` `power` vezes; `false` no overflow.
fn scale_by_power(x: &mut u64, base: u64, power: u32) -> bool {
    for _ in 0..power {
        if u64::MAX / base < *x {
            return false;
        }
        *x *= base;
    }
    true
}

/// `parse_size`/`strtosize` do util-linux: número (decimal, octal `0..` ou hexa `0x..`) com sufixo
/// opcional `K`, `M`, `G`, `T`, `P`, `E`, `Z`, `Y` (potência de 1024; `KiB`, ou minúsculas) ou `KB`
/// (potência de 1000), e fração com ponto (`1.5K`, `0.5MB`). Erro é EINVAL (lixo, vazio, negativo) ou
/// ERANGE (não cabe em 64 bits).
pub fn parse_size(s: &[u8]) -> Result<u64, Errno> {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    if s.is_empty() {
        return Err(Errno::EINVAL);
    }
    let mut p = 0;
    while is_space(at(p)) {
        p += 1;
    }
    if at(p) == b'-' {
        return Err(Errno::EINVAL);
    }
    let (mut x, end) = strtoumax0(s)?;
    if end == 0 {
        return Err(Errno::EINVAL);
    }
    if at(end) == 0 {
        return Ok(x); // without suffix
    }
    p = end;
    let mut frac: u64 = 0;
    let mut frac_zeros = 0u32;
    let mut base: u64 = 1024;
    loop {
        if p >= s.len() {
            // Depois da fração sobrou só o fim da cadeia: o original lê além dela e dá EINVAL.
            return Err(Errno::EINVAL);
        }
        if at(p + 1) == b'i' && matches!(at(p + 2), b'B' | b'b') && at(p + 3) == 0 {
            base = 1024; // XiB, 2^N
            break;
        } else if matches!(at(p + 1), b'B' | b'b') && at(p + 2) == 0 {
            base = 1000; // XB, 10^N
            break;
        } else if at(p + 1) != 0 {
            if frac == 0 && at(p) == b'.' {
                let mut q = p + 1;
                while at(q) == b'0' {
                    frac_zeros += 1;
                    q += 1;
                }
                let fstr = q;
                let end2;
                if at(fstr).is_ascii_digit() {
                    let (f, e) = strtoumax0(&s[fstr..]).map_err(|_| Errno::ERANGE)?;
                    frac = f;
                    end2 = fstr + e;
                } else {
                    end2 = q;
                }
                if frac != 0 && at(end2) == 0 {
                    return Err(Errno::EINVAL); // without suffix, but with frac
                }
                p = end2;
                continue;
            }
            return Err(Errno::EINVAL); // unexpected suffix
        } else {
            break;
        }
    }

    const SUF: &[u8] = b"KMGTPEZY";
    const SUF2: &[u8] = b"kmgtpezy";
    let c = at(p);
    let pwr: u32 = if let Some(i) = SUF.iter().position(|&b| b == c) {
        i as u32 + 1
    } else if let Some(i) = SUF2.iter().position(|&b| b == c) {
        i as u32 + 1
    } else {
        return Err(Errno::EINVAL);
    };

    if !scale_by_power(&mut x, base, pwr) {
        return Err(Errno::ERANGE);
    }
    if frac != 0 && pwr != 0 {
        let mut frac_div: u64 = 10;
        let mut frac_poz: u64 = 1;
        let mut frac_base: u64 = 1;
        // mega, giga, ...
        scale_by_power(&mut frac_base, base, pwr);
        // divisor máximo pro último dígito (0.05 dá 100, 0.054 dá 1000...)
        while frac_div < frac {
            if frac_div <= u64::MAX / 10 {
                frac_div *= 10;
            } else {
                frac /= 10;
            }
        }
        // 'frac' não tem os zeros à esquerda (5 vale 0.5 e 0.05)
        for _ in 0..frac_zeros {
            if frac_div <= u64::MAX / 10 {
                frac_div *= 10;
            } else {
                frac /= 10;
            }
        }
        // do último dígito pra trás, soma o que o dígito vale na base (0.25G: 5 é 1GiB / (100/5))
        loop {
            let seg = frac % 10;
            let seg_div = frac_div.checked_div(frac_poz).unwrap_or(0);
            frac /= 10;
            frac_poz = frac_poz.wrapping_mul(10);
            if seg != 0 && seg_div / seg != 0 {
                x = x.wrapping_add(frac_base / (seg_div / seg));
            }
            if frac == 0 {
                break;
            }
        }
    }
    Ok(x)
}

/// `strtosize_or_err`: como [`parse_size`], com a mensagem `<what>: '<arg>': <strerror>` do `err()`.
pub fn strtosize_or_err(arg: &[u8], what: &str) -> Result<u64, String> {
    parse_size(arg).map_err(|e| format!("{what}: '{}': {}", io::lossy(arg), e.message()))
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

/// Uma entrada lida inteira e entregue caractere a caractere como o `fgetwc` da glibc em C.UTF-8
/// (`fgetwc_or_err` do util-linux): UTF-8 inválido é erro (EILSEQ), sequência incompleta no fim da
/// entrada conta como fim, e uma falha de leitura (diretório, por exemplo) aparece na primeira leitura.
pub struct WideReader {
    data: Vec<u8>,
    pos: usize,
    read_err: Option<Errno>,
    eof: bool,
}

impl WideReader {
    pub fn new(content: Result<Vec<u8>, Errno>) -> WideReader {
        match content {
            Ok(data) => WideReader { data, pos: 0, read_err: None, eof: false },
            Err(e) => WideReader { data: Vec::new(), pos: 0, read_err: Some(e), eof: false },
        }
    }

    /// `true` depois que uma leitura bateu no fim da entrada (o `feof`).
    pub fn at_eof(&self) -> bool {
        self.eof
    }

    /// O próximo caractere sem consumir (o que o `ungetwc` devolveria): `Ok(None)` no fim.
    pub fn peek(&self) -> Result<Option<char>, Errno> {
        if let Some(e) = self.read_err {
            return Err(e);
        }
        match peek_wide(&self.data, self.pos) {
            Wide::Char(c) => Ok(Some(c)),
            Wide::Eof | Wide::Truncated => Ok(None),
            Wide::Invalid(_) => Err(Errno::EILSEQ),
        }
    }

    /// `fgetwc_or_err`: consome e devolve o próximo caractere, `Ok(None)` no fim.
    pub fn getwc(&mut self) -> Result<Option<char>, Errno> {
        let c = self.peek()?;
        match c {
            Some(ch) => self.pos += ch.len_utf8(),
            None => self.eof = true,
        }
        Ok(c)
    }
}
