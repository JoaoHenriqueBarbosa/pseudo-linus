//
// Copyright (c) 2024-2026 Hemi Labs, Inc.
//
// This file is part of the posixutils-rs project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT
//
// Modificado no pseudo-linus (2026, MIT): largura vinda do `BC_LINE_LENGTH` (o upstream fixava 68),
// contagem por byte como o `out_char` do GNU e saída pelo stdout do `sysio`.

//! Saída do bc quebrada na largura de linha, como o `out_char` do GNU bc 1.07.1.
//!
//! A coluna é do fluxo, não de cada valor: um número escrito depois de uma string continua a linha
//! dela. Cada byte que não é `\n` avança a coluna; quando ela chega a `largura - 1` o GNU escreve
//! `\` e `\n` antes do byte e recomeça a contar em 1, então cada linha quebrada tem `largura - 2`
//! bytes mais a barra. Largura 0 desliga a quebra.

use std::io::Write;

use crate::util::io;

/// Largura padrão (sem `BC_LINE_LENGTH`).
pub const DEFAULT_LINE_LENGTH: i64 = 70;

/// A largura que o GNU usa pra um valor de `BC_LINE_LENGTH`: `atoi`, e qualquer coisa abaixo de 3
/// que não seja 0 vira 70.
pub fn line_length_from_env(value: Option<&[u8]>) -> i64 {
    let Some(v) = value else {
        return DEFAULT_LINE_LENGTH;
    };
    let n = atoi(v);
    if n != 0 && n < 3 {
        DEFAULT_LINE_LENGTH
    } else {
        n
    }
}

/// `atoi` da glibc: espaços à esquerda, sinal opcional, dígitos até o primeiro não dígito; o valor
/// de `strtol` truncado pra `int`.
pub fn atoi(v: &[u8]) -> i64 {
    let mut i = 0;
    while i < v.len() && matches!(v[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut neg = false;
    if i < v.len() && (v[i] == b'+' || v[i] == b'-') {
        neg = v[i] == b'-';
        i += 1;
    }
    let mut acc: i64 = 0;
    while i < v.len() && v[i].is_ascii_digit() {
        acc = acc
            .saturating_mul(10)
            .saturating_add(i64::from(v[i] - b'0'));
        i += 1;
    }
    let acc = if neg { acc.saturating_neg() } else { acc };
    // strtol satura em LONG_MAX/LONG_MIN e o atoi corta pra int.
    i64::from(acc as i32)
}

/// O fluxo de saída com a coluna corrente.
pub struct Output {
    line_size: i64,
    col: i64,
    buf: Vec<u8>,
}

impl Output {
    pub fn new(line_size: i64) -> Output {
        Output {
            line_size,
            col: 0,
            buf: Vec::new(),
        }
    }

    pub fn line_size(&self) -> i64 {
        self.line_size
    }

    /// Um byte, com a regra de quebra do GNU.
    pub fn put(&mut self, ch: u8) {
        if ch == b'\n' {
            self.col = 0;
            self.buf.push(b'\n');
        } else {
            self.col += 1;
            if self.line_size != 0 && self.col == self.line_size - 1 {
                self.buf.extend_from_slice(b"\\\n");
                self.col = 1;
            }
            self.buf.push(ch);
        }
        if self.buf.len() >= 8192 {
            self.drain();
        }
    }

    pub fn put_bytes(&mut self, s: &[u8]) {
        for &c in s {
            self.put(c);
        }
    }

    /// Escreve sem passar pela coluna (o GNU usa `printf` direto no `limits` e no `warranty`).
    pub fn raw(&mut self, s: &[u8]) {
        self.buf.extend_from_slice(s);
        if self.buf.len() >= 8192 {
            self.drain();
        }
    }

    /// Passa o que está acumulado pro stdout do processo (que tem o buffer da glibc).
    pub fn drain(&mut self) {
        if !self.buf.is_empty() {
            let _ = io::stdout().write_all(&self.buf);
            self.buf.clear();
        }
    }

    /// `fflush(stdout)`: o GNU descarrega antes de cada mensagem no stderr.
    pub fn flush(&mut self) {
        self.drain();
        let _ = io::flush_stdout();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(line: i64, chunks: &[&str]) -> String {
        let mut out = Output::new(line);
        for c in chunks {
            out.put_bytes(c.as_bytes());
        }
        String::from_utf8(out.buf).unwrap()
    }

    #[test]
    fn wraps_like_gnu() {
        let digits = "1".repeat(73);
        let text = written(70, &[&digits, "\n"]);
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines[0].len(), 69);
        assert!(lines[0].ends_with('\\'));
        assert_eq!(text.replace("\\\n", ""), format!("{digits}\n"));
        assert_eq!(
            written(70, &[&"1".repeat(68), "\n"]),
            format!("{}\n", "1".repeat(68))
        );
        assert_eq!(written(3, &["123"]), "1\\\n2\\\n3");
        assert_eq!(written(0, &[&"1".repeat(200)]), "1".repeat(200));
    }

    #[test]
    fn column_carries_and_resets() {
        let text = written(70, &["ab", &"1".repeat(73), "\n"]);
        assert!(text.split('\n').next().unwrap().starts_with("ab1"));
        assert_eq!(
            written(70, &["ab\n", &"1".repeat(68), "\n"]),
            format!("ab\n{}\n", "1".repeat(68))
        );
    }

    #[test]
    fn env_line_length() {
        assert_eq!(line_length_from_env(None), 70);
        assert_eq!(line_length_from_env(Some(b"0")), 0);
        assert_eq!(line_length_from_env(Some(b"abc")), 0);
        assert_eq!(line_length_from_env(Some(b"2")), 70);
        assert_eq!(line_length_from_env(Some(b"-1")), 70);
        assert_eq!(line_length_from_env(Some(b" 20x")), 20);
        assert_eq!(line_length_from_env(Some(b"")), 0);
    }
}
