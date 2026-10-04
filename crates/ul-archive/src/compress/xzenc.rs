//! Codificador .xz no formato que o xz 5.8 grava no modo de várias threads (o padrão, `-T0`): cada
//! bloco é comprimido inteiro antes de ser gravado, e o cabeçalho do bloco leva o tamanho comprimido e
//! o descomprimido. Escrito a partir da especificação do formato .xz; os dados LZMA2 (e o pré-filtro
//! BCJ ou delta) são do `lzma-rust2`.

use std::io::{self, Write};

use lzma_rust2::{CheckType, FilterType, Lzma2Options, Lzma2Writer, LzmaOptions};
use sha2::Digest;

/// CRC-64 do .xz (ECMA-182, refletido).
pub struct Crc64 {
    table: [u64; 256],
    value: u64,
}

impl Crc64 {
    pub fn new() -> Crc64 {
        let mut table = [0u64; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut c = i as u64;
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ 0xC96C_5795_D787_0F42 } else { c >> 1 };
            }
            *slot = c;
        }
        Crc64 { table, value: !0 }
    }

    pub fn update(&mut self, data: &[u8]) {
        let mut v = self.value;
        for &b in data {
            v = self.table[((v ^ b as u64) & 0xff) as usize] ^ (v >> 8);
        }
        self.value = v;
    }

    pub fn finish(&self) -> u64 {
        !self.value
    }
}

impl Default for Crc64 {
    fn default() -> Self {
        Crc64::new()
    }
}

/// Valor da verificação de um bloco, no tamanho do tipo.
fn check_bytes(check: CheckType, data: &[u8]) -> Vec<u8> {
    match check {
        CheckType::None => Vec::new(),
        CheckType::Crc32 => crc32fast::hash(data).to_le_bytes().to_vec(),
        CheckType::Crc64 => {
            let mut c = Crc64::new();
            c.update(data);
            c.finish().to_le_bytes().to_vec()
        }
        CheckType::Sha256 => sha2::Sha256::digest(data).to_vec(),
    }
}

fn varint_len(mut v: u64) -> usize {
    let mut n = 1;
    while v >= 0x80 {
        v >>= 7;
        n += 1;
    }
    n
}

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Byte de propriedades do LZMA2 que descreve o menor dicionário que cabe `dict`.
fn lzma2_dict_props(dict: u32) -> u8 {
    for p in 0u8..40 {
        let size = (2u64 | (p as u64 & 1)) << (p / 2 + 11);
        if size >= dict as u64 {
            return p;
        }
    }
    40
}

fn filter_id(t: FilterType) -> u8 {
    match t {
        FilterType::Delta => 0x03,
        FilterType::BcjX86 => 0x04,
        FilterType::BcjPpc => 0x05,
        FilterType::BcjIa64 => 0x06,
        FilterType::BcjArm => 0x07,
        FilterType::BcjArmThumb => 0x08,
        FilterType::BcjSparc => 0x09,
        FilterType::BcjArm64 => 0x0a,
        FilterType::BcjRiscv => 0x0b,
        _ => 0x04,
    }
}

/// Codificador em blocos com tamanhos no cabeçalho.
pub struct XzBlockEncoder<W: Write> {
    out: W,
    check: CheckType,
    lzma: LzmaOptions,
    filter: Option<(FilterType, u32)>,
    block_size: usize,
    pending: Vec<u8>,
    records: Vec<(u64, u64)>,
    /// Bytes gravados até agora.
    pub written: u64,
    started: bool,
}

impl<W: Write> XzBlockEncoder<W> {
    /// `block_size` `None` usa o padrão do xz: três vezes o dicionário, no mínimo 1 MiB.
    pub fn new(out: W, lzma: LzmaOptions, check: CheckType, filter: Option<(FilterType, u32)>, block_size: Option<u64>) -> Self {
        let default = (3 * lzma.dict_size as u64).max(1 << 20);
        let block_size = block_size.unwrap_or(default).clamp(1, 1 << 40) as usize;
        XzBlockEncoder { out, check, lzma, filter, block_size, pending: Vec::new(), records: Vec::new(), written: 0, started: false }
    }

    fn emit(&mut self, data: &[u8]) -> io::Result<()> {
        self.out.write_all(data)?;
        self.written += data.len() as u64;
        Ok(())
    }

    fn stream_flags(&self) -> [u8; 2] {
        [0, self.check as u8]
    }

    fn start(&mut self) -> io::Result<()> {
        if self.started {
            return Ok(());
        }
        self.started = true;
        let mut h = vec![0xfd, b'7', b'z', b'X', b'Z', 0];
        let flags = self.stream_flags();
        h.extend_from_slice(&flags);
        h.extend_from_slice(&crc32fast::hash(&flags).to_le_bytes());
        self.emit(&h)
    }

    fn compress_block(&self, data: &[u8]) -> io::Result<Vec<u8>> {
        let opts = Lzma2Options { lzma_options: self.lzma.clone(), chunk_size: None };
        let mut w = Lzma2Writer::new(Vec::new(), opts);
        match self.filter {
            None => {
                w.write_all(data)?;
                w.finish()
            }
            Some((FilterType::Delta, dist)) => {
                let mut d = lzma_rust2::filter::delta::DeltaWriter::new(w, dist as usize);
                d.write_all(data)?;
                d.into_inner().finish()
            }
            Some((kind, start)) => {
                let start = start as usize;
                let mut b = match kind {
                    FilterType::BcjPpc => lzma_rust2::filter::bcj::BcjWriter::new_ppc(w, start),
                    FilterType::BcjIa64 => lzma_rust2::filter::bcj::BcjWriter::new_ia64(w, start),
                    FilterType::BcjArm => lzma_rust2::filter::bcj::BcjWriter::new_arm(w, start),
                    FilterType::BcjArmThumb => lzma_rust2::filter::bcj::BcjWriter::new_arm_thumb(w, start),
                    FilterType::BcjSparc => lzma_rust2::filter::bcj::BcjWriter::new_sparc(w, start),
                    FilterType::BcjArm64 => lzma_rust2::filter::bcj::BcjWriter::new_arm64(w, start),
                    FilterType::BcjRiscv => lzma_rust2::filter::bcj::BcjWriter::new_riscv(w, start),
                    _ => lzma_rust2::filter::bcj::BcjWriter::new_x86(w, start),
                };
                b.write_all(data)?;
                b.finish()?.finish()
            }
        }
    }

    fn flush_block(&mut self) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        sysabi::sys::checkpoint();
        let data = std::mem::take(&mut self.pending);
        let comp = self.compress_block(&data)?;
        // Cabeçalho do bloco: tamanho, flags (filtros e os dois tamanhos), tamanhos, filtros,
        // enchimento até múltiplo de 4 e CRC32.
        let nfilters = 1 + self.filter.is_some() as u8;
        let mut h = vec![0u8, (nfilters - 1) | 0x40 | 0x80];
        put_varint(&mut h, comp.len() as u64);
        put_varint(&mut h, data.len() as u64);
        if let Some((kind, prop)) = self.filter {
            h.push(filter_id(kind));
            if kind == FilterType::Delta {
                h.push(1);
                h.push((prop.clamp(1, 256) - 1) as u8);
            } else if prop != 0 {
                h.push(4);
                h.extend_from_slice(&prop.to_le_bytes());
            } else {
                h.push(0);
            }
        }
        h.push(0x21);
        h.push(1);
        h.push(lzma2_dict_props(self.lzma.dict_size));
        // Como o xz no modo de várias threads, o tamanho do cabeçalho é reservado antes da compressão,
        // com os campos de tamanho no maior valor possível pro bloco (o tamanho do bloco e o limite de
        // saída dele); os campos reais, menores, ficam seguidos de enchimento.
        let bound = self.block_size as u64 + (self.block_size as u64 / 65536 + 1) * 3 + 64;
        let reserved = 2 + varint_len(bound) + varint_len(self.block_size as u64) + (h.len() - 2 - varint_len(comp.len() as u64) - varint_len(data.len() as u64));
        let total = (reserved + 4).div_ceil(4) * 4;
        while h.len() + 4 < total {
            h.push(0);
        }
        h[0] = (total / 4 - 1) as u8;
        let crc = crc32fast::hash(&h);
        h.extend_from_slice(&crc.to_le_bytes());
        let check = check_bytes(self.check, &data);
        self.emit(&h)?;
        self.emit(&comp)?;
        let pad = (4 - comp.len() % 4) % 4;
        self.emit(&[0u8; 3][..pad])?;
        self.emit(&check)?;
        self.records.push(((h.len() + comp.len() + check.len()) as u64, data.len() as u64));
        Ok(())
    }

    /// Fecha o fluxo: último bloco, índice e rodapé.
    pub fn finish(mut self) -> io::Result<W> {
        self.start()?;
        self.flush_block()?;
        let mut idx = vec![0u8];
        put_varint(&mut idx, self.records.len() as u64);
        for (unpadded, uncomp) in &self.records {
            put_varint(&mut idx, *unpadded);
            put_varint(&mut idx, *uncomp);
        }
        while idx.len() % 4 != 0 {
            idx.push(0);
        }
        let crc = crc32fast::hash(&idx);
        idx.extend_from_slice(&crc.to_le_bytes());
        self.emit(&idx)?;
        let backward = (idx.len() / 4 - 1) as u32;
        let mut f = backward.to_le_bytes().to_vec();
        f.extend_from_slice(&self.stream_flags());
        let crc = crc32fast::hash(&f);
        let mut footer = crc.to_le_bytes().to_vec();
        footer.extend_from_slice(&f);
        footer.extend_from_slice(b"YZ");
        self.emit(&footer)?;
        Ok(self.out)
    }
}

impl<W: Write> Write for XzBlockEncoder<W> {
    fn write(&mut self, mut data: &[u8]) -> io::Result<usize> {
        self.start()?;
        let total = data.len();
        while !data.is_empty() {
            let room = self.block_size - self.pending.len();
            let n = room.min(data.len());
            if self.pending.try_reserve(n).is_err() {
                return Err(io::Error::other("out of memory"));
            }
            self.pending.extend_from_slice(&data[..n]);
            data = &data[n..];
            if self.pending.len() == self.block_size {
                self.flush_block()?;
            }
        }
        Ok(total)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc64_reference() {
        let mut c = Crc64::new();
        c.update(b"123456789");
        assert_eq!(c.finish(), 0x995D_C9BB_DF19_39FA);
    }

    #[test]
    fn dict_props() {
        assert_eq!(lzma2_dict_props(8 << 20), 22);
        assert_eq!(lzma2_dict_props(1 << 20), 16);
        assert_eq!(lzma2_dict_props(3 << 20), 19);
    }

    #[test]
    fn roundtrip_with_lzma_rust2_reader() {
        use std::io::Read;
        let data: Vec<u8> = (0..200_000u32).flat_map(|i| (i % 251).to_le_bytes()).collect();
        let mut e = XzBlockEncoder::new(Vec::new(), LzmaOptions::with_preset(1), CheckType::Crc64, None, Some(100_000));
        e.write_all(&data).unwrap();
        let enc = e.finish().unwrap();
        let mut out = Vec::new();
        lzma_rust2::XzReader::new(&enc[..], true).read_to_end(&mut out).unwrap();
        assert_eq!(out, data);
        let info = super::super::xzlist::parse(&enc).unwrap();
        assert_eq!(info.block_count(), 8);
    }
}
