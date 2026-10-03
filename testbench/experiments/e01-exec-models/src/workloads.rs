//! Programas de pseudo-processo usados nas medições, em versão síncrona (`&dyn Sys`, modelos A e B) e
//! assíncrona (`CtxC`, modelo C). A lógica é a mesma; só a forma de esperar muda.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::model_c::CtxC;
use crate::sys::Sys;

/// Tamanho de leitura/escrita dos estágios "em bloco" (o de um stdio típico com buffer cheio).
pub const CHUNK: usize = 16 * 1024;

/// Uma linha de texto de 32 bytes repetida, pra gerar dados de pipeline.
pub const LINE: &[u8; 32] = b"lorem ipsum dolor sit amet 0123\n";

fn yes_buffer() -> Vec<u8> {
    b"y\n".repeat(4096)
}

fn text_chunk() -> Vec<u8> {
    LINE.repeat(CHUNK / LINE.len())
}

/// Conta quebras de linha (laço que o compilador vetoriza; o `memchr_iter` paga por ocorrência e fica
/// lento com linhas curtas como as do `yes`).
#[inline]
pub fn count_newlines(buf: &[u8]) -> u64 {
    buf.iter().filter(|&&b| b == b'\n').count() as u64
}

/// Onde cortar um bloco pra completar `want` linhas (posição depois da quebra de número `want`).
fn cut_after_lines(buf: &[u8], want: u64) -> usize {
    let mut seen = 0u64;
    for pos in memchr::memchr_iter(b'\n', buf) {
        seen += 1;
        if seen == want {
            return pos + 1;
        }
    }
    buf.len()
}

/// Resultado do `wc`: bytes e linhas.
#[derive(Default)]
pub struct WcOut {
    pub bytes: AtomicU64,
    pub lines: AtomicU64,
}

pub mod sync {
    use super::*;

    /// `yes`: escreve "y\n" pra sempre; morre por SIGPIPE quando o leitor fecha.
    pub fn yes(sys: &dyn Sys) -> i32 {
        let buf = yes_buffer();
        loop {
            if sys.write(1, &buf).is_err() {
                return 1;
            }
        }
    }

    /// `head -n n`: lê até ver `n` quebras de linha, repassa o prefixo pro fd 1 e sai.
    pub fn head(sys: &dyn Sys, n: u64) -> i32 {
        let mut buf = vec![0u8; CHUNK];
        let mut seen = 0u64;
        loop {
            let got = match sys.read(0, &mut buf) {
                Ok(0) | Err(_) => return 0,
                Ok(k) => k,
            };
            let chunk = &buf[..got];
            let lines = count_newlines(chunk);
            if seen + lines < n {
                seen += lines;
                let _ = sys.write(1, chunk);
                continue;
            }
            let cut = cut_after_lines(chunk, n - seen);
            let _ = sys.write(1, &chunk[..cut]);
            return 0;
        }
    }

    /// Gera `total` bytes de texto em blocos de `CHUNK`.
    pub fn gen_text(sys: &dyn Sys, total: u64) -> i32 {
        let chunk = text_chunk();
        let mut left = total;
        while left > 0 {
            let n = (chunk.len() as u64).min(left) as usize;
            if sys.write(1, &chunk[..n]).is_err() {
                return 1;
            }
            left -= n as u64;
        }
        0
    }

    /// `tr a-z A-Z`.
    pub fn upper(sys: &dyn Sys) -> i32 {
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = match sys.read(0, &mut buf) {
                Ok(0) | Err(_) => return 0,
                Ok(n) => n,
            };
            buf[..n].make_ascii_uppercase();
            if sys.write(1, &buf[..n]).is_err() {
                return 1;
            }
        }
    }

    /// `cat` com buffer de tamanho `rec`.
    pub fn relay(sys: &dyn Sys, rec: usize) -> i32 {
        let mut buf = vec![0u8; rec];
        loop {
            let n = match sys.read(0, &mut buf) {
                Ok(0) | Err(_) => return 0,
                Ok(n) => n,
            };
            if sys.write(1, &buf[..n]).is_err() {
                return 1;
            }
        }
    }

    /// Gera `total` bytes em registros de `rec` bytes (uma escrita por registro).
    pub fn gen_records(sys: &dyn Sys, total: u64, rec: usize) -> i32 {
        let chunk = text_chunk();
        let mut left = total;
        while left > 0 {
            let n = (rec as u64).min(left) as usize;
            if sys.write(1, &chunk[..n]).is_err() {
                return 1;
            }
            left -= n as u64;
        }
        0
    }

    /// `wc -lc` com buffer de tamanho `rec`.
    pub fn wc(sys: &dyn Sys, out: &Arc<WcOut>, rec: usize) -> i32 {
        let mut buf = vec![0u8; rec];
        let (mut bytes, mut lines) = (0u64, 0u64);
        loop {
            let n = match sys.read(0, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            bytes += n as u64;
            lines += count_newlines(&buf[..n]);
        }
        out.bytes.store(bytes, Ordering::Relaxed);
        out.lines.store(lines, Ordering::Relaxed);
        0
    }
}

pub mod asyncs {
    use super::*;

    pub async fn yes(ctx: CtxC) -> i32 {
        let buf = yes_buffer();
        loop {
            if ctx.write(1, &buf).await.is_err() {
                return 1;
            }
        }
    }

    pub async fn head(ctx: CtxC, n: u64) -> i32 {
        let mut buf = vec![0u8; CHUNK];
        let mut seen = 0u64;
        loop {
            let got = match ctx.read(0, &mut buf).await {
                Ok(0) | Err(_) => return 0,
                Ok(k) => k,
            };
            let lines = count_newlines(&buf[..got]);
            if seen + lines < n {
                seen += lines;
                let _ = ctx.write(1, &buf[..got]).await;
                continue;
            }
            let cut = cut_after_lines(&buf[..got], n - seen);
            let _ = ctx.write(1, &buf[..cut]).await;
            return 0;
        }
    }

    pub async fn gen_text(ctx: CtxC, total: u64) -> i32 {
        let chunk = text_chunk();
        let mut left = total;
        while left > 0 {
            let n = (chunk.len() as u64).min(left) as usize;
            if ctx.write(1, &chunk[..n]).await.is_err() {
                return 1;
            }
            left -= n as u64;
        }
        0
    }

    pub async fn upper(ctx: CtxC) -> i32 {
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = match ctx.read(0, &mut buf).await {
                Ok(0) | Err(_) => return 0,
                Ok(n) => n,
            };
            buf[..n].make_ascii_uppercase();
            if ctx.write(1, &buf[..n]).await.is_err() {
                return 1;
            }
        }
    }

    pub async fn relay(ctx: CtxC, rec: usize) -> i32 {
        let mut buf = vec![0u8; rec];
        loop {
            let n = match ctx.read(0, &mut buf).await {
                Ok(0) | Err(_) => return 0,
                Ok(n) => n,
            };
            if ctx.write(1, &buf[..n]).await.is_err() {
                return 1;
            }
        }
    }

    pub async fn gen_records(ctx: CtxC, total: u64, rec: usize) -> i32 {
        let chunk = text_chunk();
        let mut left = total;
        while left > 0 {
            let n = (rec as u64).min(left) as usize;
            if ctx.write(1, &chunk[..n]).await.is_err() {
                return 1;
            }
            left -= n as u64;
        }
        0
    }

    pub async fn wc(ctx: CtxC, out: Arc<WcOut>, rec: usize) -> i32 {
        let mut buf = vec![0u8; rec];
        let (mut bytes, mut lines) = (0u64, 0u64);
        loop {
            let n = match ctx.read(0, &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            bytes += n as u64;
            lines += count_newlines(&buf[..n]);
        }
        out.bytes.store(bytes, Ordering::Relaxed);
        out.lines.store(lines, Ordering::Relaxed);
        0
    }
}
