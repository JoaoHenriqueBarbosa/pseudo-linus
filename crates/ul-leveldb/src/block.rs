//! Os blocos de uma tabela (`table/block_builder.cc`, `table/block.cc`): entradas com a chave comprimida pelo prefixo
//! da anterior, pontos de reinício a cada `restart_interval` chaves e, no fim, a lista dos reinícios.

use crate::coding::{put_fixed32, put_varint32, Input};
use crate::{Error, Result};

#[derive(Debug)]
pub struct BlockBuilder {
    restart_interval: usize,
    buffer: Vec<u8>,
    restarts: Vec<u32>,
    counter: usize,
    last_key: Vec<u8>,
}

impl BlockBuilder {
    pub fn new(restart_interval: usize) -> BlockBuilder {
        BlockBuilder { restart_interval, buffer: Vec::new(), restarts: vec![0], counter: 0, last_key: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// O tamanho que o bloco teria agora (`CurrentSizeEstimate`).
    pub fn size_estimate(&self) -> usize {
        self.buffer.len() + self.restarts.len() * 4 + 4
    }

    /// As chaves chegam em ordem crescente.
    pub fn add(&mut self, key: &[u8], value: &[u8]) {
        let mut shared = 0;
        if self.counter < self.restart_interval {
            let min_length = self.last_key.len().min(key.len());
            while shared < min_length && self.last_key[shared] == key[shared] {
                shared += 1;
            }
        } else {
            self.restarts.push(self.buffer.len() as u32);
            self.counter = 0;
        }
        put_varint32(&mut self.buffer, shared as u32);
        put_varint32(&mut self.buffer, (key.len() - shared) as u32);
        put_varint32(&mut self.buffer, value.len() as u32);
        self.buffer.extend_from_slice(&key[shared..]);
        self.buffer.extend_from_slice(value);
        self.last_key.clear();
        self.last_key.extend_from_slice(key);
        self.counter += 1;
    }

    /// O conteúdo do bloco, com a lista dos reinícios.
    pub fn finish(&mut self) -> Vec<u8> {
        let mut out = std::mem::take(&mut self.buffer);
        for &restart in &self.restarts {
            put_fixed32(&mut out, restart);
        }
        put_fixed32(&mut out, self.restarts.len() as u32);
        self.restarts = vec![0];
        self.counter = 0;
        self.last_key.clear();
        out
    }
}

/// Decodifica todas as entradas de um bloco, na ordem.
pub fn read_block(contents: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let corrupt = || Error::Corruption("bad block contents".to_owned());
    if contents.len() < 4 {
        return Err(corrupt());
    }
    let restart_count = u32::from_le_bytes(contents[contents.len() - 4..].try_into().map_err(|_| corrupt())?) as usize;
    let restarts_size = restart_count.checked_mul(4).and_then(|size| size.checked_add(4)).ok_or_else(corrupt)?;
    if restarts_size > contents.len() {
        return Err(corrupt());
    }
    let mut input = Input::new(&contents[..contents.len() - restarts_size]);
    let mut entries = Vec::new();
    let mut key: Vec<u8> = Vec::new();
    while !input.is_empty() {
        let shared = input.varint32()? as usize;
        let non_shared = input.varint32()? as usize;
        let value_length = input.varint32()? as usize;
        if shared > key.len() {
            return Err(corrupt());
        }
        key.truncate(shared);
        key.extend_from_slice(input.take(non_shared)?);
        let value = input.take(value_length)?.to_vec();
        entries.push((key.clone(), value));
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_round_trip_with_prefix_compression_and_restarts() {
        let mut builder = BlockBuilder::new(16);
        let keys: Vec<Vec<u8>> = (0..40).map(|i| format!("prefix/{i:04}").into_bytes()).collect();
        for key in &keys {
            builder.add(key, b"value");
        }
        let block = builder.finish();
        let entries = read_block(&block).unwrap();
        assert_eq!(entries.len(), 40);
        assert_eq!(entries[17].0, keys[17]);
        // Três reinícios (0, 16, 32) mais a contagem.
        assert_eq!(&block[block.len() - 4..], &[3, 0, 0, 0]);
    }

    #[test]
    fn an_empty_block_is_just_one_restart() {
        let block = BlockBuilder::new(16).finish();
        assert_eq!(block, [0, 0, 0, 0, 1, 0, 0, 0]);
        assert!(read_block(&block).unwrap().is_empty());
    }

    #[test]
    fn bad_blocks_are_corruption() {
        assert!(read_block(&[1, 2]).is_err());
        assert!(read_block(&[9, 0, 0, 0]).is_err());
    }
}
