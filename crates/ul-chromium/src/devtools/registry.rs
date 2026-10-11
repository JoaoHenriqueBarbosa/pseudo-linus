//! O domínio registrável (`domainAndRegistry` do `Page.Frame`): o `GetDomainAndRegistry` de
//! `net/base/registry_controlled_domains` com `INCLUDE_PRIVATE_REGISTRIES`, sobre a lista de sufixos públicos do
//! Chromium 154 (`data/devtools/effective_tld_names.dat`, a mesma revisão do oráculo). Regras curinga (`*.`) e de
//! exceção (`!`) como na especificação da lista; TLD que a lista não conhece não tem domínio registrável.

use std::collections::HashSet;
use std::sync::OnceLock;

const LIST: &str = include_str!("../../data/devtools/effective_tld_names.dat");

struct Rules {
    exact: HashSet<String>,
    wildcard: HashSet<String>,
    exception: HashSet<String>,
}

fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| {
        let mut rules = Rules { exact: HashSet::new(), wildcard: HashSet::new(), exception: HashSet::new() };
        for line in LIST.lines() {
            let rule = line.split_whitespace().next().unwrap_or_default();
            if rule.is_empty() || rule.starts_with("//") {
                continue;
            }
            let rule = rule.to_lowercase();
            if let Some(rest) = rule.strip_prefix('!') {
                rules.exception.insert(rest.to_owned());
            } else if let Some(rest) = rule.strip_prefix("*.") {
                rules.wildcard.insert(rest.to_owned());
            } else {
                rules.exact.insert(rule);
            }
        }
        rules
    })
}

/// O comprimento em rótulos do sufixo público de `labels` (do fim), ou `None` sem regra que case.
fn suffix_labels(labels: &[&str]) -> Option<usize> {
    let rules = rules();
    let mut best = None;
    for take in 1..=labels.len() {
        let candidate = labels[labels.len() - take..].join(".");
        if rules.exception.contains(&candidate) {
            return Some(take - 1);
        }
        if rules.exact.contains(&candidate) {
            best = Some(take);
        }
        if take < labels.len() && rules.wildcard.contains(&candidate) {
            best = Some(take + 1);
        }
    }
    best
}

/// O domínio registrável do host (sufixo público mais um rótulo), vazio para IP, host sem ponto, TLD desconhecido ou
/// host que é ele mesmo um sufixo público. Os nomes chegam já em ASCII (o host da URL canônica).
pub fn domain_and_registry(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') {
        return String::new();
    }
    let labels: Vec<&str> = host.split('.').collect();
    match suffix_labels(&labels) {
        Some(suffix) if suffix > 0 && suffix < labels.len() => labels[labels.len() - suffix - 1..].join("."),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::domain_and_registry;

    #[test]
    fn registrable_domains() {
        assert_eq!(domain_and_registry("www.example.com"), "example.com");
        assert_eq!(domain_and_registry("a.b.example.co.uk"), "example.co.uk");
        assert_eq!(domain_and_registry("co.uk"), "");
        assert_eq!(domain_and_registry("localhost"), "");
        assert_eq!(domain_and_registry("127.0.0.1"), "");
        assert_eq!(domain_and_registry("foo.github.io"), "foo.github.io");
        assert_eq!(domain_and_registry("x.city.kawasaki.jp"), "city.kawasaki.jp");
        assert_eq!(domain_and_registry("a.b.kawasaki.jp"), "a.b.kawasaki.jp");
        assert_eq!(domain_and_registry("t.test"), "");
    }
}
