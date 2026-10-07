//! CRC-32 e a cifra tradicional do PKZIP (crypt.c): chaves, cabeçalho aleatório e `zencode`.

use ul_common::codec::crc32_step;
pub use ul_common::codec::crc32_update;

use super::consts::RAND_HEAD_LEN;

/// As três chaves da cifra.
pub struct Keys {
    k: [u32; 3],
}

impl Keys {
    /// `init_keys`: chaves iniciais modificadas pela senha.
    pub fn new(passwd: &[u8]) -> Keys {
        let mut keys = Keys { k: [305_419_896, 591_751_049, 878_082_192] };
        for &c in passwd {
            keys.update(c);
        }
        keys
    }

    /// `update_keys`.
    fn update(&mut self, c: u8) {
        self.k[0] = crc32_step(self.k[0], c);
        self.k[1] = (self.k[1].wrapping_add(self.k[0] & 0xff)).wrapping_mul(134_775_813).wrapping_add(1);
        let keyshift = (self.k[1] >> 24) as u8;
        self.k[2] = crc32_step(self.k[2], keyshift);
    }

    /// `decrypt_byte`: o próximo byte da seqüência pseudo-aleatória.
    fn decrypt_byte(&self) -> u8 {
        let temp = (self.k[2] & 0xffff) | 2;
        (((temp.wrapping_mul(temp ^ 1)) >> 8) & 0xff) as u8
    }

    /// `zencode`: cifra um byte.
    pub fn encode(&mut self, c: u8) -> u8 {
        let t = self.decrypt_byte();
        self.update(c);
        t ^ c
    }
}

/// `crypthead`: o cabeçalho de 12 bytes. Os 10 primeiros vêm do gerador aleatório (o zip usa `rand()`
/// semeado pelo relógio, então só o resto do arquivo é reproduzível); os dois últimos são a parte alta
/// do crc (ou do horário, quando o crc ainda não existe). Devolve o cabeçalho e as chaves já no estado
/// em que a cifra dos dados continua.
pub fn crypthead(passwd: &[u8], crc: u32, random: &[u8; RAND_HEAD_LEN - 2]) -> ([u8; RAND_HEAD_LEN], Keys) {
    let mut header = [0u8; RAND_HEAD_LEN];
    let mut keys = Keys::new(passwd);
    for n in 0..RAND_HEAD_LEN - 2 {
        let c = random[n];
        header[n] = keys.encode(c);
    }
    let mut keys = Keys::new(passwd);
    for h in header.iter_mut().take(RAND_HEAD_LEN - 2) {
        *h = keys.encode(*h);
    }
    header[RAND_HEAD_LEN - 2] = keys.encode(((crc >> 16) & 0xff) as u8);
    header[RAND_HEAD_LEN - 1] = keys.encode(((crc >> 24) & 0xff) as u8);
    (header, keys)
}
