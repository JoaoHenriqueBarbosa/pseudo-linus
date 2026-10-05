//! MD5 (RFC 1321), o que o `mcookie` usa pra misturar as sementes e o aleatório. Não serve pra
//! segurança, só pra reproduzir o formato do cookie.

pub struct Md5 {
    state: [u32; 4],
    buf: [u8; 64],
    buflen: usize,
    total: u64,
}

const SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11,
    16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// As 64 constantes `floor(2^32 * abs(sin(i + 1)))`.
fn k(i: usize) -> u32 {
    ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32
}

impl Default for Md5 {
    fn default() -> Self {
        Md5::new()
    }
}

impl Md5 {
    pub fn new() -> Md5 {
        Md5 { state: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476], buf: [0; 64], buflen: 0, total: 0 }
    }

    fn block(&mut self, chunk: &[u8; 64]) {
        let mut m = [0u32; 16];
        for (i, w) in m.iter_mut().enumerate() {
            *w = u32::from_le_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
        }
        let [mut a, mut b, mut c, mut d] = self.state;
        for (i, &shift) in SHIFTS.iter().enumerate() {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let tmp = d;
            d = c;
            c = b;
            b = b.wrapping_add(a.wrapping_add(f).wrapping_add(k(i)).wrapping_add(m[g]).rotate_left(shift));
            a = tmp;
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        if self.buflen > 0 {
            let take = (64 - self.buflen).min(data.len());
            self.buf[self.buflen..self.buflen + take].copy_from_slice(&data[..take]);
            self.buflen += take;
            data = &data[take..];
            if self.buflen == 64 {
                let chunk = self.buf;
                self.block(&chunk);
                self.buflen = 0;
            }
        }
        while data.len() >= 64 {
            let mut chunk = [0u8; 64];
            chunk.copy_from_slice(&data[..64]);
            self.block(&chunk);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buflen = data.len();
        }
    }

    pub fn finish(mut self) -> [u8; 16] {
        let bits = self.total.wrapping_mul(8);
        let mut pad = vec![0x80u8];
        let used = (self.buflen + 1) % 64;
        let zeros = if used <= 56 { 56 - used } else { 120 - used };
        pad.extend(std::iter::repeat_n(0u8, zeros));
        pad.extend_from_slice(&bits.to_le_bytes());
        // `update` conta o preenchimento no total, mas o comprimento já foi capturado.
        self.update(&pad);
        let mut out = [0u8; 16];
        for (i, w) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::Md5;

    fn hex(d: [u8; 16]) -> String {
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn rfc_vectors() {
        assert_eq!(hex(Md5::new().finish()), "d41d8cd98f00b204e9800998ecf8427e");
        let mut m = Md5::new();
        m.update(b"abc");
        assert_eq!(hex(m.finish()), "900150983cd24fb0d6963f7d28e17f72");
        let mut m = Md5::new();
        m.update(b"message digest");
        assert_eq!(hex(m.finish()), "f96b697d7cb7938d525a2f31aaf161d0");
        let mut m = Md5::new();
        m.update(&[b'a'; 200]);
        let mut n = Md5::new();
        for _ in 0..200 {
            n.update(b"a");
        }
        assert_eq!(hex(m.finish()), hex(n.finish()));
    }
}
