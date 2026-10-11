//! Gerador de números pseudoaleatórios do SQLite (random.c do 3.46.1): ChaCha20 (RFC 7539).
//!
//! No C, o estado é global e protegido por mutex (`SQLITE_MUTEX_STATIC_PRNG`), e é inicializado
//! na primeira chamada de `sqlite3_randomness` com 44 bytes de `xRandomness` do VFS padrão.
//! Aqui o estado é a struct [`Prng`]; a semente (o que o VFS devolveria) entra pelo
//! construtor, e o dono global (`crate::global`, com o mutex) guarda a instância. Usam-se só
//! os primeiros 44 bytes da semente, como no C.
//!
//! `sqlite3PrngSaveState` e `sqlite3PrngRestoreState` (só para `sqlite3_test_control`) ficam
//! de fora a pedido do porte.

/// O estado do gerador (`struct sqlite3PrngType`).
pub struct Prng {
    /// 64 bytes do estado do ChaCha20.
    s: [u32; 16],
    /// Bytes de saída.
    out: [u8; 64],
    /// Bytes de saída que restam (os últimos `n` de `out`).
    n: u8,
}

/// Constantes de inicialização do ChaCha20 ("expand 32-byte k").
const CHACHA20_INIT: [u32; 4] = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574];

/// A macro `QR` do C: um quarto de rodada.
#[inline]
fn qr(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    x[a] = x[a].wrapping_add(x[b]);
    x[d] ^= x[a];
    x[d] = x[d].rotate_left(16);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] ^= x[c];
    x[b] = x[b].rotate_left(12);
    x[a] = x[a].wrapping_add(x[b]);
    x[d] ^= x[a];
    x[d] = x[d].rotate_left(8);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] ^= x[c];
    x[b] = x[b].rotate_left(7);
}

/// `chacha_block`: a função de bloco do ChaCha20 da RFC 7539.
fn chacha_block(input: &[u32; 16]) -> [u32; 16] {
    let mut x = *input;
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
    let mut out = [0u32; 16];
    for i in 0..16 {
        out[i] = x[i].wrapping_add(input[i]);
    }
    out
}

impl Prng {
    /// Inicializa o estado como `sqlite3_randomness` faz na primeira chamada: constantes do
    /// ChaCha20 em `s[0..4]`, 44 bytes da semente em `s[4..15]` (ordem de bytes nativa, little
    /// endian), `s[15] = s[12]` e `s[12] = 0` (contador de blocos).
    pub fn new(seed: &[u8; 256]) -> Prng {
        let mut s = [0u32; 16];
        s[..4].copy_from_slice(&CHACHA20_INIT);
        for (i, chunk) in seed[..44].chunks_exact(4).enumerate() {
            s[4 + i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        s[15] = s[12];
        s[12] = 0;
        Prng { s, out: [0; 64], n: 0 }
    }

    /// O ramo `N<=0 || pBuf==0` do C (`sqlite3_randomness(0, 0)`): zera `s[0]`, o que força a
    /// reinicialização na chamada seguinte, com bytes novos do VFS. Aqui a reinicialização
    /// é imediata, com a semente nova dada.
    pub fn reset(&mut self, seed: &[u8; 256]) {
        *self = Prng::new(seed);
    }

    /// `sqlite3_randomness`: preenche `out` com bytes pseudoaleatórios. Um `out` vazio não faz
    /// nada (o `N<=0` do C vira [`Prng::reset`], pedido explicitamente pelo chamador).
    pub fn randomness(&mut self, out: &mut [u8]) {
        let mut n_left = out.len();
        let mut pos = 0;
        if n_left == 0 {
            return;
        }
        loop {
            let have = self.n as usize;
            if n_left <= have {
                out[pos..pos + n_left].copy_from_slice(&self.out[have - n_left..have]);
                self.n -= n_left as u8;
                break;
            }
            if have > 0 {
                out[pos..pos + have].copy_from_slice(&self.out[..have]);
                n_left -= have;
                pos += have;
            }
            self.s[12] = self.s[12].wrapping_add(1);
            let block = chacha_block(&self.s);
            for (i, w) in block.iter().enumerate() {
                self.out[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
            }
            self.n = 64;
        }
    }
}
