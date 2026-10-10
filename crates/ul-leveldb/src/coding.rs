//! Os varints e as strings prefixadas por tamanho do leveldb (`util/coding.cc`).

use crate::{Error, Result};

pub fn put_varint32(out: &mut Vec<u8>, value: u32) {
    put_varint64(out, u64::from(value));
}

pub fn put_varint64(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value & 0x7f) as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Quantos bytes o varint de `value` ocupa (`VarintLength`).
pub fn varint_length(mut value: u64) -> usize {
    let mut length = 1;
    while value >= 0x80 {
        value >>= 7;
        length += 1;
    }
    length
}

pub fn put_length_prefixed(out: &mut Vec<u8>, slice: &[u8]) {
    put_varint32(out, slice.len() as u32);
    out.extend_from_slice(slice);
}

pub fn put_fixed32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub fn put_fixed64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Um cursor sobre bytes, para decodificar na ordem em que o leveldb codifica.
pub struct Input<'a> {
    data: &'a [u8],
}

impl<'a> Input<'a> {
    pub fn new(data: &'a [u8]) -> Input<'a> {
        Input { data }
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn remaining(&self) -> &'a [u8] {
        self.data
    }

    pub fn varint64(&mut self) -> Result<u64> {
        let mut result = 0u64;
        for (index, &byte) in self.data.iter().enumerate().take(10) {
            result |= u64::from(byte & 0x7f) << (7 * index);
            if byte & 0x80 == 0 {
                self.data = &self.data[index + 1..];
                return Ok(result);
            }
        }
        Err(Error::Corruption("varint inválido".to_owned()))
    }

    pub fn varint32(&mut self) -> Result<u32> {
        let value = self.varint64()?;
        u32::try_from(value).map_err(|_| Error::Corruption("varint32 grande demais".to_owned()))
    }

    pub fn length_prefixed(&mut self) -> Result<&'a [u8]> {
        let length = self.varint32()? as usize;
        self.take(length)
    }

    pub fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        if length > self.data.len() {
            return Err(Error::Corruption("fim inesperado dos dados".to_owned()));
        }
        let (head, tail) = self.data.split_at(length);
        self.data = tail;
        Ok(head)
    }

    pub fn fixed32(&mut self) -> Result<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn fixed64(&mut self) -> Result<u64> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_round_trip() {
        for value in [0u64, 1, 127, 128, 300, 16_383, 16_384, u64::from(u32::MAX), u64::MAX] {
            let mut out = Vec::new();
            put_varint64(&mut out, value);
            assert_eq!(Input::new(&out).varint64().unwrap(), value);
        }
        let mut out = Vec::new();
        put_varint32(&mut out, 300);
        assert_eq!(out, [0xac, 0x02]);
    }

    #[test]
    fn length_prefixed_and_fixed_round_trip() {
        let mut out = Vec::new();
        put_length_prefixed(&mut out, b"leveldb");
        put_fixed32(&mut out, 7);
        put_fixed64(&mut out, 9);
        let mut input = Input::new(&out);
        assert_eq!(input.length_prefixed().unwrap(), b"leveldb");
        assert_eq!(input.fixed32().unwrap(), 7);
        assert_eq!(input.fixed64().unwrap(), 9);
        assert!(input.is_empty());
        assert!(input.take(1).is_err());
    }

    #[test]
    fn truncated_varint_is_corruption() {
        assert!(Input::new(&[0x80]).varint64().is_err());
        assert!(Input::new(&[0xff; 11]).varint64().is_err());
    }
}
