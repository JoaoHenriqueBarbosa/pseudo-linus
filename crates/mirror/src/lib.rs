//! Espelho do PyPI servido pelo host ao sandbox: Simple API (PEP 503, 658 e 691) sobre HTTP/1.1,
//! com TLS (certificado de `pypi.sandbox` assinado pela CA embutida em [`CA_PEM`]).

mod http;
mod index;
mod zip;

use index::Index;
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::io::{self, Read, Write};
use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::Arc;

/// Nome do servidor, como o sandbox o resolve.
pub const HOST: &str = "pypi.sandbox";
/// Endereço de loopback ao qual o nome resolve dentro do sandbox.
pub const ADDR: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 80);
pub const PORT: u16 = 443;
/// CA que assina o certificado do servidor, para instalar no repositório de confiança do sandbox.
pub const CA_PEM: &[u8] = include_bytes!("../certs/ca.crt");

const SERVER_CERT_PEM: &[u8] = include_bytes!("../certs/server.crt");
const SERVER_KEY_PEM: &[u8] = include_bytes!("../certs/server.key");

pub struct Mirror {
    index: Index,
    tls: Arc<ServerConfig>,
}

fn tls_config() -> io::Result<Arc<ServerConfig>> {
    let certs = CertificateDer::pem_slice_iter(SERVER_CERT_PEM)
        .collect::<Result<Vec<_>, _>>()
        .map_err(io::Error::other)?;
    let key = PrivateKeyDer::from_pem_slice(SERVER_KEY_PEM).map_err(io::Error::other)?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS12, &rustls::version::TLS13])
        .map_err(io::Error::other)?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(io::Error::other)?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

impl Mirror {
    /// Indexa os `.whl`, `.tar.gz` e `.zip` do diretório (sem recursão).
    pub fn from_dir(dir: &Path) -> io::Result<Mirror> {
        Ok(Mirror {
            index: Index::load(dir)?,
            tls: tls_config()?,
        })
    }

    /// TLS servidor sobre o stream; atende requisições HTTP/1.1 com keep-alive até EOF.
    pub fn serve_tls<S: Read + Write>(&self, stream: S) -> io::Result<()> {
        let conn = ServerConnection::new(self.tls.clone()).map_err(io::Error::other)?;
        let mut tls = StreamOwned::new(conn, stream);
        let result = http::serve(&self.index, &mut tls);
        tls.conn.send_close_notify();
        let _ = tls.flush();
        result
    }

    /// O mesmo atendimento sem TLS.
    pub fn serve_plain<S: Read + Write>(&self, mut stream: S) -> io::Result<()> {
        http::serve(&self.index, &mut stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::path::PathBuf;

    struct Mem {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl Read for Mem {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.input.read(buf)
        }
    }

    impl Write for Mem {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.output.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Zip com entradas armazenadas (método 0) ou em um bloco deflate "stored" (método 8).
    /// O CRC fica zerado: o leitor não o confere.
    fn make_zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data, deflate) in entries {
            let payload: Vec<u8> = if *deflate {
                let n = data.len() as u16;
                let mut p = vec![0x01];
                p.extend_from_slice(&n.to_le_bytes());
                p.extend_from_slice(&(!n).to_le_bytes());
                p.extend_from_slice(data);
                p
            } else {
                data.to_vec()
            };
            let method: u16 = if *deflate { 8 } else { 0 };
            let offset = out.len() as u32;
            let mut fixed = Vec::new();
            fixed.extend_from_slice(&20u16.to_le_bytes());
            fixed.extend_from_slice(&0u16.to_le_bytes());
            fixed.extend_from_slice(&method.to_le_bytes());
            fixed.extend_from_slice(&[0; 4]);
            fixed.extend_from_slice(&0u32.to_le_bytes());
            fixed.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            fixed.extend_from_slice(&(data.len() as u32).to_le_bytes());
            fixed.extend_from_slice(&(name.len() as u16).to_le_bytes());
            fixed.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
            out.extend_from_slice(&fixed);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&payload);
            central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0]);
            central.extend_from_slice(&fixed);
            central.extend_from_slice(&[0; 10]);
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    const METADATA: &str =
        "Metadata-Version: 2.1\r\nName: Demo.Pkg\r\nVersion: 1.0\r\nRequires-Python: >=3.8\r\n\r\nCorpo\r\n";
    const WHEEL: &str = "demo_pkg-1.0-py3-none-any.whl";

    struct Fixture {
        dir: PathBuf,
        mirror: Mirror,
        wheel_bytes: Vec<u8>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn fixture(tag: &str, deflate: bool) -> Fixture {
        let dir = std::env::temp_dir().join(format!("mirror-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wheel_bytes = make_zip(&[
            ("demo_pkg/__init__.py", b"x = 1\n", false),
            ("demo_pkg-1.0.dist-info/METADATA", METADATA.as_bytes(), deflate),
        ]);
        std::fs::write(dir.join(WHEEL), &wheel_bytes).unwrap();
        std::fs::write(dir.join("Other_Thing-2.0.tar.gz"), b"tarball").unwrap();
        std::fs::write(dir.join("leia-me.txt"), b"ignorado").unwrap();
        let mirror = Mirror::from_dir(&dir).unwrap();
        Fixture { dir, mirror, wheel_bytes }
    }

    fn ask(m: &Mirror, raw: &str) -> (String, Vec<u8>) {
        let mut mem = Mem {
            input: Cursor::new(raw.as_bytes().to_vec()),
            output: Vec::new(),
        };
        m.serve_plain(&mut mem).unwrap();
        let pos = mem.output.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8(mem.output[..pos].to_vec()).unwrap();
        (head, mem.output[pos + 4..].to_vec())
    }

    fn get(m: &Mirror, path: &str, accept: Option<&str>) -> (String, Vec<u8>) {
        let acc = accept.map_or(String::new(), |a| format!("Accept: {a}\r\n"));
        ask(m, &format!("GET {path} HTTP/1.1\r\nHost: pypi.sandbox\r\n{acc}Connection: close\r\n\r\n"))
    }

    #[test]
    fn html_project_page() {
        let f = fixture("html", false);
        let (head, body) = get(&f.mirror, "/simple/demo-pkg/", None);
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "{head}");
        assert!(head.contains("Content-Type: text/html\r\n"));
        assert!(head.contains(&format!("Content-Length: {}\r\n", body.len())));
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("<meta name=\"pypi:repository-version\" content=\"1.1\">"));
        assert!(text.contains(&format!("href=\"../../files/{WHEEL}#sha256=")));
        assert!(text.contains("data-requires-python=\"&gt;=3.8\""));
        assert!(text.contains("data-dist-info-metadata=\"sha256="));
        assert!(text.contains("data-core-metadata=\"sha256="));
        assert!(!text.contains(f.dir.to_str().unwrap()));
        let (head, _) = get(&f.mirror, "/simple/demo-pkg/", Some("application/vnd.pypi.simple.v1+html"));
        assert!(head.contains("Content-Type: application/vnd.pypi.simple.v1+html\r\n"));
    }

    #[test]
    fn json_project_page_and_root() {
        let f = fixture("json", true);
        let accept = "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.2, text/html;q=0.01";
        let (head, body) = get(&f.mirror, "/simple/demo-pkg/", Some(accept));
        assert!(head.contains("Content-Type: application/vnd.pypi.simple.v1+json\r\n"));
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("\"meta\":{\"api-version\":\"1.1\"}"));
        assert!(text.contains(&format!("\"filename\":\"{WHEEL}\"")));
        assert!(text.contains("\"requires-python\":\">=3.8\""));
        assert!(text.contains("\"core-metadata\":{\"sha256\":\""));
        assert!(text.contains("\"versions\":[\"1.0\"]"));
        let (_, body) = get(&f.mirror, "/simple/", Some(accept));
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("{\"name\":\"demo-pkg\"}"));
        assert!(text.contains("{\"name\":\"other-thing\"}"));
        let (_, body) = get(&f.mirror, "/simple/", None);
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("<a href=\"/simple/other-thing/\">other-thing</a>"));
    }

    #[test]
    fn redirect_and_not_found() {
        let f = fixture("redirect", false);
        let (head, _) = get(&f.mirror, "/simple/Demo.Pkg/", None);
        assert!(head.starts_with("HTTP/1.1 301 Moved Permanently\r\n"), "{head}");
        assert!(head.contains("Location: /simple/demo-pkg/\r\n"));
        let (head, _) = get(&f.mirror, "/simple/demo-pkg", None);
        assert!(head.starts_with("HTTP/1.1 301"));
        let (head, _) = get(&f.mirror, "/simple/nao-existe/", None);
        assert!(head.starts_with("HTTP/1.1 404 Not Found\r\n"));
        let (head, _) = get(&f.mirror, "/files/nao-existe.whl", None);
        assert!(head.starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn download_and_metadata() {
        for deflate in [false, true] {
            let f = fixture(if deflate { "dl-d" } else { "dl-s" }, deflate);
            let (head, body) = get(&f.mirror, &format!("/files/{WHEEL}"), None);
            assert!(head.contains("Content-Type: application/octet-stream\r\n"));
            assert_eq!(body, f.wheel_bytes);
            let (head, body) = get(&f.mirror, &format!("/files/{WHEEL}.metadata"), None);
            assert!(head.starts_with("HTTP/1.1 200 OK"));
            assert_eq!(body, METADATA.as_bytes());
            let (head, body) = ask(
                &f.mirror,
                &format!("HEAD /files/{WHEEL} HTTP/1.1\r\nConnection: close\r\n\r\n"),
            );
            assert!(head.contains(&format!("Content-Length: {}\r\n", f.wheel_bytes.len())));
            assert!(body.is_empty());
        }
    }

    #[test]
    fn keep_alive_serves_two_requests() {
        let f = fixture("keepalive", false);
        let raw = "GET /simple/ HTTP/1.1\r\nHost: x\r\n\r\nGET /simple/demo-pkg/ HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n";
        let mut mem = Mem { input: Cursor::new(raw.as_bytes().to_vec()), output: Vec::new() };
        f.mirror.serve_plain(&mut mem).unwrap();
        let text = String::from_utf8(mem.output).unwrap();
        assert_eq!(text.matches("HTTP/1.1 200 OK").count(), 2);
        assert!(text.contains("Connection: keep-alive\r\n"));
        assert!(text.contains("Connection: close\r\n"));
    }

    #[test]
    fn normalize_follows_pep_503() {
        assert_eq!(index::normalize("Friendly-Bard"), "friendly-bard");
        assert_eq!(index::normalize("Friendly.Bard__x"), "friendly-bard-x");
    }
}
