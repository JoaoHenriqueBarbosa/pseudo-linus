//! Leitura incremental do stdin: o buffer de bytes cresce a cada `read` no descritor 0, de modo que
//! `for line in sys.stdin` acompanha um pipe vivo (`tail -f | python`), um terminal devolve linha a linha
//! e `sys.stdin.buffer` entrega os bytes exatos (sem passar por UTF-8). O texto decodifica por cima, com
//! newlines universais (`\r\n` e `\r` viram `\n`), como o `TextIOWrapper` do CPython.

use sysabi::{sys, Fd};

use crate::object::PyFile;

const CHUNK: usize = 64 * 1024;

/// Lê um bloco do descritor 0 para o fim do buffer. `false` no fim do arquivo (ou em erro).
fn fill(f: &mut PyFile) -> bool {
    if f.raw_eof {
        return false;
    }
    let mut chunk = vec![0u8; CHUNK];
    match sys::read(Fd::STDIN, &mut chunk) {
        Ok(0) | Err(_) => {
            f.raw_eof = true;
            false
        }
        Ok(n) => {
            f.raw.extend_from_slice(&chunk[..n]);
            true
        }
    }
}

/// UTF-8 tolerante: bytes inválidos viram U+FFFD.
fn decode(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Próxima linha de texto, com `\n` no fim (se a entrada tinha terminador). `None` no fim do arquivo.
pub fn text_line(f: &mut PyFile) -> Option<String> {
    loop {
        if let Some(i) = f.raw.iter().position(|&b| b == b'\n' || b == b'\r') {
            if f.raw[i] == b'\r' && i + 1 == f.raw.len() && !f.raw_eof {
                // um `\r` no fim do bloco pode ser o começo de `\r\n`: espera o próximo byte
                if fill(f) {
                    continue;
                }
            }
            let crlf = f.raw[i] == b'\r' && f.raw.get(i + 1) == Some(&b'\n');
            let end = if crlf { i + 2 } else { i + 1 };
            let mut line = decode(&f.raw[..i]);
            line.push('\n');
            f.raw.drain(..end);
            return Some(line);
        }
        if !fill(f) {
            if f.raw.is_empty() {
                return None;
            }
            let rest = decode(&f.raw);
            f.raw.clear();
            return Some(rest);
        }
    }
}

/// Converte `\r\n` e `\r` em `\n`.
fn universal_newlines(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_string();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Tudo o que falta até o fim do arquivo, como texto.
pub fn text_all(f: &mut PyFile) -> String {
    while fill(f) {}
    let text = universal_newlines(&decode(&f.raw));
    f.raw.clear();
    text
}

/// Até `n` caracteres de texto (menos só no fim do arquivo).
pub fn text_chars(f: &mut PyFile, n: usize) -> String {
    loop {
        let (mut text, used, count) = decode_prefix(&f.raw, n);
        if count >= n || f.raw_eof {
            if count < n && used < f.raw.len() {
                // no fim do arquivo, uma sequência UTF-8 cortada vira U+FFFD
                text.push('\u{fffd}');
                f.raw.clear();
            } else {
                f.raw.drain(..used);
            }
            return universal_newlines(&text);
        }
        fill(f);
    }
}

/// Decodifica no máximo `n` caracteres do início de `raw`: devolve o texto, os bytes que ele ocupou e
/// quantos caracteres saíram. Uma sequência UTF-8 cortada no fim do buffer não conta (espera mais bytes).
fn decode_prefix(raw: &[u8], n: usize) -> (String, usize, usize) {
    let mut out = String::new();
    let mut used = 0;
    let mut count = 0;
    while count < n && used < raw.len() {
        match std::str::from_utf8(&raw[used..]) {
            Ok(valid) => {
                for c in valid.chars().take(n - count) {
                    out.push(c);
                    used += c.len_utf8();
                    count += 1;
                }
                break;
            }
            Err(e) => {
                let good = e.valid_up_to();
                // até `good` o trecho é UTF-8 válido, por contrato de `valid_up_to`
                let valid = std::str::from_utf8(&raw[used..used + good]).unwrap_or("");
                for c in valid.chars() {
                    if count == n {
                        return (out, used, count);
                    }
                    out.push(c);
                    used += c.len_utf8();
                    count += 1;
                }
                match e.error_len() {
                    Some(bad) => {
                        if count == n {
                            break;
                        }
                        out.push('\u{fffd}');
                        used += bad;
                        count += 1;
                    }
                    // sequência incompleta no fim do buffer: ainda pode completar
                    None => break,
                }
            }
        }
    }
    (out, used, count)
}

/// Bytes exatos: `n` deles (bloqueia até juntar, ou até o fim do arquivo) ou tudo o que falta.
pub fn bytes_read(f: &mut PyFile, take: Option<usize>) -> Vec<u8> {
    match take {
        Some(n) => {
            while f.raw.len() < n && fill(f) {}
            let n = n.min(f.raw.len());
            f.raw.drain(..n).collect()
        }
        None => {
            while fill(f) {}
            std::mem::take(&mut f.raw)
        }
    }
}

/// O que já está disponível, esperando só se o buffer estiver vazio (`read1`).
pub fn bytes_read1(f: &mut PyFile, take: Option<usize>) -> Vec<u8> {
    if f.raw.is_empty() {
        fill(f);
    }
    let n = take.map_or(f.raw.len(), |n| n.min(f.raw.len()));
    f.raw.drain(..n).collect()
}

/// Próxima linha em bytes (até `\n` inclusive, sem tradução).
pub fn bytes_line(f: &mut PyFile) -> Vec<u8> {
    loop {
        if let Some(i) = f.raw.iter().position(|&b| b == b'\n') {
            return f.raw.drain(..=i).collect();
        }
        if !fill(f) {
            return std::mem::take(&mut f.raw);
        }
    }
}
