//! TLS com rustls sobre a conexão do kernel.
//!
//! As raízes vêm do arquivo de CAs do sandbox (`/etc/ssl/certs/ca-certificates.crt` no Debian, ou o
//! que `--cacert`/`CURL_CA_BUNDLE`/`SSL_CERT_FILE` indicarem): nada é lido do host. Sem verificação
//! (`curl -k`, `wget --no-check-certificate`) a cadeia não é conferida, mas a assinatura do handshake
//! continua sendo.

use std::io::{self, Read, Write};
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, ring};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore, SignatureScheme, StreamOwned};

use super::io::Tcp;

/// Arquivo de CAs padrão do Debian.
pub const DEFAULT_CA_BUNDLE: &str = "/etc/ssl/certs/ca-certificates.crt";

/// Como conferir o certificado do servidor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verify {
    /// Raízes deste PEM (conteúdo já lido do FS do sandbox).
    Roots(Vec<u8>),
    /// Não confere (`-k`).
    Insecure,
}

/// Erro de TLS classificado como o curl e o wget distinguem.
#[derive(Debug)]
pub enum TlsError {
    /// Certificado recusado: a mensagem no estilo do OpenSSL.
    Verify(String),
    /// Nome do certificado não confere com o host.
    NameMismatch,
    /// Falha do handshake (protocolo, alerta do servidor).
    Handshake(String),
    /// Erro de I/O no meio do handshake.
    Io(io::Error),
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(ring::default_provider())
}

#[derive(Debug)]
struct NoVerify(Arc<CryptoProvider>);

impl ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Configuração do cliente. `Err` se o PEM não tem nenhum certificado utilizável.
pub fn client_config(verify: &Verify, alpn: &[&[u8]]) -> Result<Arc<ClientConfig>, String> {
    let prov = provider();
    let builder = ClientConfig::builder_with_provider(prov.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?;
    let mut cfg = match verify {
        Verify::Roots(pem) => {
            let mut roots = RootCertStore::empty();
            let mut any = false;
            for cert in CertificateDer::pem_slice_iter(pem).flatten() {
                if roots.add(cert).is_ok() {
                    any = true;
                }
            }
            if !any {
                return Err("no certificates".into());
            }
            builder.with_root_certificates(roots).with_no_client_auth()
        }
        Verify::Insecure => builder.dangerous().with_custom_certificate_verifier(Arc::new(NoVerify(prov))).with_no_client_auth(),
    };
    cfg.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    Ok(Arc::new(cfg))
}

/// Mensagem do OpenSSL pra um erro de certificado do rustls.
fn openssl_verify_message(e: &rustls::CertificateError) -> Option<String> {
    use rustls::CertificateError as C;
    Some(match e {
        C::UnknownIssuer => "unable to get local issuer certificate".into(),
        C::Expired | C::ExpiredContext { .. } => "certificate has expired".into(),
        C::NotValidYet | C::NotValidYetContext { .. } => "certificate is not yet valid".into(),
        C::Revoked => "certificate revoked".into(),
        C::BadSignature => "certificate signature failure".into(),
        C::NotValidForName | C::NotValidForNameContext { .. } => return None,
        other => format!("{other:?}"),
    })
}

fn classify(e: rustls::Error) -> TlsError {
    match e {
        rustls::Error::InvalidCertificate(ce) => match openssl_verify_message(&ce) {
            Some(m) => TlsError::Verify(m),
            None => TlsError::NameMismatch,
        },
        other => TlsError::Handshake(other.to_string()),
    }
}

/// Conexão TLS pronta (handshake feito).
pub struct TlsStream {
    pub inner: StreamOwned<ClientConnection, Tcp>,
}

impl TlsStream {
    /// Faz o handshake com `host` (SNI só quando o host é nome, como o OpenSSL do curl).
    pub fn handshake(cfg: Arc<ClientConfig>, host: &str, tcp: Tcp) -> Result<TlsStream, TlsError> {
        let name = match host.parse::<std::net::IpAddr>() {
            Ok(ip) => ServerName::IpAddress(ip.into()),
            Err(_) => ServerName::try_from(host.to_string()).map_err(|e| TlsError::Handshake(e.to_string()))?,
        };
        let conn = ClientConnection::new(cfg, name).map_err(classify)?;
        let mut s = StreamOwned::new(conn, tcp);
        while s.conn.is_handshaking() {
            match s.conn.complete_io(&mut s.sock) {
                Ok(_) => {}
                Err(e) => {
                    // O rustls embrulha o erro de protocolo num io::Error InvalidData.
                    if e.kind() == io::ErrorKind::InvalidData
                        && let Some(inner) = e.get_ref().and_then(|i| i.downcast_ref::<rustls::Error>())
                    {
                        return Err(classify(inner.clone()));
                    }
                    return Err(TlsError::Io(e));
                }
            }
        }
        Ok(TlsStream { inner: s })
    }

    pub fn tcp(&self) -> &Tcp {
        &self.inner.sock
    }

    pub fn tcp_mut(&mut self) -> &mut Tcp {
        &mut self.inner.sock
    }

    /// Versão e suíte negociadas, no formato do `curl -v` ("TLSv1.3 / TLS_AES_256_GCM_SHA384").
    pub fn describe(&self) -> (String, String) {
        let v = match self.inner.conn.protocol_version() {
            Some(rustls::ProtocolVersion::TLSv1_3) => "TLSv1.3".to_string(),
            Some(rustls::ProtocolVersion::TLSv1_2) => "TLSv1.2".to_string(),
            Some(other) => format!("{other:?}"),
            None => String::new(),
        };
        let suite = self.inner.conn.negotiated_cipher_suite().map(|s| format!("{:?}", s.suite())).unwrap_or_default();
        (v, suite)
    }

    pub fn alpn(&self) -> Option<Vec<u8>> {
        self.inner.conn.alpn_protocol().map(|p| p.to_vec())
    }

    /// Certificados que o servidor mandou (DER).
    pub fn peer_certs(&self) -> Vec<Vec<u8>> {
        self.inner.conn.peer_certificates().map(|c| c.iter().map(|d| d.as_ref().to_vec()).collect()).unwrap_or_default()
    }
}

impl Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.inner.read(buf) {
            // Servidor fechou sem close_notify: o curl trata como fim (com OpenSSL 3 sai erro só
            // quando ainda faltam bytes, o que o leitor do corpo já detecta).
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(0),
            r => r,
        }
    }
}

impl Write for TlsStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
