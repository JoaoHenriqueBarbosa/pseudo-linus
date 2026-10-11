//! As tabelas ordenadas (`.ldb`): `table/table_builder.cc` e `table/table.cc`. Blocos de dados de cerca de 4 KiB
//! (comprimidos com snappy quando isso poupa mais de um oitavo), o bloco de metaíndice, o bloco de índice (uma chave
//! separadora por bloco de dados) e o rodapé de 48 bytes com os dois ponteiros e o número mágico.

use std::cmp::Ordering;

use ul_common::codec::{crc32c_mask, crc32c_unmask, crc32c_update};

use crate::block::{read_block, BlockBuilder};
use crate::coding::{put_fixed32, put_varint64, Input};
use crate::dbformat::{compare_internal, internal_short_successor, internal_shortest_separator};
use crate::{snappy, Error, Result};

const TABLE_MAGIC: u64 = 0xdb47_7524_8b80_fb57;
const FOOTER_LENGTH: usize = 48;
const BLOCK_TRAILER_SIZE: usize = 5;
const NO_COMPRESSION: u8 = 0;
const SNAPPY_COMPRESSION: u8 = 1;

/// As opções que mudam os bytes de uma tabela.
#[derive(Debug, Clone, Copy)]
pub struct TableOptions {
    pub block_size: usize,
    pub block_restart_interval: usize,
    pub snappy: bool,
}

impl Default for TableOptions {
    fn default() -> TableOptions {
        TableOptions { block_size: 4096, block_restart_interval: 16, snappy: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BlockHandle {
    offset: u64,
    size: u64,
}

impl BlockHandle {
    fn encode(&self, out: &mut Vec<u8>) {
        put_varint64(out, self.offset);
        put_varint64(out, self.size);
    }

    fn decode(input: &mut Input) -> Result<BlockHandle> {
        Ok(BlockHandle { offset: input.varint64()?, size: input.varint64()? })
    }
}

#[derive(Debug)]
pub struct TableBuilder {
    options: TableOptions,
    file: Vec<u8>,
    data_block: BlockBuilder,
    index_block: BlockBuilder,
    last_key: Vec<u8>,
    entries: u64,
    pending_index_entry: bool,
    pending_handle: BlockHandle,
}

impl TableBuilder {
    pub fn new(options: TableOptions) -> TableBuilder {
        TableBuilder {
            options,
            file: Vec::new(),
            data_block: BlockBuilder::new(options.block_restart_interval),
            index_block: BlockBuilder::new(1),
            last_key: Vec::new(),
            entries: 0,
            pending_index_entry: false,
            pending_handle: BlockHandle { offset: 0, size: 0 },
        }
    }

    pub fn num_entries(&self) -> u64 {
        self.entries
    }

    /// O tamanho do arquivo até agora.
    pub fn file_size(&self) -> u64 {
        self.file.len() as u64
    }

    /// As chaves (internas) chegam em ordem crescente.
    pub fn add(&mut self, key: &[u8], value: &[u8]) {
        if self.pending_index_entry {
            internal_shortest_separator(&mut self.last_key, key);
            let mut handle = Vec::new();
            self.pending_handle.encode(&mut handle);
            self.index_block.add(&self.last_key, &handle);
            self.pending_index_entry = false;
        }
        self.last_key.clear();
        self.last_key.extend_from_slice(key);
        self.entries += 1;
        self.data_block.add(key, value);
        if self.data_block.size_estimate() >= self.options.block_size {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.data_block.is_empty() {
            return;
        }
        let raw = self.data_block.finish();
        self.pending_handle = self.write_block(&raw);
        self.pending_index_entry = true;
    }

    fn write_block(&mut self, raw: &[u8]) -> BlockHandle {
        if self.options.snappy {
            let compressed = snappy::compress(raw);
            if compressed.len() < raw.len() - raw.len() / 8 {
                return self.write_raw_block(&compressed, SNAPPY_COMPRESSION);
            }
        }
        self.write_raw_block(raw, NO_COMPRESSION)
    }

    fn write_raw_block(&mut self, contents: &[u8], block_type: u8) -> BlockHandle {
        let handle = BlockHandle { offset: self.file.len() as u64, size: contents.len() as u64 };
        self.file.extend_from_slice(contents);
        let crc = crc32c_mask(crc32c_update(crc32c_update(0, contents), &[block_type]));
        self.file.push(block_type);
        put_fixed32(&mut self.file, crc);
        handle
    }

    /// Fecha a tabela e devolve o arquivo inteiro.
    pub fn finish(mut self) -> Vec<u8> {
        self.flush();
        let mut metaindex = BlockBuilder::new(self.options.block_restart_interval);
        let metaindex_raw = metaindex.finish();
        let metaindex_handle = self.write_block(&metaindex_raw);
        if self.pending_index_entry {
            internal_short_successor(&mut self.last_key);
            let mut handle = Vec::new();
            self.pending_handle.encode(&mut handle);
            self.index_block.add(&self.last_key, &handle);
            self.pending_index_entry = false;
        }
        let index_raw = self.index_block.finish();
        let index_handle = self.write_block(&index_raw);
        let mut footer = Vec::with_capacity(FOOTER_LENGTH);
        metaindex_handle.encode(&mut footer);
        index_handle.encode(&mut footer);
        footer.resize(FOOTER_LENGTH - 8, 0);
        footer.extend_from_slice(&(TABLE_MAGIC as u32).to_le_bytes());
        footer.extend_from_slice(&((TABLE_MAGIC >> 32) as u32).to_le_bytes());
        self.file.extend_from_slice(&footer);
        self.file
    }
}

/// Uma tabela aberta para leitura: o arquivo inteiro e o índice dos blocos.
#[derive(Debug)]
pub struct Table {
    data: Vec<u8>,
    /// A chave separadora de cada bloco de dados e o ponteiro dele.
    index: Vec<(Vec<u8>, BlockHandle)>,
}

fn corruption(message: &str) -> Error {
    Error::Corruption(message.to_owned())
}

fn read_block_contents(data: &[u8], handle: BlockHandle) -> Result<Vec<u8>> {
    let start = usize::try_from(handle.offset).map_err(|_| corruption("bad block handle"))?;
    let size = usize::try_from(handle.size).map_err(|_| corruption("bad block handle"))?;
    let end = start.checked_add(size).ok_or_else(|| corruption("bad block handle"))?;
    let block = data.get(start..end).ok_or_else(|| corruption("truncated block read"))?;
    let trailer = data.get(end..end + BLOCK_TRAILER_SIZE).ok_or_else(|| corruption("truncated block read"))?;
    let stored = crc32c_unmask(u32::from_le_bytes([trailer[1], trailer[2], trailer[3], trailer[4]]));
    if stored != crc32c_update(crc32c_update(0, block), &trailer[..1]) {
        return Err(corruption("block checksum mismatch"));
    }
    match trailer[0] {
        NO_COMPRESSION => Ok(block.to_vec()),
        SNAPPY_COMPRESSION => snappy::uncompress(block),
        _ => Err(corruption("bad block type")),
    }
}

impl Table {
    pub fn open(data: Vec<u8>) -> Result<Table> {
        if data.len() < FOOTER_LENGTH {
            return Err(corruption("file is too short to be an sstable"));
        }
        let footer = &data[data.len() - FOOTER_LENGTH..];
        let magic = u64::from(u32::from_le_bytes([footer[40], footer[41], footer[42], footer[43]]))
            | (u64::from(u32::from_le_bytes([footer[44], footer[45], footer[46], footer[47]])) << 32);
        if magic != TABLE_MAGIC {
            return Err(corruption("not an sstable (bad magic number)"));
        }
        let mut input = Input::new(&footer[..40]);
        let _metaindex = BlockHandle::decode(&mut input)?;
        let index_handle = BlockHandle::decode(&mut input)?;
        let index_entries = read_block(&read_block_contents(&data, index_handle)?)?;
        let mut index = Vec::with_capacity(index_entries.len());
        for (key, value) in index_entries {
            index.push((key, BlockHandle::decode(&mut Input::new(&value))?));
        }
        Ok(Table { data, index })
    }

    fn data_block(&self, handle: BlockHandle) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        read_block(&read_block_contents(&self.data, handle)?)
    }

    /// A primeira entrada com chave interna `>= target`, ou `None`.
    pub fn seek(&self, target: &[u8]) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
        let first = self.index.partition_point(|(key, _)| compare_internal(key, target) == Ordering::Less);
        for (_, handle) in &self.index[first..] {
            for (key, value) in self.data_block(*handle)? {
                if compare_internal(&key, target) != Ordering::Less {
                    return Ok(Some((key, value)));
                }
            }
        }
        Ok(None)
    }

    /// Todas as entradas, em ordem.
    pub fn entries(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut all = Vec::new();
        for (_, handle) in &self.index {
            all.extend(self.data_block(*handle)?);
        }
        Ok(all)
    }

    /// Os blocos de dados como estão no arquivo: o conteúdo gravado (comprimido ou não) e o byte de tipo. Serve à
    /// conferência do compressor contra tabelas escritas por outro leveldb.
    pub fn stored_data_blocks(&self) -> Vec<(Vec<u8>, u8)> {
        self.index
            .iter()
            .filter_map(|(_, handle)| {
                let start = usize::try_from(handle.offset).ok()?;
                let end = start.checked_add(usize::try_from(handle.size).ok()?)?;
                Some((self.data.get(start..end)?.to_vec(), *self.data.get(end)?))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dbformat::{internal_key, ValueType};

    fn build(count: usize) -> (Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>) {
        let mut builder = TableBuilder::new(TableOptions::default());
        let mut expected = Vec::new();
        for i in 0..count {
            let key = internal_key(format!("key{i:06}").as_bytes(), 100 + i as u64, ValueType::Value);
            let value = format!("value-{i}-{}", "x".repeat(i % 50)).into_bytes();
            builder.add(&key, &value);
            expected.push((key, value));
        }
        (builder.finish(), expected)
    }

    #[test]
    fn a_table_reads_back_what_it_was_given() {
        let (file, expected) = build(2000);
        let table = Table::open(file).unwrap();
        assert_eq!(table.entries().unwrap(), expected);
        assert_eq!(table.seek(&expected[1234].0).unwrap(), Some(expected[1234].clone()));
        let between = internal_key(b"key000500x", 1, ValueType::Value);
        assert_eq!(table.seek(&between).unwrap(), Some(expected[501].clone()));
        assert_eq!(table.seek(&internal_key(b"zzz", 1, ValueType::Value)).unwrap(), None);
    }

    #[test]
    fn the_footer_has_the_leveldb_magic_and_a_flipped_byte_is_corruption() {
        let (mut file, _) = build(10);
        assert_eq!(&file[file.len() - 8..], &[0x57, 0xfb, 0x80, 0x8b, 0x24, 0x75, 0x47, 0xdb]);
        file[3] ^= 1;
        let table = Table::open(file).unwrap();
        assert!(table.entries().is_err());
    }

    #[test]
    fn an_empty_table_is_a_footer_and_two_small_blocks() {
        let file = TableBuilder::new(TableOptions::default()).finish();
        assert_eq!(file.len(), 13 + 13 + FOOTER_LENGTH);
        assert!(Table::open(file).unwrap().entries().unwrap().is_empty());
    }
}
