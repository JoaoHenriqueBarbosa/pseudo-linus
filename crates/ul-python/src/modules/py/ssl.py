"""ssl do sandbox: sem rede não há handshake. As classes existem para que `http.client`, `urllib` e afins
importem e falhem de forma natural (`SSLError`) ao tentar abrir uma conexão segura."""
import enum as _enum

OPENSSL_VERSION = 'OpenSSL 3.0.0 (sandbox sem TLS)'
OPENSSL_VERSION_INFO = (3, 0, 0, 0, 0)
HAS_SNI = True
HAS_TLSv1_2 = True
HAS_TLSv1_3 = True
HAS_ALPN = True
HAS_NPN = False

PROTOCOL_TLS = 2
PROTOCOL_TLS_CLIENT = 16
PROTOCOL_TLS_SERVER = 17
CERT_NONE = 0
CERT_OPTIONAL = 1
CERT_REQUIRED = 2
OP_NO_SSLv2 = 0
OP_NO_SSLv3 = 0x2000000
OP_NO_TLSv1 = 0x4000000
OP_NO_COMPRESSION = 0x20000
VERIFY_DEFAULT = 0
VERIFY_X509_STRICT = 32


class TLSVersion(_enum.IntEnum):
    MINIMUM_SUPPORTED = -2
    TLSv1_2 = 771
    TLSv1_3 = 772
    MAXIMUM_SUPPORTED = -1


class SSLError(OSError):
    pass


class SSLZeroReturnError(SSLError):
    pass


class SSLWantReadError(SSLError):
    pass


class SSLWantWriteError(SSLError):
    pass


class SSLEOFError(SSLError):
    pass


class SSLCertVerificationError(SSLError, ValueError):
    pass


CertificateError = SSLCertVerificationError


class SSLContext:
    def __init__(self, protocol=PROTOCOL_TLS):
        self.protocol = protocol
        self.verify_mode = CERT_REQUIRED if protocol == PROTOCOL_TLS_CLIENT else CERT_NONE
        self.check_hostname = protocol == PROTOCOL_TLS_CLIENT
        self.options = 0
        self.minimum_version = TLSVersion.MINIMUM_SUPPORTED
        self.maximum_version = TLSVersion.MAXIMUM_SUPPORTED
        self.verify_flags = 0
        self.post_handshake_auth = None
        self.keylog_filename = None
        self.hostname_checks_common_name = False

    def load_default_certs(self, purpose=None):
        pass

    def load_verify_locations(self, cafile=None, capath=None, cadata=None):
        pass

    def load_cert_chain(self, certfile, keyfile=None, password=None):
        pass

    def set_ciphers(self, ciphers):
        pass

    def set_alpn_protocols(self, protocols):
        pass

    def wrap_socket(self, sock, *args, **kwargs):
        raise SSLError(1, '[SSL] sem TLS neste sandbox (sem rede)')


class Purpose(_enum.Enum):
    SERVER_AUTH = 1
    CLIENT_AUTH = 2


def create_default_context(purpose=Purpose.SERVER_AUTH, *, cafile=None, capath=None, cadata=None):
    return SSLContext(PROTOCOL_TLS_CLIENT if purpose == Purpose.SERVER_AUTH else PROTOCOL_TLS_SERVER)


def _create_unverified_context(*args, **kwargs):
    ctx = SSLContext(PROTOCOL_TLS_CLIENT)
    ctx.check_hostname = False
    ctx.verify_mode = CERT_NONE
    return ctx


_create_default_https_context = create_default_context


def get_default_verify_paths():
    return ('', '', '', '', '')


def match_hostname(cert, hostname):
    raise SSLCertVerificationError('match_hostname removido')
