//! Escrita de arquivos tar: cabeçalhos em cada formato do jeito que o GNU tar 1.35 grava (conferido
//! byte a byte no oráculo) e a saída em registros (`-b`, 20 blocos de 512 bytes por padrão), com o
//! último registro completado com zeros.

use std::io::Write;

use super::header::{self, Block, Format, kind};
use super::member::{Member, Time};

/// Destino dos blocos.
pub enum Sink {
    Fd(sysabi::Fd),
    Encoder(crate::codec::Encoder<sysabi::FdWriter>),
    /// Em memória (programa externo de compressão, `--delete` sobre arquivo comum).
    Mem(Vec<u8>),
}

pub struct Writer {
    sink: Option<Sink>,
    record: usize,
    buf: Vec<u8>,
    /// Blocos escritos (o ordinal do próximo bloco).
    pub blocks: u64,
    /// Bytes escritos no total (pro `--totals`).
    pub bytes: u64,
    /// Registros completos já escritos (pros checkpoints).
    pub records: u64,
    pub error: Option<std::io::Error>,
}

impl Writer {
    pub fn new(sink: Sink, record_size: usize) -> Writer {
        Writer {
            sink: Some(sink),
            record: record_size.max(512),
            buf: Vec::new(),
            blocks: 0,
            bytes: 0,
            records: 0,
            error: None,
        }
    }

    /// Começa no meio de um registro (no `-r`, quando o fim do arquivo não cai na borda).
    pub fn with_pending(sink: Sink, record_size: usize, pending: Vec<u8>, blocks: u64) -> Writer {
        let mut w = Writer::new(sink, record_size);
        w.buf = pending;
        w.blocks = blocks;
        w
    }

    fn emit(&mut self, data: &[u8]) {
        if self.error.is_some() {
            return;
        }
        let r = match self.sink.as_mut().expect("destino") {
            Sink::Fd(fd) => sysabi::sys::write_all(*fd, data).map_err(|e| e.to_io()),
            Sink::Encoder(e) => e.write_all(data),
            Sink::Mem(v) => {
                v.extend_from_slice(data);
                Ok(())
            }
        };
        if let Err(e) = r {
            self.error = Some(e);
        }
    }

    fn flush_full_records(&mut self) {
        while self.buf.len() >= self.record {
            let rec: Vec<u8> = self.buf.drain(..self.record).collect();
            self.emit(&rec);
            self.bytes += rec.len() as u64;
            self.records += 1;
        }
    }

    /// Blocos por registro.
    pub fn record_blocks(&self) -> u64 {
        (self.record / 512) as u64
    }

    pub fn write_block(&mut self, b: &[u8]) {
        debug_assert_eq!(b.len(), 512);
        self.buf.extend_from_slice(b);
        self.blocks += 1;
        if self.buf.len() >= self.record {
            self.flush_full_records();
        }
    }

    /// Dados de membro: `data` vira blocos, o último completado com zeros.
    pub fn write_data(&mut self, data: &[u8]) {
        for chunk in data.chunks(512) {
            if chunk.len() == 512 {
                self.write_block(chunk);
            } else {
                let mut b = [0u8; 512];
                b[..chunk.len()].copy_from_slice(chunk);
                self.write_block(&b);
            }
        }
    }

    /// Bytes crus já em blocos (cópia de membros no `--delete` e no `-A`).
    pub fn write_raw(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
        self.blocks += (data.len() / 512) as u64;
        self.flush_full_records();
    }

    /// Fecha o arquivo: dois blocos de zeros (se `eof_marker`) e o registro completado.
    pub fn finish(mut self, eof_marker: bool) -> Result<Option<Vec<u8>>, std::io::Error> {
        if eof_marker {
            let z = [0u8; 512];
            self.write_block(&z);
            self.write_block(&z);
        }
        if !self.buf.is_empty() {
            let pad = self.record - self.buf.len() % self.record;
            if pad != self.record {
                self.buf.resize(self.buf.len() + pad, 0);
            }
            self.flush_full_records();
        }
        if let Some(e) = self.error.take() {
            return Err(e);
        }
        match self.sink.take().expect("destino") {
            Sink::Fd(_) => Ok(None),
            Sink::Encoder(e) => e.finish().map(|_| None),
            Sink::Mem(v) => Ok(Some(v)),
        }
    }
}

/// O que não coube num cabeçalho: o nome não pode ir pro arquivo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeaderError {
    /// ustar: "file name is too long (cannot be split); not dumped".
    CannotSplit,
    /// v7: "file name is too long (max 99); not dumped".
    TooLong(usize),
    /// Valor numérico fora da faixa do formato.
    Range { field: &'static str, value: i128, max: u64 },
}

fn base_header(m: &Member, fmt: Format) -> Block {
    let mut b = header::zero_block();
    let typeflag = if fmt == Format::V7 && m.typeflag == kind::REG { kind::AREG } else { m.typeflag };
    b[header::TYPEFLAG] = typeflag;
    match fmt {
        Format::Gnu | Format::OldGnu => header::put_bytes(&mut b, (257, 8), b"ustar  \0"),
        Format::Ustar | Format::Pax => {
            header::put_bytes(&mut b, header::MAGIC, b"ustar\0");
            header::put_bytes(&mut b, header::VERSION, b"00");
        }
        Format::V7 => {}
    }
    b
}

/// Grava um campo numérico do jeito do formato: octal se cabe; base 256 no gnu/oldgnu; senão erro.
fn put_num(b: &mut Block, field: (usize, usize), value: i128, fmt: Format, name: &'static str) -> Result<(), HeaderError> {
    if value >= 0 && (value as u128) <= header::octal_max(field.1) as u128 {
        header::put_octal(b, field, value as u64);
        return Ok(());
    }
    if fmt.allows_base256() {
        header::put_base256(b, field, value);
        return Ok(());
    }
    Err(HeaderError::Range { field: name, value, max: header::octal_max(field.1) })
}

/// Separação ustar: o prefixo mais longo (até 155) cujo resto (até 100, não vazio) cabe no nome.
pub fn ustar_split(name: &[u8]) -> Option<(usize, usize)> {
    if name.len() <= 100 {
        return Some((0, 0));
    }
    let limit = name.len().saturating_sub(1);
    let mut best = None;
    for i in (1..limit).rev() {
        if name[i] == b'/' && i <= 155 && name.len() - i - 1 <= 100 {
            best = Some((i, i + 1));
            break;
        }
    }
    best
}

/// Registros de um cabeçalho estendido pax.
pub fn pax_record(key: &[u8], value: &[u8]) -> Vec<u8> {
    let body = key.len() + value.len() + 3; // espaço, '=', '\n'
    let mut len = body + 1;
    loop {
        let digits = len.to_string().len();
        let total = body + digits;
        if total == len {
            break;
        }
        len = total;
    }
    let mut r = format!("{len} ").into_bytes();
    r.extend_from_slice(key);
    r.push(b'=');
    r.extend_from_slice(value);
    r.push(b'\n');
    r
}

fn rec(k: &[u8], v: &[u8]) -> (Vec<u8>, Vec<u8>) {
    (k.to_vec(), v.to_vec())
}

/// Instante no formato dos registros pax ("1768478400.5"; sem fração quando é inteiro).
pub fn pax_time(t: Time) -> String {
    let (sec, nsec) = (t.sec, t.nsec);
    if nsec == 0 {
        return sec.to_string();
    }
    let (s, n) = if sec < 0 { (sec + 1, 1_000_000_000 - nsec) } else { (sec, nsec) };
    let mut frac = format!("{n:09}");
    while frac.ends_with('0') {
        frac.pop();
    }
    if sec < 0 && s == 0 { format!("-0.{frac}") } else { format!("{s}.{frac}") }
}

/// Configuração do `--pax-option`.
#[derive(Clone, Debug, Default)]
pub struct PaxConfig {
    /// `delete=PADRÃO`: palavras-chave omitidas.
    pub delete: Vec<Vec<u8>>,
    /// `exthdr.name=MODELO` (`%d`, `%f`, `%p`, `%n`, `%%`).
    pub exthdr_name: Option<Vec<u8>>,
    /// `chave=valor` e `chave:=valor`: registros acrescentados a cada cabeçalho estendido.
    pub extra: Vec<(Vec<u8>, Vec<u8>)>,
}

impl PaxConfig {
    /// Analisa os `--pax-option` (listas separadas por vírgula).
    pub fn parse(opts: &[Vec<u8>]) -> Result<PaxConfig, Vec<u8>> {
        let mut c = PaxConfig::default();
        for o in opts {
            for item in o.split(|&b| b == b',') {
                if item.is_empty() {
                    continue;
                }
                let Some(eq) = item.iter().position(|&b| b == b'=') else {
                    let mut m = b"Malformed pax option: ".to_vec();
                    m.extend_from_slice(&super::quote::locale(item));
                    return Err(m);
                };
                let (k, v) = (&item[..eq], &item[eq + 1..]);
                let k = k.strip_suffix(b":").unwrap_or(k);
                match k {
                    b"delete" => c.delete.push(v.to_vec()),
                    b"exthdr.name" => c.exthdr_name = Some(v.to_vec()),
                    b"globexthdr.name" | b"invalid" | b"linkdata" | b"times" => {}
                    _ => c.extra.push((k.to_vec(), v.to_vec())),
                }
            }
        }
        Ok(c)
    }

    fn deleted(&self, key: &[u8]) -> bool {
        self.delete.iter().any(|p| super::fnmatch::fnmatch(p, key, Default::default()))
    }
}

/// Nome do cabeçalho estendido: o modelo (padrão `%d/PaxHeaders/%f`).
fn pax_header_name(name: &[u8], template: Option<&[u8]>) -> Vec<u8> {
    let trimmed = super::names::trim_trailing_slashes(name);
    let (dir, base) = match trimmed.iter().rposition(|&c| c == b'/') {
        Some(p) => (&trimmed[..p], &trimmed[p + 1..]),
        None => (&b"."[..], trimmed),
    };
    let dir: &[u8] = if dir.is_empty() { b"/" } else { dir };
    let template = template.unwrap_or(b"%d/PaxHeaders/%f");
    let mut out = Vec::new();
    let mut i = 0;
    while i < template.len() {
        if template[i] == b'%' && i + 1 < template.len() {
            match template[i + 1] {
                b'd' => out.extend_from_slice(dir),
                b'f' => out.extend_from_slice(base),
                b'p' => out.extend_from_slice(
                    sysabi::sys::try_current().map(|s| s.getpid()).unwrap_or(0).to_string().as_bytes(),
                ),
                b'n' => out.push(b'0'),
                b'%' => out.push(b'%'),
                other => {
                    out.push(b'%');
                    out.push(other);
                }
            }
            i += 2;
        } else {
            out.push(template[i]);
            i += 1;
        }
    }
    out
}

/// Monta os blocos de cabeçalho de um membro no formato pedido: nomes longos do GNU, cabeçalho
/// estendido pax e o cabeçalho principal. Os dados do membro vêm depois, por conta de quem chama.
pub fn headers(m: &Member, fmt: Format, cfg: &PaxConfig) -> Result<Vec<u8>, HeaderError> {
    let mut out: Vec<u8> = Vec::new();
    let mut b = base_header(m, fmt);
    let is_dev = matches!(m.typeflag, kind::CHR | kind::BLK);
    let mut pax: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    match fmt {
        Format::Gnu | Format::OldGnu => {
            let max = if fmt == Format::Gnu { 100 } else { 99 };
            if m.linkname.len() > max {
                out.extend(long_link_blocks(kind::GNU_LONGLINK, &m.linkname, m));
            }
            if m.name.len() > max {
                out.extend(long_link_blocks(kind::GNU_LONGNAME, &m.name, m));
            }
            // O oldgnu guarda no máximo 99 bytes do nome, com o NUL no fim; o alvo usa o campo inteiro.
            header::put_bytes(&mut b, (header::NAME.0, max), &m.name);
            header::put_bytes(&mut b, header::LINKNAME, &m.linkname);
        }
        Format::Ustar => {
            let (p, n) = ustar_split(&m.name).ok_or(HeaderError::CannotSplit)?;
            if p > 0 {
                header::put_bytes(&mut b, header::PREFIX, &m.name[..p]);
                header::put_bytes(&mut b, header::NAME, &m.name[n..]);
            } else {
                header::put_bytes(&mut b, header::NAME, &m.name);
            }
            // Alvo longo demais: o GNU avisa (quem chama) mas grava o membro com o alvo cortado.
            header::put_bytes(&mut b, header::LINKNAME, &m.linkname);
        }
        Format::V7 => {
            if m.name.len() > 100 {
                return Err(HeaderError::TooLong(99));
            }
            header::put_bytes(&mut b, header::NAME, &m.name);
            header::put_bytes(&mut b, header::LINKNAME, &m.linkname);
        }
        Format::Pax => {
            // O GNU não usa o prefixo do ustar no pax: nome longo vai inteiro no registro `path`.
            if m.name.len() > 100 {
                pax.push(rec(b"path", &m.name));
            }
            header::put_bytes(&mut b, header::NAME, &m.name);
            if m.linkname.len() > 100 {
                pax.push(rec(b"linkpath", &m.linkname));
            }
            header::put_bytes(&mut b, header::LINKNAME, &m.linkname);
        }
    }
    header::put_octal(&mut b, header::MODE, (m.mode & 0o7777) as u64);
    let size = if matches!(m.typeflag, kind::LNK | kind::SYM | kind::DIR | kind::CHR | kind::BLK | kind::FIFO) {
        0
    } else {
        m.size
    };
    if fmt == Format::Pax {
        // No pax o que não cabe vai pro cabeçalho estendido e o campo leva o que der.
        for (field, key, v) in [(header::UID, &b"uid"[..], m.uid as i128), (header::GID, &b"gid"[..], m.gid as i128)] {
            if v < 0 || v as u128 > header::octal_max(field.1) as u128 {
                pax.push(rec(key, v.to_string().as_bytes()));
                header::put_octal(&mut b, field, 0);
            } else {
                header::put_octal(&mut b, field, v as u64);
            }
        }
        if size > header::octal_max(header::SIZE.1) {
            pax.push(rec(b"size", size.to_string().as_bytes()));
            header::put_octal(&mut b, header::SIZE, 0);
        } else {
            header::put_octal(&mut b, header::SIZE, size);
        }
        let mt = m.mtime;
        if mt.nsec != 0 || mt.sec < 0 || mt.sec as u64 > header::octal_max(12) {
            pax.push(rec(b"mtime", pax_time(mt).as_bytes()));
        }
        let clamp = mt.sec.clamp(0, header::octal_max(12) as i64) as u64;
        header::put_octal(&mut b, header::MTIME, clamp);
        if let Some(a) = m.atime {
            pax.push(rec(b"atime", pax_time(a).as_bytes()));
        }
        if let Some(c) = m.ctime {
            pax.push(rec(b"ctime", pax_time(c).as_bytes()));
        }
        for (key, v, field) in [(&b"uname"[..], &m.uname, header::UNAME), (&b"gname"[..], &m.gname, header::GNAME)] {
            if v.len() > 31 || !v.is_ascii() {
                pax.push(rec(key, v));
            }
            header::put_bytes(&mut b, field, v);
        }
        if !m.name.is_ascii() && !pax.iter().any(|(k, _)| k == b"path") {
            // O GNU guarda em UTF-8 o nome com bytes fora do ASCII.
            pax.insert(0, rec(b"path", &m.name));
        }
        if !m.linkname.is_ascii() && !m.linkname.is_empty() && !pax.iter().any(|(k, _)| k == b"linkpath") {
            pax.push(rec(b"linkpath", &m.linkname));
        }
    } else {
        put_num(&mut b, header::UID, m.uid as i128, fmt, "uid_t")?;
        put_num(&mut b, header::GID, m.gid as i128, fmt, "gid_t")?;
        put_num(&mut b, header::SIZE, size as i128, fmt, "off_t")?;
        put_num(&mut b, header::MTIME, m.mtime.sec as i128, fmt, "time_t")?;
        if fmt != Format::V7 {
            header::put_bytes(&mut b, header::UNAME, &m.uname);
            header::put_bytes(&mut b, header::GNAME, &m.gname);
        }
    }
    if is_dev {
        header::put_octal(&mut b, header::DEVMAJOR, m.devmajor as u64);
        header::put_octal(&mut b, header::DEVMINOR, m.devminor as u64);
    }
    let mut records: Vec<u8> = Vec::new();
    if fmt == Format::Pax {
        for (k, v) in pax.iter().filter(|(k, _)| !cfg.deleted(k)).chain(cfg.extra.iter()) {
            records.extend(pax_record(k, v));
        }
    }
    let pax = records;
    if !pax.is_empty() {
        let mut x = base_header(&Member { typeflag: kind::XHD, ..Member::default() }, Format::Pax);
        let xname = pax_header_name(&m.name, cfg.exthdr_name.as_deref());
        header::put_bytes(&mut x, header::NAME, &xname);
        header::put_octal(&mut x, header::MODE, 0o644);
        header::put_octal(&mut x, header::UID, 0);
        header::put_octal(&mut x, header::GID, 0);
        header::put_octal(&mut x, header::SIZE, pax.len() as u64);
        header::put_octal(&mut x, header::MTIME, m.mtime.sec.clamp(0, header::octal_max(12) as i64) as u64);
        header::put_checksum(&mut x);
        out.extend_from_slice(&x);
        out.extend_from_slice(&pax);
        let pad = (512 - pax.len() % 512) % 512;
        out.resize(out.len() + pad, 0);
    }
    header::put_checksum(&mut b);
    out.extend_from_slice(&b);
    Ok(out)
}

/// `././@LongLink` com o nome (ou alvo) completo, como o GNU grava (modo 644, dono 0, mtime 0, nome
/// do usuário e do grupo do membro).
fn long_link_blocks(typeflag: u8, value: &[u8], m: &Member) -> Vec<u8> {
    let mut b = header::zero_block();
    header::put_bytes(&mut b, header::NAME, b"././@LongLink");
    header::put_octal(&mut b, header::MODE, 0o644);
    header::put_octal(&mut b, header::UID, 0);
    header::put_octal(&mut b, header::GID, 0);
    header::put_octal(&mut b, header::SIZE, value.len() as u64 + 1);
    header::put_octal(&mut b, header::MTIME, 0);
    b[header::TYPEFLAG] = typeflag;
    header::put_bytes(&mut b, (257, 8), b"ustar  \0");
    header::put_bytes(&mut b, header::UNAME, &m.uname);
    header::put_bytes(&mut b, header::GNAME, &m.gname);
    header::put_checksum(&mut b);
    let mut out = b.to_vec();
    let mut data = value.to_vec();
    data.push(0);
    let pad = (512 - data.len() % 512) % 512;
    data.resize(data.len() + pad, 0);
    out.extend_from_slice(&data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pax_record_length_counts_itself() {
        assert_eq!(pax_record(b"atime", b"1790996485.347498756"), b"30 atime=1790996485.347498756\n");
        assert_eq!(pax_record(b"mtime", b"1768478400.5"), b"22 mtime=1768478400.5\n");
        let r = pax_record(b"a", &[b'x'; 95]);
        assert_eq!(r.len(), 100);
        assert!(r.starts_with(b"100 "));
    }

    #[test]
    fn ustar_split_prefers_longest_prefix() {
        let name = [b"a".repeat(40), b"b".repeat(40), b"c".repeat(40), b"d".repeat(40)].join(&b'/');
        let (p, n) = ustar_split(&name).unwrap();
        assert_eq!(&name[n..], &b"d".repeat(40)[..]);
        assert_eq!(p, 122);
        assert!(ustar_split(&b"x".repeat(101)).is_none());
    }

    #[test]
    fn pax_header_names() {
        assert_eq!(pax_header_name(b"d/", None), b"./PaxHeaders/d");
        assert_eq!(pax_header_name(b"d/a.txt", None), b"d/PaxHeaders/a.txt");
        assert_eq!(pax_header_name(b"d/a.txt", Some(b"%d/X/%f%%")), b"d/X/a.txt%");
        assert_eq!(pax_time(Time::new(1_768_478_400, 500_000_000)), "1768478400.5");
    }
}
