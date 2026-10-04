//! HTTP/1.x do lado do cliente: fluxo (TCP ou TLS), leitura da cabeça da resposta com os bytes
//! originais (o `curl -i` e o `wget -S` mostram exatamente o que veio), corpo por `Content-Length`,
//! `chunked` ou fechamento, e os decodificadores de `Content-Encoding`.

use std::io::{self, Read, Write};

use super::io::{Deadline, Tcp};
use super::tls::TlsStream;

/// Fluxo de uma conexão.
pub enum Stream {
    Plain(Tcp),
    Tls(Box<TlsStream>),
}

impl Stream {
    pub fn tcp(&self) -> &Tcp {
        match self {
            Stream::Plain(t) => t,
            Stream::Tls(t) => t.tcp(),
        }
    }

    pub fn tcp_mut(&mut self) -> &mut Tcp {
        match self {
            Stream::Plain(t) => t,
            Stream::Tls(t) => t.tcp_mut(),
        }
    }

    pub fn set_deadline(&mut self, d: Deadline) {
        self.tcp_mut().deadline = d;
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(t) => t.read(buf),
            Stream::Tls(t) => t.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(t) => t.write(buf),
            Stream::Tls(t) => t.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Plain(t) => t.flush(),
            Stream::Tls(t) => t.flush(),
        }
    }
}

/// Cabeça de uma resposta.
#[derive(Clone, Debug, Default)]
pub struct Head {
    /// Linhas como chegaram (sem o fim de linha), a primeira é a de status.
    pub lines: Vec<Vec<u8>>,
    /// Bytes exatos da cabeça, linha vazia final incluída.
    pub raw: Vec<u8>,
    /// (1, 0) ou (1, 1); (0, 9) quando não veio linha de status.
    pub version: (u8, u8),
    pub status: u16,
    pub reason: Vec<u8>,
    /// Nome e valor de cada cabeçalho, como chegaram (valor sem espaço nas pontas).
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Head {
    /// Primeiro valor do cabeçalho `name` (sem diferenciar caixa).
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.as_bytes())).map(|(_, v)| v.as_slice())
    }

    pub fn get_all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a [u8]> + 'a {
        self.headers.iter().filter(move |(n, _)| n.eq_ignore_ascii_case(name.as_bytes())).map(|(_, v)| v.as_slice())
    }

    pub fn get_str(&self, name: &str) -> Option<String> {
        self.get(name).map(|v| String::from_utf8_lossy(v).into_owned())
    }
}

/// Erro de protocolo da resposta.
#[derive(Debug)]
pub enum HttpError {
    Io(io::Error),
    /// Conexão fechou antes de qualquer byte de resposta.
    Empty,
    /// Linha de status inválida.
    WeirdReply,
    /// Cabeça grande demais.
    TooLarge,
}

impl From<io::Error> for HttpError {
    fn from(e: io::Error) -> Self {
        HttpError::Io(e)
    }
}

/// Conexão com buffer de leitura.
pub struct Conn {
    pub stream: Stream,
    buf: Vec<u8>,
    pos: usize,
    /// Bytes recebidos (cabeças e corpos), pro `size_download`/`size_header`.
    pub received: u64,
}

const MAX_HEAD: usize = 300 * 1024;

impl Conn {
    pub fn new(stream: Stream) -> Conn {
        Conn { stream, buf: Vec::new(), pos: 0, received: 0 }
    }

    fn fill(&mut self) -> io::Result<usize> {
        if self.pos >= self.buf.len() {
            self.buf.clear();
            self.pos = 0;
        }
        let mut chunk = [0u8; 16384];
        let n = self.stream.read(&mut chunk)?;
        self.buf.extend_from_slice(&chunk[..n]);
        self.received += n as u64;
        Ok(n)
    }

    fn buffered(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    /// Lê até `n` bytes do que já chegou ou do fluxo; 0 = fim.
    pub fn read_some(&mut self, max: usize) -> io::Result<Vec<u8>> {
        if self.buffered().is_empty() && self.fill()? == 0 {
            return Ok(Vec::new());
        }
        let take = self.buffered().len().min(max);
        let out = self.buf[self.pos..self.pos + take].to_vec();
        self.pos += take;
        Ok(out)
    }

    /// Uma linha (com o fim de linha); vazio no fim do fluxo.
    fn read_line(&mut self) -> io::Result<Vec<u8>> {
        let mut line = Vec::new();
        loop {
            if let Some(p) = self.buffered().iter().position(|&b| b == b'\n') {
                line.extend_from_slice(&self.buf[self.pos..self.pos + p + 1]);
                self.pos += p + 1;
                return Ok(line);
            }
            let rest = self.buffered().to_vec();
            line.extend_from_slice(&rest);
            self.pos = self.buf.len();
            if line.len() > MAX_HEAD {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "too large"));
            }
            if self.fill()? == 0 {
                return Ok(line);
            }
        }
    }

    /// Lê a cabeça de uma resposta (sem as informativas 1xx: quem chama decide repetir).
    pub fn read_head(&mut self) -> Result<Head, HttpError> {
        let mut head = Head::default();
        let first = self.read_line()?;
        if first.is_empty() {
            return Err(HttpError::Empty);
        }
        head.raw.extend_from_slice(&first);
        let status_line = strip_eol(&first).to_vec();
        if !status_line.starts_with(b"HTTP/") {
            return Err(HttpError::WeirdReply);
        }
        let (version, status, reason) = parse_status(&status_line).ok_or(HttpError::WeirdReply)?;
        head.version = version;
        head.status = status;
        head.reason = reason;
        head.lines.push(status_line);
        loop {
            let line = self.read_line()?;
            if line.is_empty() {
                // Fim do fluxo no meio da cabeça: o que veio vale.
                break;
            }
            head.raw.extend_from_slice(&line);
            if head.raw.len() > MAX_HEAD {
                return Err(HttpError::TooLarge);
            }
            let l = strip_eol(&line);
            if l.is_empty() {
                break;
            }
            head.lines.push(l.to_vec());
            if let Some(c) = l.iter().position(|&b| b == b':') {
                let name = l[..c].to_vec();
                let value = trim(&l[c + 1..]).to_vec();
                head.headers.push((name, value));
            }
        }
        Ok(head)
    }

    /// Corpo inteiro conforme o modo (pra respostas pequenas e erros).
    pub fn body_reader(&mut self, mode: BodyMode) -> BodyReader<'_> {
        BodyReader { conn: self, mode, done: false, chunk_left: 0, chunk_state: ChunkState::Size }
    }
}

fn strip_eol(l: &[u8]) -> &[u8] {
    let l = l.strip_suffix(b"\n").unwrap_or(l);
    l.strip_suffix(b"\r").unwrap_or(l)
}

fn trim(v: &[u8]) -> &[u8] {
    let start = v.iter().position(|&c| c != b' ' && c != b'\t').unwrap_or(v.len());
    let end = v.iter().rposition(|&c| c != b' ' && c != b'\t').map(|p| p + 1).unwrap_or(start);
    &v[start..end.max(start)]
}

/// `HTTP/1.1 200 OK` -> ((1,1), 200, "OK").
fn parse_status(l: &[u8]) -> Option<((u8, u8), u16, Vec<u8>)> {
    let rest = l.strip_prefix(b"HTTP/")?;
    let sp = rest.iter().position(|&c| c == b' ')?;
    let ver = &rest[..sp];
    let version = match ver {
        b"1.0" => (1, 0),
        b"1.1" => (1, 1),
        b"2" | b"2.0" => (2, 0),
        b"3" => (3, 0),
        _ => return None,
    };
    let rest = &rest[sp + 1..];
    let code = rest.get(..3)?;
    if !code.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let status: u16 = std::str::from_utf8(code).ok()?.parse().ok()?;
    let reason = rest.get(4..).unwrap_or_default().to_vec();
    Some((version, status, reason))
}

/// Como o corpo termina.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyMode {
    None,
    Length(u64),
    Chunked,
    Close,
}

/// Escolhe o modo pela resposta e pelo método (HEAD e 1xx/204/304 não têm corpo).
pub fn body_mode(head: &Head, is_head: bool) -> BodyMode {
    if is_head || head.status == 204 || head.status == 304 || (100..200).contains(&head.status) {
        return BodyMode::None;
    }
    if head.get("Transfer-Encoding").is_some_and(|v| {
        String::from_utf8_lossy(v).split(',').any(|t| t.trim().eq_ignore_ascii_case("chunked"))
    }) {
        return BodyMode::Chunked;
    }
    if let Some(v) = head.get("Content-Length")
        && let Ok(n) = String::from_utf8_lossy(v).trim().parse::<u64>()
    {
        return BodyMode::Length(n);
    }
    BodyMode::Close
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChunkState {
    Size,
    Data,
    DataEnd,
    Trailer,
}

/// Erros do corpo, como o curl distingue.
#[derive(Debug)]
pub enum BodyError {
    Io(io::Error),
    /// Fechou antes do fim: quantos bytes faltavam (`None` no chunked).
    Partial(Option<u64>),
    /// Tamanho de bloco inválido no chunked.
    BadChunk,
}

/// Leitor do corpo: devolve os pedaços na ordem em que chegam.
pub struct BodyReader<'a> {
    pub conn: &'a mut Conn,
    mode: BodyMode,
    done: bool,
    chunk_left: u64,
    chunk_state: ChunkState,
}

impl BodyReader<'_> {
    /// Próximo pedaço do corpo (já sem o enquadramento chunked); `None` no fim.
    pub fn next_piece(&mut self) -> Result<Option<Vec<u8>>, BodyError> {
        if self.done {
            return Ok(None);
        }
        match self.mode {
            BodyMode::None => {
                self.done = true;
                Ok(None)
            }
            BodyMode::Length(left) => {
                if left == 0 {
                    self.done = true;
                    return Ok(None);
                }
                let piece = self.conn.read_some(left.min(65536) as usize).map_err(BodyError::Io)?;
                if piece.is_empty() {
                    self.done = true;
                    return Err(BodyError::Partial(Some(left)));
                }
                self.mode = BodyMode::Length(left - piece.len() as u64);
                Ok(Some(piece))
            }
            BodyMode::Close => {
                let piece = self.conn.read_some(65536).map_err(BodyError::Io)?;
                if piece.is_empty() {
                    self.done = true;
                    return Ok(None);
                }
                Ok(Some(piece))
            }
            BodyMode::Chunked => self.next_chunked(),
        }
    }

    fn next_chunked(&mut self) -> Result<Option<Vec<u8>>, BodyError> {
        loop {
            match self.chunk_state {
                ChunkState::Size => {
                    let line = self.conn.read_line().map_err(BodyError::Io)?;
                    if line.is_empty() {
                        self.done = true;
                        return Err(BodyError::Partial(None));
                    }
                    let l = strip_eol(&line);
                    let hex: Vec<u8> = l.iter().copied().take_while(|c| c.is_ascii_hexdigit()).collect();
                    if hex.is_empty() || hex.len() > 16 {
                        self.done = true;
                        return Err(BodyError::BadChunk);
                    }
                    let n = u64::from_str_radix(std::str::from_utf8(&hex).unwrap_or("0"), 16).map_err(|_| BodyError::BadChunk)?;
                    if n == 0 {
                        self.chunk_state = ChunkState::Trailer;
                    } else {
                        self.chunk_left = n;
                        self.chunk_state = ChunkState::Data;
                    }
                }
                ChunkState::Data => {
                    let piece = self.conn.read_some(self.chunk_left.min(65536) as usize).map_err(BodyError::Io)?;
                    if piece.is_empty() {
                        self.done = true;
                        return Err(BodyError::Partial(None));
                    }
                    self.chunk_left -= piece.len() as u64;
                    if self.chunk_left == 0 {
                        self.chunk_state = ChunkState::DataEnd;
                    }
                    return Ok(Some(piece));
                }
                ChunkState::DataEnd => {
                    let line = self.conn.read_line().map_err(BodyError::Io)?;
                    if line.is_empty() {
                        self.done = true;
                        return Err(BodyError::Partial(None));
                    }
                    if !strip_eol(&line).is_empty() {
                        self.done = true;
                        return Err(BodyError::BadChunk);
                    }
                    self.chunk_state = ChunkState::Size;
                }
                ChunkState::Trailer => {
                    let line = self.conn.read_line().map_err(BodyError::Io)?;
                    if line.is_empty() || strip_eol(&line).is_empty() {
                        self.done = true;
                        return Ok(None);
                    }
                }
            }
        }
    }
}

/// Decodificador de `Content-Encoding`.
pub enum Decoder {
    Identity,
    Gzip(Box<flate2::write::GzDecoder<Vec<u8>>>),
    /// `deflate`: zlib; se o primeiro pedaço não for zlib, cai pra deflate cru (como o curl).
    Deflate { z: Option<Box<flate2::write::ZlibDecoder<Vec<u8>>>>, raw: Option<Box<flate2::write::DeflateDecoder<Vec<u8>>>>, started: bool },
    Brotli(Box<brotli_decompressor::DecompressorWriter<Vec<u8>>>),
    /// zstd: o decodificador do ruzstd é de leitura; o corpo comprimido é juntado e decodificado no
    /// fim.
    Zstd(Vec<u8>),
}

/// Erro de decodificação.
#[derive(Debug)]
pub struct DecodeError(pub String);

impl Decoder {
    /// Decodificador pra uma lista de codificações (a última aplicada vem por último no cabeçalho).
    /// `Err` com o nome desconhecido.
    pub fn for_encoding(enc: &str) -> Result<Decoder, String> {
        let e = enc.trim().to_ascii_lowercase();
        Ok(match e.as_str() {
            "" | "identity" => Decoder::Identity,
            "gzip" | "x-gzip" => Decoder::Gzip(Box::new(flate2::write::GzDecoder::new(Vec::new()))),
            "deflate" => Decoder::Deflate { z: Some(Box::new(flate2::write::ZlibDecoder::new(Vec::new()))), raw: None, started: false },
            "br" => Decoder::Brotli(Box::new(brotli_decompressor::DecompressorWriter::new(Vec::new(), 4096))),
            "zstd" => Decoder::Zstd(Vec::new()),
            _ => return Err(e),
        })
    }

    /// Alimenta bytes comprimidos e devolve o que já saiu decodificado.
    pub fn feed(&mut self, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
        match self {
            Decoder::Identity => Ok(data.to_vec()),
            Decoder::Gzip(d) => {
                d.write_all(data).map_err(|e| DecodeError(e.to_string()))?;
                Ok(std::mem::take(d.get_mut()))
            }
            Decoder::Deflate { z, raw, started } => {
                if !*started {
                    *started = true;
                    // Cabeçalho zlib: CMF/FLG com checksum múltiplo de 31.
                    let looks_zlib = data.len() >= 2 && data[0] & 0x0f == 8 && (u16::from(data[0]) << 8 | u16::from(data[1])) % 31 == 0;
                    if !looks_zlib {
                        *z = None;
                        *raw = Some(Box::new(flate2::write::DeflateDecoder::new(Vec::new())));
                    }
                }
                if let Some(d) = z {
                    d.write_all(data).map_err(|e| DecodeError(e.to_string()))?;
                    return Ok(std::mem::take(d.get_mut()));
                }
                if let Some(d) = raw {
                    d.write_all(data).map_err(|e| DecodeError(e.to_string()))?;
                    return Ok(std::mem::take(d.get_mut()));
                }
                Ok(Vec::new())
            }
            Decoder::Brotli(d) => {
                d.write_all(data).map_err(|e| DecodeError(format!("{e}")))?;
                Ok(std::mem::take(d.get_mut()))
            }
            Decoder::Zstd(buf) => {
                if buf.try_reserve(data.len()).is_err() {
                    return Err(DecodeError("out of memory".into()));
                }
                buf.extend_from_slice(data);
                Ok(Vec::new())
            }
        }
    }

    /// Fim do corpo: o que sobrou.
    pub fn finish(&mut self) -> Result<Vec<u8>, DecodeError> {
        match self {
            Decoder::Identity => Ok(Vec::new()),
            Decoder::Gzip(d) => {
                d.try_finish().map_err(|e| DecodeError(e.to_string()))?;
                Ok(std::mem::take(d.get_mut()))
            }
            Decoder::Deflate { z, raw, .. } => {
                if let Some(d) = z {
                    d.try_finish().map_err(|e| DecodeError(e.to_string()))?;
                    return Ok(std::mem::take(d.get_mut()));
                }
                if let Some(d) = raw {
                    d.try_finish().map_err(|e| DecodeError(e.to_string()))?;
                    return Ok(std::mem::take(d.get_mut()));
                }
                Ok(Vec::new())
            }
            Decoder::Brotli(d) => {
                d.flush().map_err(|e| DecodeError(format!("{e}")))?;
                Ok(std::mem::take(d.get_mut()))
            }
            Decoder::Zstd(buf) => {
                let mut out = Vec::new();
                let mut dec = ruzstd::decoding::StreamingDecoder::new(&buf[..]).map_err(|e| DecodeError(e.to_string()))?;
                dec.read_to_end(&mut out).map_err(|e| DecodeError(e.to_string()))?;
                buf.clear();
                Ok(out)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines() {
        assert_eq!(parse_status(b"HTTP/1.1 200 OK"), Some(((1, 1), 200, b"OK".to_vec())));
        assert_eq!(parse_status(b"HTTP/1.0 404"), Some(((1, 0), 404, Vec::new())));
        assert_eq!(parse_status(b"HTTP/1.1 2xx"), None);
    }

    #[test]
    fn gzip_roundtrip() {
        use std::io::Write as _;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(b"hello hello hello").unwrap();
        let z = enc.finish().unwrap();
        let mut d = Decoder::for_encoding("gzip").unwrap();
        let mut out = d.feed(&z[..5]).unwrap();
        out.extend(d.feed(&z[5..]).unwrap());
        out.extend(d.finish().unwrap());
        assert_eq!(out, b"hello hello hello");
    }
}
