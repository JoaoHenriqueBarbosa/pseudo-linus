//! H34: `curl`/`wget` sobre `ureq` 3 com a allowlist aplicada num ponto só.
//!
//! A política ([`Policy`]) é um objeto só, compartilhado por dois ganchos do ureq:
//!
//! - [`PolicyResolver`] (trait `Resolver`): nega o nome antes de qualquer DNS, resolve pela tabela do
//!   sandbox ou pelo resolvedor padrão e descarta endereços privados/loopback quando a política manda.
//!   Como o ureq chama o resolvedor a cada salto de redirect, um redirect pra host negado morre aqui.
//! - [`PolicyGate`] (trait `Connector`, primeiro da cadeia): confere de novo host e endereços já
//!   resolvidos, imediatamente antes do `TcpConnector`. É a defesa em profundidade: mesmo que alguém
//!   troque o resolvedor, nenhum socket abre pra endereço negado.

pub mod experiments;

use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ureq::Agent;
use ureq::config::Config;
use ureq::http::Uri;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{ConnectionDetails, Connector, NextTimeout, RustlsConnector, TcpConnector, Transport};

/// Política de rede de um sandbox.
#[derive(Debug, Default)]
pub struct Policy {
    /// Nomes permitidos: exato (`api.example.com`) ou sufixo (`*.example.com`). IP literal só passa se
    /// estiver escrito aqui.
    pub allow: Vec<String>,
    /// Nega loopback, redes privadas, link-local, CGNAT, multicast e afins, mesmo pra nome permitido.
    pub block_private: bool,
    /// Registro das decisões (pra auditoria e pros testes).
    pub log: Mutex<Vec<String>>,
}

/// Normaliza o host como aparece na URI: minúsculas, sem ponto final, sem colchetes de IPv6.
pub fn normalize_host(host: &str) -> String {
    host.trim_start_matches('[').trim_end_matches(']').trim_end_matches('.').to_ascii_lowercase()
}

/// Endereço que não pode ser alcançado quando `block_private` está ligado.
pub fn is_internal(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64)
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 198 && (o[1] & 0xfe) == 18)
                || o[0] >= 240
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_internal(&IpAddr::V4(v4));
            }
            let s = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] == 0x64 && s[1] == 0xff9b)
                || (s[0] == 0x2001 && s[1] == 0x0db8)
        }
    }
}

fn denied(msg: String) -> ureq::Error {
    ureq::Error::Io(io::Error::new(io::ErrorKind::PermissionDenied, msg))
}

impl Policy {
    pub fn new(allow: &[&str], block_private: bool) -> Policy {
        Policy { allow: allow.iter().map(|s| s.to_ascii_lowercase()).collect(), block_private, log: Mutex::new(Vec::new()) }
    }

    fn note(&self, s: String) {
        self.log.lock().expect("log").push(s);
    }

    pub fn host_allowed(&self, host: &str) -> bool {
        let h = normalize_host(host);
        self.allow.iter().any(|a| match a.strip_prefix("*.") {
            Some(suffix) => h.ends_with(&format!(".{suffix}")),
            None => *a == h,
        })
    }

    pub fn check_host(&self, host: &str) -> Result<(), ureq::Error> {
        if self.host_allowed(host) {
            Ok(())
        } else {
            self.note(format!("deny host {host}"));
            Err(denied(format!("bloqueado pela política do sandbox: host {host} fora da allowlist")))
        }
    }

    pub fn addr_allowed(&self, ip: &IpAddr) -> bool {
        !(self.block_private && is_internal(ip))
    }
}

/// Resolvedor do sandbox: tabela de hosts própria (o `/etc/hosts` do sandbox) e, fora dela, o DNS.
#[derive(Debug)]
pub struct PolicyResolver {
    pub policy: Arc<Policy>,
    pub hosts: BTreeMap<String, Vec<IpAddr>>,
    /// Quantas vezes o resolvedor de verdade (DNS do host) foi chamado.
    pub dns_lookups: Arc<AtomicUsize>,
    inner: DefaultResolver,
}

impl PolicyResolver {
    pub fn new(policy: Arc<Policy>, hosts: BTreeMap<String, Vec<IpAddr>>) -> PolicyResolver {
        PolicyResolver { policy, hosts, dns_lookups: Arc::new(AtomicUsize::new(0)), inner: DefaultResolver::default() }
    }
}

impl Resolver for PolicyResolver {
    fn resolve(&self, uri: &Uri, config: &Config, timeout: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let host = uri.host().ok_or_else(|| ureq::Error::BadUri("sem host".into()))?;
        self.policy.check_host(host)?;
        let norm = normalize_host(host);
        let port = uri
            .port_u16()
            .unwrap_or(if uri.scheme_str() == Some("https") { 443 } else { 80 });
        let found: Vec<SocketAddr> = if let Ok(ip) = norm.parse::<IpAddr>() {
            vec![SocketAddr::new(ip, port)]
        } else if let Some(ips) = self.hosts.get(&norm) {
            ips.iter().map(|ip| SocketAddr::new(*ip, port)).collect()
        } else {
            self.dns_lookups.fetch_add(1, Ordering::Relaxed);
            self.inner.resolve(uri, config, timeout)?.iter().copied().collect()
        };
        let mut out = self.empty();
        let mut dropped = Vec::new();
        for a in found {
            if self.policy.addr_allowed(&a.ip()) {
                out.push(a);
            } else {
                dropped.push(a.ip().to_string());
            }
        }
        if out.is_empty() {
            self.policy.note(format!("deny {host}: só endereços internos {dropped:?}"));
            return Err(denied(format!("bloqueado pela política do sandbox: {host} resolve só para endereços internos {dropped:?}")));
        }
        self.policy.note(format!("allow {host} -> {:?}", out.iter().collect::<Vec<_>>()));
        Ok(out)
    }
}

/// Portão no começo da cadeia de conectores: confere host e endereços antes do TCP.
#[derive(Debug)]
pub struct PolicyGate {
    pub policy: Arc<Policy>,
}

impl<In: Transport> Connector<In> for PolicyGate {
    type Out = In;

    fn connect(&self, details: &ConnectionDetails, chained: Option<In>) -> Result<Option<In>, ureq::Error> {
        let host = details.uri.host().unwrap_or("");
        self.policy.check_host(host)?;
        if let Some(bad) = details.addrs.iter().find(|a| !self.policy.addr_allowed(&a.ip())) {
            self.policy.note(format!("gate deny {host} {bad}"));
            return Err(denied(format!("bloqueado pela política do sandbox: conexão para {bad}")));
        }
        Ok(chained)
    }
}

/// Agente HTTP do sandbox: proxy desligado (o padrão do ureq lê HTTP_PROXY do ambiente do host),
/// redirects seguidos pelo próprio ureq (cada salto passa pelo resolvedor e pelo portão).
pub fn sandbox_agent(policy: Arc<Policy>, hosts: BTreeMap<String, Vec<IpAddr>>) -> (Agent, Arc<AtomicUsize>) {
    let resolver = PolicyResolver::new(policy.clone(), hosts);
    let lookups = resolver.dns_lookups.clone();
    let config = Agent::config_builder()
        .proxy(None)
        .max_redirects(5)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(15)))
        .build();
    let connector = ().chain(PolicyGate { policy }).chain(TcpConnector::default()).chain(RustlsConnector::default());
    (Agent::with_parts(config, connector, resolver), lookups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_ranges() {
        for s in ["127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.1", "169.254.169.254", "100.64.0.1", "0.0.0.0", "::1", "fd00::1", "fe80::1", "::ffff:127.0.0.1", "224.0.0.1"] {
            assert!(is_internal(&s.parse().unwrap()), "{s}");
        }
        for s in ["93.184.215.14", "1.1.1.1", "2606:4700::1111"] {
            assert!(!is_internal(&s.parse().unwrap()), "{s}");
        }
    }

    #[test]
    fn host_matching() {
        let p = Policy::new(&["api.example.com", "*.allowed.test", "10.0.0.5"], true);
        assert!(p.host_allowed("API.Example.com."));
        assert!(p.host_allowed("x.allowed.test"));
        assert!(!p.host_allowed("allowed.test"));
        assert!(!p.host_allowed("evilapi.example.com"));
        assert!(!p.host_allowed("api.example.com.evil.net"));
        assert!(p.host_allowed("10.0.0.5"));
        assert!(!p.host_allowed("[::1]"));
    }
}
