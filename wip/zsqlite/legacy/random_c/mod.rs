// Mesclado das partes traduzidas de random_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Tipo de estado do gerador de números pseudo-aleatórios.
/// Contém o estado ChaCha20, o buffer de saída e um contador de bytes disponíveis.
#[derive(Clone, Copy)]
struct PrngType {
    /// 64 bytes de estado ChaCha20 (16 x u32)
    s: [u32; 16],
    /// Bytes de saída do bloco ChaCha20
    out: [u8; 64],
    /// Número de bytes de saída ainda disponíveis no buffer
    n: u8,
}

/// Estado zerado, como o de uma variável estática do C.
const PRNG_ZERO: PrngType = PrngType {
    s: [0; 16],
    out: [0; 64],
    n: 0,
};

/// Estado compartilhado do gerador (sqlite3Prng), protegido por mutex.
fn prng_state() -> &'static std::sync::Mutex<PrngType> {
    static STATE: std::sync::Mutex<PrngType> = std::sync::Mutex::new(PRNG_ZERO);
    &STATE
}

/// Estado salvo do gerador para testes (sqlite3SavedPrng).
fn saved_prng_state() -> &'static std::sync::Mutex<PrngType> {
    static STATE: std::sync::Mutex<PrngType> = std::sync::Mutex::new(PRNG_ZERO);
    &STATE
}

/// Rotação circular para esquerda de um inteiro de 32 bits
#[inline]
fn rotl(a: u32, b: u32) -> u32 {
    ((a) << (b)) | ((a) >> (32 - (b)))
}

/// Macro QR do RFC-7539 ChaCha20: atualiza x[a], x[b], x[c], x[d] no próprio vetor.
#[inline]
fn qr(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    x[a] = x[a].wrapping_add(x[b]);
    x[d] ^= x[a];
    x[d] = rotl(x[d], 16);

    x[c] = x[c].wrapping_add(x[d]);
    x[b] ^= x[c];
    x[b] = rotl(x[b], 12);

    x[a] = x[a].wrapping_add(x[b]);
    x[d] ^= x[a];
    x[d] = rotl(x[d], 8);

    x[c] = x[c].wrapping_add(x[d]);
    x[b] ^= x[c];
    x[b] = rotl(x[b], 7);
}

/// Função de bloco ChaCha20 conforme RFC-7539.
/// Preenche o array `out` com o resultado de uma rodada completa (10 iterações de 8 quartets)
/// do algoritmo ChaCha20 sobre o estado `prng_in`.
fn chacha_block(out: &mut [u32; 16], prng_in: &[u32; 16]) {
    let mut x = *prng_in;

    for _ in 0..10 {
        qr(&mut x, 0, 4, 8, 12);
        qr(&mut x, 1, 5, 9, 13);
        qr(&mut x, 2, 6, 10, 14);
        qr(&mut x, 3, 7, 11, 15);
        qr(&mut x, 0, 5, 10, 15);
        qr(&mut x, 1, 6, 11, 12);
        qr(&mut x, 2, 7, 8, 13);
        qr(&mut x, 3, 4, 9, 14);
    }

    for i in 0..16 {
        out[i] = x[i].wrapping_add(prng_in[i]);
    }
}

/// Retorna N bytes aleatórios gerados pelo ChaCha20 do gerador de números pseudo-aleatórios.
///
/// Se N <= 0 ou o buffer é vazio, reseta o estado do gerador (zera s[0]).
/// Todos os threads compartilham um único gerador protegido por mutex.
pub fn randomness(n_bytes: i32, buf: &mut [u8]) {
    // SQLITE_OMIT_AUTOINIT não está definido: inicializa a biblioteca antes de tudo.
    if api::initialize() != 0 {
        return;
    }

    let mut prng = match prng_state().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    if n_bytes <= 0 || buf.is_empty() {
        prng.s[0] = 0;
        return;
    }

    // Inicializa o estado do gerador na primeira chamada
    if prng.s[0] == 0 {
        // Constantes do ChaCha20 conforme RFC-7539
        const CHACHA20_INIT: [u32; 4] = [
            0x61707865, 0x3320646e, 0x79622d32, 0x6b206574
        ];
        prng.s[0..4].copy_from_slice(&CHACHA20_INIT);

        // 44 bytes de aleatoriedade do VFS preenchem s[4..15] (11 palavras u32);
        // sem VFS registrado ficam zerados.
        let mut seed = [0u8; 44];
        if let Some(vfs) = api::vfs_find(None) {
            os_randomness(&vfs, 44, &mut seed);
        }
        for i in 0..11 {
            prng.s[4 + i] = u32::from_le_bytes([
                seed[i * 4],
                seed[i * 4 + 1],
                seed[i * 4 + 2],
                seed[i * 4 + 3],
            ]);
        }

        prng.s[15] = prng.s[12];
        prng.s[12] = 0;
        prng.n = 0;
    }

    let mut n_remaining = n_bytes as usize;
    let mut buf_offset = 0;

    // Fornece bytes aleatórios do buffer de saída, regenerando quando necessário
    loop {
        if n_remaining <= prng.n as usize {
            let start = (prng.n as usize) - n_remaining;
            buf[buf_offset..(buf_offset + n_remaining)]
                .copy_from_slice(&prng.out[start..(start + n_remaining)]);
            prng.n -= n_remaining as u8;
            break;
        }

        if prng.n > 0 {
            buf[buf_offset..(buf_offset + prng.n as usize)]
                .copy_from_slice(&prng.out[0..(prng.n as usize)]);
            n_remaining -= prng.n as usize;
            buf_offset += prng.n as usize;
        }

        // Incrementa o contador de bloco e gera novo bloco de saída ChaCha20
        prng.s[12] = prng.s[12].wrapping_add(1);

        let mut out_u32: [u32; 16] = [0; 16];
        chacha_block(&mut out_u32, &prng.s);

        // Converte a saída u32 em bytes little-endian e copia para o buffer de saída u8
        for i in 0..16 {
            let bytes = out_u32[i].to_le_bytes();
            prng.out[(i * 4)..(i * 4 + 4)].copy_from_slice(&bytes);
        }

        prng.n = 64;
    }
}

/// Salva o estado atual do gerador de números pseudo-aleatórios para posteriormente restaurá-lo.
/// Utilizado somente para testes, controlado via `sqlite3_test_control()`.
pub fn prng_save_state() {
    if let Ok(prng) = prng_state().lock() {
        if let Ok(mut saved) = saved_prng_state().lock() {
            *saved = *prng;
        }
    }
}

/// Restaura o estado salvo do gerador de números pseudo-aleatórios.
/// Utilizado somente para testes, controlado via `sqlite3_test_control()`.
pub fn prng_restore_state() {
    if let Ok(mut prng) = prng_state().lock() {
        if let Ok(saved) = saved_prng_state().lock() {
            *prng = *saved;
        }
    }
}

