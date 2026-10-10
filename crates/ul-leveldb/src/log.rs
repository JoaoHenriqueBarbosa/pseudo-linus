//! O formato de log do leveldb (`db/log_format.h`, `log_writer.cc`, `log_reader.cc`): blocos de 32 KiB, cada registro
//! com um cabeçalho de 7 bytes (CRC-32C mascarado de tipo e dados, tamanho, tipo) e fragmentado entre blocos quando
//! não cabe. Serve ao `<n>.log` (de `WriteBatch`) e ao `MANIFEST` (de `VersionEdit`).

use ul_common::codec::{crc32c_mask, crc32c_unmask, crc32c_update};

use crate::{Error, Result};

pub const BLOCK_SIZE: usize = 32768;
pub const HEADER_SIZE: usize = 7;

const FULL: u8 = 1;
const FIRST: u8 = 2;
const MIDDLE: u8 = 3;
const LAST: u8 = 4;

/// O CRC guardado no cabeçalho: do byte de tipo e dos dados, mascarado.
fn record_crc(record_type: u8, data: &[u8]) -> u32 {
    crc32c_mask(crc32c_update(crc32c_update(0, &[record_type]), data))
}

/// Quem produz os bytes de um log. Não escreve em arquivo: devolve o que anexar, para quem chama gravar onde e como
/// quiser (o `fsync` fica com ele).
#[derive(Debug)]
pub struct Writer {
    block_offset: usize,
}

impl Writer {
    /// Um log novo, vazio.
    pub fn new() -> Writer {
        Writer { block_offset: 0 }
    }

    /// Continua um log que já tem `length` bytes (a posição no bloco vem do tamanho).
    pub fn resuming(length: u64) -> Writer {
        Writer { block_offset: (length % BLOCK_SIZE as u64) as usize }
    }

    /// Os bytes que o registro `payload` acrescenta ao arquivo.
    pub fn add_record(&mut self, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(payload.len() + HEADER_SIZE);
        let mut rest = payload;
        let mut begin = true;
        loop {
            let leftover = BLOCK_SIZE - self.block_offset;
            if leftover < HEADER_SIZE {
                out.extend(std::iter::repeat_n(0u8, leftover));
                self.block_offset = 0;
            }
            let available = BLOCK_SIZE - self.block_offset - HEADER_SIZE;
            let length = rest.len().min(available);
            let end = length == rest.len();
            let record_type = match (begin, end) {
                (true, true) => FULL,
                (true, false) => FIRST,
                (false, true) => LAST,
                (false, false) => MIDDLE,
            };
            let fragment = &rest[..length];
            out.extend_from_slice(&record_crc(record_type, fragment).to_le_bytes());
            out.extend_from_slice(&(length as u16).to_le_bytes());
            out.push(record_type);
            out.extend_from_slice(fragment);
            self.block_offset += HEADER_SIZE + length;
            rest = &rest[length..];
            begin = false;
            if end {
                return out;
            }
        }
    }
}

impl Default for Writer {
    fn default() -> Writer {
        Writer::new()
    }
}

/// Os registros de um log inteiro, na ordem. Um cabeçalho ou registro cortado no fim do arquivo termina a leitura sem
/// erro (o leveldb trata a escrita interrompida como fim do log); CRC errado, tipo desconhecido ou fragmentos fora de
/// ordem são corrupção.
pub fn read_records(data: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut records = Vec::new();
    let mut assembling: Option<Vec<u8>> = None;
    let mut at = 0;
    while at < data.len() {
        let block_left = BLOCK_SIZE - at % BLOCK_SIZE;
        if block_left < HEADER_SIZE {
            at += block_left;
            continue;
        }
        let Some(header) = data.get(at..at + HEADER_SIZE) else { break };
        let stored_crc = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        let length = usize::from(u16::from_le_bytes([header[4], header[5]]));
        let record_type = header[6];
        if record_type == 0 && length == 0 {
            // Bloco zerado (pré-alocado): o resto do bloco não tem registro.
            at += block_left;
            continue;
        }
        if HEADER_SIZE + length > block_left {
            return Err(Error::Corruption(format!("registro de {length} bytes passa do bloco em {at}")));
        }
        let Some(fragment) = data.get(at + HEADER_SIZE..at + HEADER_SIZE + length) else { break };
        if crc32c_unmask(stored_crc) != crc32c_update(crc32c_update(0, &[record_type]), fragment) {
            return Err(Error::Corruption(format!("checksum errado no registro em {at}")));
        }
        match (record_type, assembling.as_mut()) {
            (FULL, None) => records.push(fragment.to_vec()),
            (FIRST, None) => assembling = Some(fragment.to_vec()),
            (MIDDLE, Some(buffer)) => buffer.extend_from_slice(fragment),
            (LAST, Some(buffer)) => {
                buffer.extend_from_slice(fragment);
                records.push(assembling.take().unwrap_or_default());
            }
            _ => return Err(Error::Corruption(format!("fragmentos fora de ordem em {at} (tipo {record_type})"))),
        }
        at += HEADER_SIZE + length;
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_record_is_one_full_fragment() {
        let mut writer = Writer::new();
        let bytes = writer.add_record(b"hello");
        assert_eq!(bytes.len(), HEADER_SIZE + 5);
        assert_eq!(bytes[6], FULL);
        assert_eq!(read_records(&bytes).unwrap(), vec![b"hello".to_vec()]);
    }

    #[test]
    fn an_empty_record_is_written_and_read() {
        let mut writer = Writer::new();
        let bytes = writer.add_record(b"");
        assert_eq!(bytes.len(), HEADER_SIZE);
        assert_eq!(read_records(&bytes).unwrap(), vec![Vec::<u8>::new()]);
    }

    #[test]
    fn large_records_are_fragmented_across_blocks() {
        let big: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let mut writer = Writer::new();
        let mut file = writer.add_record(b"first");
        file.extend(writer.add_record(&big));
        file.extend(writer.add_record(b"last"));
        // O registro grande começa depois dos 12 bytes do primeiro e atravessa quatro blocos.
        assert_eq!(file[12 + 6], FIRST);
        assert_eq!(file[BLOCK_SIZE + 6], MIDDLE);
        let records = read_records(&file).unwrap();
        assert_eq!(records, vec![b"first".to_vec(), big, b"last".to_vec()]);
    }

    #[test]
    fn the_block_tail_shorter_than_a_header_is_zero_padded() {
        let mut writer = Writer::new();
        // Deixa 6 bytes no fim do bloco: menos que um cabeçalho.
        let first = writer.add_record(&vec![7u8; BLOCK_SIZE - HEADER_SIZE - 6]);
        assert_eq!(first.len(), BLOCK_SIZE - 6);
        let second = writer.add_record(b"x");
        assert_eq!(&second[..6], &[0u8; 6]);
        let mut file = first;
        file.extend(second);
        assert_eq!(file.len(), BLOCK_SIZE + HEADER_SIZE + 1);
        assert_eq!(read_records(&file).unwrap().len(), 2);
    }

    #[test]
    fn resuming_continues_the_block_position() {
        let mut first = Writer::new();
        let mut file = first.add_record(&vec![1u8; 40_000]);
        let mut resumed = Writer::resuming(file.len() as u64);
        file.extend(resumed.add_record(b"more"));
        assert_eq!(read_records(&file).unwrap().len(), 2);
    }

    #[test]
    fn a_flipped_byte_is_corruption_and_a_cut_tail_is_the_end() {
        let mut writer = Writer::new();
        let mut file = writer.add_record(b"one");
        file.extend(writer.add_record(b"two"));
        let mut broken = file.clone();
        broken[HEADER_SIZE] ^= 1;
        assert!(read_records(&broken).is_err());
        let cut = &file[..file.len() - 2];
        assert_eq!(read_records(cut).unwrap(), vec![b"one".to_vec()]);
    }
}
