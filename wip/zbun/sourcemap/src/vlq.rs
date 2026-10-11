//! VLQ de tamanho variável limitado a `i32`, como manda a especificação de mapas de fontes
//! (https://sourcemaps.info/spec.html). É o `bun_base64::vlq` do upstream.

/// `i32::MIN` e `i32::MAX` codificam como `//////D` e `+/////D`, sete bytes: o `VLQ` cabe em oito.
#[derive(Copy, Clone)]
pub struct VLQ {
    bytes: [u8; VLQ_MAX_IN_BYTES],
    pub len: u8,
}

const VLQ_MAX_IN_BYTES: usize = 7;

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

impl VLQ {
    #[inline]
    pub fn slice(&self) -> &[u8] {
        &self.bytes[0..self.len as usize]
    }

    pub const ZERO: VLQ = VLQ_LOOKUP_TABLE[0];

    #[inline]
    pub const fn encode(value: i32) -> VLQ {
        if value >= 0 && value <= 255 {
            VLQ_LOOKUP_TABLE[value as usize]
        } else {
            encode_slow_path(value)
        }
    }
}

const VLQ_LOOKUP_TABLE: [VLQ; 256] = {
    let mut entries = [VLQ { bytes: [0; VLQ_MAX_IN_BYTES], len: 0 }; 256];
    let mut i: usize = 0;
    while i < 256 {
        entries[i] = encode_slow_path(i as i32);
        i += 1;
    }
    entries
};

// Um dígito base 64 guarda 6 bits. No VLQ do mapa de fontes o primeiro bit é o sinal, os quatro seguintes são o
// valor, e o sexto é o de continuação, que diz se vem mais um dígito.
//
//   Continuação
//   |    Sinal
//   |    |
//   V    V
//   101011
const fn encode_slow_path(value: i32) -> VLQ {
    let mut len: u8 = 0;
    let mut bytes: [u8; VLQ_MAX_IN_BYTES] = [0; VLQ_MAX_IN_BYTES];

    // Sinal e magnitude: `i32::MIN` não tem representação (a magnitude estoura o `u32`), então vira "-0".
    let mut vlq: u32 = if value >= 0 { (value << 1) as u32 } else { (value.unsigned_abs() << 1) | 1 };

    let mut iter = 0;
    while iter < VLQ_MAX_IN_BYTES {
        let mut digit = vlq & 31;
        vlq >>= 5;
        // Se ainda há dígitos, marca a continuação.
        if vlq != 0 {
            digit |= 32;
        }
        bytes[len as usize] = BASE64[digit as usize];
        len += 1;
        if vlq == 0 {
            return VLQ { bytes, len };
        }
        iter += 1;
    }

    VLQ { bytes, len: 0 }
}

#[derive(Copy, Clone, Default)]
pub struct VLQResult {
    pub value: i32,
    pub start: usize,
}

const U7_MAX: u8 = 127;

// O base64 guarda valores de até 7 bits.
const BASE64_LUT: [u8; U7_MAX as usize + 1] = {
    let mut bytes = [U7_MAX; U7_MAX as usize + 1];
    let mut i = 0;
    while i < BASE64.len() {
        bytes[BASE64[i] as usize] = i as u8;
        i += 1;
    }
    bytes
};

/// Decodifica um VLQ em `encoded[start..]`. Entrada vazia ou cortada no meio devolve `start` sem avançar e valor 0,
/// para o chamador tratar como mapeamento malformado.
pub fn decode(encoded: &[u8], start: usize) -> VLQResult {
    let mut shift: u8 = 0;
    let mut vlq: u32 = 0;

    let encoded_ = &encoded[start..][0..(encoded.len() - start).min(VLQ_MAX_IN_BYTES + 1)];
    for (i, &byte) in encoded_.iter().enumerate() {
        let index = BASE64_LUT[(byte & 0x7f) as usize] as u32;
        vlq |= (index & 31) << (shift & 31);
        shift += 5;

        // Para se não há bit de continuação.
        if (index & 32) == 0 {
            return VLQResult {
                start: start + i + 1,
                value: if (vlq & 1) == 0 { (vlq >> 1) as i32 } else { -((vlq >> 1) as i32) },
            };
        }
    }
    VLQResult { start, value: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        for value in [0, 1, -1, 255, 256, -255, -256, i32::MAX, i32::MIN + 1] {
            let encoded = VLQ::encode(value);
            let result = decode(encoded.slice(), 0);
            assert_eq!(result.value, value);
            assert_eq!(result.start, encoded.len as usize);
        }
        assert_eq!(VLQ::encode(i32::MAX).slice(), b"+/////D");
        assert_eq!(VLQ::encode(i32::MIN + 1).slice(), b"//////D");
    }

    #[test]
    fn i32_min_wraps_to_negative_zero() {
        assert_eq!(decode(VLQ::encode(i32::MIN).slice(), 0).value, 0);
    }

    #[test]
    fn truncated_input_does_not_advance() {
        let r = decode(b"g", 0);
        assert_eq!((r.start, r.value), (0, 0));
        assert_eq!(decode(b"", 0).start, 0);
    }
}
