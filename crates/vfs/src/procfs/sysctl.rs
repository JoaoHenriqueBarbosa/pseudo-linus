//! Os parâmetros de `/proc/sys` que o procfs guarda e a leitura de um valor escrito, como os
//! `proc_dointvec_minmax`, `proc_doulongvec_minmax` e `proc_dostring` de `kernel/sysctl.c` do 6.12.
//!
//! Regras do kernel que valem aqui: o número aceita base 8 (`0` na frente) e 16 (`0x`), como o
//! `strtoul` com base 0; espaços antes são ignorados; depois do número só pode vir espaço, tab ou
//! quebra de linha; uma escrita sem número nenhum, fora do intervalo ou que estoura o tipo dá EINVAL.
//! Uma string (`hostname`, `domainname`) vai até a primeira quebra de linha ou NUL e é cortada no
//! tamanho do campo.

use crate::types::*;

/// `TMPBUFLEN` do `proc_get_long`: um número com 21 caracteres ou mais é recusado.
const TMPBUFLEN: usize = 22;
/// `__NEW_UTS_LEN`.
pub(super) const UTS_LEN: usize = 64;

/// Valores atuais dos parâmetros graváveis que só o procfs guarda. `None` é o padrão calculado na
/// leitura (`pid_max` vem do kernel, `threads-max` da memória).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Tunables {
    pub pid_max: Option<u64>,
    pub threads_max: Option<u64>,
    pub file_max: u64,
    pub overcommit_memory: u64,
    pub swappiness: u64,
    pub somaxconn: u64,
}

impl Default for Tunables {
    fn default() -> Tunables {
        Tunables {
            pid_max: None,
            threads_max: None,
            // O systemd do Debian 13 põe `fs.file-max` em `LONG_MAX`.
            file_max: i64::MAX as u64,
            overcommit_memory: 0,
            swappiness: 60,
            somaxconn: 4096,
        }
    }
}

/// `PID_MAX_LIMIT` e o mínimo (`RESERVED_PIDS + 1`) do `pid_max`.
pub(super) const PID_MAX_MIN: i64 = 301;
pub(super) const PID_MAX_LIMIT: i64 = 4_194_304;
/// `MAX_THREADS` (`FUTEX_TID_MASK`).
pub(super) const THREADS_MAX_LIMIT: i64 = 0x3fff_ffff;

/// `threads-max` padrão (`set_max_threads`): a memória dividida por oito pilhas de kernel de 16 KiB,
/// no intervalo de 20 a `MAX_THREADS`.
pub(super) fn default_threads_max(mem_total_kb: u64) -> u64 {
    (mem_total_kb / 128).clamp(20, THREADS_MAX_LIMIT as u64)
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// `proc_get_long`: o sinal e o valor absoluto do primeiro número da escrita.
fn get_long(buf: &[u8]) -> SysResult<(bool, u64)> {
    let start = buf.iter().position(|b| !is_space(*b)).ok_or(Errno::EINVAL)?;
    let rest = &buf[start..];
    let len = rest.iter().position(|b| *b == 0).unwrap_or(rest.len()).min(TMPBUFLEN - 1);
    let tmp = &rest[..len];
    let (neg, digits) = match tmp.first() {
        Some(b'-') => (true, &tmp[1..]),
        _ => (false, tmp),
    };
    let (radix, body) = if digits.len() > 1 && digits[0] == b'0' && matches!(digits[1], b'x' | b'X') {
        (16, &digits[2..])
    } else if digits.len() > 1 && digits[0] == b'0' {
        (8, &digits[1..])
    } else {
        (10, digits)
    };
    let n = body.iter().take_while(|b| (**b as char).is_digit(radix)).count();
    // `0x` sem dígitos ou um `0` sozinho na base 8: o `strtoul` lê só o 0.
    let (value, used) = if n == 0 && radix != 10 {
        (0, 1)
    } else if n == 0 {
        return Err(Errno::EINVAL);
    } else {
        let s = std::str::from_utf8(&body[..n]).map_err(|_| Errno::EINVAL)?;
        let v = u64::from_str_radix(s, radix).map_err(|_| Errno::EINVAL)?;
        (v, digits.len() - body.len() + n)
    };
    let consumed = usize::from(neg) + used;
    if consumed == TMPBUFLEN - 1 {
        return Err(Errno::EINVAL);
    }
    if let Some(next) = rest.get(consumed)
        && !matches!(*next, b' ' | b'\t' | b'\n')
    {
        return Err(Errno::EINVAL);
    }
    Ok((neg, value))
}

/// `proc_dointvec_minmax` de um valor só: um `int` dentro de `[min, max]`.
pub(super) fn parse_int(buf: &[u8], min: i64, max: i64) -> SysResult<u64> {
    let (neg, v) = get_long(buf)?;
    let val: i64 = if neg {
        if v > i32::MAX as u64 + 1 {
            return Err(Errno::EINVAL);
        }
        -(v as i64)
    } else {
        if v > i32::MAX as u64 {
            return Err(Errno::EINVAL);
        }
        v as i64
    };
    if val < min || val > max {
        return Err(Errno::EINVAL);
    }
    Ok(val as u64)
}

/// `proc_doulongvec_minmax` de um valor só: negativo é recusado.
pub(super) fn parse_ulong(buf: &[u8], min: u64, max: u64) -> SysResult<u64> {
    let (neg, v) = get_long(buf)?;
    if neg || v < min || v > max {
        return Err(Errno::EINVAL);
    }
    Ok(v)
}

/// `proc_dopipe_max_size`: o número escrito arredondado como o `round_pipe_size` (no mínimo uma página,
/// potência de 2); o que estoura 2^31 é EINVAL.
pub(super) fn pipe_max_value(buf: &[u8]) -> SysResult<u32> {
    let v = parse_ulong(buf, 0, u64::from(u32::MAX))?;
    sysabi::round_pipe_size(v as u32).ok_or(Errno::EINVAL)
}

/// `proc_dostring`: até a primeira quebra de linha ou NUL, cortado em `UTS_LEN` bytes.
pub(super) fn uts_value(buf: &[u8]) -> Vec<u8> {
    let end = buf.iter().position(|b| *b == b'\n' || *b == 0).unwrap_or(buf.len()).min(UTS_LEN);
    buf[..end].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ints_accept_the_bases_and_trailing_space_of_strtoul() {
        assert_eq!(parse_int(b"4096\n", 0, i32::MAX as i64), Ok(4096));
        assert_eq!(parse_int(b"  0x10 ", 0, 100), Ok(16));
        assert_eq!(parse_int(b"010", 0, 100), Ok(8));
        assert_eq!(parse_int(b"0", 0, 2), Ok(0));
        assert_eq!(parse_int(b"7 9", 0, 100), Ok(7));
    }

    #[test]
    fn ints_reject_garbage_range_and_overflow() {
        assert_eq!(parse_int(b"", 0, 2), Err(Errno::EINVAL));
        assert_eq!(parse_int(b" \n", 0, 2), Err(Errno::EINVAL));
        assert_eq!(parse_int(b"abc", 0, 2), Err(Errno::EINVAL));
        assert_eq!(parse_int(b"12abc", 0, 100), Err(Errno::EINVAL));
        assert_eq!(parse_int(b"3", 0, 2), Err(Errno::EINVAL));
        assert_eq!(parse_int(b"-1", 0, 2), Err(Errno::EINVAL));
        assert_eq!(parse_int(b"300", PID_MAX_MIN, PID_MAX_LIMIT), Err(Errno::EINVAL));
        assert_eq!(parse_int(b"4194305", PID_MAX_MIN, PID_MAX_LIMIT), Err(Errno::EINVAL));
        assert_eq!(parse_int(b"2147483648", 0, i64::MAX), Err(Errno::EINVAL));
        assert_eq!(parse_int(b"99999999999999999999999", 0, i64::MAX), Err(Errno::EINVAL));
    }

    #[test]
    fn ulongs_refuse_negatives() {
        assert_eq!(parse_ulong(b"9223372036854775807\n", 0, i64::MAX as u64), Ok(i64::MAX as u64));
        assert_eq!(parse_ulong(b"-5", 0, i64::MAX as u64), Err(Errno::EINVAL));
        assert_eq!(parse_ulong(b"9223372036854775808", 0, i64::MAX as u64), Err(Errno::EINVAL));
    }

    #[test]
    fn uts_strings_stop_at_the_newline_and_are_cut_at_64() {
        assert_eq!(uts_value(b"box\n"), b"box".to_vec());
        assert_eq!(uts_value(&[b'a'; 100]).len(), 64);
    }

    #[test]
    fn threads_max_follows_the_memory() {
        assert_eq!(default_threads_max(32_735_068), 255_742);
        assert_eq!(default_threads_max(1), 20);
    }
}
