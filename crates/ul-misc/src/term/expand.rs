//! `_nc_tic_expand` (`comp_expand.c`): a cadeia de uma capacidade como o `tic` e o `infocmp` a
//! escrevem (`\E`, `^G`, `\n`, `\s`...).

use super::{c_isprint, strtol};

const MAX_TC_FIXUPS: usize = 10;
const MIN_TC_FIXUPS: usize = 4;

fn realprint(b: u8) -> bool {
    b < 127 && c_isprint(b)
}

fn trailing_spaces(s: &[u8]) -> bool {
    s.iter().all(|b| *b == b' ')
}

/// `_nc_tic_expand(srcp, tic_format, numbers)`: `numbers` é -1 (`%'c'` vira `%{n}`), 1 (`%{n}` vira
/// `%'c'`) ou 0.
pub fn tic_expand(srcp: &[u8], tic_format: bool, numbers: i32) -> Vec<u8> {
    let src: &[u8] = match srcp.iter().position(|b| *b == 0) {
        Some(p) => &srcp[..p],
        None => srcp,
    };
    let at = |i: usize| src.get(i).copied().unwrap_or(0);
    let mut buf: Vec<u8> = Vec::with_capacity(src.len() * 2 + 8);
    let mut fixups: Vec<(u8, usize)> = Vec::new();
    let mut i = 0usize;
    while i < src.len() {
        let ch = src[i];
        if ch == b'%' && realprint(at(i + 1)) {
            buf.push(b'%');
            i += 1;
            match numbers {
                -1 => {
                    if at(i) == b'\'' && at(i + 1) != b'\\' && realprint(at(i + 1)) && at(i + 2) == b'\'' {
                        buf.extend_from_slice(format!("{{{}}}", at(i + 1)).as_bytes());
                        i += 2;
                    } else {
                        buf.push(at(i));
                    }
                }
                1 => {
                    if at(i) == b'{' && at(i + 1).is_ascii_digit() {
                        let (value, used) = strtol(&src[i + 1..]);
                        let dst = i + 1 + used;
                        if at(dst) == b'}' && value < 127 && (32..127).contains(&value) {
                            let c = value as u8;
                            buf.push(b'\'');
                            if c == b'\\' || c == b'\'' {
                                buf.push(b'\\');
                            }
                            buf.push(c);
                            buf.push(b'\'');
                            i = dst;
                        } else {
                            buf.push(at(i));
                        }
                    } else {
                        buf.push(at(i));
                    }
                }
                _ => {
                    if at(i) == b',' {
                        buf.push(b'\\');
                    }
                    buf.push(at(i));
                }
            }
        } else if ch == 128 {
            buf.extend_from_slice(b"\\0");
        } else if ch == 0x1b {
            buf.extend_from_slice(b"\\E");
        } else if ch == b'\\' && tic_format && (i == 0 || src[i - 1] != b'^') {
            buf.extend_from_slice(b"\\\\");
        } else if ch == b' ' && tic_format && (i == 0 || trailing_spaces(&src[i..])) {
            buf.extend_from_slice(b"\\s");
        } else if (ch == b',' || ch == b'^') && tic_format {
            buf.push(b'\\');
            buf.push(ch);
        } else if realprint(ch) && ch != b',' && !(ch == b':' && !tic_format) && !(ch == b'!' && !tic_format) && ch != b'^' {
            buf.push(ch);
        } else if ch == b'\r' {
            buf.extend_from_slice(b"\\r");
        } else if ch == b'\n' {
            buf.extend_from_slice(b"\\n");
        } else if ch < 32 && at(i + 1).is_ascii_digit() {
            buf.push(b'^');
            buf.push(ch + b'@');
        } else {
            let off = buf.len();
            buf.extend_from_slice(format!("\\{ch:03o}").as_bytes());
            if fixups.len() < MAX_TC_FIXUPS && ((tic_format && ch == 127) || ch < 32) {
                fixups.push((ch, off));
            }
        }
        i += 1;
    }
    // Numa cadeia curta cheia de controles, o formato `^X` é mais legível; no termcap vale sempre.
    let octals = fixups.len();
    if octals != 0 && (!tic_format || buf.len().saturating_sub(4 * octals) < MIN_TC_FIXUPS) {
        for &(c, off) in fixups.iter().rev() {
            let repl = [b'^', if c == 127 { b'?' } else { c + b'@' }];
            buf.splice(off..off + 4, repl);
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_like_tic() {
        assert_eq!(tic_expand(b"\x1b[%i%p1%d;%p2%dH", true, 0), b"\\E[%i%p1%d;%p2%dH");
        assert_eq!(tic_expand(b"\x07", true, 0), b"^G");
        assert_eq!(tic_expand(b"a b ", true, 0), b"a b\\s");
        assert_eq!(tic_expand(b"%'a'%d", true, -1), b"%{97}%d");
        assert_eq!(tic_expand(b"%{97}%d", true, 1), b"%'a'%d");
        assert_eq!(tic_expand(b"\r\n", true, 0), b"\\r\\n");
    }
}
