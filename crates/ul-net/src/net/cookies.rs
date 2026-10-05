//! Cookies no formato e com as regras que o curl usa (`-b`, `-c`): leitura de arquivo Netscape ou
//! de linhas `Set-Cookie:`, `Set-Cookie` das respostas, escolha dos que vão no `Cookie:` e o arquivo
//! escrito no fim.

/// Um cookie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    /// Domínio sem o ponto inicial.
    pub domain: String,
    /// Vale pra subdomínios (o "TRUE" da segunda coluna).
    pub tailmatch: bool,
    pub path: String,
    pub secure: bool,
    pub httponly: bool,
    /// Segundos desde a época; 0 = de sessão.
    pub expires: i64,
    creation: u64,
}

/// O pote de cookies.
#[derive(Clone, Debug, Default)]
pub struct Jar {
    pub cookies: Vec<Cookie>,
    next: u64,
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// Dias desde 1970-01-01.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Data HTTP (RFC 1123, RFC 850 ou asctime, com as variações que os servidores mandam).
pub fn parse_http_date(s: &str) -> Option<i64> {
    let mut day = None;
    let mut month = None;
    let mut year = None;
    let mut hms = None;
    for tok in s.split([' ', ',', '-', '\t']).filter(|t| !t.is_empty()) {
        if tok.contains(':') {
            let p: Vec<i64> = tok.split(':').filter_map(|x| x.parse().ok()).collect();
            if p.len() == 3 {
                hms = Some((p[0], p[1], p[2]));
            }
            continue;
        }
        let lower = tok.to_ascii_lowercase();
        if let Some(m) = MONTHS.iter().position(|m| lower.starts_with(m)) {
            month = Some(m as i64 + 1);
            continue;
        }
        if let Ok(n) = tok.parse::<i64>() {
            if day.is_none() && n <= 31 && tok.len() <= 2 {
                day = Some(n);
            } else if year.is_none() {
                year = Some(if n < 70 { n + 2000 } else if n < 100 { n + 1900 } else { n });
            }
        }
    }
    let (h, mi, se) = hms.unwrap_or((0, 0, 0));
    Some(days_from_civil(year?, month?, day?) * 86400 + h * 3600 + mi * 60 + se)
}

/// O host casa com o domínio do cookie.
fn domain_match(host: &str, domain: &str, tailmatch: bool) -> bool {
    let h = host.to_ascii_lowercase();
    let d = domain.to_ascii_lowercase();
    if h == d {
        return true;
    }
    tailmatch && h.ends_with(&d) && h.as_bytes()[h.len() - d.len() - 1] == b'.'
}

fn path_match(req: &str, cookie: &str) -> bool {
    if cookie == "/" || req == cookie {
        return true;
    }
    req.starts_with(cookie) && (cookie.ends_with('/') || req.as_bytes().get(cookie.len()) == Some(&b'/'))
}

/// Diretório padrão de um caminho de pedido (RFC 6265 5.1.4).
fn default_path(req_path: &str) -> String {
    if !req_path.starts_with('/') {
        return "/".into();
    }
    match req_path.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => req_path[..i].to_string(),
    }
}

impl Jar {
    fn insert(&mut self, mut c: Cookie) {
        self.next += 1;
        c.creation = self.next;
        if let Some(old) = self
            .cookies
            .iter_mut()
            .find(|o| o.name == c.name && o.domain.eq_ignore_ascii_case(&c.domain) && o.path == c.path)
        {
            c.creation = old.creation;
            *old = c;
        } else {
            self.cookies.push(c);
        }
    }

    /// Lê um arquivo de cookies (Netscape, ou linhas `Set-Cookie:`).
    pub fn load(&mut self, data: &[u8], now: i64) {
        for line in String::from_utf8_lossy(data).lines() {
            let line = line.trim_end_matches('\r');
            if let Some(rest) = line.strip_prefix("Set-Cookie:") {
                self.set_cookie(rest.trim(), "", "/", now);
                continue;
            }
            let (httponly, l) = match line.strip_prefix("#HttpOnly_") {
                Some(r) => (true, r),
                None => (false, line),
            };
            if l.starts_with('#') || l.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() < 6 {
                continue;
            }
            let domain = f[0].trim_start_matches('.').to_string();
            let c = Cookie {
                name: f[5].to_string(),
                value: f.get(6).copied().unwrap_or("").to_string(),
                tailmatch: f[1].eq_ignore_ascii_case("TRUE"),
                domain,
                path: f[2].to_string(),
                secure: f[3].eq_ignore_ascii_case("TRUE"),
                httponly,
                expires: f[4].parse().unwrap_or(0),
                creation: 0,
            };
            if c.expires != 0 && c.expires < now {
                continue;
            }
            self.insert(c);
        }
    }

    /// Aplica um `Set-Cookie` recebido de `host` pra `req_path`.
    pub fn set_cookie(&mut self, header: &str, host: &str, req_path: &str, now: i64) {
        let mut parts = header.split(';');
        let Some(nv) = parts.next() else { return };
        let Some((name, value)) = nv.split_once('=') else { return };
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let mut c = Cookie {
            name,
            value: value.trim().to_string(),
            domain: host.to_string(),
            tailmatch: false,
            path: default_path(req_path),
            secure: false,
            httponly: false,
            expires: 0,
            creation: 0,
        };
        let mut max_age: Option<i64> = None;
        for attr in parts {
            let (k, v) = match attr.split_once('=') {
                Some((k, v)) => (k.trim(), v.trim()),
                None => (attr.trim(), ""),
            };
            match k.to_ascii_lowercase().as_str() {
                "domain" if !v.is_empty() => {
                    let d = v.trim_start_matches('.').to_string();
                    if !host.is_empty() && !domain_match(host, &d, true) {
                        return;
                    }
                    c.domain = d;
                    c.tailmatch = true;
                }
                "path" if v.starts_with('/') => c.path = v.to_string(),
                "secure" => c.secure = true,
                "httponly" => c.httponly = true,
                "expires" => {
                    if let Some(t) = parse_http_date(v) {
                        c.expires = t.max(1);
                    }
                }
                "max-age" => max_age = v.parse().ok(),
                _ => {}
            }
        }
        if let Some(ma) = max_age {
            c.expires = if ma <= 0 { 1 } else { now.saturating_add(ma) };
        }
        if c.expires != 0 && c.expires <= now {
            self.cookies.retain(|o| !(o.name == c.name && o.domain.eq_ignore_ascii_case(&c.domain) && o.path == c.path));
            return;
        }
        self.insert(c);
    }

    /// Valor do `Cookie:` pra um pedido (ordem do curl: caminho mais longo, domínio mais longo, nome
    /// mais longo, mais novo primeiro).
    pub fn header_for(&self, host: &str, path: &str, secure: bool, now: i64) -> Option<String> {
        let mut list: Vec<&Cookie> = self
            .cookies
            .iter()
            .filter(|c| (c.expires == 0 || c.expires > now) && (!c.secure || secure))
            .filter(|c| domain_match(host, &c.domain, c.tailmatch) && path_match(path, &c.path))
            .collect();
        if list.is_empty() {
            return None;
        }
        list.sort_by(|a, b| {
            b.path
                .len()
                .cmp(&a.path.len())
                .then(b.domain.len().cmp(&a.domain.len()))
                .then(b.name.len().cmp(&a.name.len()))
                .then(b.creation.cmp(&a.creation))
        });
        Some(list.iter().map(|c| format!("{}={}", c.name, c.value)).collect::<Vec<_>>().join("; "))
    }

    /// Conteúdo do arquivo do `-c` (mais novo primeiro).
    pub fn netscape(&self, now: i64) -> Vec<u8> {
        let mut out = String::from(
            "# Netscape HTTP Cookie File\n# https://curl.se/docs/http-cookies.html\n# This file was generated by libcurl! Edit at your own risk.\n\n",
        );
        let mut list: Vec<&Cookie> = self.cookies.iter().filter(|c| c.expires == 0 || c.expires > now).collect();
        list.sort_by_key(|c| std::cmp::Reverse(c.creation));
        for c in list {
            out.push_str(&format!(
                "{}{}{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                if c.httponly { "#HttpOnly_" } else { "" },
                if c.tailmatch { "." } else { "" },
                c.domain,
                if c.tailmatch { "TRUE" } else { "FALSE" },
                c.path,
                if c.secure { "TRUE" } else { "FALSE" },
                c.expires,
                c.name,
                c.value
            ));
        }
        out.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_send() {
        let mut j = Jar::default();
        j.set_cookie("a=1; Path=/", "example.com", "/x/y", 1000);
        j.set_cookie("b=2; Domain=.example.com; Max-Age=60", "www.example.com", "/x/y", 1000);
        assert_eq!(j.header_for("www.example.com", "/x/y", false, 1001).as_deref(), Some("b=2"));
        assert_eq!(j.header_for("example.com", "/x", false, 1001).as_deref(), Some("b=2; a=1"));
        let out = String::from_utf8(j.netscape(1001)).unwrap();
        assert!(out.contains(".example.com\tTRUE\t/x\tFALSE\t1060\tb\t2\n"), "{out}");
        assert_eq!(parse_http_date("Thu, 15 Jan 2026 12:00:00 GMT"), Some(1_768_478_400));
    }
}
