//! Leitor mínimo de zip: acha uma entrada pelo diretório central e devolve o conteúdo
//! descomprimido. Só o que uma wheel precisa (métodos 0 e 8, sem zip64).

use flate2::read::DeflateDecoder;
use std::io::{self, Read, Seek, SeekFrom};

const EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const CENTRAL_SIG: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const LOCAL_SIG: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
/// Maior entrada descomprimida que aceitamos ler (o METADATA é pequeno).
const MAX_ENTRY: usize = 64 << 20;

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

fn slice(b: &[u8], at: usize, n: usize) -> io::Result<&[u8]> {
    at.checked_add(n)
        .and_then(|end| b.get(at..end))
        .ok_or_else(|| bad("zip truncado"))
}

fn le16(b: &[u8], at: usize) -> io::Result<usize> {
    let s = slice(b, at, 2)?;
    Ok(usize::from(u16::from_le_bytes([s[0], s[1]])))
}

fn le32(b: &[u8], at: usize) -> io::Result<u64> {
    let s = slice(b, at, 4)?;
    Ok(u64::from(u32::from_le_bytes([s[0], s[1], s[2], s[3]])))
}

/// Devolve o conteúdo da primeira entrada cujo nome satisfaz `wanted`.
pub(crate) fn read_entry<R: Read + Seek>(
    r: &mut R,
    wanted: impl Fn(&str) -> bool,
) -> io::Result<Option<Vec<u8>>> {
    let len = r.seek(SeekFrom::End(0))?;
    let tail_len = len.min(22 + 65535);
    r.seek(SeekFrom::Start(len - tail_len))?;
    let mut tail = vec![0u8; usize::try_from(tail_len).map_err(|_| bad("zip grande"))?];
    r.read_exact(&mut tail)?;
    let eocd = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| tail[i..i + 4] == EOCD_SIG)
        .ok_or_else(|| bad("sem fim de diretório central"))?;
    let cd_size = le32(&tail, eocd + 12)?;
    let cd_off = le32(&tail, eocd + 16)?;
    if cd_off == 0xffff_ffff || cd_size == 0xffff_ffff {
        return Err(bad("zip64 não suportado"));
    }
    if cd_off + cd_size > len {
        return Err(bad("diretório central fora do arquivo"));
    }
    r.seek(SeekFrom::Start(cd_off))?;
    let mut cd = vec![0u8; usize::try_from(cd_size).map_err(|_| bad("zip grande"))?];
    r.read_exact(&mut cd)?;

    let mut pos = 0usize;
    while pos + 46 <= cd.len() {
        if cd[pos..pos + 4] != CENTRAL_SIG {
            return Err(bad("entrada do diretório central inválida"));
        }
        let method = le16(&cd, pos + 10)?;
        let csize = le32(&cd, pos + 20)?;
        let usize_ = usize::try_from(le32(&cd, pos + 24)?).map_err(|_| bad("entrada grande"))?;
        let n = le16(&cd, pos + 28)?;
        let m = le16(&cd, pos + 30)?;
        let k = le16(&cd, pos + 32)?;
        let off = le32(&cd, pos + 42)?;
        let name = String::from_utf8_lossy(slice(&cd, pos + 46, n)?).into_owned();
        pos += 46 + n + m + k;
        if wanted(&name) {
            return extract(r, method, csize, usize_, off).map(Some);
        }
    }
    Ok(None)
}

fn extract<R: Read + Seek>(
    r: &mut R,
    method: usize,
    csize: u64,
    size: usize,
    off: u64,
) -> io::Result<Vec<u8>> {
    if size > MAX_ENTRY {
        return Err(bad("entrada grande demais"));
    }
    r.seek(SeekFrom::Start(off))?;
    let mut head = [0u8; 30];
    r.read_exact(&mut head)?;
    if head[..4] != LOCAL_SIG {
        return Err(bad("cabeçalho local inválido"));
    }
    let start = off + 30 + le16(&head, 26)? as u64 + le16(&head, 28)? as u64;
    r.seek(SeekFrom::Start(start))?;
    let mut out = Vec::with_capacity(size);
    match method {
        0 => {
            r.by_ref().take(csize).read_to_end(&mut out)?;
        }
        8 => {
            DeflateDecoder::new(r.by_ref().take(csize))
                .take(size as u64)
                .read_to_end(&mut out)?;
        }
        _ => return Err(bad("método de compressão não suportado")),
    }
    if out.len() != size {
        return Err(bad("tamanho da entrada diverge"));
    }
    Ok(out)
}
