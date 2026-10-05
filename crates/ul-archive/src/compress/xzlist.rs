//! Leitura da estrutura de um arquivo .xz pro `xz -l`: fluxos, blocos, tamanhos, verificação e
//! enchimento, lidos do fim pro começo pelo rodapé e pelo índice de cada fluxo, como a especificação
//! do formato .xz descreve (cabeçalho e rodapé de 12 bytes, índice com registros de tamanho sem
//! enchimento e tamanho descomprimido, enchimento de fluxo em múltiplos de 4 bytes zero).

/// Um bloco, como o índice descreve.
#[derive(Clone, Debug)]
pub struct Block {
    /// Número do bloco dentro do fluxo e no arquivo (a partir de 1).
    pub number_in_stream: u64,
    pub number_in_file: u64,
    pub comp_offset: u64,
    pub uncomp_offset: u64,
    /// Tamanho total (cabeçalho, dados, enchimento e verificação).
    pub total_size: u64,
    pub unpadded_size: u64,
    pub uncomp_size: u64,
}

/// Um fluxo.
#[derive(Clone, Debug)]
pub struct Stream {
    pub number: u64,
    pub check: u8,
    pub comp_offset: u64,
    pub uncomp_offset: u64,
    pub comp_size: u64,
    pub uncomp_size: u64,
    /// Enchimento depois deste fluxo.
    pub padding: u64,
    pub blocks: Vec<Block>,
}

/// O arquivo inteiro.
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub streams: Vec<Stream>,
    pub file_size: u64,
}

impl Info {
    pub fn block_count(&self) -> u64 {
        self.streams.iter().map(|s| s.blocks.len() as u64).sum()
    }

    pub fn uncomp_size(&self) -> u64 {
        self.streams.iter().map(|s| s.uncomp_size).sum()
    }

    pub fn padding(&self) -> u64 {
        self.streams.iter().map(|s| s.padding).sum()
    }

    /// Conjunto das verificações usadas (bit = id da verificação).
    pub fn checks(&self) -> u32 {
        self.streams.iter().fold(0, |m, s| m | (1 << s.check))
    }
}

/// Por que o arquivo não pôde ser lido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListError {
    TooSmall,
    NotFormat,
    Corrupt,
}

const HEADER_MAGIC: [u8; 6] = [0xfd, b'7', b'z', b'X', b'Z', 0];

fn read_varint(d: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v: u64 = 0;
    for i in 0..9 {
        let b = *d.get(*pos)?;
        *pos += 1;
        v |= ((b & 0x7f) as u64) << (7 * i);
        if b & 0x80 == 0 {
            if i > 0 && b == 0 {
                return None;
            }
            return Some(v);
        }
    }
    None
}

fn round4(x: u64) -> u64 {
    (x + 3) & !3
}

/// Analisa o arquivo inteiro (`data`).
pub fn parse(data: &[u8]) -> Result<Info, ListError> {
    let len = data.len() as u64;
    if len < 32 {
        return Err(ListError::TooSmall);
    }
    if data[..6] != HEADER_MAGIC {
        return Err(ListError::NotFormat);
    }
    let mut streams: Vec<Stream> = Vec::new();
    let mut end = data.len();
    loop {
        sysabi::sys::checkpoint();
        // Enchimento: zeros em grupos de 4 antes do rodapé.
        let mut padding = 0u64;
        while end >= 4 && data[end - 4..end] == [0, 0, 0, 0] {
            end -= 4;
            padding += 4;
        }
        if end < 12 {
            return Err(ListError::Corrupt);
        }
        let footer = &data[end - 12..end];
        if &footer[10..12] != b"YZ" {
            return Err(ListError::Corrupt);
        }
        let crc = u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]);
        if crc != crc32fast::hash(&footer[4..10]) {
            return Err(ListError::Corrupt);
        }
        let backward = (u32::from_le_bytes([footer[4], footer[5], footer[6], footer[7]]) as u64 + 1) * 4;
        let flags = [footer[8], footer[9]];
        if flags[0] != 0 || flags[1] & 0xf0 != 0 {
            return Err(ListError::Corrupt);
        }
        let check = flags[1] & 0x0f;
        let index_end = end - 12;
        if (index_end as u64) < backward + 12 {
            return Err(ListError::Corrupt);
        }
        let index_start = index_end - backward as usize;
        let index = &data[index_start..index_end];
        if index.first() != Some(&0) {
            return Err(ListError::Corrupt);
        }
        let icrc = u32::from_le_bytes([index[index.len() - 4], index[index.len() - 3], index[index.len() - 2], index[index.len() - 1]]);
        if icrc != crc32fast::hash(&index[..index.len() - 4]) {
            return Err(ListError::Corrupt);
        }
        let mut pos = 1usize;
        let count = read_varint(index, &mut pos).ok_or(ListError::Corrupt)?;
        if count > index.len() as u64 {
            return Err(ListError::Corrupt);
        }
        let mut records = Vec::new();
        for _ in 0..count {
            let unpadded = read_varint(index, &mut pos).ok_or(ListError::Corrupt)?;
            let uncomp = read_varint(index, &mut pos).ok_or(ListError::Corrupt)?;
            if unpadded == 0 {
                return Err(ListError::Corrupt);
            }
            records.push((unpadded, uncomp));
        }
        while !pos.is_multiple_of(4) {
            if index.get(pos) != Some(&0) {
                return Err(ListError::Corrupt);
            }
            pos += 1;
        }
        if pos + 4 != index.len() {
            return Err(ListError::Corrupt);
        }
        let blocks_size: u64 = records.iter().map(|r| round4(r.0)).sum();
        let stream_size = 12 + blocks_size + backward + 12;
        if stream_size > end as u64 {
            return Err(ListError::Corrupt);
        }
        let start = end - stream_size as usize;
        let header = &data[start..start + 12];
        if header[..6] != HEADER_MAGIC {
            return Err(ListError::Corrupt);
        }
        if header[6..8] != flags {
            return Err(ListError::Corrupt);
        }
        let hcrc = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
        if hcrc != crc32fast::hash(&header[6..8]) {
            return Err(ListError::Corrupt);
        }
        let mut blocks = Vec::new();
        let mut off = start as u64 + 12;
        for (i, (unpadded, uncomp)) in records.iter().enumerate() {
            blocks.push(Block {
                number_in_stream: i as u64 + 1,
                number_in_file: 0,
                comp_offset: off,
                uncomp_offset: 0,
                total_size: round4(*unpadded),
                unpadded_size: *unpadded,
                uncomp_size: *uncomp,
            });
            off += round4(*unpadded);
        }
        streams.push(Stream {
            number: 0,
            check,
            comp_offset: start as u64,
            uncomp_offset: 0,
            comp_size: stream_size,
            uncomp_size: records.iter().map(|r| r.1).sum(),
            padding,
            blocks,
        });
        end = start;
        if end == 0 {
            break;
        }
        // Antes deste fluxo só pode haver outro fluxo (com enchimento).
        if end < 12 {
            return Err(ListError::Corrupt);
        }
    }
    streams.reverse();
    let (mut uoff, mut bnum) = (0u64, 0u64);
    for (i, s) in streams.iter_mut().enumerate() {
        s.number = i as u64 + 1;
        s.uncomp_offset = uoff;
        let mut bu = uoff;
        for b in &mut s.blocks {
            bnum += 1;
            b.number_in_file = bnum;
            b.uncomp_offset = bu;
            bu += b.uncomp_size;
        }
        uoff += s.uncomp_size;
    }
    Ok(Info { streams, file_size: len })
}

/// Nome da verificação como o `xz -l` mostra.
pub fn check_name(id: u8) -> String {
    match id {
        0 => "None".into(),
        1 => "CRC32".into(),
        4 => "CRC64".into(),
        10 => "SHA-256".into(),
        n => format!("Unknown-{n}"),
    }
}

/// Lista de verificações separadas por vírgula, na ordem dos ids.
pub fn checks_names(mask: u32) -> String {
    (0..16u8).filter(|i| mask & (1 << i) != 0).map(check_name).collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_we_write() {
        let data = crate::codec::compress(crate::codec::Format::Xz, b"hello world\n", 6, &Default::default()).unwrap();
        let info = parse(&data).unwrap();
        assert_eq!(info.streams.len(), 1);
        assert_eq!(info.uncomp_size(), 12);
        assert_eq!(info.streams[0].check, 4);
        let mut two = data.clone();
        two.extend_from_slice(&[0; 8]);
        two.extend_from_slice(&data);
        let info = parse(&two).unwrap();
        assert_eq!(info.streams.len(), 2);
        assert_eq!(info.streams[0].padding, 8);
        assert_eq!(info.streams[1].uncomp_offset, 12);
        assert!(matches!(parse(b"short"), Err(ListError::TooSmall)));
    }
}
