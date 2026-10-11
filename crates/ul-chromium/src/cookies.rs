//! Os cookies do contexto: o pote do perfil visto pelo DevTools (`Storage.getCookies`, `setCookies`, `clearCookies`),
//! pelo `document.cookie` e pelo `Cookie:` e o `Set-Cookie` das requisições. O formato do JSON é o do Chromium 154
//! (cenário `playwright_state` de `tests/devtools`).

use ul_net::net::client::Response;
use ul_net::net::cookies::{Cookie, Jar};

use crate::devtools::json::Json;

/// Segundos desde a época.
pub fn now() -> i64 {
    (ul_common::time::realtime_nanos() / 1_000_000_000) as i64
}

/// O que um pedido a `url` mostra ao pote: o host, o caminho sem a query e se é `https`. `None` para o que não tem
/// cookie (`about:`, `data:`, `file:`).
pub fn scope(url: &str) -> Option<(String, String, bool)> {
    let parsed = url::Url::parse(url).ok()?;
    let secure = match parsed.scheme() {
        "https" | "wss" => true,
        "http" | "ws" => false,
        _ => return None,
    };
    Some((parsed.host_str()?.to_owned(), parsed.path().to_owned(), secure))
}

fn is_ip(host: &str) -> bool {
    host.starts_with('[') || host.parse::<std::net::IpAddr>().is_ok()
}

/// O cookie como o `Network.Cookie` do protocolo. `domain` leva o ponto à frente quando o cookie vale para subdomínios.
pub fn cookie_json(c: &Cookie) -> Json {
    let mut fields = vec![
        ("name", Json::str(&c.name)),
        ("value", Json::str(&c.value)),
        ("domain", Json::str(if c.tailmatch { format!(".{}", c.domain) } else { c.domain.clone() })),
        ("path", Json::str(&c.path)),
        ("expires", Json::Number(if c.expires == 0 { -1.0 } else { c.expires as f64 })),
        ("size", Json::Number((c.name.len() + c.value.len()) as f64)),
        ("httpOnly", Json::Bool(c.httponly)),
        ("secure", Json::Bool(c.secure)),
        ("session", Json::Bool(c.expires == 0)),
    ];
    if !c.same_site.is_empty() {
        fields.push(("sameSite", Json::str(&c.same_site)));
    }
    // O esquema e a porta de quem criou o cookie: o Chromium guarda os do `url` ou do pedido; aqui valem os padrões.
    fields.extend([
        ("priority", Json::str("Medium")),
        ("sourceScheme", Json::str(if c.secure { "Secure" } else { "NonSecure" })),
        ("sourcePort", Json::Number(if c.secure { 443.0 } else { 80.0 })),
    ]);
    Json::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// `Storage.getCookies`: todos os cookies vivos do pote.
pub fn get(jar: &Jar) -> Json {
    let now = now();
    let live: Vec<Json> = jar.cookies.iter().filter(|c| c.expires == 0 || c.expires > now).map(cookie_json).collect();
    Json::object([("cookies", Json::Array(live))])
}

/// `Storage.setCookies`: cada cookie do `CookieParam` vira uma entrada do pote (a de mesmo nome, domínio e caminho
/// é trocada). Campo que falta é erro, e nada entra.
pub fn set(jar: &mut Jar, cookies: &Json) -> Result<(), (i64, String)> {
    let Json::Array(list) = cookies else { return Err((-32602, "Invalid parameters".to_owned())) };
    let invalid = || (-32602, "Invalid cookie fields".to_owned());
    let mut parsed = Vec::new();
    for param in list {
        let text = |key: &str| param.get(key).and_then(Json::as_str).filter(|v| !v.is_empty());
        let name = param.get("name").and_then(Json::as_str).ok_or_else(invalid)?;
        let value = param.get("value").and_then(Json::as_str).ok_or_else(invalid)?;
        let url = text("url").and_then(|u| url::Url::parse(u).ok());
        let domain = text("domain").map(|d| d.trim_start_matches('.').to_owned());
        let host = domain.clone().or_else(|| url.as_ref().and_then(|u| u.host_str().map(str::to_owned))).ok_or_else(invalid)?;
        let default_path = url.as_ref().map_or("/".to_owned(), |u| {
            let path = u.path();
            match path.rfind('/') {
                Some(0) | None => "/".to_owned(),
                Some(end) => path[..end].to_owned(),
            }
        });
        let path = text("path").map_or(default_path, str::to_owned);
        let secure = param.get("secure").and_then(Json::as_bool).unwrap_or_else(|| url.as_ref().is_some_and(|u| u.scheme() == "https"));
        let http_only = param.get("httpOnly").and_then(Json::as_bool).unwrap_or(false);
        let expires = param.get("expires").and_then(Json::as_f64).filter(|e| *e > 0.0).map_or(0, |e| e as i64);
        let same_site = text("sameSite").unwrap_or("");
        parsed.push(Cookie::new(name, value, &host, domain.is_some() && !is_ip(&host), &path, secure, http_only, expires, same_site));
    }
    for cookie in parsed {
        jar.insert(cookie);
    }
    Ok(())
}

/// `associatedCookies` do `Network.requestWillBeSentExtraInfo`: os cookies que o pedido a `url` levou.
pub fn associated(jar: &Jar, url: &str) -> Json {
    let Some((host, path, secure)) = scope(url) else { return Json::Array(Vec::new()) };
    let cookies = jar.matching(&host, &path, secure, now());
    Json::Array(cookies.into_iter().map(|c| Json::object([("cookie", cookie_json(c)), ("blockedReasons", Json::Array(Vec::new())), ("exemptionReason", Json::str("None"))])).collect())
}

/// `document.cookie` de uma página em `url`: os cookies que o pedido levaria, sem os `HttpOnly`.
pub fn document_cookie(jar: &Jar, url: &str) -> String {
    let Some((host, path, secure)) = scope(url) else { return String::new() };
    let visible: Vec<String> = jar.matching(&host, &path, secure, now()).into_iter().filter(|c| !c.httponly).map(|c| format!("{}={}", c.name, c.value)).collect();
    visible.join("; ")
}

/// A atribuição a `document.cookie`: o script não cria cookie `HttpOnly`.
pub fn set_document_cookie(jar: &mut Jar, value: &str, url: &str) {
    let Some((host, path, _)) = scope(url) else { return };
    if value.split(';').skip(1).any(|a| a.trim().eq_ignore_ascii_case("httponly")) {
        return;
    }
    jar.set_cookie(value, &host, &path, now());
}

/// Os `Set-Cookie` da resposta (e dos redirecionamentos que a precederam) entram no pote.
pub fn store_response(jar: &mut Jar, response: &Response) {
    let now = now();
    let exchanges = response.hops.iter().map(|h| (&h.url, &h.headers)).chain(std::iter::once((&response.url, &response.headers)));
    for (url, headers) in exchanges {
        let host = url.host.as_str();
        let path = if url.path.is_empty() { "/" } else { url.path.as_str() };
        for (name, value) in headers {
            if name.eq_ignore_ascii_case(b"set-cookie") {
                jar.set_cookie(&String::from_utf8_lossy(value), host, path, now);
            }
        }
    }
}
