//! `diff -l`: a saída passa pelo `pr` com o título "diff OPÇÕES A B". Aqui a paginação é feita em
//! processo, com o leiaute padrão do `pr` do coreutils 9.7: páginas de 66 linhas (5 de cabeçalho, 56 de
//! corpo e 5 em branco no fim), cabeçalho com a data atual ("%Y-%m-%d %H:%M"), o título centralizado e
//! "Page N" em 72 colunas, e a última página completada com linhas em branco.

use sysabi::Clock;
use unicode_width::UnicodeWidthStr;

const PAGE_LINES: usize = 66;
const HEADER_LINES: usize = 5;
const TRAILER_LINES: usize = 5;
const LINE_WIDTH: usize = 72;

fn now() -> (i64, u32) {
    match sysabi::sys::try_current().map(|s| s.clock_gettime(Clock::Realtime)) {
        Some(Ok(t)) => (t.sec, t.nsec),
        _ => (0, 0),
    }
}

fn width(s: &[u8]) -> usize {
    match std::str::from_utf8(s) {
        Ok(t) => t.width(),
        Err(_) => s.len(),
    }
}

/// Paginação de um bloco de saída.
pub fn paginate(body: &[u8], title: &[u8]) -> Vec<u8> {
    let (sec, nsec) = now();
    let tz = crate::tz::local();
    let date = crate::tz::format(sec, nsec, &tz, "%Y-%m-%d %H:%M");
    let lines: Vec<&[u8]> = body.split_inclusive(|&b| b == b'\n').collect();
    let per_page = PAGE_LINES - HEADER_LINES - TRAILER_LINES;
    let mut out = Vec::new();
    let pages = lines.len().div_ceil(per_page).max(1);
    for page in 0..pages {
        let page_text = format!("Page {}", page + 1);
        let available = LINE_WIDTH.saturating_sub(width(date.as_bytes()) + width(title) + width(page_text.as_bytes()));
        let lhs = available / 2;
        let rhs = available - lhs;
        out.extend_from_slice(b"\n\n");
        out.extend_from_slice(date.as_bytes());
        out.extend(std::iter::repeat_n(b' ', lhs.max(1)));
        out.extend_from_slice(title);
        out.extend(std::iter::repeat_n(b' ', rhs.max(1)));
        out.extend_from_slice(page_text.as_bytes());
        out.extend_from_slice(b"\n\n\n");
        let chunk = &lines[(page * per_page).min(lines.len())..((page + 1) * per_page).min(lines.len())];
        for l in chunk {
            out.extend_from_slice(l);
            if !l.ends_with(b"\n") {
                out.push(b'\n');
            }
        }
        out.extend(std::iter::repeat_n(b'\n', per_page - chunk.len() + TRAILER_LINES));
    }
    out
}
