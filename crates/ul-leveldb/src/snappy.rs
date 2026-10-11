//! Snappy (o formato de bloco, sem moldura): o `Snappy_Compress` e o `Snappy_Uncompress` que o leveldb do Chromium
//! usa nos blocos das tabelas. O compressor segue o `CompressFragment` do snappy (tabela de hash de 16 bits sobre
//! fragmentos de 64 KiB, passo de busca crescente, `EmitCopy` em pedaços de até 64), de modo que os bytes sejam os
//! mesmos que o snappy do Chromium produz: o tamanho do arquivo `.ldb` aparece num `ls`.

use crate::{Error, Result};

const BLOCK_SIZE: usize = 1 << 16;
const MAX_HASH_TABLE_BITS: u32 = 15;
const INPUT_MARGIN_BYTES: usize = 15;

const LITERAL: u8 = 0;
const COPY_1_BYTE_OFFSET: u8 = 1;
const COPY_2_BYTE_OFFSET: u8 = 2;
const COPY_4_BYTE_OFFSET: u8 = 3;

fn load32(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

/// A função de hash do snappy: os `MAX_HASH_TABLE_BITS` bits altos do produto, mascarados para o tamanho da tabela (nas
/// tabelas menores que a máxima isso NÃO é "os bits mais altos": é o que o snappy do Chromium faz, conferido bloco a bloco
/// contra tabelas do oráculo).
fn hash(value: u32, table_size: usize) -> usize {
    ((value.wrapping_mul(0x1e35_a7bd) >> (32 - MAX_HASH_TABLE_BITS)) as usize) & (table_size - 1)
}

fn put_varint32(out: &mut Vec<u8>, mut value: u32) {
    while value >= 0x80 {
        out.push((value & 0x7f) as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn emit_literal(out: &mut Vec<u8>, literal: &[u8]) {
    let n = literal.len() - 1;
    if n < 60 {
        out.push(LITERAL | ((n as u8) << 2));
    } else {
        let mut bytes = Vec::new();
        let mut rest = n;
        while rest > 0 {
            bytes.push((rest & 0xff) as u8);
            rest >>= 8;
        }
        out.push(LITERAL | ((59 + bytes.len() as u8) << 2));
        out.extend_from_slice(&bytes);
    }
    out.extend_from_slice(literal);
}

fn emit_copy_at_most_64(out: &mut Vec<u8>, offset: usize, len: usize) {
    if len < 12 && offset < 2048 {
        out.push(COPY_1_BYTE_OFFSET | (((len - 4) as u8) << 2) | (((offset >> 8) as u8) << 5));
        out.push((offset & 0xff) as u8);
    } else {
        out.push(COPY_2_BYTE_OFFSET | (((len - 1) as u8) << 2));
        out.extend_from_slice(&(offset as u16).to_le_bytes());
    }
}

fn emit_copy(out: &mut Vec<u8>, offset: usize, mut len: usize) {
    while len >= 68 {
        emit_copy_at_most_64(out, offset, 64);
        len -= 64;
    }
    if len > 64 {
        emit_copy_at_most_64(out, offset, 60);
        len -= 60;
    }
    emit_copy_at_most_64(out, offset, len);
}

/// Quantos bytes de `data[a..]` e `data[b..]` coincidem, sem passar de `end` em `b` (`a < b`).
fn match_length(data: &[u8], a: usize, b: usize, end: usize) -> usize {
    let mut length = 0;
    while b + length < end && data[a + length] == data[b + length] {
        length += 1;
    }
    length
}

/// O `CompressFragment` do snappy sobre um fragmento de até 64 KiB.
fn compress_fragment(input: &[u8], out: &mut Vec<u8>) {
    let mut table_size = 256usize;
    while table_size < (1 << MAX_HASH_TABLE_BITS) && table_size < input.len() {
        table_size <<= 1;
    }
    let hash = |value: u32| hash(value, table_size);
    let mut table = vec![0u16; table_size];
    let end = input.len();
    let mut next_emit = 0usize;
    if end >= INPUT_MARGIN_BYTES {
        let ip_limit = end - INPUT_MARGIN_BYTES;
        let mut ip = 1usize;
        let mut next_hash = hash(load32(input, ip));
        // O `ip` começa em 1: o byte 0 nunca é candidato a casamento (como no snappy, que parte de `ip + 1`).
        'outer: loop {
            let mut skip = 32u32;
            let mut next_ip = ip;
            let mut candidate;
            loop {
                ip = next_ip;
                let current_hash = next_hash;
                // O passo cresce com os bytes percorridos sem casamento: 1 nos primeiros 32, depois 2, e assim por diante.
                let bytes_between_hash_lookups = (skip >> 5) as usize;
                skip += bytes_between_hash_lookups as u32;
                next_ip = ip + bytes_between_hash_lookups;
                if next_ip > ip_limit {
                    break 'outer;
                }
                next_hash = hash(load32(input, next_ip));
                candidate = usize::from(table[current_hash]);
                table[current_hash] = ip as u16;
                if load32(input, ip) == load32(input, candidate) {
                    break;
                }
            }
            emit_literal_if_any(out, &input[next_emit..ip]);
            loop {
                let base = ip;
                let matched = 4 + match_length(input, candidate + 4, ip + 4, end);
                ip += matched;
                emit_copy(out, base - candidate, matched);
                next_emit = ip;
                if ip >= ip_limit {
                    break 'outer;
                }
                let previous_hash = hash(load32(input, ip - 1));
                table[previous_hash] = (ip - 1) as u16;
                let current_hash = hash(load32(input, ip));
                candidate = usize::from(table[current_hash]);
                let candidate_bytes = load32(input, candidate);
                table[current_hash] = ip as u16;
                if load32(input, ip) != candidate_bytes {
                    next_hash = hash(load32(input, ip + 1));
                    ip += 1;
                    break;
                }
            }
        }
    }
    emit_literal_if_any(out, &input[next_emit..]);
}

fn emit_literal_if_any(out: &mut Vec<u8>, literal: &[u8]) {
    if !literal.is_empty() {
        emit_literal(out, literal);
    }
}

/// O texto comprimido de `input` (o comprimento em varint, depois os elementos).
pub fn compress(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() / 2 + 16);
    put_varint32(&mut out, input.len() as u32);
    for fragment in input.chunks(BLOCK_SIZE) {
        compress_fragment(fragment, &mut out);
    }
    out
}

fn corrupt() -> Error {
    Error::Corruption("snappy: dados comprimidos corrompidos".to_owned())
}

/// O tamanho que `data` tem depois de descomprimido, lido do cabeçalho.
pub fn uncompressed_length(data: &[u8]) -> Result<usize> {
    read_length(data).map(|(length, _)| length)
}

fn read_length(data: &[u8]) -> Result<(usize, usize)> {
    let mut result = 0usize;
    for (index, &byte) in data.iter().enumerate().take(5) {
        result |= usize::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Ok((result, index + 1));
        }
    }
    Err(corrupt())
}

pub fn uncompress(data: &[u8]) -> Result<Vec<u8>> {
    let (length, mut at) = read_length(data)?;
    let mut out: Vec<u8> = Vec::with_capacity(length);
    while at < data.len() {
        let tag = data[at];
        at += 1;
        match tag & 3 {
            LITERAL => {
                let mut literal_length = usize::from(tag >> 2);
                if literal_length >= 60 {
                    let extra = literal_length - 59;
                    let bytes = data.get(at..at + extra).ok_or_else(corrupt)?;
                    literal_length = bytes.iter().rev().fold(0usize, |value, &byte| (value << 8) | usize::from(byte));
                    at += extra;
                }
                literal_length += 1;
                out.extend_from_slice(data.get(at..at + literal_length).ok_or_else(corrupt)?);
                at += literal_length;
            }
            kind => {
                let (copy_length, offset) = match kind {
                    COPY_1_BYTE_OFFSET => {
                        let low = *data.get(at).ok_or_else(corrupt)?;
                        at += 1;
                        (4 + usize::from((tag >> 2) & 7), (usize::from(tag >> 5) << 8) | usize::from(low))
                    }
                    COPY_2_BYTE_OFFSET => {
                        let bytes = data.get(at..at + 2).ok_or_else(corrupt)?;
                        at += 2;
                        (1 + usize::from(tag >> 2), usize::from(u16::from_le_bytes([bytes[0], bytes[1]])))
                    }
                    COPY_4_BYTE_OFFSET => {
                        let bytes = data.get(at..at + 4).ok_or_else(corrupt)?;
                        at += 4;
                        (1 + usize::from(tag >> 2), u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
                    }
                    _ => return Err(corrupt()),
                };
                if offset == 0 || offset > out.len() {
                    return Err(corrupt());
                }
                for _ in 0..copy_length {
                    out.push(out[out.len() - offset]);
                }
            }
        }
        if out.len() > length {
            return Err(corrupt());
        }
    }
    if out.len() != length {
        return Err(corrupt());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_assorted_inputs() {
        let mut inputs: Vec<Vec<u8>> = vec![Vec::new(), b"a".to_vec(), b"abcabcabcabcabcabcabcabcabcabc".to_vec(), vec![7u8; 100_000]];
        inputs.push((0..70_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect());
        inputs.push(b"the quick brown fox jumps over the lazy dog ".repeat(2000));
        for input in inputs {
            assert_eq!(uncompress(&compress(&input)).unwrap(), input);
        }
    }

    #[test]
    fn a_run_of_one_byte_compresses_to_a_few_copies() {
        // O exemplo do formato: um literal de 1 byte e cópias de offset 1 de até 64.
        let compressed = compress(&[b'v'; 1000]);
        assert!(compressed.len() < 60, "{}", compressed.len());
    }

    #[test]
    fn corrupt_data_is_an_error() {
        assert!(uncompress(&[5, 0xff]).is_err());
        assert!(uncompress(&[3, 0x00, b'a', 0x01]).is_err());
        assert!(uncompress(&[2, 0x05, 0x01, 0x00]).is_err());
    }
}
