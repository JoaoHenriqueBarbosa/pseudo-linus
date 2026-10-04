//! Codecs de compressão, todos em Rust puro (decisão do F08/H31): deflate/gzip pelo `flate2` com o
//! backend `zlib-rs`, bzip2 pelo `bzip2` (libbz2-rs-sys), xz, lzma e lzip pelo `lzma-rust2`, zstd pelo
//! `structured-zstd`.
//!
//! O enquadramento gzip (cabeçalho, membros concatenados, CRC e ISIZE, sobra no fim) e o laço de
//! fluxos do bzip2 são nossos, sobre a API crua das crates, porque os CLIs do GNU distinguem cada caso
//! numa mensagem diferente e precisam saber exatamente onde a decodificação parou.
//!
//! Duas formas de uso: inteira na memória ([`decompress`], [`compress`]) e em fluxo ([`Encoder`] e
//! [`decoder`]).

use std::io::{self, Cursor, Read, Write};

/// Formato de compressão.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    Gzip,
    Bzip2,
    Xz,
    Lzma,
    Lzip,
    Zstd,
}

impl Format {
    pub const ALL: [Format; 6] = [Format::Gzip, Format::Bzip2, Format::Xz, Format::Lzma, Format::Lzip, Format::Zstd];

    /// Nome do formato (e do programa GNU que o trata, menos lzma, que é o `xz --format=lzma`).
    pub fn label(self) -> &'static str {
        match self {
            Format::Gzip => "gzip",
            Format::Bzip2 => "bzip2",
            Format::Xz => "xz",
            Format::Lzma => "lzma",
            Format::Lzip => "lzip",
            Format::Zstd => "zstd",
        }
    }

    /// Sufixo padrão do arquivo comprimido.
    pub fn suffix(self) -> &'static str {
        match self {
            Format::Gzip => ".gz",
            Format::Bzip2 => ".bz2",
            Format::Xz => ".xz",
            Format::Lzma => ".lzma",
            Format::Lzip => ".lz",
            Format::Zstd => ".zst",
        }
    }

    /// Nível padrão da ferramenta GNU.
    pub fn default_level(self) -> u32 {
        match self {
            Format::Gzip => 6,
            Format::Bzip2 => 9,
            Format::Xz | Format::Lzma | Format::Lzip => 6,
            Format::Zstd => 3,
        }
    }

    /// Reconhece o formato pela magia, como o `tar` faz na leitura (`.lzma` não tem magia: o GNU tar
    /// reconhece pelos três primeiros bytes do cabeçalho do preset padrão, `5d 00 00`).
    pub fn sniff(data: &[u8]) -> Option<Format> {
        if data.starts_with(&[0x1f, 0x8b]) {
            Some(Format::Gzip)
        } else if data.starts_with(b"BZh") {
            Some(Format::Bzip2)
        } else if data.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0]) {
            Some(Format::Xz)
        } else if data.starts_with(b"LZIP") {
            Some(Format::Lzip)
        } else if data.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
            Some(Format::Zstd)
        } else if data.starts_with(&[0x5d, 0, 0]) {
            Some(Format::Lzma)
        } else {
            None
        }
    }
}

/// Por que a decodificação falhou, nas categorias que as ferramentas GNU distinguem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// A entrada não começa com a magia do formato.
    NotFormat,
    /// A entrada acabou no meio do fluxo.
    Truncated,
    /// CRC (ou outra verificação de integridade) não confere.
    Checksum,
    /// O tamanho guardado no fim não confere (gzip: ISIZE).
    Length,
    /// Dados inválidos; a mensagem é a da crate, pra diagnóstico.
    Corrupt(String),
    /// Erro de leitura da fonte (no modo em fluxo).
    Io(io::ErrorKind),
}

/// O que veio depois do último fluxo válido.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Trailing {
    /// Nada.
    #[default]
    None,
    /// Só bytes zero (o gzip avisa "trailing zero bytes ignored").
    Zeros,
    /// Outra coisa, a partir do deslocamento dado.
    Garbage(usize),
}

/// Resultado de uma decodificação inteira.
#[derive(Clone, Debug, Default)]
pub struct Decoded {
    pub data: Vec<u8>,
    /// Quantos fluxos (membros, no gzip) foram lidos.
    pub streams: usize,
    pub trailing: Trailing,
}

/// Cabeçalho gzip a gravar (RFC 1952). O padrão é o que o `gzip -n` grava: sem nome, mtime 0, SO 3.
#[derive(Clone, Debug, Default)]
pub struct GzipHeader {
    pub mtime: u32,
    pub name: Option<Vec<u8>>,
}

/// Cabeçalho gzip lido.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GzipHeaderInfo {
    pub method: u8,
    pub flags: u8,
    pub mtime: u32,
    pub xfl: u8,
    pub os: u8,
    pub name: Option<Vec<u8>>,
    pub comment: Option<Vec<u8>>,
    /// Tamanho do cabeçalho em bytes.
    pub len: usize,
}

/// Lê um cabeçalho gzip do começo de `d`.
pub fn gzip_header(d: &[u8]) -> Result<GzipHeaderInfo, DecodeError> {
    if d.len() < 2 || d[0] != 0x1f || d[1] != 0x8b {
        return Err(DecodeError::NotFormat);
    }
    if d.len() < 10 {
        return Err(DecodeError::Truncated);
    }
    if d[2] != 8 {
        return Err(DecodeError::Corrupt(format!("unknown method {}", d[2])));
    }
    let flags = d[3];
    let mut h = GzipHeaderInfo {
        method: d[2],
        flags,
        mtime: u32::from_le_bytes([d[4], d[5], d[6], d[7]]),
        xfl: d[8],
        os: d[9],
        ..GzipHeaderInfo::default()
    };
    let mut p = 10usize;
    if flags & 0x04 != 0 {
        let x = d.get(p..p + 2).ok_or(DecodeError::Truncated)?;
        p += 2 + u16::from_le_bytes([x[0], x[1]]) as usize;
        if p > d.len() {
            return Err(DecodeError::Truncated);
        }
    }
    for (bit, slot) in [(0x08u8, 0), (0x10, 1)] {
        if flags & bit != 0 {
            let rest = d.get(p..).ok_or(DecodeError::Truncated)?;
            let z = rest.iter().position(|&b| b == 0).ok_or(DecodeError::Truncated)?;
            let s = Some(rest[..z].to_vec());
            if slot == 0 {
                h.name = s;
            } else {
                h.comment = s;
            }
            p += z + 1;
        }
    }
    if flags & 0x02 != 0 {
        p += 2;
    }
    if p > d.len() {
        return Err(DecodeError::Truncated);
    }
    h.len = p;
    Ok(h)
}

/// Bytes de cabeçalho gzip como o GNU gzip 1.13 grava: XFL 2 no nível 9, 4 no nível 1, SO 3 (Unix).
pub fn gzip_header_bytes(h: &GzipHeader, level: u32) -> Vec<u8> {
    let flags = if h.name.is_some() { 0x08 } else { 0 };
    let xfl = match level {
        9 => 2,
        1 => 4,
        _ => 0,
    };
    let mut out = vec![0x1f, 0x8b, 8, flags];
    out.extend_from_slice(&h.mtime.to_le_bytes());
    out.push(xfl);
    out.push(3);
    if let Some(n) = &h.name {
        out.extend_from_slice(n);
        out.push(0);
    }
    out
}

fn gzip_decode(input: &[u8]) -> Result<Decoded, DecodeError> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut streams = 0usize;
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        if streams > 0 {
            if pos >= input.len() {
                break;
            }
            let rest = &input[pos..];
            if !rest.starts_with(&[0x1f, 0x8b]) {
                let trailing =
                    if rest.iter().all(|&b| b == 0) { Trailing::Zeros } else { Trailing::Garbage(pos) };
                return Ok(Decoded { data: out, streams, trailing });
            }
        }
        let h = gzip_header(&input[pos..])?;
        pos += h.len;
        let start = out.len();
        let mut inf = flate2::Decompress::new(false);
        loop {
            sysabi::sys::checkpoint();
            let (in0, out0) = (inf.total_in(), inf.total_out());
            let st = inf
                .decompress(&input[pos..], &mut chunk, flate2::FlushDecompress::None)
                .map_err(|e| DecodeError::Corrupt(e.to_string()))?;
            let consumed = (inf.total_in() - in0) as usize;
            let produced = (inf.total_out() - out0) as usize;
            pos += consumed;
            if out.try_reserve(produced).is_err() {
                return Err(DecodeError::Corrupt("out of memory".into()));
            }
            out.extend_from_slice(&chunk[..produced]);
            if st == flate2::Status::StreamEnd {
                break;
            }
            if consumed == 0 && produced == 0 {
                return Err(DecodeError::Truncated);
            }
        }
        let t = input.get(pos..pos + 8).ok_or(DecodeError::Truncated)?;
        let crc = u32::from_le_bytes([t[0], t[1], t[2], t[3]]);
        let isize = u32::from_le_bytes([t[4], t[5], t[6], t[7]]);
        pos += 8;
        if crc != crc32fast::hash(&out[start..]) {
            return Err(DecodeError::Checksum);
        }
        if isize != (out.len() - start) as u32 {
            return Err(DecodeError::Length);
        }
        streams += 1;
    }
    Ok(Decoded { data: out, streams, trailing: Trailing::None })
}

fn bzip2_decode(input: &[u8]) -> Result<Decoded, DecodeError> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut streams = 0usize;
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        if streams > 0 {
            if pos >= input.len() {
                break;
            }
            if !input[pos..].starts_with(b"BZh") {
                return Ok(Decoded { data: out, streams, trailing: Trailing::Garbage(pos) });
            }
        } else if !input.starts_with(b"BZh") {
            return Err(DecodeError::NotFormat);
        }
        let mut dec = bzip2::Decompress::new(false);
        loop {
            sysabi::sys::checkpoint();
            let (in0, out0) = (dec.total_in(), dec.total_out());
            let st = dec.decompress(&input[pos..], &mut chunk).map_err(|e| match e {
                bzip2::Error::DataMagic => DecodeError::NotFormat,
                bzip2::Error::Data => DecodeError::Checksum,
                other => DecodeError::Corrupt(format!("{other:?}")),
            })?;
            let consumed = (dec.total_in() - in0) as usize;
            let produced = (dec.total_out() - out0) as usize;
            pos += consumed;
            if out.try_reserve(produced).is_err() {
                return Err(DecodeError::Corrupt("out of memory".into()));
            }
            out.extend_from_slice(&chunk[..produced]);
            if st == bzip2::Status::StreamEnd {
                break;
            }
            if consumed == 0 && produced == 0 {
                return Err(DecodeError::Truncated);
            }
        }
        streams += 1;
    }
    Ok(Decoded { data: out, streams, trailing: Trailing::None })
}

/// Classifica um erro de leitor de crate pelo `ErrorKind` e, na falta, pelo texto.
pub fn classify_io(e: &io::Error) -> DecodeError {
    if e.kind() == io::ErrorKind::UnexpectedEof {
        return DecodeError::Truncated;
    }
    let msg = e.to_string();
    let low = msg.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| low.contains(n));
    if has(&["checksum", "crc", "check mismatch", "hash mismatch"]) {
        DecodeError::Checksum
    } else if has(&["eof", "end of", "truncat", "premature", "unexpected end", "not enough", "too short", "incomplete"])
    {
        DecodeError::Truncated
    } else if has(&["magic", "not a", "unknown format", "bad header", "invalid header"]) {
        DecodeError::NotFormat
    } else {
        DecodeError::Corrupt(msg)
    }
}

fn read_all(mut r: impl Read) -> Result<Vec<u8>, DecodeError> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        sysabi::sys::checkpoint();
        match r.read(&mut buf) {
            Ok(0) => return Ok(out),
            Ok(n) => {
                if out.try_reserve(n).is_err() {
                    return Err(DecodeError::Corrupt("out of memory".into()));
                }
                out.extend_from_slice(&buf[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(classify_io(&e)),
        }
    }
}

fn zstd_decode(input: &[u8]) -> Result<Decoded, DecodeError> {
    if !input.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) && !is_zstd_skippable(input) {
        return Err(DecodeError::NotFormat);
    }
    let mut rest = input;
    let mut out = Vec::new();
    let mut streams = 0usize;
    while !rest.is_empty() {
        if is_zstd_skippable(rest) {
            let len = u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]) as usize;
            let total = 8usize.saturating_add(len);
            if rest.len() < total {
                return Err(DecodeError::Truncated);
            }
            rest = &rest[total..];
            continue;
        }
        if !rest.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
            let pos = input.len() - rest.len();
            return Ok(Decoded { data: out, streams, trailing: Trailing::Garbage(pos) });
        }
        let mut cursor = Cursor::new(rest);
        let dec = structured_zstd::decoding::StreamingDecoder::new(&mut cursor).map_err(|e| {
            let m = e.to_string();
            if m.to_lowercase().contains("not enough") || m.to_lowercase().contains("eof") {
                DecodeError::Truncated
            } else {
                DecodeError::Corrupt(m)
            }
        })?;
        let frame = read_all(dec)?;
        out.extend_from_slice(&frame);
        let used = cursor.position() as usize;
        if used == 0 {
            return Err(DecodeError::Truncated);
        }
        rest = &rest[used..];
        streams += 1;
    }
    Ok(Decoded { data: out, streams, trailing: Trailing::None })
}

fn is_zstd_skippable(d: &[u8]) -> bool {
    d.len() >= 8 && (d[0] & 0xf0) == 0x50 && d[1..4] == [0x2a, 0x4d, 0x18]
}

/// Decodifica um fluxo inteiro (vários fluxos concatenados, como as ferramentas GNU aceitam).
pub fn decompress(fmt: Format, input: &[u8]) -> Result<Decoded, DecodeError> {
    match fmt {
        Format::Gzip => gzip_decode(input),
        Format::Bzip2 => bzip2_decode(input),
        Format::Xz => {
            if !input.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0]) {
                return Err(DecodeError::NotFormat);
            }
            let data = read_all(lzma_rust2::XzReader::new(Cursor::new(input), true))?;
            Ok(Decoded { data, streams: 1, trailing: Trailing::None })
        }
        Format::Lzma => {
            let r = lzma_rust2::LzmaReader::new_mem_limit(Cursor::new(input), u32::MAX, None)
                .map_err(|e| classify_io(&e))?;
            let data = read_all(r)?;
            Ok(Decoded { data, streams: 1, trailing: Trailing::None })
        }
        Format::Lzip => {
            if !input.starts_with(b"LZIP") {
                return Err(DecodeError::NotFormat);
            }
            let data = read_all(lzma_rust2::LzipReader::new(Cursor::new(input)))?;
            Ok(Decoded { data, streams: 1, trailing: Trailing::None })
        }
        Format::Zstd => zstd_decode(input),
    }
}

/// Compressor em fluxo. `finish` fecha o fluxo (rodapé, checksum) e devolve o escritor de baixo.
pub struct Encoder<W: Write> {
    kind: EncoderKind<W>,
}

/// Deflate cru (RFC 1951) em fluxo, com CRC-32 e contagem da entrada, pro gzip e pro zip.
///
/// A compressão é do `miniz_oxide`: medido contra o gzip 1.13 no corpus do F08, ele fica com saída do
/// mesmo tamanho (log de texto: 1,02x no nível 1, 0,95x no 6; dados binários: 0,92x a 1,01x) e guarda
/// em bloco sem compressão o que não comprime, enquanto o `zlib-rs` (estratégias do zlib-ng) sai até
/// 31% maior nos níveis 1 a 6 e expande dado aleatório em 5% no nível 1. A descompressão continua no
/// `zlib-rs`, que é mais rápido.
pub struct RawDeflate<W: Write> {
    w: W,
    c: Box<miniz_oxide::deflate::core::CompressorOxide>,
    buf: Vec<u8>,
    crc: crc32fast::Hasher,
    size: u64,
    /// Bytes comprimidos escritos até agora.
    pub written: u64,
}

impl<W: Write> RawDeflate<W> {
    /// Nível 0 grava só blocos sem compressão; 1 a 9 como no gzip.
    pub fn new(level: u32, w: W) -> RawDeflate<W> {
        let flags = miniz_oxide::deflate::core::create_comp_flags_from_zip_params(level.min(10) as i32, -15, 0);
        RawDeflate {
            w,
            c: Box::new(miniz_oxide::deflate::core::CompressorOxide::new(flags)),
            buf: vec![0u8; 64 * 1024],
            crc: crc32fast::Hasher::new(),
            size: 0,
            written: 0,
        }
    }

    fn run(&mut self, mut input: &[u8], finish: bool) -> io::Result<()> {
        use miniz_oxide::deflate::core::{TDEFLFlush, TDEFLStatus, compress};
        let flush = if finish { TDEFLFlush::Finish } else { TDEFLFlush::None };
        loop {
            sysabi::sys::checkpoint();
            let (st, consumed, produced) = compress(&mut self.c, input, &mut self.buf, flush);
            if produced > 0 {
                self.w.write_all(&self.buf[..produced])?;
                self.written += produced as u64;
            }
            input = &input[consumed..];
            match st {
                TDEFLStatus::Done => return Ok(()),
                TDEFLStatus::Okay => {
                    if !finish && input.is_empty() && produced < self.buf.len() {
                        return Ok(());
                    }
                }
                TDEFLStatus::BadParam | TDEFLStatus::PutBufFailed => {
                    return Err(io::Error::other(format!("deflate: {st:?}")));
                }
            }
        }
    }

    /// Fecha o fluxo deflate. Devolve o escritor, o CRC-32 e o tamanho da entrada.
    pub fn finish(mut self) -> io::Result<(W, u32, u64)> {
        self.run(&[], true)?;
        Ok((self.w, self.crc.finalize(), self.size))
    }
}

impl<W: Write> Write for RawDeflate<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.crc.update(data);
        self.size += data.len() as u64;
        self.run(data, false)?;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

enum EncoderKind<W: Write> {
    Gzip(RawDeflate<W>),
    Bzip2(bzip2::write::BzEncoder<W>),
    Xz(lzma_rust2::XzWriter<W>),
    Lzma(lzma_rust2::LzmaWriter<W>),
    Lzip(lzma_rust2::LzipWriter<W>),
    Zstd(structured_zstd::encoding::StreamingEncoder<W>),
}

impl<W: Write> Encoder<W> {
    /// Compressor do formato `fmt` no nível dado. Pro gzip, `gz` define o cabeçalho.
    pub fn new(fmt: Format, level: u32, gz: &GzipHeader, mut w: W) -> io::Result<Encoder<W>> {
        let kind = match fmt {
            Format::Gzip => {
                w.write_all(&gzip_header_bytes(gz, level))?;
                EncoderKind::Gzip(RawDeflate::new(level, w))
            }
            Format::Bzip2 => EncoderKind::Bzip2(bzip2::write::BzEncoder::new(w, bzip2::Compression::new(level.clamp(1, 9)))),
            Format::Xz => EncoderKind::Xz(
                lzma_rust2::XzWriter::new(w, lzma_rust2::XzOptions::with_preset(level.min(9))).map_err(io::Error::other)?,
            ),
            Format::Lzma => {
                let opts = lzma_rust2::LzmaOptions::with_preset(level.min(9));
                // Como o `xz --format=lzma`: tamanho desconhecido no cabeçalho e marcador de fim.
                EncoderKind::Lzma(lzma_rust2::LzmaWriter::new_use_header(w, &opts, None).map_err(io::Error::other)?)
            }
            Format::Lzip => EncoderKind::Lzip(lzma_rust2::LzipWriter::new(w, lzma_rust2::LzipOptions::with_preset(level.min(9)))),
            Format::Zstd => EncoderKind::Zstd(structured_zstd::encoding::StreamingEncoder::new(
                w,
                structured_zstd::encoding::CompressionLevel::from_level(level as i32),
            )),
        };
        Ok(Encoder { kind })
    }

    /// Fecha o fluxo e devolve o escritor.
    pub fn finish(self) -> io::Result<W> {
        match self.kind {
            EncoderKind::Gzip(d) => {
                let (mut w, crc, size) = d.finish()?;
                w.write_all(&crc.to_le_bytes())?;
                w.write_all(&(size as u32).to_le_bytes())?;
                Ok(w)
            }
            EncoderKind::Bzip2(e) => e.finish(),
            EncoderKind::Xz(e) => e.finish().map_err(io::Error::other),
            EncoderKind::Lzma(e) => e.finish().map_err(io::Error::other),
            EncoderKind::Lzip(e) => e.finish().map_err(io::Error::other),
            EncoderKind::Zstd(e) => e.finish().map_err(|e| io::Error::other(format!("{e:?}"))),
        }
    }
}

impl<W: Write> Write for Encoder<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        match &mut self.kind {
            EncoderKind::Gzip(d) => d.write(data),
            EncoderKind::Bzip2(e) => e.write(data),
            EncoderKind::Xz(e) => e.write(data),
            EncoderKind::Lzma(e) => e.write(data),
            EncoderKind::Lzip(e) => e.write(data),
            EncoderKind::Zstd(e) => e.write(data),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Comprime tudo de uma vez.
pub fn compress(fmt: Format, data: &[u8], level: u32, gz: &GzipHeader) -> io::Result<Vec<u8>> {
    let mut e = Encoder::new(fmt, level, gz, Vec::new())?;
    for chunk in data.chunks(256 * 1024) {
        e.write_all(chunk)?;
    }
    e.finish()
}

/// Descompressor em fluxo sobre um leitor (pro `tar` com arquivos grandes). Os erros saem como
/// `io::Error`; [`classify_io`] devolve a categoria.
pub fn decoder<'a, R: Read + 'a>(fmt: Format, r: R) -> io::Result<Box<dyn Read + 'a>> {
    Ok(match fmt {
        Format::Gzip => Box::new(flate2::read::MultiGzDecoder::new(r)),
        Format::Bzip2 => Box::new(bzip2::read::MultiBzDecoder::new(r)),
        Format::Xz => Box::new(lzma_rust2::XzReader::new(r, true)),
        Format::Lzma => Box::new(lzma_rust2::LzmaReader::new_mem_limit(r, u32::MAX, None)?),
        Format::Lzip => Box::new(lzma_rust2::LzipReader::new(r)),
        Format::Zstd => Box::new(ZstdFrames { src: r, frame: Vec::new(), off: 0, done: false }),
    })
}

/// Leitor de zstd com frames concatenados (e frames puláveis), um frame decodificado por vez.
struct ZstdFrames<R: Read> {
    src: R,
    frame: Vec<u8>,
    off: usize,
    done: bool,
}

fn read_full(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        match r.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(got)
}

impl<R: Read> ZstdFrames<R> {
    /// Decodifica o próximo frame pra `self.frame`; `false` no fim da fonte.
    fn next_frame(&mut self) -> io::Result<bool> {
        loop {
            let mut magic = [0u8; 4];
            let got = read_full(&mut self.src, &mut magic)?;
            if got == 0 {
                return Ok(false);
            }
            if got < 4 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "premature end"));
            }
            if magic[0] & 0xf0 == 0x50 && magic[1..] == [0x2a, 0x4d, 0x18] {
                let mut len = [0u8; 4];
                if read_full(&mut self.src, &mut len)? < 4 {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "premature end"));
                }
                let n = u32::from_le_bytes(len) as u64;
                let skipped = io::copy(&mut (&mut self.src).take(n), &mut io::sink())?;
                if skipped < n {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "premature end"));
                }
                continue;
            }
            if magic != [0x28, 0xb5, 0x2f, 0xfd] {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "unknown frame descriptor"));
            }
            let mut chained = Cursor::new(magic).chain(&mut self.src);
            let mut dec = structured_zstd::decoding::StreamingDecoder::new(&mut chained)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            self.frame.clear();
            self.off = 0;
            dec.read_to_end(&mut self.frame)?;
            return Ok(true);
        }
    }
}

impl<R: Read> Read for ZstdFrames<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.off >= self.frame.len() {
            if self.done || buf.is_empty() {
                return Ok(0);
            }
            if !self.next_frame()? {
                self.done = true;
                return Ok(0);
            }
        }
        let n = (self.frame.len() - self.off).min(buf.len());
        buf[..n].copy_from_slice(&self.frame[self.off..self.off + n]);
        self.off += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut v = Vec::new();
        for i in 0..5000 {
            v.extend_from_slice(format!("line {i} {}\n", i % 17).as_bytes());
        }
        v
    }

    #[test]
    fn roundtrip_every_format() {
        let data = sample();
        for fmt in Format::ALL {
            let enc = compress(fmt, &data, fmt.default_level(), &GzipHeader::default()).unwrap();
            let dec = decompress(fmt, &enc).unwrap_or_else(|e| panic!("{fmt:?}: {e:?}"));
            assert_eq!(dec.data, data, "{fmt:?}");
            assert_eq!(Format::sniff(&enc), Some(fmt), "{fmt:?}");
        }
    }

    #[test]
    fn gzip_errors_and_trailing() {
        let data = b"hello world\n".repeat(50);
        let enc = compress(Format::Gzip, &data, 6, &GzipHeader::default()).unwrap();
        let n = enc.len();
        assert_eq!(decompress(Format::Gzip, &enc[..n - 3]).unwrap_err(), DecodeError::Truncated);
        let mut bad = enc.clone();
        bad[n - 6] ^= 0xff;
        assert_eq!(decompress(Format::Gzip, &bad).unwrap_err(), DecodeError::Checksum);
        assert_eq!(decompress(Format::Gzip, b"plain").unwrap_err(), DecodeError::NotFormat);
        let mut two = enc.clone();
        two.extend_from_slice(&enc);
        assert_eq!(decompress(Format::Gzip, &two).unwrap().streams, 2);
        let mut zeros = enc.clone();
        zeros.extend_from_slice(&[0, 0, 0]);
        assert_eq!(decompress(Format::Gzip, &zeros).unwrap().trailing, Trailing::Zeros);
        let mut junk = enc.clone();
        junk.extend_from_slice(b"junk");
        assert_eq!(decompress(Format::Gzip, &junk).unwrap().trailing, Trailing::Garbage(n));
    }

    #[test]
    fn gzip_header_like_gnu() {
        let h = GzipHeader { mtime: 0x01020304, name: Some(b"a.txt".to_vec()) };
        let b = gzip_header_bytes(&h, 9);
        assert_eq!(&b[..10], &[0x1f, 0x8b, 8, 8, 4, 3, 2, 1, 2, 3]);
        let info = gzip_header(&b).unwrap();
        assert_eq!(info.name.as_deref(), Some(&b"a.txt"[..]));
        assert_eq!(info.len, b.len());
    }
}
