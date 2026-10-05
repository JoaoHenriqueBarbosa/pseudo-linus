//! `_nc_infotocap` (`captoinfo.c`): converte uma cadeia terminfo para o formato termcap.

const MAX_TC_FIXUPS: usize = 10;
const MIN_TC_FIXUPS: usize = 4;

fn is_octal(b: u8) -> bool {
    (b'0'..=b'7').contains(&b)
}

/// Leitor com a semântica do `sscanf` pros poucos formatos usados: literais, `%d` (pula brancos,
/// aceita sinal) e `%c`. Quando uma conversão falha, as anteriores já foram gravadas.
struct Scan<'a> {
    s: &'a [u8],
    pos: usize,
    count: u32,
}

impl<'a> Scan<'a> {
    fn new(s: &'a [u8], from: usize) -> Scan<'a> {
        Scan { s, pos: from, count: 0 }
    }

    fn lit(&mut self, text: &[u8]) -> bool {
        if self.s.get(self.pos..).is_some_and(|r| r.starts_with(text)) {
            self.pos += text.len();
            true
        } else {
            false
        }
    }

    fn int(&mut self, out: &mut i32) -> bool {
        let mut p = self.pos;
        while p < self.s.len() && super::c_isspace(self.s[p]) {
            p += 1;
        }
        let mut neg = false;
        if p < self.s.len() && (self.s[p] == b'+' || self.s[p] == b'-') {
            neg = self.s[p] == b'-';
            p += 1;
        }
        let start = p;
        let mut v: i64 = 0;
        while p < self.s.len() && self.s[p].is_ascii_digit() {
            v = (v * 10 + i64::from(self.s[p] - b'0')).min(i64::from(i32::MAX) + 1);
            p += 1;
        }
        if p == start {
            return false;
        }
        *out = (if neg { -v } else { v }) as i32;
        self.pos = p;
        self.count += 1;
        true
    }

    fn chr(&mut self, out: &mut u8) -> bool {
        match self.s.get(self.pos) {
            Some(&c) => {
                *out = c;
                self.pos += 1;
                self.count += 1;
                true
            }
            None => false,
        }
    }
}

fn save_tc_char(buf: &mut Vec<u8>, c1: i32) {
    if (0..128).contains(&c1) && (32..127).contains(&c1) {
        if c1 == i32::from(b':') || c1 == i32::from(b'\\') {
            buf.push(b'\\');
        }
        buf.push(c1 as u8);
    } else if c1 == (c1 & 0x1f) {
        // `unctrl`: ^A ... ^_
        buf.push(b'^');
        buf.push((c1 as u8) + b'@');
    } else {
        buf.extend_from_slice(format!("\\{c1:03o}").as_bytes());
    }
}

fn save_tc_inequality(buf: &mut Vec<u8>, c1: i32, c2: i32) {
    buf.extend_from_slice(b"%>");
    save_tc_char(buf, c1);
    save_tc_char(buf, c2);
}

/// `bcd_expression`: `(p / 10) * 16 + (p % 10)` com o mesmo parâmetro dos dois lados.
fn bcd_expression(s: &[u8], from: usize) -> usize {
    let mut sc = Scan::new(s, from);
    let (mut c1, mut c2) = (0u8, 0u8);
    if sc.lit(b"%p") && sc.chr(&mut c1) && sc.lit(b"%{10}%/%{16}%*%p") && sc.chr(&mut c2) && c1.is_ascii_digit() && c2.is_ascii_digit() && c1 == c2 {
        28
    } else {
        0
    }
}

/// `_nc_infotocap(cap, str, parameterized)`; `strict_bsd` é o `_nc_strict_bsd` (`infocmp -K`).
/// `None` quando a cadeia não tem tradução.
pub fn infotocap(str_: &[u8], parameterized: i32, strict_bsd: bool) -> Option<Vec<u8>> {
    let s: &[u8] = match str_.iter().position(|b| *b == 0) {
        Some(p) => &str_[..p],
        None => str_,
    };
    let at = |i: isize| -> u8 {
        if i < 0 { 0 } else { s.get(i as usize).copied().unwrap_or(0) }
    };
    let mut buf: Vec<u8> = Vec::new();
    let (mut seenone, mut seentwo, mut saw_m, mut saw_n) = (false, false, 0, 0);
    let mut trimmed: Option<isize> = None;
    let mut syntax_error = false;
    let mut myfix = 0usize;
    // (valor, deslocamento no buffer)
    let mut fixups: Vec<(i32, usize)> = Vec::new();
    let mut ch1: u8 = 0;
    let mut ch2: u8 = 0;

    // Parte do preenchimento obrigatório no fim vai pro começo.
    let mut padding: isize = s.len() as isize - 1;
    if padding > 0 && at(padding) == b'>' {
        if padding > 1 {
            padding -= 1;
            if at(padding) == b'/' {
                padding -= 1;
            }
        }
        while at(padding).is_ascii_digit() || at(padding) == b'.' || at(padding) == b'*' {
            padding -= 1;
        }
        if padding > 0 && at(padding) == b'<' {
            padding -= 1;
            if at(padding) == b'$' {
                trimmed = Some(padding);
            }
        }
        padding += 2;
        while at(padding).is_ascii_digit() || at(padding) == b'.' || at(padding) == b'*' {
            buf.push(at(padding));
            padding += 1;
        }
    }

    let mut i: isize = 0;
    while !syntax_error && at(i) != 0 && trimmed.is_none_or(|t| i < t) {
        let (mut c1, mut c2) = (0i32, 0i32);
        let c = at(i);
        if c == b'^' {
            if at(i + 1) == 0 || Some(i + 1) == trimmed {
                buf.extend_from_slice(b"\\136");
                i += 1;
            } else if at(i + 1) == b'?' {
                buf.extend_from_slice(b"\\177");
                i += 1;
            } else {
                buf.push(at(i));
                i += 1;
                buf.push(at(i));
            }
        } else if c == b':' {
            buf.extend_from_slice(b"\\072");
        } else if c == b'\\' {
            if at(i + 1) == 0 || Some(i + 1) == trimmed {
                buf.extend_from_slice(b"\\134");
                i += 1;
            } else if at(i + 1) == b'^' {
                buf.extend_from_slice(b"\\136");
                i += 1;
            } else if at(i + 1) == b',' {
                i += 1;
                buf.push(at(i));
            } else {
                buf.push(at(i));
                i += 1;
                let mut xx1 = at(i);
                if strict_bsd {
                    if is_octal(xx1) {
                        let mut pad = 0;
                        if !is_octal(at(i + 1)) {
                            pad = 2;
                        } else if at(i + 1) != 0 && !is_octal(at(i + 2)) {
                            pad = 1;
                        }
                        let mut xx2: u8;
                        if xx1 == b'0' && ((pad == 2) || at(i + 1) == b'0') && ((pad >= 1) || at(i + 2) == b'0') {
                            xx2 = b'2';
                        } else {
                            xx2 = b'0';
                            pad = 0;
                        }
                        let mut fix = 0;
                        let mut fx: Option<(i32, usize)> = None;
                        if myfix < MAX_TC_FIXUPS {
                            fix = 3 - pad;
                            fx = Some((0, buf.len().wrapping_sub(1)));
                        }
                        while pad > 0 {
                            pad -= 1;
                            buf.push(xx2);
                            if let Some(f) = fx.as_mut() {
                                f.0 = (f.0 << 3) | i32::from(xx2 - b'0');
                            }
                            xx2 = b'0';
                        }
                        if let Some(mut f) = fx {
                            for n in 0..fix {
                                f.0 = (f.0 << 3) | i32::from(at(i + n as isize).wrapping_sub(b'0'));
                            }
                            if f.0 < 32 {
                                if fixups.len() <= myfix {
                                    fixups.resize(myfix + 1, (0, 0));
                                }
                                fixups[myfix] = f;
                                myfix += 1;
                            }
                        }
                    } else if !b"E\\nrtbf".contains(&xx1) {
                        match xx1 {
                            b'e' => xx1 = b'E',
                            b'l' => xx1 = b'n',
                            b's' => {
                                buf.push(b'0');
                                buf.push(b'4');
                                xx1 = b'0';
                            }
                            b':' => {
                                buf.push(b'0');
                                buf.push(b'7');
                                xx1 = b'2';
                            }
                            _ => {
                                let octal = format!("{xx1:03o}");
                                let o = octal.as_bytes();
                                buf.push(o[0]);
                                buf.push(o[1]);
                                xx1 = o[2];
                            }
                        }
                    }
                } else if myfix < MAX_TC_FIXUPS && is_octal(xx1) {
                    let mut will_fix = true;
                    let mut f: (i32, usize) = (0, buf.len().wrapping_sub(1));
                    for n in 0..3isize {
                        if is_octal(at(i + n)) {
                            f.0 = (f.0 << 3) | i32::from(at(i + n) - b'0');
                        } else {
                            will_fix = false;
                            break;
                        }
                    }
                    if will_fix && f.0 < 32 {
                        if fixups.len() <= myfix {
                            fixups.resize(myfix + 1, (0, 0));
                        }
                        fixups[myfix] = f;
                        myfix += 1;
                    }
                }
                buf.push(xx1);
            }
        } else if c == b'$' && at(i + 1) == b'<' {
            // descarta o preenchimento
            i += 2;
            while at(i).is_ascii_digit() || at(i) == b'.' || at(i) == b'*' || at(i) == b'/' || at(i) == b'>' {
                i += 1;
            }
            i -= 1;
        } else if xterm_256(s, i as usize, &mut ch1, &mut ch2).is_some() {
            // otimização do xterm-256color: volta pra forma simples do termcap
            let (in0, in2) = xterm_256(s, i as usize, &mut ch1, &mut ch2).unwrap();
            let semi_m = find_sub(&s[i as usize..], b";m");
            match semi_m {
                None => break,
                Some(p) => {
                    i += p as isize + 1;
                    if in2 == 48 {
                        buf.extend_from_slice(b"[48;5;%dm");
                    } else {
                        buf.extend_from_slice(b"[38;5;%dm");
                    }
                    let _ = in0;
                }
            }
        } else if c == b'%' && at(i + 1) == b'%' {
            buf.extend_from_slice(b"%%");
            i += 1;
        } else if c != b'%' || parameterized < 1 {
            buf.push(c);
        } else if {
            let mut sc = Scan::new(s, i as usize);
            let ok = sc.lit(b"%?%{") && sc.int(&mut c1) && sc.lit(b"}%>%t%{") && sc.int(&mut c2);
            ok && sc.count == 2
        } {
            i = semi_pos(s, i);
            save_tc_inequality(&mut buf, c1, c2);
        } else if {
            let mut sc = Scan::new(s, i as usize);
            let ok = sc.lit(b"%?%{") && sc.int(&mut c1) && sc.lit(b"}%>%t%'") && sc.chr(&mut ch2);
            ok && sc.count == 2
        } {
            i = semi_pos(s, i);
            save_tc_inequality(&mut buf, c1, i32::from(ch2));
        } else if {
            let mut sc = Scan::new(s, i as usize);
            let ok = sc.lit(b"%?%'") && sc.chr(&mut ch1) && sc.lit(b"'%>%t%{") && sc.int(&mut c2);
            ok && sc.count == 2
        } {
            i = semi_pos(s, i);
            save_tc_inequality(&mut buf, i32::from(ch1), c2);
        } else if {
            let mut sc = Scan::new(s, i as usize);
            let ok = sc.lit(b"%?%'") && sc.chr(&mut ch1) && sc.lit(b"'%>%t%'") && sc.chr(&mut ch2);
            ok && sc.count == 2
        } {
            i = semi_pos(s, i);
            save_tc_inequality(&mut buf, i32::from(ch1), i32::from(ch2));
        } else if bcd_expression(s, i as usize) != 0 {
            i += bcd_expression(s, i as usize) as isize;
            buf.extend_from_slice(b"%B");
        } else if {
            let mut sc = Scan::new(s, i as usize);
            let a = sc.lit(b"%{") && sc.int(&mut c1) && sc.lit(b"}%+%") && sc.chr(&mut ch2) && sc.count == 2;
            let hit = a || {
                let mut sc2 = Scan::new(s, i as usize);
                sc2.lit(b"%'") && sc2.chr(&mut ch1) && sc2.lit(b"'%+%") && sc2.chr(&mut ch2) && sc2.count == 2
            };
            hit && ch2 == b'c' && s[i as usize..].contains(&b'+')
        } {
            let cp = i as usize + s[i as usize..].iter().position(|b| *b == b'+').unwrap_or(0);
            i = cp as isize + 2;
            buf.extend_from_slice(b"%+");
            if ch1 != 0 {
                c1 = i32::from(ch1);
            }
            save_tc_char(&mut buf, c1);
        } else if s[i as usize..].starts_with(b"%{2}%*%-") {
            i += 7;
            buf.extend_from_slice(b"%D");
        } else if s[i as usize..].starts_with(b"%{96}%^") {
            i += 6;
            if saw_m == 0 {
                buf.extend_from_slice(b"%n");
            }
            saw_m += 1;
        } else if s[i as usize..].starts_with(b"%{127}%^") {
            i += 7;
            if saw_n == 0 {
                buf.extend_from_slice(b"%m");
            }
            saw_n += 1;
        } else {
            // elemento de formato no estilo do `cm`
            i += 1;
            match at(i) {
                b'%' => buf.push(b'%'),
                b'0'..=b'9' => {
                    buf.push(b'%');
                    ch1 = 0;
                    ch2 = 0;
                    let mut digits = 0;
                    while at(i).is_ascii_digit() {
                        digits += 1;
                        if digits > 2 {
                            syntax_error = true;
                            break;
                        }
                        ch2 = ch1;
                        ch1 = at(i);
                        i += 1;
                        if digits == 2 && ch2 != b'0' {
                            syntax_error = true;
                            break;
                        } else if strict_bsd {
                            if ch1 > b'3' {
                                syntax_error = true;
                                break;
                            }
                        } else {
                            buf.push(ch1);
                        }
                    }
                    if !syntax_error {
                        // %02 vira %2 e %03 vira %3
                        if ch2 == b'0' && !strict_bsd {
                            ch2 = 0;
                            let n = buf.len();
                            buf[n - 2] = buf[n - 1];
                            buf.pop();
                        }
                        if strict_bsd {
                            if ch2 != 0 && ch2 != b'0' {
                                syntax_error = true;
                            } else if ch1 < b'2' {
                                ch1 = b'd';
                            }
                            buf.push(ch1);
                        }
                        // o termcap não tem octal nem hexadecimal (o `strchr` também casa o NUL final)
                        if at(i) == 0 || b"oxX.".contains(&at(i)) {
                            syntax_error = true;
                        }
                    }
                }
                b'd' => buf.extend_from_slice(b"%d"),
                b'c' => buf.extend_from_slice(b"%."),
                b's' => {
                    if strict_bsd {
                        syntax_error = true;
                    } else {
                        buf.extend_from_slice(b"%s");
                    }
                }
                b'p' => {
                    i += 1;
                    if at(i) == b'1' {
                        seenone = true;
                    } else if at(i) == b'2' {
                        if !seenone && !seentwo {
                            buf.extend_from_slice(b"%r");
                            seentwo = true;
                        }
                    } else if at(i) >= b'3' {
                        syntax_error = true;
                    }
                }
                b'i' => buf.extend_from_slice(b"%i"),
                _ => {
                    buf.push(at(i));
                    syntax_error = true;
                }
            }
        }
        if at(i) == 0 {
            break;
        }
        i += 1;
    }

    if !syntax_error && myfix > 0 && buf.len().saturating_sub(4 * myfix) < MIN_TC_FIXUPS {
        for k in (0..myfix).rev() {
            let (ch, off) = fixups[k];
            if off + 4 <= buf.len() {
                let repl = [b'^', (ch as u8) | b'@'];
                buf.splice(off..off + 4, repl);
            }
        }
    }
    if syntax_error { None } else { Some(buf) }
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// `str = strchr(str, ';')`: a posição do primeiro `;` a partir de `i` (ou o fim da cadeia).
fn semi_pos(s: &[u8], i: isize) -> isize {
    match s[i as usize..].iter().position(|b| *b == b';') {
        Some(p) => i + p as isize,
        None => s.len() as isize,
    }
}

/// O padrão `[%?%p1%{8}%<%t4%p1%d%e%p1%{16}%<%t10%p1%{8}%-%d%e48;5;%p1%d%;m` (e o de 38/9/3): devolve
/// `(in0, in2)` quando os três números casam com um dos dois conjuntos conhecidos.
fn xterm_256(s: &[u8], from: usize, _ch1: &mut u8, _ch2: &mut u8) -> Option<(i32, i32)> {
    let mut sc = Scan::new(s, from);
    let (mut in0, mut in1, mut in2) = (0i32, 0i32, 0i32);
    let ok = sc.lit(b"[%?%p1%{8}%<%t")
        && sc.int(&mut in0)
        && sc.lit(b"%p1%d%e%p1%{16}%<%t")
        && sc.int(&mut in1)
        && sc.lit(b"%p1%{8}%-%d%e")
        && sc.int(&mut in2);
    if ok && sc.count == 3 && ((in0 == 4 && in1 == 10 && in2 == 48) || (in0 == 3 && in1 == 9 && in2 == 38)) {
        Some((in0, in2))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_common_strings() {
        assert_eq!(infotocap(b"\\E[%i%p1%d;%p2%dH", 1, false).unwrap(), b"\\E[%i%d;%dH");
        assert_eq!(infotocap(b"\\E[%p1%dm$<5>", 1, false).unwrap(), b"5\\E[%dm");
        assert_eq!(infotocap(b"^G", 0, false).unwrap(), b"^G");
        assert_eq!(infotocap(b"%p1%c", 1, false).unwrap(), b"%.");
    }
}
