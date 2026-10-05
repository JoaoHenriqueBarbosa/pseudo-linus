//! Leitura de um arquivo tar bloco a bloco: cabeçalhos (v7, ustar, gnu, oldgnu, pax), nomes longos do
//! GNU (`././@LongLink`), cabeçalhos estendidos pax (de membro e globais), arquivos esparsos (GNU antigo
//! e pax 0.0, 0.1 e 1.0) e os dados de cada membro.

use sysabi::{Errno, Fd};

use super::header::{self, Block, BLOCK, Magic, kind};
use super::member::{Member, Time};

/// De onde vêm os bytes do arquivo.
pub enum Source {
    /// Lido sob demanda de um fd (arquivo ou entrada padrão).
    Fd(Fd),
    /// Já na memória (arquivo descomprimido).
    Mem { data: Vec<u8>, pos: usize },
}

/// Erro de leitura dos dados.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// O arquivo acabou no meio de um membro.
    UnexpectedEof,
    Io(Errno),
}

/// Resultado de ler um cabeçalho.
pub enum Status {
    Member(Box<Member>),
    /// Bloco todo de zeros (fim do arquivo, se vier outro).
    ZeroBlock,
    /// Fim dos dados sem marcador.
    EndOfFile,
    /// Cabeçalho inválido (soma errada); o bloco já foi consumido.
    Failure,
    /// Erro de leitura ou fim inesperado dentro de um cabeçalho estendido.
    Error(ReadError),
}

/// Um registro de cabeçalho estendido pax.
pub type PaxRecord = (Vec<u8>, Vec<u8>);

pub struct Reader {
    src: Source,
    buf: Vec<u8>,
    pos: usize,
    eof: bool,
    /// Bytes já trazidos da fonte pro `buf`: o resto da divisão por 512 é o bloco incompleto do fim do
    /// `buf`, que só fica disponível quando a fonte o completar (o GNU descarta um bloco final parcial).
    filled: u64,
    /// Blocos já consumidos (o "ordinal" que o GNU informa).
    pub block: u64,
    global: Vec<PaxRecord>,
    /// Bytes consumidos desde o começo (pra saber onde o fim do arquivo está, no `-r`).
    pub offset: u64,
    /// Registros de cabeçalho pax malformados que já foram avisados.
    pub warnings: Vec<String>,
}

const CHUNK: usize = 64 * 1024;

impl Reader {
    pub fn new(src: Source) -> Reader {
        Reader { src, buf: Vec::new(), pos: 0, eof: false, filled: 0, block: 0, global: Vec::new(), offset: 0, warnings: Vec::new() }
    }

    /// Devolve bytes já lidos da fonte pra frente do fluxo (a espiada da magia de compressão).
    pub fn prepend(&mut self, data: Vec<u8>) {
        self.filled += data.len() as u64;
        let mut b = data;
        b.extend_from_slice(&self.buf[self.pos..]);
        self.buf = b;
        self.pos = 0;
    }

    fn fill(&mut self) -> Result<(), ReadError> {
        if self.eof {
            return Ok(());
        }
        match &mut self.src {
            Source::Mem { data, pos } => {
                // Repassa a memória em pedaços pra não duplicar o arquivo inteiro; o que ainda não foi
                // consumido (um bloco incompleto, no máximo) fica na frente.
                if self.pos > 0 {
                    self.buf.drain(..self.pos);
                    self.pos = 0;
                }
                let end = (*pos + CHUNK).min(data.len());
                self.buf.extend_from_slice(&data[*pos..end]);
                self.filled += (end - *pos) as u64;
                *pos = end;
                if end == data.len() {
                    self.eof = true;
                }
                Ok(())
            }
            Source::Fd(fd) => {
                let fd = *fd;
                if self.pos > 0 {
                    self.buf.drain(..self.pos);
                    self.pos = 0;
                }
                let start = self.buf.len();
                self.buf.resize(start + CHUNK, 0);
                loop {
                    match sysabi::sys::read(fd, &mut self.buf[start..]) {
                        Ok(n) => {
                            self.buf.truncate(start + n);
                            self.filled += n as u64;
                            if n == 0 {
                                self.eof = true;
                            }
                            return Ok(());
                        }
                        Err(Errno::EINTR) => {}
                        Err(e) => {
                            self.buf.truncate(start);
                            return Err(ReadError::Io(e));
                        }
                    }
                }
            }
        }
    }

    /// Bytes disponíveis sem bloquear (para o laço de leitura): só blocos completos.
    fn available(&self) -> usize {
        let partial = (self.filled % BLOCK as u64) as usize;
        (self.buf.len() - partial).saturating_sub(self.pos)
    }

    /// Lê exatamente `out.len()` bytes; devolve quantos conseguiu antes do fim.
    fn read_exact(&mut self, out: &mut [u8]) -> Result<usize, ReadError> {
        let mut got = 0;
        while got < out.len() {
            if self.available() == 0 {
                self.fill()?;
                if self.available() == 0 {
                    break;
                }
            }
            let n = self.available().min(out.len() - got);
            out[got..got + n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
            self.pos += n;
            got += n;
        }
        self.offset += got as u64;
        Ok(got)
    }

    /// Próximo bloco. `Ok(None)` no fim limpo; um bloco parcial no fim é completado com zeros e marca o
    /// fim (o GNU completa o último registro curto).
    pub fn next_block(&mut self) -> Result<Option<(Block, bool)>, ReadError> {
        let mut b = header::zero_block();
        let n = self.read_exact(&mut b)?;
        if n == 0 {
            return Ok(None);
        }
        self.block += 1;
        Ok(Some((b, n < BLOCK)))
    }

    /// Lê `size` bytes de dados de membro (mais o enchimento até o bloco), entregando em pedaços.
    pub fn read_data(&mut self, size: u64, mut sink: impl FnMut(&[u8])) -> Result<(), ReadError> {
        let mut left = size;
        let mut chunk = vec![0u8; CHUNK];
        while left > 0 {
            sysabi::sys::checkpoint();
            let want = (left.min(CHUNK as u64)) as usize;
            let n = self.read_exact(&mut chunk[..want])?;
            sink(&chunk[..n]);
            if n < want {
                return Err(ReadError::UnexpectedEof);
            }
            left -= n as u64;
        }
        self.skip_padding(size)
    }

    fn skip_padding(&mut self, size: u64) -> Result<(), ReadError> {
        let pad = (BLOCK as u64 - size % BLOCK as u64) % BLOCK as u64;
        if pad > 0 {
            let mut p = [0u8; BLOCK];
            let n = self.read_exact(&mut p[..pad as usize])?;
            if n < pad as usize {
                return Err(ReadError::UnexpectedEof);
            }
        }
        self.block += size.div_ceil(BLOCK as u64);
        Ok(())
    }

    /// Pula `size` bytes de dados (e o enchimento).
    pub fn skip_data(&mut self, size: u64) -> Result<(), ReadError> {
        let mut left = size;
        while left > 0 {
            if self.available() == 0 {
                self.fill()?;
                if self.available() == 0 {
                    return Err(ReadError::UnexpectedEof);
                }
            }
            let n = (self.available() as u64).min(left) as usize;
            self.pos += n;
            self.offset += n as u64;
            left -= n as u64;
        }
        self.skip_padding(size)
    }

    /// Lê os dados inteiros de um membro (nomes longos, cabeçalhos pax), com limite de tamanho.
    fn read_small(&mut self, size: u64) -> Result<Vec<u8>, ReadError> {
        let mut v: Vec<u8> = Vec::new();
        if v.try_reserve(size.min(1 << 24) as usize).is_err() {
            return Err(ReadError::Io(Errno::ENOMEM));
        }
        self.read_data(size, |c| v.extend_from_slice(c))?;
        Ok(v)
    }

    /// Lê o próximo cabeçalho, juntando os estendidos.
    pub fn read_header(&mut self) -> Status {
        let start_block = self.block;
        let start_offset = self.offset;
        let mut long_name: Option<Vec<u8>> = None;
        let mut long_link: Option<Vec<u8>> = None;
        let mut pax: Vec<PaxRecord> = Vec::new();
        let mut first = true;
        loop {
            let (b, partial) = match self.next_block() {
                Ok(Some(x)) => x,
                Ok(None) => {
                    return if first { Status::EndOfFile } else { Status::Error(ReadError::UnexpectedEof) };
                }
                Err(e) => return Status::Error(e),
            };
            // Bloco de cabeçalho incompleto no fim: o GNU trata como fim dos dados (sem aviso).
            if partial {
                return if first { Status::EndOfFile } else { Status::Error(ReadError::UnexpectedEof) };
            }
            if header::is_zero(&b) {
                if first {
                    return Status::ZeroBlock;
                }
                return Status::Error(ReadError::UnexpectedEof);
            }
            if !header::checksum_ok(&b) {
                return Status::Failure;
            }
            first = false;
            let main_block = self.block - 1;
            let typeflag = b[header::TYPEFLAG];
            let size = header::parse_number(&b, header::SIZE).unwrap_or(0).max(0) as u64;
            match typeflag {
                kind::GNU_LONGNAME | kind::GNU_LONGLINK => {
                    let data = match self.read_small(size) {
                        Ok(d) => d,
                        Err(e) => return Status::Error(e),
                    };
                    let data = trim_nul(&data).to_vec();
                    if typeflag == kind::GNU_LONGNAME {
                        long_name = Some(data);
                    } else {
                        long_link = Some(data);
                    }
                    continue;
                }
                kind::XHD | kind::SOLARIS_XHD => {
                    let data = match self.read_small(size) {
                        Ok(d) => d,
                        Err(e) => return Status::Error(e),
                    };
                    match parse_pax(&data) {
                        Ok(r) => pax.extend(r),
                        Err(msg) => self.warnings.push(msg),
                    }
                    continue;
                }
                kind::XGL => {
                    let data = match self.read_small(size) {
                        Ok(d) => d,
                        Err(e) => return Status::Error(e),
                    };
                    match parse_pax(&data) {
                        Ok(r) => {
                            for (k, v) in r {
                                self.global.retain(|(gk, _)| *gk != k);
                                self.global.push((k, v));
                            }
                        }
                        Err(msg) => self.warnings.push(msg),
                    }
                    // Um cabeçalho global sozinho não é membro: segue pro próximo.
                    first = true;
                    continue;
                }
                _ => {}
            }
            let mut m = decode(&b);
            m.header_block = start_block;
            m.main_block = main_block;
            if let Some(n) = long_name {
                m.name = n;
            }
            if let Some(l) = long_link {
                m.linkname = l;
            }
            let records: Vec<PaxRecord> = self.global.iter().cloned().chain(pax).collect();
            apply_pax(&mut m, &records);
            if typeflag == kind::GNU_SPARSE {
                match self.read_old_sparse(&b) {
                    Ok(map) => {
                        m.real_size = header::parse_number(&b, header::GNU_REALSIZE).unwrap_or(0).max(0) as u64;
                        m.sparse = Some(map);
                    }
                    Err(e) => return Status::Error(e),
                }
            }
            if m.sparse.is_none() && m.real_size == 0 {
                m.real_size = m.size;
            }
            // pax sparse 1.0: o mapa está no começo dos dados.
            if records.iter().any(|(k, v)| k == b"GNU.sparse.major" && v == b"1") {
                match self.read_sparse_v1(&mut m) {
                    Ok(()) => {}
                    Err(e) => return Status::Error(e),
                }
            }
            m.start_offset = start_offset;
            return Status::Member(Box::new(m));
        }
    }

    fn read_old_sparse(&mut self, b: &Block) -> Result<Vec<(u64, u64)>, ReadError> {
        let mut map = Vec::new();
        let push = |blk: &Block, base: usize, count: usize, map: &mut Vec<(u64, u64)>| {
            for i in 0..count {
                let o = base + i * 24;
                let off = header::parse_number(blk, (o, 12)).unwrap_or(0).max(0) as u64;
                let n = header::parse_number(blk, (o + 12, 12)).unwrap_or(0).max(0) as u64;
                if blk[o] == 0 && n == 0 && off == 0 {
                    continue;
                }
                map.push((off, n));
            }
        };
        push(b, header::GNU_SPARSE, 4, &mut map);
        let mut extended = b[header::GNU_ISEXTENDED] != 0;
        while extended {
            let Some((eb, _)) = self.next_block()? else { return Err(ReadError::UnexpectedEof) };
            push(&eb, 0, 21, &mut map);
            extended = eb[504] != 0;
        }
        Ok(map)
    }

    fn read_sparse_v1(&mut self, m: &mut Member) -> Result<(), ReadError> {
        // Números decimais separados por \n: quantidade, depois pares deslocamento/tamanho; o mapa
        // ocupa blocos inteiros, descontados do tamanho dos dados.
        let mut text: Vec<u8> = Vec::new();
        let mut numbers: Vec<u64> = Vec::new();
        let mut needed: Option<usize> = None;
        let mut used_blocks = 0u64;
        loop {
            let Some((blk, _)) = self.next_block()? else { return Err(ReadError::UnexpectedEof) };
            used_blocks += 1;
            text.extend_from_slice(&blk);
            numbers.clear();
            let mut complete = true;
            let mut cur: Option<u64> = None;
            for &c in &text {
                if c.is_ascii_digit() {
                    cur = Some(cur.unwrap_or(0).saturating_mul(10).saturating_add((c - b'0') as u64));
                } else if c == b'\n' {
                    if let Some(v) = cur.take() {
                        numbers.push(v);
                    }
                    if let Some(n) = needed
                        && numbers.len() > 2 * n
                    {
                        break;
                    }
                    if needed.is_none() && !numbers.is_empty() {
                        needed = Some(numbers[0] as usize);
                    }
                } else {
                    break;
                }
            }
            if let Some(n) = needed {
                if numbers.len() < 1 + 2 * n {
                    complete = false;
                }
            } else {
                complete = false;
            }
            if complete {
                break;
            }
            if used_blocks > 1 << 16 {
                return Err(ReadError::UnexpectedEof);
            }
        }
        let n = needed.unwrap_or(0);
        let map: Vec<(u64, u64)> = (0..n).map(|i| (numbers[1 + 2 * i], numbers[2 + 2 * i])).collect();
        m.sparse = Some(map);
        m.size = m.size.saturating_sub(used_blocks * BLOCK as u64);
        Ok(())
    }
}

fn trim_nul(d: &[u8]) -> &[u8] {
    match d.iter().position(|&c| c == 0) {
        Some(p) => &d[..p],
        None => d,
    }
}

/// Interpreta os campos de um cabeçalho principal.
pub fn decode(b: &Block) -> Member {
    let mag = header::magic(b);
    let mut name = header::field_str(b, header::NAME).to_vec();
    if mag == Magic::Ustar {
        let prefix = header::field_str(b, header::PREFIX);
        if !prefix.is_empty() {
            let mut full = prefix.to_vec();
            full.push(b'/');
            full.extend_from_slice(&name);
            name = full;
        }
    }
    let num = |f| header::parse_number(b, f).unwrap_or(0);
    let mut m = Member {
        name,
        linkname: header::field_str(b, header::LINKNAME).to_vec(),
        typeflag: b[header::TYPEFLAG],
        mode: (num(header::MODE) & 0o7777_7777) as u32,
        uid: num(header::UID) as i64,
        gid: num(header::GID) as i64,
        size: num(header::SIZE).max(0) as u64,
        mtime: Time::new(num(header::MTIME) as i64, 0),
        magic: mag,
        ..Member::default()
    };
    if mag != Magic::None {
        m.uname = header::field_str(b, header::UNAME).to_vec();
        m.gname = header::field_str(b, header::GNAME).to_vec();
        m.devmajor = num(header::DEVMAJOR) as u32;
        m.devminor = num(header::DEVMINOR) as u32;
    }
    if mag == Magic::Gnu {
        let at = num(header::GNU_ATIME);
        let ct = num(header::GNU_CTIME);
        if b[header::GNU_ATIME.0] != 0 {
            m.atime = Some(Time::new(at as i64, 0));
        }
        if b[header::GNU_CTIME.0] != 0 {
            m.ctime = Some(Time::new(ct as i64, 0));
        }
    }
    m
}

/// Registros de um cabeçalho estendido pax: "comprimento chave=valor\n".
pub fn parse_pax(data: &[u8]) -> Result<Vec<PaxRecord>, String> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < data.len() {
        if data[p] == 0 {
            break;
        }
        let rest = &data[p..];
        let sp = rest.iter().position(|&c| c == b' ').ok_or_else(|| "Malformed extended header: missing length".to_string())?;
        let len: usize = std::str::from_utf8(&rest[..sp])
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| "Malformed extended header: missing length".to_string())?;
        if len == 0 || len > rest.len() || len <= sp + 1 {
            return Err("Extended header length is out of allowed range".to_string());
        }
        let rec = &rest[sp + 1..len];
        if rec.last() != Some(&b'\n') {
            return Err("Malformed extended header: missing newline".to_string());
        }
        let rec = &rec[..rec.len() - 1];
        let eq = rec.iter().position(|&c| c == b'=').ok_or_else(|| "Malformed extended header: missing equal sign".to_string())?;
        out.push((rec[..eq].to_vec(), rec[eq + 1..].to_vec()));
        p += len;
    }
    Ok(out)
}

/// "1768478400.5" em segundos e nanossegundos.
pub fn parse_pax_time(v: &[u8]) -> Option<Time> {
    let s = std::str::from_utf8(v).ok()?;
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    let sec: i64 = int.parse().ok()?;
    let mut nsec: u64 = 0;
    let mut digits = 0;
    for c in frac.bytes().take(9) {
        if !c.is_ascii_digit() {
            return None;
        }
        nsec = nsec * 10 + (c - b'0') as u64;
        digits += 1;
    }
    while digits < 9 {
        nsec *= 10;
        digits += 1;
    }
    if neg {
        if nsec > 0 {
            Some(Time::new(-sec - 1, (1_000_000_000 - nsec) as u32))
        } else {
            Some(Time::new(-sec, 0))
        }
    } else {
        Some(Time::new(sec, nsec as u32))
    }
}

fn parse_u64(v: &[u8]) -> Option<u64> {
    std::str::from_utf8(v).ok()?.parse().ok()
}

/// Aplica os registros pax (globais primeiro, depois os do membro) aos metadados.
pub fn apply_pax(m: &mut Member, records: &[PaxRecord]) {
    let mut sparse_offsets: Vec<u64> = Vec::new();
    let mut sparse_sizes: Vec<u64> = Vec::new();
    let mut sparse_map: Option<Vec<(u64, u64)>> = None;
    let mut sparse_real: Option<u64> = None;
    let mut sparse_name: Option<Vec<u8>> = None;
    for (k, v) in records {
        match k.as_slice() {
            b"path" => m.name = v.clone(),
            b"linkpath" => m.linkname = v.clone(),
            b"size" => {
                if let Some(x) = parse_u64(v) {
                    m.size = x;
                }
            }
            b"uid" => {
                if let Some(x) = parse_u64(v) {
                    m.uid = x as i64;
                }
            }
            b"gid" => {
                if let Some(x) = parse_u64(v) {
                    m.gid = x as i64;
                }
            }
            b"uname" => m.uname = v.clone(),
            b"gname" => m.gname = v.clone(),
            b"mtime" => {
                if let Some(t) = parse_pax_time(v) {
                    m.mtime = t;
                }
            }
            b"atime" => m.atime = parse_pax_time(v),
            b"ctime" => m.ctime = parse_pax_time(v),
            b"GNU.sparse.size" | b"GNU.sparse.realsize" => sparse_real = parse_u64(v),
            b"GNU.sparse.name" => sparse_name = Some(v.clone()),
            b"GNU.sparse.offset" => {
                if let Some(x) = parse_u64(v) {
                    sparse_offsets.push(x);
                }
            }
            b"GNU.sparse.numbytes" => {
                if let Some(x) = parse_u64(v) {
                    sparse_sizes.push(x);
                }
            }
            b"GNU.sparse.map" => {
                let nums: Vec<u64> = v.split(|&c| c == b',').filter_map(parse_u64).collect();
                sparse_map = Some(nums.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect());
            }
            _ => {}
        }
    }
    if let Some(n) = sparse_name {
        m.name = n;
    }
    if !sparse_offsets.is_empty() {
        m.sparse = Some(sparse_offsets.into_iter().zip(sparse_sizes).collect());
    } else if let Some(map) = sparse_map {
        m.sparse = Some(map);
    }
    if let Some(r) = sparse_real {
        m.real_size = r;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pax_records_and_times() {
        let r = parse_pax(b"30 atime=1790996485.347498756\n22 mtime=1768478400.5\n").unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(parse_pax_time(&r[1].1), Some(Time::new(1_768_478_400, 500_000_000)));
        assert_eq!(parse_pax_time(b"-1.5"), Some(Time::new(-2, 500_000_000)));
        assert!(parse_pax(b"9 a=b\n").is_err());
    }
}
