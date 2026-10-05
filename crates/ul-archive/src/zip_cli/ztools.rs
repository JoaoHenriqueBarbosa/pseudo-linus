//! Leitura e remontagem de zips pelos utilitários pequenos do Info-ZIP (`zipnote` e `zipsplit`):
//! diretório central, tamanho de cada entrada no arquivo (cabeçalho local, dados e descritor) e os
//! registros que eles gravam. Não há Zip64 aqui: os dois programas, no zip 3.0, também só leem
//! arquivos de um disco com campos de 32 bits.

/// Códigos de saída do `ziperr.h` que os utilitários usam.
pub const ZE_FORM: i32 = 3;
pub const ZE_BIG: i32 = 6;
pub const ZE_ABORT: i32 = 9;
pub const ZE_NONE: i32 = 12;
pub const ZE_WRITE: i32 = 14;
pub const ZE_PARMS: i32 = 16;

/// A mensagem de `errors[]` de cada código.
pub fn error_text(code: i32) -> &'static str {
    match code {
        2 => "Unexpected end of zip file",
        3 => "Zip file structure invalid",
        4 => "Out of memory",
        6 => "Entry too large to split",
        7 => "Invalid comment format",
        9 => "Interrupted",
        10 => "Temporary file failure",
        11 => "Input file read failure",
        12 => "Nothing to do!",
        13 => "Missing or empty zip file",
        14 => "Output file write failure",
        15 => "Could not create output file",
        16 => "Invalid command arguments",
        18 => "File not found or no read permission",
        _ => "Internal logic error",
    }
}

/// Uma entrada do diretório central.
#[derive(Clone)]
pub struct Entry {
    /// O cabeçalho central fixo, com a assinatura (46 bytes).
    pub cen: Vec<u8>,
    pub name: Vec<u8>,
    pub cextra: Vec<u8>,
    pub comment: Vec<u8>,
    /// Deslocamento do cabeçalho local.
    pub off: u64,
    pub csize: u64,
    pub flg: u16,
}

pub struct Archive {
    pub entries: Vec<Entry>,
    pub comment: Vec<u8>,
}

fn u16_at(d: &[u8], p: usize) -> usize {
    d[p] as usize | (d[p + 1] as usize) << 8
}

fn u32_at(d: &[u8], p: usize) -> u64 {
    u64::from(d[p]) | u64::from(d[p + 1]) << 8 | u64::from(d[p + 2]) << 16 | u64::from(d[p + 3]) << 24
}

/// Lê o diretório central. `Err` é o código de saída do `ziperr`.
pub fn parse(data: &[u8]) -> Result<Archive, i32> {
    let start = data.len().saturating_sub(65_557);
    let mut eocd = None;
    let mut i = data.len().saturating_sub(22);
    loop {
        if i + 22 <= data.len() && &data[i..i + 4] == b"PK\x05\x06" {
            eocd = Some(i);
            break;
        }
        if i <= start {
            break;
        }
        i -= 1;
    }
    let Some(e) = eocd else { return Err(ZE_FORM) };
    let n = u16_at(data, e + 10);
    let cdoff = u32_at(data, e + 16) as usize;
    let clen = u16_at(data, e + 20);
    if e + 22 + clen > data.len() || n == 0xFFFF || cdoff == 0xFFFF_FFFF {
        return Err(ZE_FORM);
    }
    let comment = data[e + 22..e + 22 + clen].to_vec();
    let mut entries = Vec::with_capacity(n);
    let mut p = cdoff;
    for _ in 0..n {
        if p + 46 > data.len() || &data[p..p + 4] != b"PK\x01\x02" {
            return Err(ZE_FORM);
        }
        let nl = u16_at(data, p + 28);
        let el = u16_at(data, p + 30);
        let cl = u16_at(data, p + 32);
        if p + 46 + nl + el + cl > data.len() {
            return Err(ZE_FORM);
        }
        let csize = u32_at(data, p + 20);
        let off = u32_at(data, p + 42);
        if csize == 0xFFFF_FFFF || off == 0xFFFF_FFFF {
            return Err(ZE_FORM);
        }
        entries.push(Entry {
            cen: data[p..p + 46].to_vec(),
            name: data[p + 46..p + 46 + nl].to_vec(),
            cextra: data[p + 46 + nl..p + 46 + nl + el].to_vec(),
            comment: data[p + 46 + nl + el..p + 46 + nl + el + cl].to_vec(),
            off,
            csize,
            flg: u16_at(data, p + 8) as u16,
        });
        p += 46 + nl + el + cl;
    }
    Ok(Archive { entries, comment })
}

/// O tamanho no arquivo do cabeçalho local, dos dados e do descritor, se houver.
pub fn local_len(data: &[u8], e: &Entry) -> Result<usize, i32> {
    let o = e.off as usize;
    if o + 30 > data.len() || &data[o..o + 4] != b"PK\x03\x04" {
        return Err(ZE_FORM);
    }
    let mut total = 30 + u16_at(data, o + 26) + u16_at(data, o + 28) + e.csize as usize;
    if e.flg & 8 != 0 {
        let d = o + total;
        total += if d + 4 <= data.len() && &data[d..d + 4] == b"PK\x07\x08" { 16 } else { 12 };
    }
    if o + total > data.len() {
        return Err(ZE_FORM);
    }
    Ok(total)
}

/// Copia a entrada (cabeçalho local, dados, descritor) para `out`, trocando o nome se pedido.
pub fn copy_local(data: &[u8], e: &Entry, new_name: Option<&[u8]>, out: &mut Vec<u8>) -> Result<(), i32> {
    let len = local_len(data, e)?;
    let o = e.off as usize;
    let old_nl = u16_at(data, o + 26);
    match new_name {
        None => out.extend_from_slice(&data[o..o + len]),
        Some(nn) => {
            out.extend_from_slice(&data[o..o + 26]);
            out.extend_from_slice(&(nn.len() as u16).to_le_bytes());
            out.extend_from_slice(&data[o + 28..o + 30]);
            out.extend_from_slice(nn);
            out.extend_from_slice(&data[o + 30 + old_nl..o + len]);
        }
    }
    Ok(())
}

/// O registro do diretório central de `e` com o deslocamento, o nome e o comentário dados.
pub fn central(e: &Entry, off: u64, name: &[u8], comment: &[u8]) -> Vec<u8> {
    let mut c = e.cen.clone();
    c[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
    c[32..34].copy_from_slice(&(comment.len() as u16).to_le_bytes());
    c[42..46].copy_from_slice(&(off as u32).to_le_bytes());
    c.extend_from_slice(name);
    c.extend_from_slice(&e.cextra);
    c.extend_from_slice(comment);
    c
}

/// O fim do diretório central.
pub fn eocd(n: usize, cd_size: usize, cd_off: usize, comment: &[u8]) -> Vec<u8> {
    let mut b = b"PK\x05\x06\0\0\0\0".to_vec();
    b.extend_from_slice(&(n as u16).to_le_bytes());
    b.extend_from_slice(&(n as u16).to_le_bytes());
    b.extend_from_slice(&(cd_size as u32).to_le_bytes());
    b.extend_from_slice(&(cd_off as u32).to_le_bytes());
    b.extend_from_slice(&(comment.len() as u16).to_le_bytes());
    b.extend_from_slice(comment);
    b
}

/// Monta um zip com as entradas `idx` (na ordem dada) tiradas de `data`.
pub fn build(data: &[u8], arc: &Archive, idx: &[usize], comment: &[u8]) -> Result<Vec<u8>, i32> {
    let mut out = Vec::new();
    let mut cds: Vec<Vec<u8>> = Vec::new();
    for &i in idx {
        let e = &arc.entries[i];
        let off = out.len() as u64;
        copy_local(data, e, None, &mut out)?;
        cds.push(central(e, off, &e.name, &e.comment));
    }
    let cd_off = out.len();
    let mut size = 0;
    for c in &cds {
        size += c.len();
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&eocd(cds.len(), size, cd_off, comment));
    Ok(out)
}

/// Custo de uma entrada num zip: registro central mais cabeçalho local, dados e descritor.
pub fn entry_cost(data: &[u8], e: &Entry) -> Result<usize, i32> {
    Ok(46 + e.name.len() + e.cextra.len() + e.comment.len() + local_len(data, e)?)
}
