//! Codecs candidatos atrás de uma interface comum (codificar num nível, decodificar com erro classificado).
//!
//! A classificação do erro (truncado, checksum, formato) é o que o CLI precisa pra imitar as mensagens
//! do GNU; ela sai das crates por `io::ErrorKind` quando existe e por texto da mensagem quando não.

use std::io::{Read, Write};

use anyhow::{Result, anyhow, bail};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
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

    pub fn ext(self) -> &'static str {
        match self {
            Format::Gzip => "gz",
            Format::Bzip2 => "bz2",
            Format::Xz => "xz",
            Format::Lzma => "lzma",
            Format::Lzip => "lz",
            Format::Zstd => "zst",
        }
    }

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

    /// Comando GNU que descomprime pra stdout.
    pub fn gnu_decode(self) -> &'static str {
        match self {
            Format::Gzip => "gzip -dc",
            Format::Bzip2 => "bzip2 -dc",
            Format::Xz => "xz -dc",
            Format::Lzma => "xz --format=lzma -dc",
            Format::Lzip => "lzip -dc",
            Format::Zstd => "zstd -q -dc",
        }
    }

    /// Comando GNU de integridade (`-t`).
    pub fn gnu_test(self) -> &'static str {
        match self {
            Format::Gzip => "gzip -t",
            Format::Bzip2 => "bzip2 -t",
            Format::Xz => "xz -t",
            Format::Lzma => "xz --format=lzma -t",
            Format::Lzip => "lzip -t",
            Format::Zstd => "zstd -q -t",
        }
    }

    /// Nível padrão da ferramenta GNU (o que `gzip arquivo` usa sem flag).
    pub fn gnu_default_level(self) -> i32 {
        match self {
            Format::Gzip => 6,
            Format::Bzip2 => 9,
            Format::Xz | Format::Lzma | Format::Lzip => 6,
            Format::Zstd => 3,
        }
    }
}

/// Erro de decodificação classificado como as ferramentas GNU classificam.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "lowercase")]
pub enum DecodeError {
    Truncated(String),
    Checksum(String),
    Format(String),
    Other(String),
}

impl DecodeError {
    pub fn message(&self) -> &str {
        match self {
            DecodeError::Truncated(m) | DecodeError::Checksum(m) | DecodeError::Format(m) | DecodeError::Other(m) => m,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            DecodeError::Truncated(_) => "truncated",
            DecodeError::Checksum(_) => "checksum",
            DecodeError::Format(_) => "format",
            DecodeError::Other(_) => "other",
        }
    }
}

/// Classifica um `io::Error` pelo `ErrorKind` e, na falta, pelo texto.
pub fn classify_io(e: &std::io::Error) -> DecodeError {
    let msg = e.to_string();
    if e.kind() == std::io::ErrorKind::UnexpectedEof {
        return DecodeError::Truncated(msg);
    }
    classify_text(&msg)
}

pub fn classify_text(msg: &str) -> DecodeError {
    let low = msg.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| low.contains(n));
    if has(&["checksum", "crc", "check mismatch", "hash mismatch"]) {
        DecodeError::Checksum(msg.to_string())
    } else if has(&["eof", "end of", "truncat", "premature", "unexpected end", "not enough", "too short", "incomplete"]) {
        DecodeError::Truncated(msg.to_string())
    } else {
        DecodeError::Format(msg.to_string())
    }
}

/// Um codec candidato.
pub trait Codec: Send + Sync {
    /// Chave estável (ex.: "flate2-gzip").
    fn id(&self) -> &'static str;
    /// Candidato (crate) a que o codec pertence, igual ao `name` do `CandidateResult`.
    fn candidate(&self) -> &'static str;
    fn format(&self) -> Format;
    /// Níveis suportados na codificação (vazio quando a crate só decodifica).
    fn levels(&self) -> &'static [i32];
    /// Nível usado na comparação com o GNU (o padrão do GNU quando a crate suporta).
    fn bench_level(&self) -> Option<i32>;
    fn encode(&self, data: &[u8], level: i32) -> Result<Vec<u8>>;
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>, DecodeError>;
}

/// Roda `f` convertendo panic em erro (algumas crates entram em panic com entrada inválida).
pub fn guard<T>(f: impl FnOnce() -> T) -> std::result::Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|p| {
        p.downcast_ref::<String>()
            .cloned()
            .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "panic sem mensagem".into())
    })
}

pub fn encode_guarded(codec: &dyn Codec, data: &[u8], level: i32) -> Result<Vec<u8>> {
    match guard(|| codec.encode(data, level)) {
        Ok(r) => r,
        Err(p) => Err(anyhow!("panic: {p}")),
    }
}

pub fn decode_guarded(codec: &dyn Codec, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
    match guard(|| codec.decode(data)) {
        Ok(r) => r,
        Err(p) => Err(DecodeError::Other(format!("panic: {p}"))),
    }
}

fn read_all(mut r: impl Read) -> Result<Vec<u8>, DecodeError> {
    let mut out = Vec::new();
    r.read_to_end(&mut out).map_err(|e| classify_io(&e))?;
    Ok(out)
}

/// Todos os codecs testados.
pub fn all() -> Vec<Box<dyn Codec>> {
    vec![
        Box::new(Flate2Gzip),
        Box::new(ZlibRsGzip),
        Box::new(Bzip2Rs),
        Box::new(LzmaRust2Xz),
        Box::new(LzmaRust2Lzma),
        Box::new(LzmaRust2Lzip),
        Box::new(LzmaRsXz),
        Box::new(LzmaRsLzma),
        Box::new(Xz4RustXz),
        Box::new(Ruzstd),
        Box::new(StructuredZstd),
    ]
}

// --- gzip: flate2 (backend miniz_oxide) ---

pub struct Flate2Gzip;

impl Codec for Flate2Gzip {
    fn id(&self) -> &'static str {
        "flate2-gzip"
    }
    fn candidate(&self) -> &'static str {
        "flate2"
    }
    fn format(&self) -> Format {
        Format::Gzip
    }
    fn levels(&self) -> &'static [i32] {
        &[1, 6, 9]
    }
    fn bench_level(&self) -> Option<i32> {
        Some(6)
    }
    fn encode(&self, data: &[u8], level: i32) -> Result<Vec<u8>> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(level as u32));
        e.write_all(data)?;
        Ok(e.finish()?)
    }
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
        read_all(flate2::read::MultiGzDecoder::new(data))
    }
}

// --- gzip: zlib-rs pela API segura (Inflate/Deflate), com o enquadramento gzip feito aqui ---

/// O flate2 com a feature `zlib-rs` usa exatamente `zlib_rs::{Deflate, Inflate}` em modo raw e escreve o
/// cabeçalho gzip ele mesmo; aqui fazemos o mesmo, porque as features do flate2 unificam no grafo e não
/// dá pra ter os dois backends no mesmo binário.
pub struct ZlibRsGzip;

impl Codec for ZlibRsGzip {
    fn id(&self) -> &'static str {
        "zlib-rs-gzip"
    }
    fn candidate(&self) -> &'static str {
        "zlib-rs"
    }
    fn format(&self) -> Format {
        Format::Gzip
    }
    fn levels(&self) -> &'static [i32] {
        &[1, 6, 9]
    }
    fn bench_level(&self) -> Option<i32> {
        Some(6)
    }
    fn encode(&self, data: &[u8], level: i32) -> Result<Vec<u8>> {
        zlibrs_gzip_encode(data, level)
    }
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
        zlibrs_gzip_decode(data)
    }
}

/// Cabeçalho igual ao do `GzBuilder` do flate2: mtime 0, sem nome, SO 255.
fn gzip_header(level: i32) -> [u8; 10] {
    let xfl = match level {
        9 => 2,
        1 => 4,
        _ => 0,
    };
    [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, xfl, 0xff]
}

pub fn zlibrs_gzip_encode(data: &[u8], level: i32) -> Result<Vec<u8>> {
    use zlib_rs::{Deflate, DeflateFlush, Status};
    let mut out = gzip_header(level).to_vec();
    let mut d = Deflate::new(level, false, 15);
    let mut buf = vec![0u8; 64 * 1024];
    let mut input = data;
    loop {
        let (in0, out0) = (d.total_in(), d.total_out());
        let st = d.compress(input, &mut buf, DeflateFlush::Finish).map_err(|e| anyhow!("deflate: {}", e.as_str()))?;
        let consumed = (d.total_in() - in0) as usize;
        let produced = (d.total_out() - out0) as usize;
        out.extend_from_slice(&buf[..produced]);
        input = &input[consumed..];
        if st == Status::StreamEnd {
            break;
        }
        if consumed == 0 && produced == 0 {
            bail!("deflate não progrediu");
        }
    }
    out.extend_from_slice(&zlib_rs::crc32::crc32(0, data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    Ok(out)
}

/// Tamanho do cabeçalho gzip (RFC 1952), validando magia e método.
pub fn gzip_header_len(d: &[u8]) -> Result<usize, DecodeError> {
    if d.len() < 2 || d[0] != 0x1f || d[1] != 0x8b {
        return Err(DecodeError::Format("not in gzip format".into()));
    }
    if d.len() < 10 {
        return Err(DecodeError::Truncated("unexpected end of file".into()));
    }
    if d[2] != 8 {
        return Err(DecodeError::Format(format!("unknown method {}", d[2])));
    }
    let flg = d[3];
    let mut p = 10usize;
    let trunc = || DecodeError::Truncated("unexpected end of file".into());
    if flg & 4 != 0 {
        let xlen = d.get(p..p + 2).ok_or_else(trunc)?;
        p += 2 + u16::from_le_bytes([xlen[0], xlen[1]]) as usize;
    }
    for bit in [8u8, 16] {
        if flg & bit != 0 {
            let rest = d.get(p..).ok_or_else(trunc)?;
            let z = rest.iter().position(|&b| b == 0).ok_or_else(trunc)?;
            p += z + 1;
        }
    }
    if flg & 2 != 0 {
        p += 2;
    }
    if p > d.len() {
        return Err(trunc());
    }
    Ok(p)
}

pub fn zlibrs_gzip_decode(data: &[u8]) -> Result<Vec<u8>, DecodeError> {
    use zlib_rs::{Inflate, InflateFlush, Status};
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut members = 0;
    let mut chunk = vec![0u8; 64 * 1024];
    while pos < data.len() || members == 0 {
        if members > 0 && data[pos..].iter().all(|&b| b == 0) {
            break; // o gzip do GNU ignora zeros no fim (com aviso)
        }
        pos += gzip_header_len(&data[pos..])?;
        let start = out.len();
        let mut inf = Inflate::new(false, 15);
        loop {
            let (in0, out0) = (inf.total_in(), inf.total_out());
            let st = inf
                .decompress(&data[pos..], &mut chunk, InflateFlush::NoFlush)
                .map_err(|e| DecodeError::Format(e.as_str().to_string()))?;
            let consumed = (inf.total_in() - in0) as usize;
            let produced = (inf.total_out() - out0) as usize;
            pos += consumed;
            out.extend_from_slice(&chunk[..produced]);
            if st == Status::StreamEnd {
                break;
            }
            if consumed == 0 && produced == 0 {
                return Err(DecodeError::Truncated("unexpected end of file".into()));
            }
        }
        let trailer = data.get(pos..pos + 8).ok_or_else(|| DecodeError::Truncated("unexpected end of file".into()))?;
        let crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
        let isize = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);
        pos += 8;
        if crc != zlib_rs::crc32::crc32(0, &out[start..]) {
            return Err(DecodeError::Checksum("invalid compressed data--crc error".into()));
        }
        if isize != (out.len() - start) as u32 {
            return Err(DecodeError::Format("invalid compressed data--length error".into()));
        }
        members += 1;
    }
    Ok(out)
}

// --- bzip2: crate bzip2 0.6 com o backend padrão libbz2-rs-sys (Rust) ---

pub struct Bzip2Rs;

impl Codec for Bzip2Rs {
    fn id(&self) -> &'static str {
        "bzip2-rs"
    }
    fn candidate(&self) -> &'static str {
        "bzip2"
    }
    fn format(&self) -> Format {
        Format::Bzip2
    }
    fn levels(&self) -> &'static [i32] {
        &[1, 9]
    }
    fn bench_level(&self) -> Option<i32> {
        Some(9)
    }
    fn encode(&self, data: &[u8], level: i32) -> Result<Vec<u8>> {
        let mut e = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::new(level as u32));
        e.write_all(data)?;
        Ok(e.finish()?)
    }
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
        read_all(bzip2::read::MultiBzDecoder::new(data))
    }
}

// --- xz, lzma e lzip: lzma-rust2 ---

pub struct LzmaRust2Xz;

impl Codec for LzmaRust2Xz {
    fn id(&self) -> &'static str {
        "lzma-rust2-xz"
    }
    fn candidate(&self) -> &'static str {
        "lzma-rust2"
    }
    fn format(&self) -> Format {
        Format::Xz
    }
    fn levels(&self) -> &'static [i32] {
        &[0, 6, 9]
    }
    fn bench_level(&self) -> Option<i32> {
        Some(6)
    }
    fn encode(&self, data: &[u8], level: i32) -> Result<Vec<u8>> {
        let opts = lzma_rust2::XzOptions::with_preset(level as u32);
        let mut w = lzma_rust2::XzWriter::new(Vec::new(), opts)?;
        w.write_all(data)?;
        Ok(w.finish()?)
    }
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
        read_all(lzma_rust2::XzReader::new(data, true))
    }
}

pub struct LzmaRust2Lzma;

impl Codec for LzmaRust2Lzma {
    fn id(&self) -> &'static str {
        "lzma-rust2-lzma"
    }
    fn candidate(&self) -> &'static str {
        "lzma-rust2"
    }
    fn format(&self) -> Format {
        Format::Lzma
    }
    fn levels(&self) -> &'static [i32] {
        &[0, 6, 9]
    }
    fn bench_level(&self) -> Option<i32> {
        Some(6)
    }
    fn encode(&self, data: &[u8], level: i32) -> Result<Vec<u8>> {
        let opts = lzma_rust2::LzmaOptions::with_preset(level as u32);
        // Igual ao `xz --format=lzma`: tamanho desconhecido no cabeçalho e marcador de fim.
        let mut w = lzma_rust2::LzmaWriter::new_use_header(Vec::new(), &opts, None)?;
        w.write_all(data)?;
        Ok(w.finish()?)
    }
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
        let r = lzma_rust2::LzmaReader::new_mem_limit(data, u32::MAX, None).map_err(|e| classify_io(&e))?;
        read_all(r)
    }
}

pub struct LzmaRust2Lzip;

impl Codec for LzmaRust2Lzip {
    fn id(&self) -> &'static str {
        "lzma-rust2-lzip"
    }
    fn candidate(&self) -> &'static str {
        "lzma-rust2"
    }
    fn format(&self) -> Format {
        Format::Lzip
    }
    fn levels(&self) -> &'static [i32] {
        &[0, 6, 9]
    }
    fn bench_level(&self) -> Option<i32> {
        Some(6)
    }
    fn encode(&self, data: &[u8], level: i32) -> Result<Vec<u8>> {
        let opts = lzma_rust2::LzipOptions::with_preset(level as u32);
        let mut w = lzma_rust2::LzipWriter::new(Vec::new(), opts);
        w.write_all(data)?;
        Ok(w.finish()?)
    }
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
        read_all(lzma_rust2::LzipReader::new(data))
    }
}

// --- xz e lzma: lzma-rs ---

pub struct LzmaRsXz;

impl Codec for LzmaRsXz {
    fn id(&self) -> &'static str {
        "lzma-rs-xz"
    }
    fn candidate(&self) -> &'static str {
        "lzma-rs"
    }
    fn format(&self) -> Format {
        Format::Xz
    }
    /// lzma-rs não tem nível: um codificador só (o número é nominal).
    fn levels(&self) -> &'static [i32] {
        &[6]
    }
    fn bench_level(&self) -> Option<i32> {
        Some(6)
    }
    fn encode(&self, data: &[u8], _level: i32) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        lzma_rs::xz_compress(&mut std::io::BufReader::new(data), &mut out)?;
        Ok(out)
    }
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
        let mut out = Vec::new();
        lzma_rs::xz_decompress(&mut std::io::BufReader::new(data), &mut out).map_err(lzma_rs_error)?;
        Ok(out)
    }
}

pub struct LzmaRsLzma;

impl Codec for LzmaRsLzma {
    fn id(&self) -> &'static str {
        "lzma-rs-lzma"
    }
    fn candidate(&self) -> &'static str {
        "lzma-rs"
    }
    fn format(&self) -> Format {
        Format::Lzma
    }
    fn levels(&self) -> &'static [i32] {
        &[6]
    }
    fn bench_level(&self) -> Option<i32> {
        Some(6)
    }
    fn encode(&self, data: &[u8], _level: i32) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        lzma_rs::lzma_compress(&mut std::io::BufReader::new(data), &mut out)?;
        Ok(out)
    }
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
        let mut out = Vec::new();
        lzma_rs::lzma_decompress(&mut std::io::BufReader::new(data), &mut out).map_err(lzma_rs_error)?;
        Ok(out)
    }
}

fn lzma_rs_error(e: lzma_rs::error::Error) -> DecodeError {
    match e {
        lzma_rs::error::Error::IoError(io) | lzma_rs::error::Error::HeaderTooShort(io) => classify_io(&io),
        lzma_rs::error::Error::LzmaError(m) | lzma_rs::error::Error::XzError(m) => classify_text(&m),
    }
}

// --- xz: xz4rust (só decodificador) ---

pub struct Xz4RustXz;

impl Codec for Xz4RustXz {
    fn id(&self) -> &'static str {
        "xz4rust-xz"
    }
    fn candidate(&self) -> &'static str {
        "xz4rust"
    }
    fn format(&self) -> Format {
        Format::Xz
    }
    fn levels(&self) -> &'static [i32] {
        &[]
    }
    fn bench_level(&self) -> Option<i32> {
        None
    }
    fn encode(&self, _data: &[u8], _level: i32) -> Result<Vec<u8>> {
        bail!("xz4rust só decodifica")
    }
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
        // O XzReader exige leitor 'static: copia a entrada.
        read_all(xz4rust::XzReader::new(std::io::Cursor::new(data.to_vec())))
    }
}

// --- zstd: ruzstd e structured-zstd ---

pub struct Ruzstd;

impl Codec for Ruzstd {
    fn id(&self) -> &'static str {
        "ruzstd"
    }
    fn candidate(&self) -> &'static str {
        "ruzstd"
    }
    fn format(&self) -> Format {
        Format::Zstd
    }
    /// Só `Fastest` (equivalente ao nível 1) está implementado; Default/Better/Best são `unimplemented!()`.
    fn levels(&self) -> &'static [i32] {
        &[1]
    }
    fn bench_level(&self) -> Option<i32> {
        Some(1)
    }
    fn encode(&self, data: &[u8], _level: i32) -> Result<Vec<u8>> {
        Ok(ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest))
    }
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
        // Um StreamingDecoder por frame, como o `zstd -d` faz com frames concatenados.
        let mut input = data;
        let mut out = Vec::new();
        while !input.is_empty() {
            let mut dec = ruzstd::decoding::StreamingDecoder::new(&mut input)
                .map_err(|e| classify_text(&e.to_string()))?;
            dec.read_to_end(&mut out).map_err(|e| classify_io(&e))?;
        }
        Ok(out)
    }
}

pub struct StructuredZstd;

impl Codec for StructuredZstd {
    fn id(&self) -> &'static str {
        "structured-zstd"
    }
    fn candidate(&self) -> &'static str {
        "structured-zstd"
    }
    fn format(&self) -> Format {
        Format::Zstd
    }
    fn levels(&self) -> &'static [i32] {
        &[1, 3, 19]
    }
    fn bench_level(&self) -> Option<i32> {
        Some(3)
    }
    fn encode(&self, data: &[u8], level: i32) -> Result<Vec<u8>> {
        Ok(structured_zstd::encoding::compress_to_vec(
            data,
            structured_zstd::encoding::CompressionLevel::from_level(level),
        ))
    }
    fn decode(&self, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
        let mut input = data;
        let mut out = Vec::new();
        while !input.is_empty() {
            let mut dec = structured_zstd::decoding::StreamingDecoder::new(&mut input)
                .map_err(|e| classify_text(&e.to_string()))?;
            dec.read_to_end(&mut out).map_err(|e| classify_io(&e))?;
        }
        Ok(out)
    }
}

/// Detecta o formato pela magia (usado pelo `tar` com compressão automática).
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
    } else {
        None
    }
}

/// Codec de cada formato usado pelo shim de CLI quando o backend não é escolhido explicitamente.
pub fn default_for(format: Format) -> Box<dyn Codec> {
    match format {
        Format::Gzip => Box::new(Flate2Gzip),
        Format::Bzip2 => Box::new(Bzip2Rs),
        Format::Xz => Box::new(LzmaRust2Xz),
        Format::Lzma => Box::new(LzmaRust2Lzma),
        Format::Lzip => Box::new(LzmaRust2Lzip),
        Format::Zstd => Box::new(StructuredZstd),
    }
}

/// Roundtrip simples, usado nos testes.
#[cfg(test)]
pub fn roundtrip(codec: &dyn Codec, data: &[u8], level: i32) -> Result<()> {
    let enc = encode_guarded(codec, data, level).map_err(|e| anyhow!("{} encode: {e:#}", codec.id()))?;
    let dec = decode_guarded(codec, &enc).map_err(|e| anyhow!("{} decode: {e:?}", codec.id()))?;
    if dec != data {
        bail!("{}: roundtrip devolveu {} bytes, esperado {}", codec.id(), dec.len(), data.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_encoder_roundtrips() {
        let data = crate::h31_archive::corpus::log_text(50_000);
        for codec in all() {
            for &level in codec.levels() {
                roundtrip(codec.as_ref(), &data, level).unwrap();
            }
        }
    }

    #[test]
    fn zlibrs_gzip_detects_crc_and_truncation() {
        let data = b"hello world\n".repeat(50);
        let mut enc = zlibrs_gzip_encode(&data, 6).unwrap();
        let n = enc.len();
        assert!(matches!(zlibrs_gzip_decode(&enc[..n - 3]), Err(DecodeError::Truncated(_))));
        enc[n - 6] ^= 0xff;
        assert!(matches!(zlibrs_gzip_decode(&enc), Err(DecodeError::Checksum(_))));
        assert!(matches!(zlibrs_gzip_decode(b"plain"), Err(DecodeError::Format(_))));
    }

    #[test]
    fn sniff_recognizes_magic() {
        let gz = Flate2Gzip.encode(b"x", 6).unwrap();
        assert_eq!(sniff(&gz), Some(Format::Gzip));
        assert_eq!(sniff(b"plain"), None);
    }
}
