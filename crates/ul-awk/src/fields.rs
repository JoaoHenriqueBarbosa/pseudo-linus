//! Divisão de registros em campos: FS padrão, caractere, regex, `FS = ""`, `FIELDWIDTHS` e `FPAT`, e
//! o vetor de separadores do `split`/`patsplit` do gawk.

use crate::interp::{Interp, R, SplitMode, char_len};
use crate::regex::Regex;

fn is_default_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n')
}

/// Divide `rec` conforme `mode`. `paragraph`: no modo parágrafo o newline também separa campos.
/// `seps` recebe os separadores no formato do `split` do gawk (índice 0 é o que vem antes do primeiro
/// campo, quando existe).
pub fn split_record(
    it: &mut Interp<'_>,
    rec: &[u8],
    mode: &SplitMode,
    paragraph: bool,
    fields: &mut Vec<Vec<u8>>,
    seps: Option<&mut Vec<(usize, Vec<u8>)>>,
) -> R<()> {
    let _ = &it;
    let mut dummy = Vec::new();
    let seps = seps.unwrap_or(&mut dummy);
    match mode {
        SplitMode::Default => split_default(rec, fields, seps),
        SplitMode::Char(c, icase) => {
            if paragraph && c.as_slice() != b"\n" {
                split_multi(rec, |s, i| {
                    if s[i] == b'\n' {
                        return Some(1);
                    }
                    match_char(s, i, c, *icase)
                }, fields, seps);
            } else {
                split_multi(rec, |s, i| match_char(s, i, c, *icase), fields, seps);
            }
        }
        SplitMode::Chars => {
            let mut i = 0;
            while i < rec.len() {
                if i > 0 {
                    // O `split` com separador vazio registra um separador vazio entre cada caractere.
                    seps.push((fields.len(), Vec::new()));
                }
                let n = char_len(&rec[i..]).max(1);
                fields.push(rec[i..i + n].to_vec());
                i += n;
            }
        }
        SplitMode::Regex(re) => {
            if paragraph {
                // O newline separa campos também: divide por linha e depois pela regex.
                let mut first = true;
                for line in rec.split(|b| *b == b'\n') {
                    if !first {
                        seps.push((fields.len(), b"\n".to_vec()));
                    }
                    first = false;
                    split_regex(line, re, fields, seps);
                }
            } else {
                split_regex(rec, re, fields, seps);
            }
        }
        SplitMode::Widths(ws) => split_widths(rec, ws, fields),
        SplitMode::Fpat(re) => split_fpat(rec, re, fields, seps),
    }
    Ok(())
}

fn match_char(s: &[u8], i: usize, c: &[u8], icase: bool) -> Option<usize> {
    let n = c.len();
    if i + n > s.len() {
        return None;
    }
    let w = &s[i..i + n];
    if w == c || (icase && w.eq_ignore_ascii_case(c)) { Some(n) } else { None }
}

fn split_default(rec: &[u8], fields: &mut Vec<Vec<u8>>, seps: &mut Vec<(usize, Vec<u8>)>) {
    let mut i = 0;
    let n = rec.len();
    let lead_start = i;
    while i < n && is_default_space(rec[i]) {
        i += 1;
    }
    if i > lead_start {
        seps.push((0, rec[lead_start..i].to_vec()));
    }
    while i < n {
        let start = i;
        while i < n && !is_default_space(rec[i]) {
            i += 1;
        }
        fields.push(rec[start..i].to_vec());
        let sep_start = i;
        while i < n && is_default_space(rec[i]) {
            i += 1;
        }
        if i > sep_start {
            seps.push((fields.len(), rec[sep_start..i].to_vec()));
        }
    }
}

/// Divide por um separador literal dado por `at` (comprimento da casada na posição, se houver).
fn split_multi(rec: &[u8], at: impl Fn(&[u8], usize) -> Option<usize>, fields: &mut Vec<Vec<u8>>, seps: &mut Vec<(usize, Vec<u8>)>) {
    if rec.is_empty() {
        return;
    }
    let mut start = 0;
    let mut i = 0;
    while i < rec.len() {
        if let Some(n) = at(rec, i) {
            fields.push(rec[start..i].to_vec());
            seps.push((fields.len(), rec[i..i + n].to_vec()));
            i += n;
            start = i;
        } else {
            i += 1;
        }
    }
    fields.push(rec[start..].to_vec());
}

fn split_regex(rec: &[u8], re: &Regex, fields: &mut Vec<Vec<u8>>, seps: &mut Vec<(usize, Vec<u8>)>) {
    if rec.is_empty() {
        return;
    }
    let mut start = 0;
    let mut from = 0;
    loop {
        if from > rec.len() {
            fields.push(rec[start..].to_vec());
            return;
        }
        match re.find_at(rec, from, false) {
            Some((s, e)) if e > s => {
                fields.push(rec[start..s].to_vec());
                seps.push((fields.len(), rec[s..e].to_vec()));
                start = e;
                from = e;
                if start == rec.len() {
                    fields.push(Vec::new());
                    return;
                }
            }
            Some((s, _)) => {
                // Casada vazia não separa; segue procurando um caractere adiante.
                from = s + char_len(&rec[s..]).max(1);
            }
            None => {
                fields.push(rec[start..].to_vec());
                return;
            }
        }
    }
}

/// Posição em bytes depois de `n` caracteres a partir de `i`.
fn advance_chars(rec: &[u8], mut i: usize, n: usize) -> usize {
    for _ in 0..n {
        if i >= rec.len() {
            break;
        }
        i += char_len(&rec[i..]).max(1);
    }
    i.min(rec.len())
}

fn split_widths(rec: &[u8], ws: &[(usize, Option<usize>)], fields: &mut Vec<Vec<u8>>) {
    let mut i = 0;
    for (skip, w) in ws {
        if i >= rec.len() {
            break;
        }
        i = advance_chars(rec, i, *skip);
        if i >= rec.len() && *skip > 0 {
            break;
        }
        let end = match w {
            Some(w) => advance_chars(rec, i, *w),
            None => rec.len(),
        };
        fields.push(rec[i..end].to_vec());
        i = end;
    }
}

fn split_fpat(rec: &[u8], re: &Regex, fields: &mut Vec<Vec<u8>>, seps: &mut Vec<(usize, Vec<u8>)>) {
    if rec.is_empty() {
        return;
    }
    let mut pos = 0;
    let mut last_end: Option<usize> = None;
    let mut sep_start = 0;
    while pos <= rec.len() {
        let Some((s, e)) = re.find_at(rec, pos, false) else { break };
        if s == e {
            // Casada vazia logo depois de um campo não vazio não conta como campo.
            if last_end == Some(s) {
                if s >= rec.len() {
                    break;
                }
                pos = s + char_len(&rec[s..]).max(1);
                continue;
            }
        }
        seps.push((fields.len(), rec[sep_start..s].to_vec()));
        fields.push(rec[s..e].to_vec());
        sep_start = e;
        last_end = Some(e);
        if e == s {
            if s >= rec.len() {
                break;
            }
            pos = s + char_len(&rec[s..]).max(1);
            sep_start = s;
        } else {
            pos = e;
        }
    }
    // O separador final existe sempre no `patsplit`, mesmo vazio.
    seps.push((fields.len(), rec[sep_start.min(rec.len())..].to_vec()));
}
