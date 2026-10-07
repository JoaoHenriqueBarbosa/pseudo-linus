//! Leitura e reescrita dos formatos (porte do `text-utils/hexdump-parse.c` do util-linux 2.41, BSD;
//! ver o cabeçalho de licença em `mod.rs`).
//!
//! Um formato (`-e`, linha de `-f`) vira uma lista de unidades (`Fu`): `[reps][/bcnt] "texto"`. Cada
//! unidade é quebrada em unidades de impressão (`Pr`), uma por conversão, com o texto anterior
//! colado no formato printf da conversão e o texto final numa unidade só de texto. As peculiaridades
//! do original ficam: o `fmt` de cada `Pr` é a cadeia que o C passaria ao `printf`, com `ll` antes
//! das conversões inteiras, e as mensagens de erro saem com os mesmos pedaços que o C imprime.

use ul_common::ctype::{at, is_space, strtoull_fixed_base, strtol};

use super::{Clr, Fs, Fu, Hexdump, Kind, Pr};

pub(super) const SPEC: &[u8] = b".#-+ 0123456789";

/// `strchr(list, c) != NULL`: o NUL também "acha" (o terminador da lista), como no C.
fn first_letter(c: u8, list: &[u8]) -> bool {
    c == 0 || list.contains(&c)
}

fn skip_space(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    i
}

/// `strtol(str, &end, 10)` guardado num `int`: `None` sem dígitos ou com estouro do `long`.
fn next_number(s: &[u8], i: usize) -> Option<(i32, usize)> {
    let c = strtol(&s[i.min(s.len())..], 10);
    if c.used == 0 || c.overflow {
        return None;
    }
    // O C guarda o `long` num `int`: trunca.
    Some((c.value as i32, i + c.used))
}

fn badfmt(fmt: &[u8]) -> String {
    format!("bad format {{{}}}", String::from_utf8_lossy(fmt))
}

fn badcnt(s: &[u8]) -> String {
    format!(
        "bad byte count for conversion character {}",
        String::from_utf8_lossy(s)
    )
}

fn badconv(s: &[u8]) -> String {
    format!("bad conversion character %{}", String::from_utf8_lossy(s))
}

/// As sequências de escape do formato, no lugar (`\n`, `\t`... e `\x` vira `x`). Barra invertida no
/// fim corta a cadeia ali (no C o NUL é copiado e o resto some).
pub(super) fn escape(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'\\' {
            i += 1;
            let Some(&e) = s.get(i) else { break };
            out.push(match e {
                b'a' => 0x07,
                b'b' => 0x08,
                b'f' => 0x0c,
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                b'v' => 0x0b,
                other => other,
            });
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

impl Hexdump {
    /// `add_fmt`: um formato novo (uma lista de unidades) no fim da lista.
    pub(super) fn add_fmt(&mut self, fmt: &[u8]) -> Result<(), String> {
        // O C para no primeiro NUL; argumento não tem NUL, linha de arquivo pode ter.
        let fmt = match fmt.iter().position(|&b| b == 0) {
            Some(n) => &fmt[..n],
            None => fmt,
        };
        let mut fs = Fs {
            fus: Vec::new(),
            bcnt: 0,
        };
        let mut p = 0;
        loop {
            p = skip_space(fmt, p);
            if p >= fmt.len() {
                break;
            }
            let mut fu = Fu {
                reps: 1,
                bcnt: 0,
                setrep: false,
                ignore: false,
                fmt: Vec::new(),
                prs: Vec::new(),
            };
            if fmt[p].is_ascii_digit() {
                let Some((n, q)) = next_number(fmt, p) else {
                    return Err(badfmt(fmt));
                };
                fu.reps = n;
                p = q;
                if !is_space(at(fmt, p)) && at(fmt, p) != b'/' {
                    return Err(badfmt(fmt));
                }
                // Pode pular o espaço ou a barra.
                fu.setrep = true;
                p = skip_space(fmt, p + 1);
            }
            if at(fmt, p) == b'/' {
                p = skip_space(fmt, p + 1);
            }
            if at(fmt, p).is_ascii_digit() {
                let Some((n, q)) = next_number(fmt, p) else {
                    return Err(badfmt(fmt));
                };
                fu.bcnt = n;
                p = q;
                if !is_space(at(fmt, p)) {
                    return Err(badfmt(fmt));
                }
                p = skip_space(fmt, p + 1);
            }
            if at(fmt, p) != b'"' {
                return Err(badfmt(fmt));
            }
            p += 1;
            let start = p;
            while at(fmt, p) != b'"' {
                if p >= fmt.len() {
                    return Err(badfmt(fmt));
                }
                p += 1;
            }
            fu.fmt = escape(&fmt[start..p]);
            p += 1;
            fs.fus.push(fu);
        }
        self.fss.push(fs);
        Ok(())
    }

    /// `block_size`: quantos bytes um formato consome por bloco.
    pub(super) fn block_size(fs: &Fs) -> Result<i64, String> {
        let mut cursize: i64 = 0;
        for fu in &fs.fus {
            if fu.bcnt != 0 {
                cursize =
                    cursize.saturating_add(i64::from(fu.bcnt).saturating_mul(i64::from(fu.reps)));
                continue;
            }
            let mut bcnt: i64 = 0;
            let mut prec: i64 = 0;
            let f = &fu.fmt;
            let mut i = 0;
            while i < f.len() {
                if f[i] != b'%' {
                    i += 1;
                    continue;
                }
                i += 1;
                while at(f, i) != 0 && SPEC[1..].contains(&at(f, i)) {
                    i += 1;
                }
                if at(f, i) == b'.' {
                    i += 1;
                    if at(f, i).is_ascii_digit() {
                        match next_number(f, i) {
                            Some((n, q)) => {
                                prec = i64::from(n);
                                i = q;
                            }
                            None => return Err(badfmt(f)),
                        }
                    }
                }
                let c = at(f, i);
                if c == 0 {
                    return Err(badfmt(f));
                }
                if b"diouxX".contains(&c) {
                    bcnt += 4;
                } else if b"efgEG".contains(&c) {
                    bcnt += 8;
                } else if c == b's' {
                    bcnt += prec;
                } else if c == b'c' {
                    bcnt += 1;
                } else if c == b'_' {
                    i += 1;
                    if first_letter(at(f, i), b"cpu") {
                        bcnt += 1;
                    }
                }
                i += 1;
            }
            cursize = cursize.saturating_add(bcnt.saturating_mul(i64::from(fu.reps)));
        }
        Ok(cursize)
    }

    /// `rewrite_rules`: quebra cada unidade em unidades de impressão, confere as conversões e as
    /// contagens, acerta a repetição da última unidade e marca o espaço final que some na última
    /// repetição.
    pub(super) fn rewrite_rules(&mut self, fsi: usize) -> Result<(), String> {
        let blocksize = self.blocksize;
        let colors = self.colors;
        let nfus = self.fss[fsi].fus.len();
        for fui in 0..nfus {
            let fu_bcnt = self.fss[fsi].fus[fui].bcnt;
            // Cópia de trabalho do formato (o C escreve NULs nele no caminho).
            let f = self.fss[fsi].fus[fui].fmt.clone();
            let mut prs: Vec<Pr> = Vec::new();
            let mut nconv = 0;
            let mut fmtp = 0usize;
            while fmtp < f.len() {
                let mut p1 = fmtp;
                while p1 < f.len() && f[p1] != b'%' {
                    p1 += 1;
                }
                if p1 >= f.len() {
                    prs.push(Pr::text(f[fmtp..].to_vec()));
                    break;
                }
                #[derive(PartialEq)]
                enum Sokay {
                    NotOkay,
                    UseBcnt,
                    UsePrec,
                }
                let sokay;
                let mut prec = 0i32;
                if fu_bcnt != 0 {
                    sokay = Sokay::UseBcnt;
                    p1 += 1;
                    // O C segue até um caractere fora de SPEC, inclusive além do NUL (lixo da
                    // memória); aqui para no fim.
                    while p1 < f.len() && SPEC.contains(&f[p1]) {
                        p1 += 1;
                    }
                } else {
                    p1 += 1;
                    while p1 < f.len() && SPEC[1..].contains(&f[p1]) {
                        p1 += 1;
                    }
                    if at(&f, p1) == b'.' && at(&f, p1 + 1).is_ascii_digit() {
                        p1 += 1;
                        let (n, q) = next_number(&f, p1).ok_or_else(|| badfmt(&f))?;
                        prec = n;
                        p1 = q;
                        sokay = Sokay::UsePrec;
                    } else {
                        if at(&f, p1) == b'.' {
                            p1 += 1;
                        }
                        sokay = Sokay::NotOkay;
                    }
                }
                let mut p2 = p1 + 1;
                let c0 = at(&f, p1);
                let mut cs: Vec<u8> = vec![c0];
                let kind;
                let bcnt;
                let cut = |n: usize| -> &[u8] { &f[p1..(p1 + n).min(f.len())] };
                if c0 == b'c' {
                    kind = Kind::Char;
                    bcnt = match fu_bcnt {
                        0 | 1 => 1,
                        _ => return Err(badcnt(cut(1))),
                    };
                } else if c0 != 0 && b"diouxX".contains(&c0) || c0 == 0 {
                    // `first_letter` com NUL dá verdadeiro: cai como inteiro, como no C.
                    kind = if c0 == 0 || b"di".contains(&c0) {
                        Kind::Int
                    } else {
                        Kind::Uint
                    };
                    cs = vec![b'l', b'l', c0];
                    if c0 == 0 {
                        cs.pop();
                    }
                    bcnt = match fu_bcnt {
                        0 => 4,
                        1 | 2 | 4 | 8 => fu_bcnt,
                        _ => return Err(badcnt(cut(1))),
                    };
                } else if b"efgEG".contains(&c0) {
                    kind = Kind::Dbl;
                    bcnt = match fu_bcnt {
                        0 => 8,
                        4 | 8 => fu_bcnt,
                        _ => return Err(badcnt(cut(1))),
                    };
                } else if c0 == b's' {
                    kind = Kind::Str;
                    bcnt = match sokay {
                        Sokay::NotOkay => {
                            return Err("%s requires a precision or a byte count".to_string());
                        }
                        Sokay::UseBcnt => fu_bcnt,
                        Sokay::UsePrec => prec,
                    };
                } else if c0 == b'_' {
                    p2 += 1;
                    let c1 = at(&f, p1 + 1);
                    match c1 {
                        b'A' | b'a' => {
                            if c1 == b'A' {
                                self.endfu = Some((fsi, fui));
                                self.fss[fsi].fus[fui].ignore = true;
                            }
                            kind = Kind::Address;
                            bcnt = 0;
                            p2 += 1;
                            let c2 = at(&f, p1 + 2);
                            if first_letter(c2, b"dox") {
                                cs = vec![b'l', b'l'];
                                if c2 != 0 {
                                    cs.push(c2);
                                }
                            } else {
                                return Err(badconv(cut(3)));
                            }
                        }
                        b'c' | b'p' | b'u' => {
                            kind = match c1 {
                                b'c' => Kind::C,
                                b'p' => Kind::P,
                                _ => Kind::U,
                            };
                            if c1 == b'p' {
                                cs = vec![b'c'];
                            }
                            bcnt = match fu_bcnt {
                                0 | 1 => 1,
                                _ => return Err(badcnt(cut(2))),
                            };
                        }
                        _ => return Err(badconv(cut(2))),
                    }
                } else {
                    return Err(badconv(cut(1)));
                }

                // Unidades de cor depois da conversão: `_L[...]`.
                let mut colorlist = None;
                if at(&f, p2) == b'_' && at(&f, p2 + 1) == b'L' {
                    let tail = &f[p2.min(f.len())..];
                    let last = tail.iter().rposition(|&b| b == b']').map(|k| p2 + k);
                    if colors {
                        let open = tail.iter().position(|&b| b == b'[').map(|k| p2 + k);
                        match (open, last) {
                            (Some(a), Some(z)) if a < z => {
                                colorlist = color_fmt(&f[a + 1..z], bcnt)?;
                                p2 = z + 1;
                            }
                            (_, Some(z)) => return Err(badconv(&f[z..])),
                            (_, None) => return Err(badconv(b"(null)")),
                        }
                    } else {
                        match last {
                            Some(z) => p2 = z + 1,
                            None => return Err(badconv(b"_L")),
                        }
                    }
                }

                let mut pfmt = f[fmtp..p1].to_vec();
                let cchar = pfmt.len();
                pfmt.extend_from_slice(&cs);
                prs.push(Pr {
                    kind,
                    bcnt,
                    fmt: pfmt,
                    cchar,
                    colorlist,
                    nospace: None,
                });
                fmtp = p2;

                if kind != Kind::Address && fu_bcnt != 0 {
                    nconv += 1;
                    if nconv > 1 {
                        return Err("byte count with multiple conversion characters".to_string());
                    }
                }
            }
            let fu = &mut self.fss[fsi].fus[fui];
            fu.prs = prs;
            if fu.bcnt == 0 {
                fu.bcnt = fu.prs.iter().map(|p| p.bcnt).sum();
            }
        }

        let fs = &mut self.fss[fsi];
        let fs_bcnt = fs.bcnt;
        let n = fs.fus.len();
        for (i, fu) in fs.fus.iter_mut().enumerate() {
            if i + 1 == n && fs_bcnt < blocksize as i64 && !fu.setrep && fu.bcnt != 0 {
                let extra = (blocksize as i64 - fs_bcnt) / i64::from(fu.bcnt);
                fu.reps = fu
                    .reps
                    .saturating_add(extra.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32);
            }
            if fu.reps > 1
                && let Some(pr) = fu.prs.last_mut()
            {
                pr.nospace = match pr.fmt.last() {
                    Some(&b) if is_space(b) => Some(pr.fmt.len() - 1),
                    _ => None,
                };
            }
        }
        Ok(())
    }
}

/// `addfile`: cada linha não vazia e que não começa com `#` (depois dos espaços) é um formato.
pub(super) fn parse_format_file(data: &[u8], hex: &mut Hexdump) -> Result<(), String> {
    for line in data.split_inclusive(|&b| b == b'\n') {
        let start = skip_space(line, 0);
        if start >= line.len() || line[start] == b'#' {
            continue;
        }
        hex.add_fmt(&line[start..])?;
    }
    Ok(())
}

/// Sequência de escape de uma cor pelo nome (a tabela do `lib/colors.c`, conferida no oráculo).
pub(super) fn color_sequence(name: &[u8]) -> Option<&'static str> {
    Some(match name {
        b"black" => "\x1b[30m",
        b"blink" => "\x1b[5m",
        b"blue" => "\x1b[34m",
        b"bold" => "\x1b[1m",
        b"brown" => "\x1b[33m",
        b"cyan" => "\x1b[36m",
        b"darkgray" => "\x1b[1;30m",
        b"gray" => "\x1b[37m",
        b"green" => "\x1b[32m",
        b"halfbright" => "\x1b[2m",
        b"lightblue" => "\x1b[1;34m",
        b"lightcyan" => "\x1b[1;36m",
        b"lightgray" => "\x1b[37m",
        b"lightgreen" => "\x1b[1;32m",
        b"lightmagenta" => "\x1b[1;35m",
        b"lightred" => "\x1b[1;31m",
        b"magenta" => "\x1b[35m",
        b"red" => "\x1b[31m",
        b"reset" => "\x1b[0m",
        b"reverse" => "\x1b[7m",
        b"yellow" => "\x1b[1;33m",
        b"white" => "\x1b[1;37m",
        _ => return None,
    })
}

/// `strtoul` de um prefixo: valor (truncado pra `int`, como o C guarda) e onde parou; `None` com
/// estouro (ERANGE).
fn strtoul(s: &[u8], i: usize, base: u32) -> (Option<i64>, usize) {
    let c = strtoull_fixed_base(&s[i.min(s.len())..], base);
    if c.overflow {
        return (None, i + c.used);
    }
    (Some(c.value as i64), i + c.used)
}

/// `color_fmt`: `[!]cor[:valor|:'texto'][@início[-fim]],...`. `Ok(None)` quando um nome de cor não
/// existe (o C devolve NULL e a unidade fica sem cor nenhuma).
fn color_fmt(cfmt: &[u8], bcnt: i32) -> Result<Option<Vec<Clr>>, String> {
    let whole = cfmt;
    let mut list: Vec<Clr> = Vec::new();
    let mut cur = Clr::default();
    let mut c = 0usize;
    while c < cfmt.len() {
        if cfmt[c] == b'!' {
            cur.invert = true;
            c += 1;
        }
        let name_len = cfmt[c..]
            .iter()
            .position(|b| b":@,".contains(b))
            .unwrap_or(cfmt.len() - c);
        let Some(seq) = color_sequence(&cfmt[c..c + name_len]) else {
            return Ok(None);
        };
        cur.fmt = seq;
        c += name_len;

        if at(cfmt, c) == b':' {
            c += 1;
            if at(cfmt, c) == b'0' {
                // Com "0x" sem dígitos o `strtoul` para logo depois do "0x", que não é o começo do
                // valor: o C não acusa erro e o valor fica 0.
                let (v, end) = if matches!(at(cfmt, c + 1), b'x' | b'X') {
                    strtoul(cfmt, c + 2, 16)
                } else {
                    strtoul(cfmt, c, 8)
                };
                let Some(v) = v else {
                    return Err(badfmt(whole));
                };
                if end == c {
                    return Err(badfmt(whole));
                }
                cur.val = v as i32;
                c = end;
            } else {
                cur.val = -1;
                let fmt_end = cfmt[c..]
                    .iter()
                    .position(|&b| b == b',')
                    .map_or(cfmt.len(), |k| c + k);
                let seg = &cfmt[c..fmt_end];
                let s = match seg.iter().rposition(|&b| b == b'@') {
                    Some(k) => {
                        let mut endstr = k;
                        if k + 1 < seg.len() {
                            endstr = k.wrapping_sub(1);
                        }
                        // `endstr - cfmt + 1` bytes (com `@` no fim, ele entra no texto).
                        seg[..endstr.wrapping_add(1).min(seg.len())].to_vec()
                    }
                    None => seg.to_vec(),
                };
                c += s.len();
                cur.str = Some(s);
            }
        } else {
            cur.val = -1;
        }

        cur.range = bcnt;
        if at(cfmt, c) == b'@' {
            let (v, end) = strtoul(cfmt, c + 1, 10);
            let Some(v) = v else {
                return Err(badfmt(whole));
            };
            cur.offt = v;
            c = end;
            if at(cfmt, c) == b'-' {
                let (v, end) = strtoul(cfmt, c + 1, 10);
                let Some(v) = v else {
                    return Err(badfmt(whole));
                };
                c = end;
                cur.range = (v - cur.offt + 1) as i32;
                if cur.range < 0 {
                    return Err(badcnt(b"_L"));
                }
                while cur.range > bcnt {
                    let mut piece = cur.clone();
                    piece.range = bcnt;
                    list.push(piece);
                    cur.offt += i64::from(bcnt);
                    cur.range -= bcnt;
                    if bcnt <= 0 {
                        break;
                    }
                }
            }
        } else {
            cur.offt = -1;
        }

        if let Some(s) = &cur.str
            && s.len() as i64 != i64::from(cur.range)
        {
            return Err(badcnt(b"_L"));
        }

        if at(cfmt, c) == b',' {
            c += 1;
            list.push(std::mem::take(&mut cur));
        } else if c >= cfmt.len() {
            list.push(std::mem::take(&mut cur));
            return Ok(Some(list));
        }
    }
    // Lista vazia (`_L[]`) ou terminada em vírgula: a unidade corrente, ainda sem cor, fica na lista
    // como no C (ela não casa nada porque não tem sequência).
    list.push(cur);
    Ok(Some(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes() {
        assert_eq!(escape(br"a\nb\tc\\d\qe\0"), b"a\nb\tc\\dqe0");
        assert_eq!(escape(br"abc\"), b"abc");
        assert_eq!(escape(br"\a\b\f\r\v"), b"\x07\x08\x0c\r\x0b");
    }

    #[test]
    fn numbers() {
        assert_eq!(next_number(b"16/1", 0), Some((16, 2)));
        assert_eq!(next_number(b"x", 0), None);
        assert_eq!(next_number(b"99999999999999999999", 0), None);
        assert_eq!(next_number(b"4294967297", 0), Some((1, 10)));
    }

    #[test]
    fn colors_parse() {
        let l = color_fmt(b"red:0x41,!blue@2-3", 1).unwrap().unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].val, 0x41);
        assert_eq!(l[0].offt, -1);
        assert!(l[1].invert && l[1].offt == 2 && l[1].range == 1);
        assert_eq!(l[2].offt, 3);
        assert!(color_fmt(b"nocolor", 1).unwrap().is_none());
        assert_eq!(
            color_fmt(b"red:65", 1).unwrap_err(),
            "bad byte count for conversion character _L"
        );
        assert_eq!(color_fmt(b"red:0101", 1).unwrap().unwrap()[0].val, 0o101);
    }
}
