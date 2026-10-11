//! Os eventos de `Network` do DevTools montados a partir de uma [`NetExchange`]: o JSON de cada mensagem como o
//! Chromium 154 a mandou ao oráculo (cenários `network_navigate` e `network_http` de `tests/devtools`).

use super::json::Json;
use crate::netmeta::NetExchange;

fn pairs(headers: &[(String, String)]) -> Json {
    Json::Object(headers.iter().map(|(k, v)| (k.clone(), Json::str(v))).collect())
}

fn number(value: f64) -> Json {
    Json::Number(value)
}

/// O espaço de endereços do servidor, como o `resourceIPAddressSpace`.
fn address_space(ip: &str) -> &'static str {
    let private = ip.starts_with("10.") || ip.starts_with("192.168.") || ip.strip_prefix("172.").and_then(|r| r.split('.').next()).and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (16..32).contains(&n));
    if ip.starts_with("127.") || ip == "::1" {
        "Loopback"
    } else if private {
        "Local"
    } else {
        "Public"
    }
}

/// `scheme://host` sem a porta: a `topLevelSite` da partição de cookies.
fn site(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else { return String::new() };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = if let Some(end) = host.rfind(']') { &host[..=end] } else { host.split(':').next().unwrap_or_default() };
    format!("{scheme}://{host}")
}

/// A `Network.Response` de uma troca (`responseReceived` e `redirectResponse`).
pub fn response(e: &NetExchange, secure: bool, request_time: f64, response_time_ms: f64) -> Json {
    let data = e.protocol == "data";
    // Sem conexão (`data:` ou a resposta do `Fetch.fulfillRequest`): nem endereço nem porta.
    let wireless = data || e.synthetic;
    let mut fields = vec![
        ("url", Json::str(&e.url)),
        ("status", number(f64::from(e.status))),
        ("statusText", Json::str(&e.status_text)),
        ("headers", pairs(&e.headers)),
        ("mimeType", Json::str(&e.mime_type)),
        ("charset", Json::str(&e.charset)),
        ("connectionReused", Json::Bool(false)),
        ("connectionId", number(0.0)),
    ];
    if wireless {
        fields.push(("remoteIPAddress", Json::str("")));
        fields.push(("remotePort", number(0.0)));
    } else {
        fields.push(("remoteIPAddress", Json::str(&e.remote_ip)));
        fields.push(("remotePort", number(f64::from(e.remote_port))));
    }
    fields.extend([
        ("fromDiskCache", Json::Bool(false)),
        ("fromServiceWorker", Json::Bool(false)),
        ("fromPrefetchCache", Json::Bool(false)),
        ("encodedDataLength", number(if data { -1.0 } else { e.head.len() as f64 })),
    ]);
    if !data {
        let unset = ["proxyStart", "proxyEnd", "dnsStart", "dnsEnd", "connectStart", "connectEnd", "sslStart", "sslEnd", "workerStart", "workerReady", "workerFetchStart", "workerRespondWithSettled"];
        let mut timing = vec![("requestTime", number(request_time))];
        timing.extend(unset.iter().map(|k| (*k, number(-1.0))));
        timing.extend(["sendStart", "sendEnd", "pushStart", "pushEnd", "receiveHeadersStart", "receiveHeadersEnd"].iter().map(|k| (*k, number(0.0))));
        fields.push(("timing", Json::Object(timing.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())));
        fields.push(("responseTime", number(response_time_ms)));
    }
    fields.push(("protocol", Json::str(&e.protocol)));
    fields.push(("alternateProtocolUsage", Json::str(if wireless { "alternativeJobWonWithoutRace" } else { "unspecifiedReason" })));
    fields.push(("securityState", Json::str(if secure { "secure" } else { "insecure" })));
    Json::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// O corpo do `requestWillBeSent`: o pedido, quem o iniciou e, num redirecionamento, a resposta que o causou.
#[allow(clippy::too_many_arguments)]
pub fn request_will_be_sent(request_id: &str, loader_id: &str, frame_id: &str, document_url: &str, url: &str, headers: &[(&str, &str)], resource_type: &str, priority: &str, same_site: bool, initiator: Json, redirect: Option<Json>, blocking: bool, timestamp: f64, wall_time: f64) -> Json {
    let headers: Vec<(String, String)> = headers.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    let mut fields = vec![
        ("requestId", Json::str(request_id)),
        ("loaderId", Json::str(loader_id)),
        ("documentURL", Json::str(document_url)),
        (
            "request",
            Json::object([
                ("url", Json::str(url)),
                ("method", Json::str("GET")),
                ("headers", pairs(&headers)),
                ("mixedContentType", Json::str("none")),
                ("initialPriority", Json::str(priority)),
                ("referrerPolicy", Json::str("strict-origin-when-cross-origin")),
                ("isSameSite", Json::Bool(same_site)),
            ]),
        ),
        ("timestamp", number(timestamp)),
        ("wallTime", number(wall_time)),
        ("initiator", initiator),
        ("redirectHasExtraInfo", Json::Bool(redirect.is_some())),
    ];
    if let Some(redirect) = redirect {
        fields.push(("redirectResponse", redirect));
    }
    fields.extend([("type", Json::str(resource_type)), ("frameId", Json::str(frame_id)), ("hasUserGesture", Json::Bool(false))]);
    if blocking {
        fields.push(("renderBlockingBehavior", Json::str("Blocking")));
    }
    Json::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// O espaço de endereços do documento que pede os subrecursos: o do servidor que o entregou, `Unknown` quando a resposta é
/// do cliente do `Fetch` ou não veio da rede.
pub fn document_space(meta: Option<&crate::netmeta::NetMeta>) -> &'static str {
    meta.filter(|m| !m.last.synthetic && m.last.protocol != "data").map_or("Unknown", |m| address_space(&m.last.remote_ip))
}

/// `Network.requestWillBeSentExtraInfo`: os cabeçalhos que saíram no fio. O pedido de subrecurso leva o
/// `clientSecurityState` do iniciador, com o espaço de endereços do documento (`initiator`).
pub fn request_extra_info(request_id: &str, e: &NetExchange, request_time: f64, initiator: Option<&str>, cookies: Json) -> Json {
    let mut fields = vec![
        ("requestId", Json::str(request_id)),
        ("associatedCookies", cookies),
        ("headers", pairs(&e.sent)),
        ("connectTiming", Json::object([("requestTime", number(request_time))])),
    ];
    if let Some(space) = initiator {
        fields.push((
            "clientSecurityState",
            Json::object([
                ("initiatorIsSecureContext", Json::Bool(true)),
                ("initiatorIPAddressSpace", Json::str(space)),
                ("localNetworkAccessRequestPolicy", Json::str("PermissionBlock")),
            ]),
        ));
    }
    fields.push(("siteHasCookieInOtherPartition", Json::Bool(false)));
    Json::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// `Network.responseReceivedExtraInfo`: a cabeça crua e o espaço de endereços do servidor.
pub fn response_extra_info(request_id: &str, e: &NetExchange) -> Json {
    Json::object([
        ("requestId", Json::str(request_id)),
        ("blockedCookies", Json::Array(Vec::new())),
        ("headers", pairs(&e.headers)),
        ("resourceIPAddressSpace", Json::str(address_space(&e.remote_ip))),
        ("statusCode", number(f64::from(e.status))),
        ("headersText", Json::str(&e.head)),
        ("cookiePartitionKey", Json::object([("topLevelSite", Json::str(site(&e.url))), ("hasCrossSiteAncestor", Json::Bool(false))])),
        ("cookiePartitionKeyOpaque", Json::Bool(false)),
        ("exemptedCookies", Json::Array(Vec::new())),
    ])
}

/// A posição, no HTML do documento, da tag que pediu `address`: `(linha, coluna)` logo depois do `>` da tag de abertura,
/// onde o parser estava ao pedir o recurso (medido no oráculo: a coluna 33 de `<link rel=stylesheet href=/s.css>`).
/// `None` quando nenhuma tag `link`, `script` ou `img` do texto aponta para o endereço.
pub fn parser_position(source: &str, document_url: &str, address: &str) -> Option<(usize, usize)> {
    let base = url::Url::parse(document_url).ok()?;
    let bytes = source.as_bytes();
    let mut at = 0;
    while let Some(found) = source[at..].find('<') {
        let start = at + found;
        at = start + 1;
        let rest = &source[start + 1..];
        let name_len = rest.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(rest.len());
        let attribute = match rest[..name_len].to_ascii_lowercase().as_str() {
            "link" => "href",
            "script" | "img" => "src",
            _ => continue,
        };
        // O fim da tag: o primeiro `>` fora de aspas.
        let mut quote = 0u8;
        let mut end = None;
        for (i, &b) in bytes.iter().enumerate().skip(start) {
            match (quote, b) {
                (0, b'"' | b'\'') => quote = b,
                (q, b) if q != 0 && q == b => quote = 0,
                (0, b'>') => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let end = end?;
        let tag = &source[start..end];
        if attribute_value(tag, attribute).and_then(|v| base.join(&v).ok()).is_some_and(|u| u.as_str() == address) {
            let line = source[..=end].matches('\n').count();
            let line_start = source[..=end].rfind('\n').map_or(0, |i| i + 1);
            return Some((line, end + 1 - line_start));
        }
    }
    None
}

/// O valor do atributo `name` de uma tag de abertura (sem o `>`), com ou sem aspas.
fn attribute_value(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        let before_ok = at > 0 && lower.as_bytes()[at - 1].is_ascii_whitespace();
        let after = tag[at + name.len()..].trim_start();
        if !before_ok || !after.starts_with('=') {
            continue;
        }
        let value = after[1..].trim_start();
        return Some(match value.chars().next()? {
            q @ ('"' | '\'') => value[1..].split(q).next()?.to_owned(),
            _ => value.split(|c: char| c.is_ascii_whitespace() || c == '>').next()?.to_owned(),
        });
    }
    None
}
