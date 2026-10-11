//! Os escritores de bits do libwebp 1.5.0 (`utils/bit_writer_utils.c`): o codificador booleano do VP8 e o fluxo de
//! bits do VP8L (bit menos significativo primeiro).

/// `VP8BitWriter`.
#[derive(Clone)]
pub struct Vp8BitWriter {
    range: i32,
    value: i32,
    run: i32,
    nb_bits: i32,
    buf: Vec<u8>,
}

/// `kNorm[i] = 7 - log2(i + 1)`: os bits que a renormalização emite.
fn norm(range: i32) -> i32 {
    7 - (31 - ((range + 1) as u32).leading_zeros() as i32)
}

impl Default for Vp8BitWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl Vp8BitWriter {
    /// `VP8BitWriterInit`.
    pub fn new() -> Self {
        Vp8BitWriter { range: 255 - 1, value: 0, run: 0, nb_bits: -8, buf: Vec::new() }
    }

    fn flush(&mut self) {
        let s = 8 + self.nb_bits;
        let bits = self.value >> s;
        self.value -= bits << s;
        self.nb_bits -= 8;
        if bits & 0xff != 0xff {
            if bits & 0x100 != 0 {
                if let Some(last) = self.buf.last_mut() {
                    *last = last.wrapping_add(1);
                }
            }
            let value = if bits & 0x100 != 0 { 0x00 } else { 0xff };
            for _ in 0..self.run {
                self.buf.push(value);
            }
            self.run = 0;
            self.buf.push((bits & 0xff) as u8);
        } else {
            self.run += 1;
        }
    }

    fn renormalize(&mut self, shift: i32) {
        // `kNewRange[i] = ((i + 1) << kNorm[i]) - 1`.
        self.range = ((self.range + 1) << shift) - 1;
        self.value <<= shift;
        self.nb_bits += shift;
        if self.nb_bits > 0 {
            self.flush();
        }
    }

    /// `VP8PutBit`.
    pub fn put_bit(&mut self, bit: bool, prob: i32) -> bool {
        let split = (self.range * prob) >> 8;
        if bit {
            self.value += split + 1;
            self.range -= split + 1;
        } else {
            self.range = split;
        }
        if self.range < 127 {
            let shift = norm(self.range);
            self.renormalize(shift);
        }
        bit
    }

    /// `VP8PutBitUniform`.
    pub fn put_bit_uniform(&mut self, bit: bool) -> bool {
        let split = self.range >> 1;
        if bit {
            self.value += split + 1;
            self.range -= split + 1;
        } else {
            self.range = split;
        }
        if self.range < 127 {
            // `kNewRange` com um bit só: o intervalo nunca cai abaixo de 63 aqui.
            self.range = ((self.range + 1) << norm(self.range)) - 1;
            self.value <<= 1;
            self.nb_bits += 1;
            if self.nb_bits > 0 {
                self.flush();
            }
        }
        bit
    }

    /// `VP8PutBits`.
    pub fn put_bits(&mut self, value: u32, nb_bits: u32) {
        let mut mask = 1u32 << (nb_bits - 1);
        while mask != 0 {
            self.put_bit_uniform(value & mask != 0);
            mask >>= 1;
        }
    }

    /// `VP8PutSignedBits`.
    pub fn put_signed_bits(&mut self, value: i32, nb_bits: u32) {
        if !self.put_bit_uniform(value != 0) {
            return;
        }
        if value < 0 {
            self.put_bits(((-value as u32) << 1) | 1, nb_bits + 1);
        } else {
            self.put_bits((value as u32) << 1, nb_bits + 1);
        }
    }

    /// `VP8BitWriterFinish`.
    pub fn finish(&mut self) {
        self.put_bits(0, (9 - self.nb_bits) as u32);
        self.nb_bits = 0;
        self.flush();
    }

    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }
}

/// `VP8LBitWriter`: o fluxo do VP8L sai do bit menos significativo para o mais significativo, em bytes; o
/// acumulador por palavras do C produz os mesmos bytes.
#[derive(Clone, Default)]
pub struct Vp8lBitWriter {
    bits: u64,
    used: u32,
    buf: Vec<u8>,
}

impl Vp8lBitWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// `VP8LPutBits`.
    pub fn put_bits(&mut self, bits: u32, n_bits: u32) {
        if n_bits == 0 {
            return;
        }
        self.bits |= u64::from(bits) << self.used;
        self.used += n_bits;
        while self.used >= 8 {
            self.buf.push(self.bits as u8);
            self.bits >>= 8;
            self.used -= 8;
        }
    }

    /// `VP8LBitWriterNumBytes`.
    pub fn num_bytes(&self) -> usize {
        self.buf.len() + self.used.div_ceil(8) as usize
    }

    /// `VP8LBitWriterFinish`.
    pub fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.buf.push(self.bits as u8);
        }
        self.buf
    }
}
