//! O cache de disco do navegador (`Default/Cache/Cache_Data`), no formato do simple cache do Chromium 154 (medido no
//! oráculo com `testbench/oracle/measure-navigation.sh`): um arquivo `<hash da chave>_0` por recurso com o cabeçalho, a
//! chave, o corpo (fluxo 1), o registro de fim do corpo, a resposta HTTP serializada (fluxo 0), o SHA-256 da chave e o
//! registro de fim do fluxo 0.
//!
//! A chave é `1/0/_dk_<site> <site> <endereço>` (partição de rede do site de topo); o nome do arquivo é o começo do
//! SHA-1 da chave lido como inteiro de 64 bits little endian, em 16 dígitos hexadecimais.

use ul_common::codec::crc32_update;
use ul_common::hash::{sha1, sha256, superfast::persistent_hash};

const INITIAL_MAGIC: u64 = 0xfcfb_6d1b_a772_5c30;
const FINAL_MAGIC: u64 = 0xf4fa_6f45_970d_41d8;
const VERSION: u32 = 5;
/// Os sinalizadores do registro de fim: há CRC-32 e, no do fluxo 0, o SHA-256 da chave.
const FLAG_CRC: u32 = 1;
const FLAG_SHA256: u32 = 2;
/// `HttpResponseInfo`: a versão do formato (3) com os sinalizadores extras, o tempo original da resposta e (bit 31).
const RESPONSE_FLAGS: u32 = 0x8004_0003;
const RESPONSE_EXTRA_FLAGS: u32 = 6;
const CONNECTION_INFO: u32 = 9;

/// Um recurso a guardar.
#[derive(Debug, Clone)]
pub struct Stored<'a> {
    /// A chave completa (`1/0/_dk_...`).
    pub key: String,
    /// A linha de status como veio (`HTTP/1.0 200 OK`) e os cabeçalhos, na ordem do fio.
    pub status_line: String,
    pub headers: Vec<(String, String)>,
    pub body: &'a [u8],
    /// Tempos em microssegundos desde 1601 (relógio WebKit): o pedido e a resposta.
    pub request_time: i64,
    pub response_time: i64,
    /// O servidor remoto: o endereço (texto) e a porta.
    pub remote_host: String,
    pub remote_port: u16,
}

/// A chave do cache de uma página servida pelo site `site` (`http://127.0.0.1`) no endereço `url`.
pub fn key(site: &str, url: &str) -> String {
    format!("1/0/_dk_{site} {site} {url}")
}

/// O nome do arquivo da chave.
pub fn file_name(key: &str) -> String {
    let digest = sha1(key.as_bytes());
    let value = u64::from_le_bytes(digest[..8].try_into().expect("oito bytes"));
    format!("{value:016x}_0")
}

/// Uma resposta HTTP que o navegador buscou, como o cache a guarda: o endereço, a linha de status e os cabeçalhos
/// como vieram, o corpo e os instantes (WebKit, em microssegundos) do pedido e da resposta.
#[derive(Debug, Clone)]
pub struct Captured {
    pub url: String,
    pub status_line: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub request_time: i64,
    pub response_time: i64,
    pub remote_host: String,
    pub remote_port: u16,
}

impl Captured {
    /// A resposta de `response` ao pedido feito em `request_time` e recebida em `response_time`.
    pub fn from_response(response: &ul_net::net::client::Response, request_time: i64, response_time: i64) -> Captured {
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        let (major, minor) = response.transfer.version;
        Captured {
            url: response.url.to_string_with(false),
            status_line: format!("HTTP/{major}.{minor} {} {}", response.status, response.reason),
            headers: response.headers.iter().map(|(name, value)| (text(name), text(value))).collect(),
            body: response.body.clone(),
            request_time,
            response_time,
            remote_host: response.url.host_for_url(),
            remote_port: response.url.port_or_default(),
        }
    }

    /// O site da partição de rede de `url`: o esquema e o domínio registrável (ou o próprio host, sem ele).
    fn site(url: &str) -> String {
        let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
        let host = rest.split(['/', ':', '?', '#']).next().unwrap_or_default();
        let registrable = crate::devtools::registry::domain_and_registry(host);
        format!("{scheme}://{}", if registrable.is_empty() { host } else { &registrable })
    }

    /// O arquivo e a linha de índice da entrada, na partição do site de topo `top_url` (o documento principal tem
    /// os sinalizadores de memória 2 no índice, os demais recursos 1).
    pub fn entry(&self, top_url: &str) -> CacheEntry {
        let site = Captured::site(top_url);
        let key = key(&site, &self.url);
        let stored = Stored {
            key,
            status_line: self.status_line.clone(),
            headers: self.headers.clone(),
            body: &self.body,
            request_time: self.request_time,
            response_time: self.response_time,
            remote_host: self.remote_host.clone(),
            remote_port: self.remote_port,
        };
        let content = entry_file(&stored);
        let flags = if self.url == top_url { 2 } else { 1 };
        let index = IndexEntry { id: key_id(&stored.key), last_used: self.response_time / 1_000_000 * 1_000_000, size_field: index_size_field(content.len() as u64, flags) };
        CacheEntry { name: file_name(&stored.key), content, index }
    }
}

/// O que a entrada escreve no diretório do cache: o arquivo e a linha do índice.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub name: String,
    pub content: Vec<u8>,
    pub index: IndexEntry,
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn pad4(out: &mut Vec<u8>) {
    out.resize(out.len().next_multiple_of(4), 0);
}

fn put_string(out: &mut Vec<u8>, bytes: &[u8]) {
    put_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
    pad4(out);
}

/// O registro de fim de um fluxo: o número mágico, os sinalizadores, o CRC-32 e o tamanho do fluxo.
fn eof(flags: u32, crc: u32, size: u32) -> Vec<u8> {
    let mut out = FINAL_MAGIC.to_le_bytes().to_vec();
    put_u32(&mut out, flags);
    put_u32(&mut out, crc);
    put_u32(&mut out, size);
    put_u32(&mut out, 0);
    out
}

/// A resposta HTTP serializada (`HttpResponseInfo::Persist`), com o cabeçalho de tamanho do pickle.
fn response_info(stored: &Stored) -> Vec<u8> {
    let mut body = Vec::new();
    put_u32(&mut body, RESPONSE_FLAGS);
    put_u32(&mut body, RESPONSE_EXTRA_FLAGS);
    put_i64(&mut body, stored.request_time);
    put_i64(&mut body, stored.response_time);
    put_i64(&mut body, stored.response_time);
    let mut raw = stored.status_line.as_bytes().to_vec();
    raw.push(0);
    for (name, value) in &stored.headers {
        raw.extend_from_slice(format!("{name}: {value}").as_bytes());
        raw.push(0);
    }
    raw.push(0);
    put_string(&mut body, &raw);
    put_string(&mut body, stored.remote_host.as_bytes());
    put_u32(&mut body, u32::from(stored.remote_port));
    put_u32(&mut body, CONNECTION_INFO);
    put_u32(&mut body, u32::MAX);
    put_u32(&mut body, 0);
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend(body);
    out
}

/// O conteúdo do arquivo da entrada.
pub fn entry_file(stored: &Stored) -> Vec<u8> {
    let key = stored.key.as_bytes();
    let mut out = INITIAL_MAGIC.to_le_bytes().to_vec();
    put_u32(&mut out, VERSION);
    put_u32(&mut out, key.len() as u32);
    put_u32(&mut out, persistent_hash(key));
    put_u32(&mut out, 0);
    out.extend_from_slice(key);
    out.extend_from_slice(stored.body);
    out.extend(eof(FLAG_CRC, crc32_update(0, stored.body), 0));
    let info = response_info(stored);
    out.extend_from_slice(&info);
    out.extend_from_slice(&sha256(key));
    out.extend(eof(FLAG_CRC | FLAG_SHA256, crc32_update(0, &info), info.len() as u32));
    out
}

const INDEX_MAGIC: u64 = 0x656e_7465_7220_796f;
const INDEX_VERSION: u32 = 9;
/// O que o índice soma ao tamanho do arquivo de uma entrada pequena antes de contar em pedaços de 256 bytes
/// (calibrado contra as entradas do oráculo: 519, 565, 578, 746, 819, 3580 e 4796 bytes).
const INDEX_SIZE_OVERHEAD: u64 = 205;

/// O identificador de 64 bits da chave: o começo do SHA-1 como inteiro little endian (o nome do arquivo, sem o `_0`).
pub fn key_id(key: &str) -> u64 {
    u64::from_le_bytes(sha1(key.as_bytes())[..8].try_into().expect("oito bytes"))
}

/// O campo de tamanho e sinalizadores de uma entrada pequena no índice: os pedaços de 256 bytes do arquivo (com o
/// custo fixo da entrada) acima e os sinalizadores de dados em memória no byte baixo (2 na entrada do documento
/// principal, 1 nas demais).
pub fn index_size_field(file_size: u64, flags: u8) -> u64 {
    ((file_size + INDEX_SIZE_OVERHEAD).div_ceil(256) << 8) | u64::from(flags)
}

/// Uma linha do índice: o identificador da chave, o último uso (WebKit, em segundos inteiros) e o campo de tamanho.
#[derive(Debug, Clone, Copy)]
pub struct IndexEntry {
    pub id: u64,
    pub last_used: i64,
    pub size_field: u64,
}

/// O `the-real-index` do cache: o cabeçalho de pickle (tamanho e CRC-32 do resto), o número mágico, a versão, a
/// contagem, o tamanho total em pedaços, o motivo da escrita, as entradas e o instante em que o índice foi escrito.
pub fn index_file(entries: &[IndexEntry], written: i64) -> Vec<u8> {
    let mut payload = INDEX_MAGIC.to_le_bytes().to_vec();
    put_u32(&mut payload, INDEX_VERSION);
    payload.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    let total: u64 = entries.iter().map(|entry| (entry.size_field >> 8) * 256).sum();
    payload.extend_from_slice(&total.to_le_bytes());
    put_u32(&mut payload, 0);
    for entry in entries {
        payload.extend_from_slice(&entry.id.to_le_bytes());
        put_i64(&mut payload, entry.last_used);
        payload.extend_from_slice(&entry.size_field.to_le_bytes());
    }
    put_i64(&mut payload, written);
    let mut out = (payload.len() as u32).to_le_bytes().to_vec();
    put_u32(&mut out, crc32_update(0, &payload));
    out.extend(payload);
    out
}

/// As linhas de um `the-real-index` já escrito (execuções anteriores), ou `None` se o arquivo não tem o formato.
pub fn parse_index(bytes: &[u8]) -> Option<Vec<IndexEntry>> {
    let payload = bytes.get(8..)?;
    let size = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    let crc = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    if payload.len() != size || crc32_update(0, payload) != crc {
        return None;
    }
    let word = |at: usize, len: usize| payload.get(at..at + len);
    if u64::from_le_bytes(word(0, 8)?.try_into().ok()?) != INDEX_MAGIC || u32::from_le_bytes(word(8, 4)?.try_into().ok()?) != INDEX_VERSION {
        return None;
    }
    let count = u64::from_le_bytes(word(12, 8)?.try_into().ok()?) as usize;
    (0..count)
        .map(|index| {
            let at = 32 + index * 24;
            Some(IndexEntry { id: u64::from_le_bytes(word(at, 8)?.try_into().ok()?), last_used: i64::from_le_bytes(word(at + 8, 8)?.try_into().ok()?), size_field: u64::from_le_bytes(word(at + 16, 8)?.try_into().ok()?) })
        })
        .collect()
}
