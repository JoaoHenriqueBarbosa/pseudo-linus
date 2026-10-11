//! O que o domínio `Network` do DevTools mostra de uma troca HTTP: a resposta, o que o pedido levou no fio e a cabeça
//! crua, tirados da resposta do `ul-net` (e do `data:`, que não tem fio).

use ul_net::net::client::{Hop, Response};
use ul_net::net::url::Url;

/// Uma troca: o pedido que saiu e a resposta que voltou (a final ou a de um redirecionamento).
#[derive(Debug, Clone)]
pub struct NetExchange {
    pub url: String,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    /// A cabeça crua da resposta (`headersText`); o seu tamanho é o `encodedDataLength` da `responseReceived`.
    pub head: String,
    /// Os cabeçalhos que o pedido levou (`requestWillBeSentExtraInfo`).
    pub sent: Vec<(String, String)>,
    pub mime_type: String,
    pub charset: String,
    /// `data`, `http/1.0`, `http/1.1`, `h2`.
    pub protocol: String,
    pub remote_ip: String,
    pub remote_port: u16,
    /// A resposta veio do `Fetch.fulfillRequest`: sem conexão, sem `ExtraInfo`.
    pub synthetic: bool,
}

/// O documento (ou o favicon): os redirecionamentos seguidos e a troca final, mais os tamanhos do corpo.
#[derive(Debug, Clone)]
pub struct NetMeta {
    pub redirects: Vec<NetExchange>,
    pub last: NetExchange,
    /// Bytes do corpo decodificado (`dataLength` do `dataReceived`).
    pub data_length: usize,
    /// Bytes do corpo no fio, sem a cabeça.
    pub body_encoded: usize,
    /// O corpo decodificado, para o `Network.getResponseBody`.
    pub body: Vec<u8>,
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn exchange(url: &Url, status: u16, reason: &str, headers: &[(Vec<u8>, Vec<u8>)], head: &[u8], sent: &[(String, String)], version: (u8, u8), synthetic: bool) -> NetExchange {
    let headers: Vec<(String, String)> = headers.iter().map(|(n, v)| (text(n), text(v))).collect();
    let content_type = headers.iter().find(|(n, _)| n.eq_ignore_ascii_case("Content-Type")).map(|(_, v)| v.clone());
    let mime_type = content_type.as_deref().and_then(|c| c.split(';').next()).map(|m| m.trim().to_ascii_lowercase());
    let charset = content_type.as_deref().and_then(|c| c.split(';').skip(1).find_map(|p| p.trim().strip_prefix("charset=").map(|v| v.trim_matches('"').to_owned()))).unwrap_or_default();
    NetExchange {
        url: url.to_string_with(true),
        status,
        status_text: reason.to_owned(),
        headers,
        head: text(head),
        sent: sent.to_vec(),
        // Sem `Content-Type`, o Chromium cheirava o corpo: o que o oráculo mediu (404 vazio) é `text/plain`; o cheiro
        // do corpo com conteúdo não foi medido e também sai como `text/plain` (resposta de redirecionamento: vazio).
        mime_type: match mime_type {
            Some(m) => m,
            None if (300..400).contains(&status) => String::new(),
            None => "text/plain".to_owned(),
        },
        charset,
        protocol: match version {
            (2, _) => "h2".to_owned(),
            (major, minor) => format!("http/{major}.{minor}"),
        },
        remote_ip: if synthetic { String::new() } else { url.host_for_url().trim_matches(|c| c == '[' || c == ']').to_owned() },
        remote_port: if synthetic { 0 } else { url.port_or_default() },
        synthetic,
    }
}

impl NetMeta {
    pub fn from_response(response: &Response) -> NetMeta {
        let hop = |h: &Hop| exchange(&h.url, h.status, &h.reason, &h.headers, &h.head, &h.sent, h.version, false);
        NetMeta {
            redirects: response.hops.iter().map(hop).collect(),
            last: exchange(&response.url, response.status, &response.reason, &response.headers, &response.head, &response.sent, response.transfer.version, response.synthetic),
            data_length: response.body.len(),
            body_encoded: response.transfer.encoded_size,
            body: response.body.clone(),
        }
    }

    /// O documento `data:`: sem fio, sem cabeça, sem pedido.
    pub fn data(address: &str, mime: &str, length: usize) -> NetMeta {
        NetMeta {
            redirects: Vec::new(),
            last: NetExchange {
                url: address.to_owned(),
                status: 200,
                status_text: "OK".to_owned(),
                headers: vec![("Content-Type".to_owned(), mime.to_owned())],
                head: String::new(),
                sent: Vec::new(),
                mime_type: mime.to_owned(),
                charset: String::new(),
                protocol: "data".to_owned(),
                remote_ip: String::new(),
                remote_port: 0,
                synthetic: false,
            },
            data_length: length,
            body_encoded: length,
            body: Vec::new(),
        }
    }
}
