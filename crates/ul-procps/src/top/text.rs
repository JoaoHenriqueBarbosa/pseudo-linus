//! Formatação e justificação do top (top.c: `justify_pad`, `make_*`, `scale_*` e as rotinas
//! `utf8_*`). Tudo trabalha em bytes, com as mesmas regras de largura do `snprintf` do C: a largura
//! e a precisão contam bytes, e o texto multibyte é compensado por `utf8_delta`.

use crate::ps::util::{decode_utf8, wcwidth};

/// `SCREENMAX`: tamanho dos buffers de linha do original (o texto útil tem um byte a menos).
const SCREENMAX: usize = 512;
/// `SMLBUFSIZ`.
const SMLBUFSIZ: usize = 128;
/// O caractere que marca um texto truncado.
const COLPLUSCH: u8 = b'+';

/// Sufixos de escala de `Scaled_sfxtab` (minúsculos, sem `CASEUP_SUFIX`).
const SCALED_SFX: [u8; 6] = *b"kmgtpe";

/// `UTF8_tab`: quantos bytes tem o caractere que começa com `b`; -1 para um byte inválido.
pub fn utf8_tab(b: u8) -> i32 {
    match b {
        0x00..=0x7f => 1,
        0x80..=0xc1 => -1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => -1,
    }
}

/// `utf8_cols`: colunas de tela que o caractere de `n` bytes em `s` ocupa.
fn utf8_cols(s: &[u8], n: i32) -> i32 {
    if n > 1 {
        let end = (n as usize).min(s.len());
        match decode_utf8(&s[..end]) {
            Some((cp, _)) => {
                let w = wcwidth(cp);
                if w < 0 { 1 } else { w }
            }
            None => 1,
        }
    } else {
        n
    }
}

/// `utf8_delta`: bytes menos caracteres imprimíveis (0 se há erro de decodificação).
pub fn utf8_delta(s: &[u8]) -> i32 {
    let mut p = 0usize;
    let mut cnum = 0i32;
    while p < s.len() {
        let clen = utf8_tab(s[p]);
        if clen < 0 {
            return 0;
        }
        cnum += utf8_cols(&s[p..], clen);
        p += clen as usize;
    }
    p as i32 - cnum
}

/// `utf8_embody`: quantos bytes de `s` cabem em `width` colunas (devolve `width` se há um byte
/// inválido, como o original).
pub fn utf8_embody(s: &[u8], width: i32) -> usize {
    let mut p = 0usize;
    let mut cnum = 0i32;
    if width > 0 {
        while p < s.len() {
            let clen = utf8_tab(s[p]);
            if clen < 0 {
                return width as usize;
            }
            cnum += utf8_cols(&s[p..], clen);
            if width < cnum {
                break;
            }
            p += clen as usize;
        }
    }
    p
}

/// `%.*s`: no máximo `prec` bytes de `s`.
fn take(s: &[u8], prec: usize) -> &[u8] {
    &s[..s.len().min(prec)]
}

/// `justify_pad`: `%-*.*s%s` ou `%*.*s%s` com a largura dada e o espaço de separação de coluna.
pub fn justify_pad(s: &[u8], width: i32, justr: bool) -> Vec<u8> {
    let w = width.max(0) as usize;
    let s = take(s, w);
    let mut out = Vec::with_capacity(w + 1);
    if justr {
        out.resize(w - s.len(), b' ');
        out.extend_from_slice(s);
    } else {
        out.extend_from_slice(s);
        out.resize(w, b' ');
    }
    out.push(b' ');
    out.truncate(SCREENMAX - 1);
    out
}

/// `utf8_justify`: como `justify_pad`, mas a largura é em colunas e não em bytes (cabeçalhos).
pub fn utf8_justify(s: &[u8], width: i32, justr: bool) -> Vec<u8> {
    let cut = utf8_embody(s, width);
    let tmp = take(s, cut).to_vec();
    let w = width + utf8_delta(&tmp);
    justify_pad(&tmp, w, justr)
}

/// `make_chr`: um caractere justificado.
pub fn make_chr(ch: u8, width: i32, justr: bool) -> Vec<u8> {
    justify_pad(&[ch], width, justr)
}

/// `make_num`: um inteiro sem escala; se não cabe, o último caractere vira `+`.
pub fn make_num(num: i64, width: i32, justr: bool) -> Vec<u8> {
    let mut buf = num.to_string().into_bytes();
    buf.truncate(SMLBUFSIZ - 1);
    if width < num.to_string().len() as i32 {
        let mut w = width;
        if w <= 0 || w as usize >= SMLBUFSIZ {
            w = SMLBUFSIZ as i32 - 1;
        }
        buf.resize(w as usize, 0);
        buf[w as usize - 1] = COLPLUSCH;
    }
    justify_pad(&buf, width, justr)
}

/// `make_str`: um texto justificado; se não cabe, o último caractere vira `+`.
pub fn make_str(s: &[u8], width: i32, justr: bool) -> Vec<u8> {
    let mut buf = take(s, SCREENMAX - 1).to_vec();
    if width < s.len() as i32 {
        let mut w = width;
        if w <= 0 || w as usize >= SCREENMAX {
            w = SCREENMAX as i32 - 1;
        }
        buf.resize(w as usize, 0);
        buf[w as usize - 1] = COLPLUSCH;
    }
    justify_pad(&buf, width, justr)
}

/// `make_str_utf8`: como `make_str`, mas conta colunas (texto multibyte).
pub fn make_str_utf8(s: &[u8], width: i32, justr: bool) -> Vec<u8> {
    let mut delta = utf8_delta(s);
    let mut buf = take(s, SCREENMAX - 1).to_vec();
    if width + delta < s.len() as i32 {
        let cut = utf8_embody(s, width - 1);
        buf = take(s, cut).to_vec();
        buf.push(COLPLUSCH);
        buf.truncate(SCREENMAX - 1);
        delta = utf8_delta(&buf);
    }
    justify_pad(&buf, width + delta, justr)
}

/// `%.Nf` de um número de ponto flutuante (o `float` do C chega ao `printf` como `double`; o NaN
/// do x86 sai com o sinal, `-nan`).
fn fixed(v: f32, prec: usize) -> String {
    if v.is_nan() {
        return "-nan".to_string();
    }
    format!("{:.*}", prec, f64::from(v))
}

/// `scale_mem`: `num` em KiB, escalado para atingir `target` (0 = KiB ... 4 = PiB) e caber em
/// `width`; `?` se nada cabe.
pub fn scale_mem(target: usize, num: f32, width: i32, justr: bool) -> Vec<u8> {
    let mut num = num;
    for (i, sfx) in SCALED_SFX.iter().enumerate().take(5) {
        if i >= target {
            let s = if i == 0 { fixed(num, 0) } else { format!("{}{}", fixed(num, 1), *sfx as char) };
            if width >= s.len() as i32 {
                return justify_pad(s.as_bytes(), width, justr);
            }
        }
        num /= 1024.0;
    }
    justify_pad(b"?", width, justr)
}

/// `scale_pcnt`: um percentual com precisão decrescente (`xtra` pede 3 e 2 casas antes de 1).
pub fn scale_pcnt(num: f32, width: i32, justr: bool, xtra: bool) -> Vec<u8> {
    let mut attempts: Vec<String> = Vec::new();
    if xtra {
        attempts.push(fixed(num, 3));
        attempts.push(fixed(num, 2));
    }
    attempts.push(fixed(num, 1));
    attempts.push(format!("{:>w$.0}", f64::from(num), w = width.max(0) as usize));
    for s in attempts {
        if width >= s.len() as i32 {
            return justify_pad(s.as_bytes(), width, justr);
        }
    }
    justify_pad(b"?", width, justr)
}

/// Alvos de `scale_tics`.
pub const TICS_AS_SECS: u32 = 0;

/// `scale_tics`: ticks de CPU como `min:seg.cent` (e, se não couber, horas, dias, semanas).
pub fn scale_tics(tics: u64, hertz: u64, width: i32, justr: bool, target: u32) -> Vec<u8> {
    let nt = tics.wrapping_mul(100) / hertz.max(1);
    let cent = nt % 100;
    let secs = nt / 100;
    let mins = secs / 60;
    let hour = mins / 60;
    let days = hour / 24;
    let week = days / 7;
    let fits = |s: String| -> Option<Vec<u8>> {
        (width >= s.len() as i32).then(|| justify_pad(s.as_bytes(), width, justr))
    };
    if target == 0 && mins < 361
        && let Some(r) = fits(format!("{}:{:02}.{:02}", mins, secs % 60, cent)) {
            return r;
        }
    if target <= 1 && mins < 361
        && let Some(r) = fits(format!("{}:{:02}", mins, secs % 60)) {
            return r;
        }
    if target <= 2 && hour < 97
        && let Some(r) = fits(format!("{},{:02}", hour, mins % 60)) {
            return r;
        }
    if target <= 3 {
        if days < 15 {
            if let Some(r) = fits(format!("{}d+{}h", days, hour % 24)) {
                return r;
            }
            if let Some(r) = fits(format!("{days}d")) {
                return r;
            }
        }
    } else if target == 4
        && let Some(r) = fits(format!("{days}d")) {
            return r;
        }
    if target <= 5
        && let Some(r) = fits(format!("{}w+{}d", week, days % 7)) {
            return r;
        }
    if let Some(r) = fits(format!("{week}w")) {
        return r;
    }
    justify_pad(b"?", width, justr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn justify_and_truncate() {
        assert_eq!(justify_pad(b"ab", 5, true), b"   ab ");
        assert_eq!(justify_pad(b"ab", 5, false), b"ab    ");
        assert_eq!(make_str(b"kworker/u8:2", 8, false), b"kworker+ ");
        assert_eq!(make_num(123456, 3, true), b"12+ ");
    }

    #[test]
    fn memory_scaling() {
        assert_eq!(scale_mem(0, 4488.0, 7, true), b"   4488 ");
        assert_eq!(scale_mem(1, 4488.0, 7, true), b"   4.4m ");
        assert_eq!(scale_mem(1, 214_844.0, 7, true), b" 209.8m ");
        assert_eq!(scale_mem(0, 214_844_000.0, 6, true), b"204.9g ");
    }

    #[test]
    fn percent_and_tics() {
        assert_eq!(scale_pcnt(0.04, 5, true, false), b"  0.0 ");
        // 99.95 em `float` é 99.9499969..., e o `%.1f` do C arredonda pra baixo, como aqui.
        assert_eq!(scale_pcnt(99.95, 5, true, false), b" 99.9 ");
        assert_eq!(scale_pcnt(99.96, 5, true, false), b"100.0 ");
        assert_eq!(scale_tics(17, 100, 9, true, TICS_AS_SECS), b"  0:00.17 ");
        assert_eq!(scale_tics(52_000, 100, 9, true, TICS_AS_SECS), b"  8:40.00 ");
    }
}
