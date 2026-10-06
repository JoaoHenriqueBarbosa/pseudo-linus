//! `curl` e `wget` do pseudo-linus: HTTP/1.1 e HTTPS (rustls) sobre o `net_connect` do kernel.
//!
//! - [`net`]: a pilha comum (URL, TCP com prazo, TLS, HTTP/1.1, decodificação de corpo, cookies).

pub mod net {
    pub mod cookies;
    pub mod http;
    pub mod io;
    pub mod tls;
    pub mod tz;
    pub mod url;
}

pub mod curl;
pub mod wget;

/// Os programas deste crate.
pub fn programs() -> Vec<sysabi::Program> {
    vec![sysabi::Program::bin("curl", curl::main), sysabi::Program::bin("wget", wget::main)]
}
