"""_ssl: a parte em C do módulo `ssl` do CPython 3.13 (OpenSSL 3.5.7 do Debian 13).

O `ssl.py` do CPython roda por cima deste módulo. Contextos, certificados, MemoryBIO e exceções
seguem o `_ssl.c`. O cliente fala TLS 1.3 de verdade, em Python puro: troca de chaves X25519, AEAD
ChaCha20-Poly1305 ou AES-128-GCM, validação da cadeia contra as CAs carregadas no contexto (ECDSA P-256 e
P-384, RSA PKCS#1 e PSS) e conferência do nome por SAN. Um par que não fala TLS recebe
`WRONG_VERSION_NUMBER`, um par que fecha recebe `UNEXPECTED_EOF_WHILE_READING`, e um alerta do par vira o
erro que o OpenSSL dá a ele. O lado servidor ainda não negocia: recusa como quem não achou cifra.
"""

import os as _os
import hashlib as _hashlib
import _socket
import _tls_crypto

OPENSSL_VERSION = 'OpenSSL 3.5.7 9 Jun 2026'
OPENSSL_VERSION_INFO = (3, 5, 0, 7, 0)
OPENSSL_VERSION_NUMBER = 810549360
_OPENSSL_API_VERSION = (3, 5, 0, 7, 0)
_DEFAULT_CIPHERS = 'ALL:!COMPLEMENTOFDEFAULT:!eNULL'

ALERT_DESCRIPTION_ACCESS_DENIED = 49
ALERT_DESCRIPTION_BAD_CERTIFICATE = 42
ALERT_DESCRIPTION_BAD_CERTIFICATE_HASH_VALUE = 114
ALERT_DESCRIPTION_BAD_CERTIFICATE_STATUS_RESPONSE = 113
ALERT_DESCRIPTION_BAD_RECORD_MAC = 20
ALERT_DESCRIPTION_CERTIFICATE_EXPIRED = 45
ALERT_DESCRIPTION_CERTIFICATE_REVOKED = 44
ALERT_DESCRIPTION_CERTIFICATE_UNKNOWN = 46
ALERT_DESCRIPTION_CERTIFICATE_UNOBTAINABLE = 111
ALERT_DESCRIPTION_CLOSE_NOTIFY = 0
ALERT_DESCRIPTION_DECODE_ERROR = 50
ALERT_DESCRIPTION_DECOMPRESSION_FAILURE = 30
ALERT_DESCRIPTION_DECRYPT_ERROR = 51
ALERT_DESCRIPTION_HANDSHAKE_FAILURE = 40
ALERT_DESCRIPTION_ILLEGAL_PARAMETER = 47
ALERT_DESCRIPTION_INSUFFICIENT_SECURITY = 71
ALERT_DESCRIPTION_INTERNAL_ERROR = 80
ALERT_DESCRIPTION_NO_RENEGOTIATION = 100
ALERT_DESCRIPTION_PROTOCOL_VERSION = 70
ALERT_DESCRIPTION_RECORD_OVERFLOW = 22
ALERT_DESCRIPTION_UNEXPECTED_MESSAGE = 10
ALERT_DESCRIPTION_UNKNOWN_CA = 48
ALERT_DESCRIPTION_UNKNOWN_PSK_IDENTITY = 115
ALERT_DESCRIPTION_UNRECOGNIZED_NAME = 112
ALERT_DESCRIPTION_UNSUPPORTED_CERTIFICATE = 43
ALERT_DESCRIPTION_UNSUPPORTED_EXTENSION = 110
ALERT_DESCRIPTION_USER_CANCELLED = 90
CERT_NONE = 0
CERT_OPTIONAL = 1
CERT_REQUIRED = 2
ENCODING_DER = 2
ENCODING_PEM = 1
HAS_ALPN = True
HAS_ECDH = True
HAS_NPN = False
HAS_PSK = True
HAS_SNI = True
HAS_SSLv2 = False
HAS_SSLv3 = False
HAS_TLS_UNIQUE = True
HAS_TLSv1 = True
HAS_TLSv1_1 = True
HAS_TLSv1_2 = True
HAS_TLSv1_3 = True
HOSTFLAG_ALWAYS_CHECK_SUBJECT = 1
HOSTFLAG_MULTI_LABEL_WILDCARDS = 8
HOSTFLAG_NEVER_CHECK_SUBJECT = 32
HOSTFLAG_NO_PARTIAL_WILDCARDS = 4
HOSTFLAG_NO_WILDCARDS = 2
HOSTFLAG_SINGLE_LABEL_SUBDOMAINS = 16
OP_ALL = 2147483728
OP_CIPHER_SERVER_PREFERENCE = 4194304
OP_ENABLE_KTLS = 8
OP_ENABLE_MIDDLEBOX_COMPAT = 1048576
OP_IGNORE_UNEXPECTED_EOF = 128
OP_LEGACY_SERVER_CONNECT = 4
OP_NO_COMPRESSION = 131072
OP_NO_RENEGOTIATION = 1073741824
OP_NO_SSLv2 = 0
OP_NO_SSLv3 = 33554432
OP_NO_TICKET = 16384
OP_NO_TLSv1 = 67108864
OP_NO_TLSv1_1 = 268435456
OP_NO_TLSv1_2 = 134217728
OP_NO_TLSv1_3 = 536870912
OP_SINGLE_DH_USE = 0
OP_SINGLE_ECDH_USE = 0
PROTOCOL_SSLv23 = 2
PROTOCOL_TLS = 2
PROTOCOL_TLS_CLIENT = 16
PROTOCOL_TLS_SERVER = 17
PROTOCOL_TLSv1 = 3
PROTOCOL_TLSv1_1 = 4
PROTOCOL_TLSv1_2 = 5
PROTO_MAXIMUM_SUPPORTED = -1
PROTO_MINIMUM_SUPPORTED = -2
PROTO_SSLv3 = 768
PROTO_TLSv1 = 769
PROTO_TLSv1_1 = 770
PROTO_TLSv1_2 = 771
PROTO_TLSv1_3 = 772
SSL_ERROR_EOF = 8
SSL_ERROR_INVALID_ERROR_CODE = 10
SSL_ERROR_SSL = 1
SSL_ERROR_SYSCALL = 5
SSL_ERROR_WANT_CONNECT = 7
SSL_ERROR_WANT_READ = 2
SSL_ERROR_WANT_WRITE = 3
SSL_ERROR_WANT_X509_LOOKUP = 4
SSL_ERROR_ZERO_RETURN = 6
VERIFY_ALLOW_PROXY_CERTS = 64
VERIFY_CRL_CHECK_CHAIN = 12
VERIFY_CRL_CHECK_LEAF = 4
VERIFY_DEFAULT = 0
VERIFY_X509_PARTIAL_CHAIN = 524288
VERIFY_X509_STRICT = 32
VERIFY_X509_TRUSTED_FIRST = 32768

_OPENSSLDIR = '/usr/lib/ssl'
_DEFAULT_OPTIONS = 2186412112
_PROTOCOLS = (PROTOCOL_TLS, PROTOCOL_TLSv1, PROTOCOL_TLSv1_1, PROTOCOL_TLSv1_2, PROTOCOL_TLS_CLIENT, PROTOCOL_TLS_SERVER)
_DEPRECATED_PROTOCOLS = {PROTOCOL_TLS: 'ssl.PROTOCOL_TLS', PROTOCOL_TLSv1: 'ssl.PROTOCOL_TLSv1',
                         PROTOCOL_TLSv1_1: 'ssl.PROTOCOL_TLSv1_1', PROTOCOL_TLSv1_2: 'ssl.PROTOCOL_TLSv1_2'}
_VERSIONS = (PROTO_MINIMUM_SUPPORTED, PROTO_SSLv3, PROTO_TLSv1, PROTO_TLSv1_1, PROTO_TLSv1_2, PROTO_TLSv1_3,
             PROTO_MAXIMUM_SUPPORTED)
_DEPRECATED_VERSIONS = {PROTO_SSLv3: 'ssl.TLSVersion.SSLv3', PROTO_TLSv1: 'ssl.TLSVersion.TLSv1',
                        PROTO_TLSv1_1: 'ssl.TLSVersion.TLSv1_1'}
_DEPRECATED_OPTIONS = OP_NO_SSLv2 | OP_NO_SSLv3 | OP_NO_TLSv1 | OP_NO_TLSv1_1 | OP_NO_TLSv1_2 | OP_NO_TLSv1_3


def _warn(message):
    import warnings
    warnings.warn(message, DeprecationWarning, stacklevel=3)


# --- exceções -------------------------------------------------------------------------------------

class SSLError(OSError):
    """An error occurred in the SSL implementation."""

    def __str__(self):
        if isinstance(self.strerror, str):
            return self.strerror
        return str(self.args)


SSLError.__module__ = 'ssl'


class SSLCertVerificationError(SSLError, ValueError):
    """A certificate could not be verified."""


class SSLZeroReturnError(SSLError):
    """SSL/TLS session closed cleanly."""


class SSLWantReadError(SSLError):
    """Non-blocking SSL socket needs to read more data
before the requested operation can be completed."""


class SSLWantWriteError(SSLError):
    """Non-blocking SSL socket needs to write more data
before the requested operation can be completed."""


class SSLSyscallError(SSLError):
    """System error when attempting SSL operation."""


class SSLEOFError(SSLError):
    """SSL/TLS connection terminated abruptly."""


for _cls in (SSLCertVerificationError, SSLZeroReturnError, SSLWantReadError, SSLWantWriteError,
             SSLSyscallError, SSLEOFError):
    _cls.__module__ = 'ssl'
del _cls


def _ssl_error(cls, errcode, library, reason, text, line):
    """Exceção com os atributos que o `fill_and_set_sslerror` do `_ssl.c` preenche."""
    if library:
        message = '[%s: %s] %s (_ssl.c:%d)' % (library, reason, text, line)
    else:
        message = '[%s] %s (_ssl.c:%d)' % (reason, text, line)
    err = cls(errcode, message)
    err.library = library or reason
    err.reason = None if not library else reason
    return err


def _want_read():
    """O `SSL_ERROR_WANT_READ`: sem o par biblioteca/motivo, que o OpenSSL não registra nesse caso."""
    err = SSLWantReadError(SSL_ERROR_WANT_READ, 'The operation did not complete (read) (_ssl.c:1029)')
    err.library = None
    err.reason = None
    return err


# --- objetos ASN.1 --------------------------------------------------------------------------------

# (nid, nome curto, nome longo, OID) dos objetos que o `ssl.py` e os certificados usam.
_OBJECTS = (
    (13, 'CN', 'commonName', '2.5.4.3'),
    (100, 'SN', 'surname', '2.5.4.4'),
    (105, 'serialNumber', 'serialNumber', '2.5.4.5'),
    (14, 'C', 'countryName', '2.5.4.6'),
    (15, 'L', 'localityName', '2.5.4.7'),
    (16, 'ST', 'stateOrProvinceName', '2.5.4.8'),
    (660, 'street', 'streetAddress', '2.5.4.9'),
    (17, 'O', 'organizationName', '2.5.4.10'),
    (18, 'OU', 'organizationalUnitName', '2.5.4.11'),
    (106, 'title', 'title', '2.5.4.12'),
    (107, 'description', 'description', '2.5.4.13'),
    (860, 'businessCategory', 'businessCategory', '2.5.4.15'),
    (661, 'postalCode', 'postalCode', '2.5.4.17'),
    (173, 'name', 'name', '2.5.4.41'),
    (99, 'GN', 'givenName', '2.5.4.42'),
    (101, 'initials', 'initials', '2.5.4.43'),
    (509, 'generationQualifier', 'generationQualifier', '2.5.4.44'),
    (174, 'dnQualifier', 'dnQualifier', '2.5.4.46'),
    (510, 'pseudonym', 'pseudonym', '2.5.4.65'),
    (1089, 'organizationIdentifier', 'organizationIdentifier', '2.5.4.97'),
    (48, 'emailAddress', 'emailAddress', '1.2.840.113549.1.9.1'),
    (391, 'DC', 'domainComponent', '0.9.2342.19200300.100.1.25'),
    (458, 'UID', 'userId', '0.9.2342.19200300.100.1.1'),
    (957, 'jurisdictionL', 'jurisdictionLocalityName', '1.3.6.1.4.1.311.60.2.1.1'),
    (958, 'jurisdictionST', 'jurisdictionStateOrProvinceName', '1.3.6.1.4.1.311.60.2.1.2'),
    (955, 'jurisdictionC', 'jurisdictionCountryName', '1.3.6.1.4.1.311.60.2.1.3'),
    (71, 'nsCertType', 'Netscape Cert Type', '2.16.840.1.113730.1.1'),
    (72, 'nsBaseUrl', 'Netscape Base Url', '2.16.840.1.113730.1.2'),
    (73, 'nsRevocationUrl', 'Netscape Revocation Url', '2.16.840.1.113730.1.3'),
    (129, 'serverAuth', 'TLS Web Server Authentication', '1.3.6.1.5.5.7.3.1'),
    (130, 'clientAuth', 'TLS Web Client Authentication', '1.3.6.1.5.5.7.3.2'),
    (131, 'codeSigning', 'Code Signing', '1.3.6.1.5.5.7.3.3'),
    (132, 'emailProtection', 'E-mail Protection', '1.3.6.1.5.5.7.3.4'),
    (133, 'timeStamping', 'Time Stamping', '1.3.6.1.5.5.7.3.8'),
    (180, 'OCSPSigning', 'OCSP Signing', '1.3.6.1.5.5.7.3.9'),
    (82, 'subjectKeyIdentifier', 'X509v3 Subject Key Identifier', '2.5.29.14'),
    (83, 'keyUsage', 'X509v3 Key Usage', '2.5.29.15'),
    (85, 'subjectAltName', 'X509v3 Subject Alternative Name', '2.5.29.17'),
    (86, 'issuerAltName', 'X509v3 Issuer Alternative Name', '2.5.29.18'),
    (87, 'basicConstraints', 'X509v3 Basic Constraints', '2.5.29.19'),
    (103, 'crlDistributionPoints', 'X509v3 CRL Distribution Points', '2.5.29.31'),
    (89, 'certificatePolicies', 'X509v3 Certificate Policies', '2.5.29.32'),
    (90, 'authorityKeyIdentifier', 'X509v3 Authority Key Identifier', '2.5.29.35'),
    (126, 'extendedKeyUsage', 'X509v3 Extended Key Usage', '2.5.29.37'),
    (177, 'authorityInfoAccess', 'Authority Information Access', '1.3.6.1.5.5.7.1.1'),
    (178, 'OCSP', 'OCSP', '1.3.6.1.5.5.7.48.1'),
    (179, 'caIssuers', 'CA Issuers', '1.3.6.1.5.5.7.48.2'),
    (6, 'rsaEncryption', 'rsaEncryption', '1.2.840.113549.1.1.1'),
    (408, 'id-ecPublicKey', 'id-ecPublicKey', '1.2.840.10045.2.1'),
)
_BY_NID = {o[0]: o for o in _OBJECTS}
_BY_OID = {o[3]: o for o in _OBJECTS}
_BY_NAME = {}
for _o in _OBJECTS:
    _BY_NAME.setdefault(_o[1], _o)
    _BY_NAME.setdefault(_o[2], _o)
del _o


def txt2obj(txt, name=False):
    """Lookup NID, short name, long name and OID of an ASN1_OBJECT.

By default objects are looked up by OID. With name=True short and
long name are also matched."""
    if not isinstance(txt, str):
        raise TypeError("txt2obj() argument 'txt' must be str, not %s" % type(txt).__name__)
    obj = _BY_OID.get(txt)
    if obj is None and name:
        obj = _BY_NAME.get(txt)
    if obj is None:
        raise ValueError("unknown object '%s'" % txt)
    return obj


def nid2obj(nid):
    """Lookup NID, short name, long name and OID of an ASN1_OBJECT by NID."""
    if nid < 1:
        raise ValueError('NID must be positive.')
    obj = _BY_NID.get(nid)
    if obj is None:
        raise ValueError('unknown NID %i' % nid)
    return obj


# --- certificados (DER) ---------------------------------------------------------------------------

def _der_read(data, pos):
    """(tag, início do conteúdo, fim do conteúdo) do elemento DER em `pos`."""
    tag = data[pos]
    length = data[pos + 1]
    pos += 2
    if length & 0x80:
        count = length & 0x7f
        length = int.from_bytes(data[pos:pos + count], 'big')
        pos += count
    return tag, pos, pos + length


def _der_children(data, start, end):
    out = []
    pos = start
    while pos < end:
        tag, cstart, cend = _der_read(data, pos)
        out.append((tag, cstart, cend))
        pos = cend
    return out


def _oid_text(raw):
    first = raw[0]
    parts = [str(min(first // 40, 2)), str(first - 40 * min(first // 40, 2))]
    value = 0
    for byte in raw[1:]:
        value = (value << 7) | (byte & 0x7f)
        if not byte & 0x80:
            parts.append(str(value))
            value = 0
    return '.'.join(parts)


def _oid_name(raw):
    oid = _oid_text(raw)
    obj = _BY_OID.get(oid)
    return obj[2] if obj else oid


def _asn1_string(tag, raw):
    if tag == 0x1e:
        return raw.decode('utf-16-be')
    if tag == 0x1c:
        return raw.decode('utf-32-be')
    if tag in (0x14, 0x15):
        return raw.decode('latin-1')
    return raw.decode('utf-8', 'replace')


def _name_tuple(data, start, end):
    rdns = []
    for _, sstart, send in _der_children(data, start, end):
        pairs = []
        for _, astart, aend in _der_children(data, sstart, send):
            (_, ostart, oend), (vtag, vstart, vend) = _der_children(data, astart, aend)[:2]
            pairs.append((_oid_name(data[ostart:oend]), _asn1_string(vtag, data[vstart:vend])))
        rdns.append(tuple(pairs))
    return tuple(rdns)


_MONTHS = ('Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec')


def _asn1_time(tag, raw):
    text = raw.decode('ascii')
    if tag == 0x17:
        year = int(text[0:2])
        year += 1900 if year >= 50 else 2000
        rest = text[2:]
    else:
        year = int(text[0:4])
        rest = text[4:]
    month, day, hh, mm, ss = int(rest[0:2]), int(rest[2:4]), rest[4:6], rest[6:8], rest[8:10]
    return '%s %2d %s:%s:%s %d GMT' % (_MONTHS[month - 1], day, hh, mm, ss, year)


def _ip_text(raw):
    if len(raw) == 4:
        return '.'.join(str(b) for b in raw)
    if len(raw) == 16:
        return ':'.join('%X' % int.from_bytes(raw[i:i + 2], 'big') for i in range(0, 16, 2))
    return '<invalid>'


def _general_names(data, start, end):
    out = []
    for tag, gstart, gend in _der_children(data, start, end):
        raw = data[gstart:gend]
        kind = tag & 0x1f
        if kind == 1:
            out.append(('email', raw.decode('ascii', 'replace')))
        elif kind == 2:
            out.append(('DNS', raw.decode('ascii', 'replace')))
        elif kind == 4:
            (_, nstart, nend), = _der_children(data, gstart, gend)[:1]
            out.append(('DirName', _name_tuple(data, nstart, nend)))
        elif kind == 6:
            out.append(('URI', raw.decode('ascii', 'replace')))
        elif kind == 7:
            out.append(('IP Address', _ip_text(raw)))
        elif kind == 8:
            out.append(('Registered ID', _oid_name(raw)))
        else:
            out.append(('othername', '<unsupported>'))
    return out


def _decode_der(der):
    """O dicionário que o `_decode_certificate` do `_ssl.c` monta."""
    _, cstart, cend = _der_read(der, 0)
    _, tstart, tend = _der_read(der, cstart)
    fields = _der_children(der, tstart, tend)
    version = 1
    if fields[0][0] == 0xa0:
        _, vstart, vend = _der_read(der, fields[0][1])
        version = int.from_bytes(der[vstart:vend], 'big') + 1
        fields = fields[1:]
    serial = der[fields[0][1]:fields[0][2]].lstrip(b'\x00') or b'\x00'
    issuer = _name_tuple(der, fields[2][1], fields[2][2])
    (ntag, nstart, nend), (atag, astart, aend) = _der_children(der, fields[3][1], fields[3][2])
    subject = _name_tuple(der, fields[4][1], fields[4][2])
    result = {
        'subject': subject,
        'issuer': issuer,
        'version': version,
        'serialNumber': serial.hex().upper(),
        'notBefore': _asn1_time(ntag, der[nstart:nend]),
        'notAfter': _asn1_time(atag, der[astart:aend]),
    }
    extensions = {}
    for tag, estart, eend in fields[6:]:
        if tag != 0xa3:
            continue
        (_, sstart, send), = _der_children(der, estart, eend)[:1]
        for _, xstart, xend in _der_children(der, sstart, send):
            parts = _der_children(der, xstart, xend)
            oid = _oid_text(der[parts[0][1]:parts[0][2]])
            value = parts[-1]
            extensions[oid] = (value[1], value[2])
    if '2.5.29.17' in extensions:
        vstart, vend = extensions['2.5.29.17']
        _, gstart, gend = _der_read(der, vstart)
        result['subjectAltName'] = tuple(_general_names(der, gstart, gend))
    if '1.3.6.1.5.5.7.1.1' in extensions:
        vstart, vend = extensions['1.3.6.1.5.5.7.1.1']
        _, sstart, send = _der_read(der, vstart)
        ocsp, issuers = [], []
        for _, dstart, dend in _der_children(der, sstart, send):
            (_, ostart, oend), (gtag, gstart, gend) = _der_children(der, dstart, dend)
            if gtag & 0x1f != 6:
                continue
            uri = der[gstart:gend].decode('ascii', 'replace')
            method = _oid_text(der[ostart:oend])
            if method == '1.3.6.1.5.5.7.48.1':
                ocsp.append(uri)
            elif method == '1.3.6.1.5.5.7.48.2':
                issuers.append(uri)
        if ocsp:
            result['OCSP'] = tuple(ocsp)
        if issuers:
            result['caIssuers'] = tuple(issuers)
    if '2.5.29.31' in extensions:
        vstart, vend = extensions['2.5.29.31']
        _, sstart, send = _der_read(der, vstart)
        points = []
        for _, pstart, pend in _der_children(der, sstart, send):
            for tag, nstart, nend in _der_children(der, pstart, pend):
                if tag != 0xa0:
                    continue
                for ftag, fstart, fend in _der_children(der, nstart, nend):
                    if ftag == 0xa0:
                        points.extend(v for k, v in _general_names(der, fstart, fend) if k == 'URI')
        if points:
            result['crlDistributionPoints'] = tuple(points)
    return result


def _is_ca(der):
    """O certificado tem `basicConstraints` com `cA` verdadeiro."""
    try:
        at = der.index(b'\x06\x03\x55\x1d\x13')
    except ValueError:
        return False
    window = der[at + 5:at + 24]
    return b'\x01\x01\xff' in window


_PEM_BEGIN = '-----BEGIN CERTIFICATE-----'
_PEM_END = '-----END CERTIFICATE-----'


def _pem_certificates(text):
    import binascii
    out = []
    pos = 0
    while True:
        start = text.find(_PEM_BEGIN, pos)
        if start < 0:
            return out
        end = text.find(_PEM_END, start)
        if end < 0:
            return out
        body = text[start + len(_PEM_BEGIN):end]
        out.append(binascii.a2b_base64(''.join(body.split())))
        pos = end + len(_PEM_END)


def _test_decode_cert(path):
    with open(path, 'rb') as f:
        text = f.read().decode('latin-1')
    certs = _pem_certificates(text)
    if not certs:
        raise _ssl_error(SSLError, 9, 'PEM', 'NO_START_LINE', 'no start line', 1836)
    return _decode_der(certs[0])


class Certificate:
    def __new__(cls, *args, **kwargs):
        raise TypeError("cannot create '_ssl.Certificate' instances")

    def public_bytes(self, format=ENCODING_PEM):
        if format == ENCODING_DER:
            return self._der
        import base64
        b64 = base64.encodebytes(self._der).decode('ascii').replace('\n', '')
        lines = [b64[i:i + 64] for i in range(0, len(b64), 64)]
        return _PEM_BEGIN + '\n' + '\n'.join(lines) + '\n' + _PEM_END + '\n'

    def get_info(self):
        return _decode_der(self._der)


Certificate.__module__ = '_ssl'


# --- cifras ---------------------------------------------------------------------------------------

# (id, nome, versão, troca de chaves, autenticação, cifra, MAC) da lista padrão do OpenSSL 3.5.7 do
# Debian; o resto do dicionário do `get_ciphers()` sai destes campos.
_CIPHERS = (
    (50336514, 'TLS_AES_256_GCM_SHA384', 'TLSv1.3', 'any', 'any', 'AESGCM(256)', 'AEAD'),
    (50336515, 'TLS_CHACHA20_POLY1305_SHA256', 'TLSv1.3', 'any', 'any', 'CHACHA20/POLY1305(256)', 'AEAD'),
    (50336513, 'TLS_AES_128_GCM_SHA256', 'TLSv1.3', 'any', 'any', 'AESGCM(128)', 'AEAD'),
    (50380844, 'ECDHE-ECDSA-AES256-GCM-SHA384', 'TLSv1.2', 'ECDH', 'ECDSA', 'AESGCM(256)', 'AEAD'),
    (50380848, 'ECDHE-RSA-AES256-GCM-SHA384', 'TLSv1.2', 'ECDH', 'RSA', 'AESGCM(256)', 'AEAD'),
    (50331807, 'DHE-RSA-AES256-GCM-SHA384', 'TLSv1.2', 'DH', 'RSA', 'AESGCM(256)', 'AEAD'),
    (50384041, 'ECDHE-ECDSA-CHACHA20-POLY1305', 'TLSv1.2', 'ECDH', 'ECDSA', 'CHACHA20/POLY1305(256)', 'AEAD'),
    (50384040, 'ECDHE-RSA-CHACHA20-POLY1305', 'TLSv1.2', 'ECDH', 'RSA', 'CHACHA20/POLY1305(256)', 'AEAD'),
    (50384042, 'DHE-RSA-CHACHA20-POLY1305', 'TLSv1.2', 'DH', 'RSA', 'CHACHA20/POLY1305(256)', 'AEAD'),
    (50380843, 'ECDHE-ECDSA-AES128-GCM-SHA256', 'TLSv1.2', 'ECDH', 'ECDSA', 'AESGCM(128)', 'AEAD'),
    (50380847, 'ECDHE-RSA-AES128-GCM-SHA256', 'TLSv1.2', 'ECDH', 'RSA', 'AESGCM(128)', 'AEAD'),
    (50331806, 'DHE-RSA-AES128-GCM-SHA256', 'TLSv1.2', 'DH', 'RSA', 'AESGCM(128)', 'AEAD'),
    (50380836, 'ECDHE-ECDSA-AES256-SHA384', 'TLSv1.2', 'ECDH', 'ECDSA', 'AES(256)', 'SHA384'),
    (50380840, 'ECDHE-RSA-AES256-SHA384', 'TLSv1.2', 'ECDH', 'RSA', 'AES(256)', 'SHA384'),
    (50331755, 'DHE-RSA-AES256-SHA256', 'TLSv1.2', 'DH', 'RSA', 'AES(256)', 'SHA256'),
    (50380835, 'ECDHE-ECDSA-AES128-SHA256', 'TLSv1.2', 'ECDH', 'ECDSA', 'AES(128)', 'SHA256'),
    (50380839, 'ECDHE-RSA-AES128-SHA256', 'TLSv1.2', 'ECDH', 'RSA', 'AES(128)', 'SHA256'),
    (50331751, 'DHE-RSA-AES128-SHA256', 'TLSv1.2', 'DH', 'RSA', 'AES(128)', 'SHA256'),
    (50380810, 'ECDHE-ECDSA-AES256-SHA', 'TLSv1.0', 'ECDH', 'ECDSA', 'AES(256)', 'SHA1'),
    (50380820, 'ECDHE-RSA-AES256-SHA', 'TLSv1.0', 'ECDH', 'RSA', 'AES(256)', 'SHA1'),
    (50331705, 'DHE-RSA-AES256-SHA', 'SSLv3', 'DH', 'RSA', 'AES(256)', 'SHA1'),
    (50380809, 'ECDHE-ECDSA-AES128-SHA', 'TLSv1.0', 'ECDH', 'ECDSA', 'AES(128)', 'SHA1'),
    (50380819, 'ECDHE-RSA-AES128-SHA', 'TLSv1.0', 'ECDH', 'RSA', 'AES(128)', 'SHA1'),
    (50331699, 'DHE-RSA-AES128-SHA', 'SSLv3', 'DH', 'RSA', 'AES(128)', 'SHA1'),
    (50331821, 'RSA-PSK-AES256-GCM-SHA384', 'TLSv1.2', 'RSAPSK', 'RSA', 'AESGCM(256)', 'AEAD'),
    (50331819, 'DHE-PSK-AES256-GCM-SHA384', 'TLSv1.2', 'DHEPSK', 'PSK', 'AESGCM(256)', 'AEAD'),
    (50384046, 'RSA-PSK-CHACHA20-POLY1305', 'TLSv1.2', 'RSAPSK', 'RSA', 'CHACHA20/POLY1305(256)', 'AEAD'),
    (50384045, 'DHE-PSK-CHACHA20-POLY1305', 'TLSv1.2', 'DHEPSK', 'PSK', 'CHACHA20/POLY1305(256)', 'AEAD'),
    (50384044, 'ECDHE-PSK-CHACHA20-POLY1305', 'TLSv1.2', 'ECDHEPSK', 'PSK', 'CHACHA20/POLY1305(256)', 'AEAD'),
    (50331805, 'AES256-GCM-SHA384', 'TLSv1.2', 'RSA', 'RSA', 'AESGCM(256)', 'AEAD'),
    (50331817, 'PSK-AES256-GCM-SHA384', 'TLSv1.2', 'PSK', 'PSK', 'AESGCM(256)', 'AEAD'),
    (50384043, 'PSK-CHACHA20-POLY1305', 'TLSv1.2', 'PSK', 'PSK', 'CHACHA20/POLY1305(256)', 'AEAD'),
    (50331820, 'RSA-PSK-AES128-GCM-SHA256', 'TLSv1.2', 'RSAPSK', 'RSA', 'AESGCM(128)', 'AEAD'),
    (50331818, 'DHE-PSK-AES128-GCM-SHA256', 'TLSv1.2', 'DHEPSK', 'PSK', 'AESGCM(128)', 'AEAD'),
    (50331804, 'AES128-GCM-SHA256', 'TLSv1.2', 'RSA', 'RSA', 'AESGCM(128)', 'AEAD'),
    (50331816, 'PSK-AES128-GCM-SHA256', 'TLSv1.2', 'PSK', 'PSK', 'AESGCM(128)', 'AEAD'),
    (50331709, 'AES256-SHA256', 'TLSv1.2', 'RSA', 'RSA', 'AES(256)', 'SHA256'),
    (50331708, 'AES128-SHA256', 'TLSv1.2', 'RSA', 'RSA', 'AES(128)', 'SHA256'),
    (50380856, 'ECDHE-PSK-AES256-CBC-SHA384', 'TLSv1.0', 'ECDHEPSK', 'PSK', 'AES(256)', 'SHA384'),
    (50380854, 'ECDHE-PSK-AES256-CBC-SHA', 'TLSv1.0', 'ECDHEPSK', 'PSK', 'AES(256)', 'SHA1'),
    (50380833, 'SRP-RSA-AES-256-CBC-SHA', 'SSLv3', 'SRP', 'RSA', 'AES(256)', 'SHA1'),
    (50380832, 'SRP-AES-256-CBC-SHA', 'SSLv3', 'SRP', 'SRP', 'AES(256)', 'SHA1'),
    (50331831, 'RSA-PSK-AES256-CBC-SHA384', 'TLSv1.0', 'RSAPSK', 'RSA', 'AES(256)', 'SHA384'),
    (50331827, 'DHE-PSK-AES256-CBC-SHA384', 'TLSv1.0', 'DHEPSK', 'PSK', 'AES(256)', 'SHA384'),
    (50331797, 'RSA-PSK-AES256-CBC-SHA', 'SSLv3', 'RSAPSK', 'RSA', 'AES(256)', 'SHA1'),
    (50331793, 'DHE-PSK-AES256-CBC-SHA', 'SSLv3', 'DHEPSK', 'PSK', 'AES(256)', 'SHA1'),
    (50331701, 'AES256-SHA', 'SSLv3', 'RSA', 'RSA', 'AES(256)', 'SHA1'),
    (50331823, 'PSK-AES256-CBC-SHA384', 'TLSv1.0', 'PSK', 'PSK', 'AES(256)', 'SHA384'),
    (50331789, 'PSK-AES256-CBC-SHA', 'SSLv3', 'PSK', 'PSK', 'AES(256)', 'SHA1'),
    (50380855, 'ECDHE-PSK-AES128-CBC-SHA256', 'TLSv1.0', 'ECDHEPSK', 'PSK', 'AES(128)', 'SHA256'),
    (50380853, 'ECDHE-PSK-AES128-CBC-SHA', 'TLSv1.0', 'ECDHEPSK', 'PSK', 'AES(128)', 'SHA1'),
    (50380830, 'SRP-RSA-AES-128-CBC-SHA', 'SSLv3', 'SRP', 'RSA', 'AES(128)', 'SHA1'),
    (50380829, 'SRP-AES-128-CBC-SHA', 'SSLv3', 'SRP', 'SRP', 'AES(128)', 'SHA1'),
    (50331830, 'RSA-PSK-AES128-CBC-SHA256', 'TLSv1.0', 'RSAPSK', 'RSA', 'AES(128)', 'SHA256'),
    (50331826, 'DHE-PSK-AES128-CBC-SHA256', 'TLSv1.0', 'DHEPSK', 'PSK', 'AES(128)', 'SHA256'),
    (50331796, 'RSA-PSK-AES128-CBC-SHA', 'SSLv3', 'RSAPSK', 'RSA', 'AES(128)', 'SHA1'),
    (50331792, 'DHE-PSK-AES128-CBC-SHA', 'SSLv3', 'DHEPSK', 'PSK', 'AES(128)', 'SHA1'),
    (50331695, 'AES128-SHA', 'SSLv3', 'RSA', 'RSA', 'AES(128)', 'SHA1'),
    (50331822, 'PSK-AES128-CBC-SHA256', 'TLSv1.0', 'PSK', 'PSK', 'AES(128)', 'SHA256'),
    (50331788, 'PSK-AES128-CBC-SHA', 'SSLv3', 'PSK', 'PSK', 'AES(128)', 'SHA1'),
)
_KEA = {'any': 'kx-any', 'ECDH': 'kx-ecdhe', 'DH': 'kx-dhe', 'RSA': 'kx-rsa', 'RSAPSK': 'kx-rsa-psk',
        'DHEPSK': 'kx-dhe-psk', 'ECDHEPSK': 'kx-ecdhe-psk', 'PSK': 'kx-psk', 'SRP': 'kx-srp'}
# Palavras da sintaxe de cifras do OpenSSL que selecionam alguma das cifras acima.
_CIPHER_KEYWORDS = frozenset((
    'ALL', 'DEFAULT', 'HIGH', 'MEDIUM', 'COMPLEMENTOFDEFAULT', 'COMPLEMENTOFALL', 'aRSA', 'aECDSA',
    'aPSK', 'aSRP', 'kRSA', 'kECDHE', 'kEECDH', 'kDHE', 'kEDH', 'kPSK', 'kECDHEPSK', 'kDHEPSK',
    'kRSAPSK', 'kSRP', 'ECDHE', 'EECDH', 'DHE', 'EDH', 'RSA', 'ECDSA', 'PSK', 'SRP', 'AES', 'AES128',
    'AES256', 'AESGCM', 'CHACHA20', 'SHA', 'SHA1', 'SHA256', 'SHA384', 'TLSv1.2', 'TLSv1', 'SSLv3',
    'SUITEB128', 'SUITEB192', 'SUITEB128ONLY', 'FIPS',
))


def _cipher_dict(entry):
    cid, name, proto, kx, au, enc, mac = entry
    bits = int(enc[enc.index('(') + 1:-1])
    if enc.startswith('AESGCM'):
        symmetric = 'aes-%d-gcm' % bits
    elif enc.startswith('AES'):
        symmetric = 'aes-%d-cbc' % bits
    else:
        symmetric = 'chacha20-poly1305'
    shown = 'TLSv1' if proto == 'TLSv1.0' else proto
    return {
        'id': cid, 'name': name, 'protocol': proto,
        'description': '%-30s %-7s Kx=%-8s Au=%-5s Enc=%-22s Mac=%s' % (name, shown, kx, au, enc, mac),
        'strength_bits': bits, 'alg_bits': bits, 'aead': mac == 'AEAD', 'symmetric': symmetric,
        'digest': None if mac == 'AEAD' else mac.lower(), 'kea': _KEA[kx], 'auth': 'auth-' + au.lower(),
    }


# --- ClientHello ----------------------------------------------------------------------------------

# Os conjuntos de cifras do TLS 1.3 que o cliente sabe negociar: (nome, hash, tamanho da chave, AEAD, bits).
_SUITES = {
    0x1303: ('TLS_CHACHA20_POLY1305_SHA256', 'sha256', 32, 'chacha', 256),
    0x1301: ('TLS_AES_128_GCM_SHA256', 'sha256', 16, 'aes', 128),
}
# Ordem de preferência na oferta: o ChaCha20 primeiro, porque em Python puro ele custa bem menos que o AES.
_CIPHER_SUITES = b'\x13\x03\x13\x01'
# ecdsa_secp256r1_sha256, ecdsa_secp384r1_sha384, rsa_pss_rsae_sha256/384/512.
_SIGALGS = bytes.fromhex('000a04030503080408050806')
_CERT_VERIFY_ALGS = {0x0403: ('ecdsa', 'sha256'), 0x0503: ('ecdsa', 'sha384'), 0x0804: ('pss', 'sha256'),
                     0x0805: ('pss', 'sha384'), 0x0806: ('pss', 'sha512')}
_HELLO_RETRY_RANDOM = bytes.fromhex('cf21ad74e59a6111be1d8c021e65b891c2a211167abb8c5e079e09e2c8a8339c')


def _ext(kind, body):
    return kind.to_bytes(2, 'big') + len(body).to_bytes(2, 'big') + body


def _is_ip(host):
    if ':' in host:
        return True
    parts = host.split('.')
    return len(parts) == 4 and all(p.isdigit() and int(p) < 256 for p in parts)


def _client_hello(server_hostname, alpn, options):
    """O ClientHello do TLS 1.3 (registro inteiro, mensagem de handshake e a chave privada X25519 do key share)."""
    priv = _os.urandom(32)
    share = b'\x00\x1d\x00\x20' + _x25519(priv, _X25519_BASE)
    extensions = b''
    if server_hostname and not _is_ip(server_hostname):
        host = server_hostname.encode('ascii')
        entry = b'\x00' + len(host).to_bytes(2, 'big') + host
        extensions += _ext(0, len(entry).to_bytes(2, 'big') + entry)
    extensions += _ext(10, b'\x00\x02\x00\x1d')
    extensions += _ext(13, _SIGALGS)
    if alpn:
        extensions += _ext(16, len(alpn).to_bytes(2, 'big') + alpn)
    extensions += _ext(43, b'\x02\x03\x04') + _ext(45, b'\x01\x01')
    extensions += _ext(51, len(share).to_bytes(2, 'big') + share)
    body = (b'\x03\x03' + _os.urandom(32) + b'\x20' + _os.urandom(32)
            + len(_CIPHER_SUITES).to_bytes(2, 'big') + _CIPHER_SUITES + b'\x01\x00'
            + len(extensions).to_bytes(2, 'big') + extensions)
    handshake = b'\x01' + len(body).to_bytes(3, 'big') + body
    return b'\x16\x03\x01' + len(handshake).to_bytes(2, 'big') + handshake, handshake, priv


# --- criptografia do TLS 1.3 (primitivas em Rust, no `_tls_crypto`) -------------------------------

def _hash(name, data):
    return getattr(_hashlib, name)(data).digest()


_hmac = _tls_crypto.hmac
_x25519 = _tls_crypto.x25519


def _expand_label(name, secret, label, context, length):
    """HKDF-Expand-Label do RFC 8446, seção 7.1."""
    full = b'tls13 ' + label
    info = length.to_bytes(2, 'big') + bytes([len(full)]) + full + bytes([len(context)]) + context
    out = b''
    block = b''
    counter = 1
    while len(out) < length:
        block = _hmac(name, secret, block + info + bytes([counter]))
        out += block
        counter += 1
    return out[:length]


_X25519_BASE = b'\x09' + bytes(31)


class _Keys:
    """Chaves de tráfego de um sentido (RFC 8446, seção 7.3) e o número de sequência dos registros."""

    def __init__(self, suite, secret):
        _, hash_name, key_len, kind, _ = _SUITES[suite]
        self.suite = suite
        self.secret = secret
        self.hash_name = hash_name
        self.kind = kind
        self.key = _expand_label(hash_name, secret, b'key', b'', key_len)
        self.iv = _expand_label(hash_name, secret, b'iv', b'', 12)
        self.seq = 0
        if kind == 'chacha':
            self._seal = _tls_crypto.chacha20_poly1305_seal
            self._open = _tls_crypto.chacha20_poly1305_open
        else:
            self._seal = _tls_crypto.aes_gcm_seal
            self._open = _tls_crypto.aes_gcm_open

    def _nonce(self):
        nonce = (int.from_bytes(self.iv, 'big') ^ self.seq).to_bytes(12, 'big')
        self.seq += 1
        return nonce

    def seal(self, plain, aad):
        return self._seal(self.key, self._nonce(), aad, plain)

    def open(self, data, aad):
        """O texto claro, ou `None` quando a etiqueta de autenticação não confere."""
        return self._open(self.key, self._nonce(), aad, data)

    def update(self):
        """As chaves do KeyUpdate (RFC 8446, seção 7.2)."""
        return _Keys(self.suite, _expand_label(self.hash_name, self.secret, b'traffic upd', b'', len(self.secret)))


# --- curvas elípticas e RSA (verificação de assinatura) -------------------------------------------

class _Curve:
    """Curva de Weierstrass `y^2 = x^3 - 3x + b` sobre o corpo primo `p`, em coordenadas jacobianas."""

    def __init__(self, p, n, b, gx, gy):
        self.p = p
        self.n = n
        self.b = b
        self.g = (gx, gy)
        self.valid = (gy * gy - (gx * gx * gx - 3 * gx + b)) % p == 0

    def _double(self, point):
        x, y, z = point
        p = self.p
        if z == 0 or y == 0:
            return (1, 1, 0)
        delta = z * z % p
        gamma = y * y % p
        beta = x * gamma % p
        alpha = 3 * (x - delta) * (x + delta) % p
        x3 = (alpha * alpha - 8 * beta) % p
        z3 = ((y + z) * (y + z) - gamma - delta) % p
        y3 = (alpha * (4 * beta - x3) - 8 * gamma * gamma) % p
        return (x3, y3, z3)

    def _add(self, a, b):
        x1, y1, z1 = a
        x2, y2, z2 = b
        p = self.p
        if z1 == 0:
            return b
        if z2 == 0:
            return a
        z1z1 = z1 * z1 % p
        z2z2 = z2 * z2 % p
        u1 = x1 * z2z2 % p
        u2 = x2 * z1z1 % p
        s1 = y1 * z2 * z2z2 % p
        s2 = y2 * z1 * z1z1 % p
        if u1 == u2:
            if s1 == s2:
                return self._double(a)
            return (1, 1, 0)
        h = (u2 - u1) % p
        i = (2 * h) * (2 * h) % p
        j = h * i % p
        r = 2 * (s2 - s1) % p
        v = u1 * i % p
        x3 = (r * r - j - 2 * v) % p
        y3 = (r * (v - x3) - 2 * s1 * j) % p
        z3 = (((z1 + z2) * (z1 + z2) - z1z1 - z2z2) * h) % p
        return (x3, y3, z3)

    def _mul(self, k, point):
        result = (1, 1, 0)
        for i in range(k.bit_length() - 1, -1, -1):
            result = self._double(result)
            if (k >> i) & 1:
                result = self._add(result, point)
        return result

    def verify(self, q, digest, r, s):
        """ECDSA (FIPS 186-4): `q` é a chave pública `(x, y)`, `digest` o resumo da mensagem."""
        n, p = self.n, self.p
        if not (0 < r < n and 0 < s < n):
            return False
        qx, qy = q
        if not (0 <= qx < p and 0 <= qy < p) or (qy * qy - (qx * qx * qx - 3 * qx + self.b)) % p != 0:
            return False
        e = int.from_bytes(digest, 'big')
        extra = len(digest) * 8 - n.bit_length()
        if extra > 0:
            e >>= extra
        w = pow(s, n - 2, n)
        u1 = e * w % n
        u2 = r * w % n
        point = self._add(self._mul(u1, (self.g[0], self.g[1], 1)), self._mul(u2, (qx, qy, 1)))
        if point[2] == 0:
            return False
        zinv = pow(point[2], p - 2, p)
        return point[0] * zinv * zinv % p % n == r


_CURVES = {}


def _curve(oid):
    """A curva de um OID de `namedCurve` (P-256 ou P-384), ou `None` se não for suportada."""
    if not _CURVES:
        _CURVES['1.2.840.10045.3.1.7'] = _Curve(
            0xffffffff00000001000000000000000000000000ffffffffffffffffffffffff,
            0xffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551,
            0x5ac635d8aa3a93e7b3ebbd55769886bc651d06b0cc53b0f63bce3c3e27d2604b,
            0x6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296,
            0x4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5)
        _CURVES['1.3.132.0.34'] = _Curve(
            (1 << 384) - (1 << 128) - (1 << 96) + (1 << 32) - 1,
            0xffffffffffffffffffffffffffffffffffffffffffffffffc7634d81f4372ddf581a0db248b0a77aecec196accc52973,
            0xb3312fa7e23ee7e4988e056be3f82d19181d9c6efe8141120314088f5013875ac656398d8a2ed19d2a85c8edd3ec2aef,
            0xaa87ca22be8b05378eb1c71ef320ad746e1d3b628ba79b9859f741e082542a385502f25dbf55296c3a545e3872760ab7,
            0x3617de4a96262c6f5d9e98bf9292dc29f8f41dbd289a147ce9da3113b5f0b8c00a60b1ce1d7e819d7a431d7c90ea0e5f)
    found = _CURVES.get(oid)
    if found is not None and not found.valid:
        return None
    return found


_DIGEST_INFO = {
    'sha1': bytes.fromhex('3021300906052b0e03021a05000414'),
    'sha256': bytes.fromhex('3031300d060960864801650304020105000420'),
    'sha384': bytes.fromhex('3041300d060960864801650304020205000430'),
    'sha512': bytes.fromhex('3051300d060960864801650304020305000440'),
}


def _mgf1(name, seed, length):
    out = b''
    counter = 0
    while len(out) < length:
        out += _hash(name, seed + counter.to_bytes(4, 'big'))
        counter += 1
    return out[:length]


def _rsa_public(bits):
    """`(n, e)` de uma chave `RSAPublicKey` em DER."""
    _, start, end = _der_read(bits, 0)
    (_, ns, ne), (_, es, ee) = _der_children(bits, start, end)[:2]
    return int.from_bytes(bits[ns:ne], 'big'), int.from_bytes(bits[es:ee], 'big')


def _rsa_pkcs1_verify(n, e, name, msg, sig):
    size = (n.bit_length() + 7) // 8
    value = int.from_bytes(sig, 'big')
    if len(sig) != size or value >= n or name not in _DIGEST_INFO:
        return False
    block = pow(value, e, n).to_bytes(size, 'big')
    tail = _DIGEST_INFO[name] + _hash(name, msg)
    return block == b'\x00\x01' + b'\xff' * (size - len(tail) - 3) + b'\x00' + tail


def _rsa_pss_verify(n, e, name, msg, sig):
    """RSASSA-PSS com MGF1 do mesmo hash e sal do tamanho do hash, como o TLS 1.3 exige."""
    hlen = len(_hash(name, b''))
    mod_bits = n.bit_length()
    value = int.from_bytes(sig, 'big')
    if len(sig) != (mod_bits + 7) // 8 or value >= n:
        return False
    em_bits = mod_bits - 1
    em_len = (em_bits + 7) // 8
    em_value = pow(value, e, n)
    if em_value.bit_length() > em_bits or em_len < 2 * hlen + 2:
        return False
    em = em_value.to_bytes(em_len, 'big')
    if em[-1] != 0xbc:
        return False
    masked, digest = em[:em_len - hlen - 1], em[em_len - hlen - 1:-1]
    top = 8 * em_len - em_bits
    if top and masked[0] >> (8 - top):
        return False
    block = bytearray(a ^ b for a, b in zip(masked, _mgf1(name, digest, len(masked))))
    if top:
        block[0] &= 0xff >> top
    pad = em_len - 2 * hlen - 2
    if any(block[:pad]) or block[pad] != 1:
        return False
    salt = bytes(block[pad + 1:])
    return _hash(name, bytes(8) + _hash(name, msg) + salt) == digest


def _asn1_epoch(tag, raw):
    """UTCTime ou GeneralizedTime em segundos desde 1970."""
    text = raw.decode('ascii')
    if tag == 0x17:
        year = int(text[0:2])
        year += 1900 if year >= 50 else 2000
        rest = text[2:]
    else:
        year = int(text[0:4])
        rest = text[4:]
    month, day, hour, minute = int(rest[0:2]), int(rest[2:4]), int(rest[4:6]), int(rest[6:8])
    second = int(rest[8:10]) if rest[8:10].isdigit() else 0
    if month <= 2:
        year -= 1
    era = year // 400
    yoe = year - era * 400
    doy = (153 * (month + (-3 if month > 2 else 9)) + 2) // 5 + day - 1
    days = era * 146097 + yoe * 365 + yoe // 4 - yoe // 100 + doy - 719468
    return days * 86400 + hour * 3600 + minute * 60 + second


# Algoritmos de assinatura de certificado: OID -> (tipo, hash).
_CERT_SIG_OIDS = {
    '1.2.840.10045.4.3.2': ('ecdsa', 'sha256'), '1.2.840.10045.4.3.3': ('ecdsa', 'sha384'),
    '1.2.840.10045.4.3.4': ('ecdsa', 'sha512'), '1.2.840.113549.1.1.5': ('rsa', 'sha1'),
    '1.2.840.113549.1.1.11': ('rsa', 'sha256'), '1.2.840.113549.1.1.12': ('rsa', 'sha384'),
    '1.2.840.113549.1.1.13': ('rsa', 'sha512'),
}


class _X509:
    """O que a validação da cadeia precisa de um certificado DER."""

    def __init__(self, der):
        d = self.der = bytes(der)
        _, cs, _ = _der_read(d, 0)
        _, tcs, tce = _der_read(d, cs)
        self.tbs = d[cs:tce]
        _, acs, ace = _der_read(d, tce)
        alg = _der_children(d, acs, ace)
        self.sig_oid = _oid_text(d[alg[0][1]:alg[0][2]])
        _, bcs, bce = _der_read(d, ace)
        self.sig = d[bcs + 1:bce]
        fields = []
        pos = tcs
        while pos < tce:
            tag, start, end = _der_read(d, pos)
            fields.append((tag, pos, start, end))
            pos = end
        i = 1 if fields[0][0] == 0xa0 else 0
        self.issuer = d[fields[i + 2][1]:fields[i + 2][3]]
        self.subject = d[fields[i + 4][1]:fields[i + 4][3]]
        validity = _der_children(d, fields[i + 3][2], fields[i + 3][3])
        self.not_before = _asn1_epoch(validity[0][0], d[validity[0][1]:validity[0][2]])
        self.not_after = _asn1_epoch(validity[1][0], d[validity[1][1]:validity[1][2]])
        spki = _der_children(d, fields[i + 5][2], fields[i + 5][3])
        key_alg = _der_children(d, spki[0][1], spki[0][2])
        self.key_oid = _oid_text(d[key_alg[0][1]:key_alg[0][2]])
        self.key_param = None
        if len(key_alg) > 1 and key_alg[1][0] == 0x06:
            self.key_param = _oid_text(d[key_alg[1][1]:key_alg[1][2]])
        self.key_bits = d[spki[1][1] + 1:spki[1][2]]
        self.is_ca = None
        self.names = []
        self.common_name = None
        for rdn in _name_tuple(d, fields[i + 4][2], fields[i + 4][3]):
            for key, value in rdn:
                if key == 'commonName' and self.common_name is None:
                    self.common_name = value
        for tag, _, start, end in fields[i + 6:]:
            if tag != 0xa3:
                continue
            (_, sstart, send), = _der_children(d, start, end)[:1]
            for _, xstart, xend in _der_children(d, sstart, send):
                parts = _der_children(d, xstart, xend)
                oid = _oid_text(d[parts[0][1]:parts[0][2]])
                value_start = parts[-1][1]
                if oid == '2.5.29.17':
                    _, gstart, gend = _der_read(d, value_start)
                    self.names = _general_names(d, gstart, gend)
                elif oid == '2.5.29.19':
                    _, bstart, bend = _der_read(d, value_start)
                    kids = _der_children(d, bstart, bend)
                    self.is_ca = bool(kids and kids[0][0] == 1 and d[kids[0][1]] != 0)

    def verify(self, kind, name, msg, sig):
        """Confere `sig` sobre `msg` com a chave pública deste certificado."""
        if kind == 'ecdsa':
            curve = _curve(self.key_param) if self.key_oid == '1.2.840.10045.2.1' else None
            if curve is None:
                return False
            size = (curve.n.bit_length() + 7) // 8
            bits = self.key_bits
            if len(bits) != 1 + 2 * size or bits[0] != 4:
                return False
            try:
                _, start, end = _der_read(sig, 0)
                (_, rs, re_), (_, ss, se) = _der_children(sig, start, end)[:2]
            except (IndexError, ValueError):
                return False
            point = (int.from_bytes(bits[1:1 + size], 'big'), int.from_bytes(bits[1 + size:], 'big'))
            return curve.verify(point, _hash(name, msg), int.from_bytes(sig[rs:re_], 'big'),
                                int.from_bytes(sig[ss:se], 'big'))
        if self.key_oid != '1.2.840.113549.1.1.1':
            return False
        try:
            n, e = _rsa_public(self.key_bits)
        except (IndexError, ValueError):
            return False
        if kind == 'pss':
            return _rsa_pss_verify(n, e, name, msg, sig)
        return _rsa_pkcs1_verify(n, e, name, msg, sig)

    def signed_by(self, issuer):
        spec = _CERT_SIG_OIDS.get(self.sig_oid)
        return spec is not None and issuer.verify(spec[0], spec[1], self.tbs, self.sig)


def _dns_match(pattern, host):
    pattern = pattern.rstrip('.').lower()
    if '*' not in pattern:
        return pattern == host
    # Só `*` como o rótulo inteiro à esquerda (HOSTFLAG_NO_PARTIAL_WILDCARDS), com ao menos dois rótulos depois.
    if not pattern.startswith('*.') or pattern.count('*') != 1 or '.' not in pattern[2:]:
        return False
    first, _, rest = host.partition('.')
    return bool(first) and rest == pattern[2:]


def _same_ip(a, b):
    try:
        fam_a = _socket.AF_INET6 if ':' in a else _socket.AF_INET
        fam_b = _socket.AF_INET6 if ':' in b else _socket.AF_INET
        return fam_a == fam_b and _socket.inet_pton(fam_a, a) == _socket.inet_pton(fam_b, b)
    except OSError:
        return False


def _cert_matches_host(cert, host):
    """Os nomes do certificado cobrem `host` (SAN; o CN só vale sem nenhum SAN DNS)."""
    if _is_ip(host):
        return any(kind == 'IP Address' and _same_ip(value, host) for kind, value in cert.names)
    host = host.rstrip('.').lower()
    dns = [value for kind, value in cert.names if kind == 'DNS']
    if not dns and cert.common_name:
        dns = [cert.common_name]
    return any(_dns_match(pattern, host) for pattern in dns)


def _verify_failure(code, message):
    """O `SSLCertVerificationError` do `_ssl.c`, com `verify_code` e `verify_message`."""
    err = SSLCertVerificationError(1, '[SSL: CERTIFICATE_VERIFY_FAILED] certificate verify failed: %s (_ssl.c:1029)'
                                   % message)
    err.library = 'SSL'
    err.reason = 'CERTIFICATE_VERIFY_FAILED'
    err.verify_code = code
    err.verify_message = message
    return err


# Código de erro do X509 -> alerta TLS que o OpenSSL manda ao par.
_VERIFY_ALERTS = {20: 48, 18: 48, 19: 48, 2: 48, 10: 45, 9: 45, 7: 42, 24: 42, 22: 42, 62: 42, 64: 42}


def _verify_chain(context, certs, host):
    """Valida a cadeia que o servidor mandou contra as CAs do contexto; levanta `SSLCertVerificationError`."""
    import time
    now = time.time()
    chain = [_X509(der) for der in certs]
    anchors = context._trust_index()
    pool = chain[1:]
    current = chain[0]
    path = [current]
    depth = 0
    while True:
        if current.not_after < now:
            raise _verify_failure(10, 'certificate has expired')
        if current.not_before > now:
            raise _verify_failure(9, 'certificate is not yet valid')
        if any(a.der == current.der for a in anchors.get(current.subject, ())):
            break
        issuer = None
        for candidate in context._trusted_issuers(current.issuer):
            if current.signed_by(candidate):
                issuer = candidate
                break
        if issuer is not None:
            if issuer.is_ca is False:
                raise _verify_failure(24, 'invalid CA certificate')
            if issuer.not_after < now:
                raise _verify_failure(10, 'certificate has expired')
            if issuer.not_before > now:
                raise _verify_failure(9, 'certificate is not yet valid')
            break
        following = None
        for candidate in pool:
            if candidate.subject == current.issuer and candidate not in path and current.signed_by(candidate):
                following = candidate
                break
        if following is None:
            if current.subject == current.issuer:
                raise _verify_failure(18 if depth == 0 else 19, 'self-signed certificate' if depth == 0
                                      else 'self-signed certificate in certificate chain')
            raise _verify_failure(20, 'unable to get local issuer certificate')
        if following.is_ca is False:
            raise _verify_failure(24, 'invalid CA certificate')
        path.append(following)
        current = following
        depth += 1
        if depth > 9:
            raise _verify_failure(22, 'certificate chain too long')
    if host:
        if not _cert_matches_host(chain[0], host):
            if _is_ip(host):
                raise _verify_failure(64, "IP address mismatch, certificate is not valid for '%s'." % host)
            raise _verify_failure(62, "Hostname mismatch, certificate is not valid for '%s'." % host)
    return path


# --- contexto -------------------------------------------------------------------------------------

def _index(value):
    import operator
    return operator.index(value)


class _SSLContext:
    def __new__(cls, protocol):
        protocol = _index(protocol)
        if protocol not in _PROTOCOLS:
            raise ValueError('invalid or unsupported protocol version %d' % protocol)
        if protocol in _DEPRECATED_PROTOCOLS:
            _warn('%s is deprecated' % _DEPRECATED_PROTOCOLS[protocol])
        self = object.__new__(cls)
        client = protocol == PROTOCOL_TLS_CLIENT
        self._protocol = protocol
        self._verify_mode = CERT_REQUIRED if client else CERT_NONE
        self._check_hostname = client
        self._options = _DEFAULT_OPTIONS
        self._verify_flags = VERIFY_X509_TRUSTED_FIRST
        self._min = PROTO_MINIMUM_SUPPORTED
        self._max = PROTO_MAXIMUM_SUPPORTED
        self._post_handshake_auth = False
        self._keylog = None
        self._num_tickets = 2
        self._host_flags = HOSTFLAG_NO_PARTIAL_WILDCARDS
        self._sni = None
        self._msg_cb = None
        self._alpn = b''
        self._ca = []
        self._capaths = []
        self._trust = None
        self._capath_loaded = False
        self._certs = 0
        self._cert_chain = False
        self._ciphers = _DEFAULT_CIPHERS
        return self

    @property
    def protocol(self):
        return self._protocol

    @property
    def verify_mode(self):
        return self._verify_mode

    @verify_mode.setter
    def verify_mode(self, value):
        value = _index(value)
        if value not in (CERT_NONE, CERT_OPTIONAL, CERT_REQUIRED):
            raise ValueError('invalid value for verify_mode')
        if value == CERT_NONE and self._check_hostname:
            raise ValueError('Cannot set verify_mode to CERT_NONE when check_hostname is enabled.')
        self._verify_mode = value

    @property
    def check_hostname(self):
        return self._check_hostname

    @check_hostname.setter
    def check_hostname(self, value):
        value = bool(value)
        if value and self._verify_mode == CERT_NONE:
            self._verify_mode = CERT_REQUIRED
        self._check_hostname = value

    @property
    def options(self):
        return self._options

    @options.setter
    def options(self, value):
        value = _index(value)
        if value < 0:
            raise OverflowError('can\'t convert negative int to unsigned')
        if (value & ~self._options) & _DEPRECATED_OPTIONS:
            _warn('ssl.OP_NO_SSL*/ssl.OP_NO_TLS* options are deprecated')
        self._options = value

    @property
    def verify_flags(self):
        return self._verify_flags

    @verify_flags.setter
    def verify_flags(self, value):
        self._verify_flags = _index(value)

    def _set_version(self, value, which):
        value = _index(value)
        if self._protocol not in (PROTOCOL_TLS, PROTOCOL_TLS_CLIENT, PROTOCOL_TLS_SERVER):
            raise ValueError("The context's protocol doesn't support modification of highest and lowest version.")
        if value not in _VERSIONS:
            raise ValueError('Unsupported TLS/SSL version 0x%x' % value)
        if value in _DEPRECATED_VERSIONS:
            _warn('%s is deprecated' % _DEPRECATED_VERSIONS[value])
        if which == 'min':
            self._min = value
        else:
            self._max = value

    @property
    def minimum_version(self):
        return self._min

    @minimum_version.setter
    def minimum_version(self, value):
        self._set_version(value, 'min')

    @property
    def maximum_version(self):
        return self._max

    @maximum_version.setter
    def maximum_version(self, value):
        self._set_version(value, 'max')

    @property
    def post_handshake_auth(self):
        return self._post_handshake_auth

    @post_handshake_auth.setter
    def post_handshake_auth(self, value):
        self._post_handshake_auth = bool(value)

    @property
    def keylog_filename(self):
        return self._keylog

    @keylog_filename.setter
    def keylog_filename(self, value):
        if value is not None:
            value = _os.fspath(value)
            with open(value, 'a'):
                pass
        self._keylog = value

    @property
    def num_tickets(self):
        return self._num_tickets

    @num_tickets.setter
    def num_tickets(self, value):
        value = _index(value)
        if value < 0:
            raise ValueError('value must be non-negative')
        if self._protocol != PROTOCOL_TLS_SERVER:
            raise ValueError('SSLContext is not a server context.')
        self._num_tickets = value

    @property
    def _msg_callback(self):
        return self._msg_cb

    @_msg_callback.setter
    def _msg_callback(self, value):
        if value is not None and not callable(value):
            raise TypeError('not a callable object')
        self._msg_cb = value

    @property
    def security_level(self):
        return 2

    @property
    def sni_callback(self):
        return self._sni

    @sni_callback.setter
    def sni_callback(self, value):
        if self._protocol == PROTOCOL_TLS_CLIENT:
            raise ValueError('sni_callback cannot be set on TLS_CLIENT context')
        if value is not None and not callable(value):
            raise TypeError('not a callable object')
        self._sni = value

    def _load_pem_file(self, path):
        with open(path, 'rb') as f:
            return f.read().decode('latin-1')

    def _add_certificates(self, ders):
        for der in ders:
            if der not in self._ca:
                self._ca.append(der)
        self._trust = None

    def _trust_index(self):
        """As CAs confiáveis do contexto, indexadas pelo nome do titular (DER)."""
        if self._trust is None:
            index = {}
            for der in self._ca:
                try:
                    cert = _X509(der)
                except (IndexError, ValueError):
                    continue
                index.setdefault(cert.subject, []).append(cert)
            self._trust = index
        return self._trust

    def _trusted_issuers(self, subject):
        """As CAs de nome `subject`; sem nenhuma, procura também nos diretórios de `capath`."""
        found = self._trust_index().get(subject, ())
        if not found and self._capaths and not self._capath_loaded:
            self._capath_loaded = True
            for path in self._capaths:
                try:
                    names = sorted(_os.listdir(path))
                except OSError:
                    continue
                for name in names:
                    try:
                        text = self._load_pem_file(_os.path.join(path, name))
                        self._add_certificates(_pem_certificates(text))
                    except (OSError, ValueError):
                        continue
            found = self._trust_index().get(subject, ())
        return found

    def load_verify_locations(self, cafile=None, capath=None, cadata=None):
        if cafile is None and capath is None and cadata is None:
            raise TypeError('cafile, capath and cadata cannot be all omitted')
        if cafile is not None:
            cafile = _os.fspath(cafile)
        if capath is not None:
            capath = _os.fspath(capath)
        if cadata is not None:
            if isinstance(cadata, str):
                ders = _pem_certificates(cadata)
                if not ders:
                    raise _ssl_error(SSLError, 0, None, 'no start line', 'cadata does not contain a certificate', 4212)
            else:
                ders = [bytes(cadata)]
            self._add_certificates(ders)
        if cafile is not None:
            try:
                text = self._load_pem_file(cafile)
            except FileNotFoundError:
                raise FileNotFoundError(2, 'No such file or directory') from None
            ders = _pem_certificates(text)
            if not ders:
                raise _ssl_error(SSLError, 136, 'X509', 'NO_CERTIFICATE_OR_CRL_FOUND', 'no certificate or crl found', 4346)
            self._add_certificates(ders)
        if capath is not None and not _os.path.isdir(capath):
            raise FileNotFoundError(2, 'No such file or directory')
        if capath is not None and capath not in self._capaths:
            self._capaths.append(capath)
            self._capath_loaded = False

    def set_default_verify_paths(self):
        cafile = _os.environ.get('SSL_CERT_FILE', _OPENSSLDIR + '/cert.pem')
        try:
            self._add_certificates(_pem_certificates(self._load_pem_file(cafile)))
        except OSError:
            pass
        # No Debian o cafile padrão não existe: as CAs vêm do diretório de hash (`certs`), lido sob demanda.
        capath = _os.environ.get('SSL_CERT_DIR', _OPENSSLDIR + '/certs')
        if _os.path.isdir(capath) and capath not in self._capaths:
            self._capaths.append(capath)
            self._capath_loaded = False

    def load_cert_chain(self, certfile, keyfile=None, password=None):
        certfile = _os.fspath(certfile)
        if keyfile is not None:
            keyfile = _os.fspath(keyfile)
        if password is not None and not (isinstance(password, (str, bytes, bytearray)) or callable(password)):
            raise TypeError('password should be a string or callable')
        try:
            text = self._load_pem_file(certfile)
        except FileNotFoundError:
            raise FileNotFoundError(2, 'No such file or directory') from None
        if not _pem_certificates(text):
            raise _ssl_error(SSLError, 524297, None, 'SSL', 'PEM lib', 4093)
        key_text = text if keyfile is None else self._load_pem_file(keyfile)
        if 'PRIVATE KEY-----' not in key_text:
            raise _ssl_error(SSLError, 524297, None, 'SSL', 'PEM lib', 4093)
        self._cert_chain = True

    def load_dh_params(self, path):
        try:
            text = self._load_pem_file(_os.fspath(path))
        except FileNotFoundError:
            raise FileNotFoundError(2, 'No such file or directory') from None
        if '-----BEGIN DH PARAMETERS-----' not in text:
            raise _ssl_error(SSLError, 108, 'PEM', 'NO_START_LINE', 'no start line', 4401)

    def set_ecdh_curve(self, name):
        if isinstance(name, bytes):
            name = name.decode('ascii')
        if name not in ('prime256v1', 'secp256r1', 'secp384r1', 'secp521r1', 'P-256', 'P-384', 'P-521',
                        'X25519', 'X448'):
            raise ValueError('unknown elliptic curve name %r' % name)

    def set_ciphers(self, cipherlist):
        names = {c[1] for c in _CIPHERS}
        selected = False
        for token in cipherlist.replace(',', ':').replace(' ', ':').split(':'):
            if not token or token.startswith('@'):
                continue
            if token[0] in '!-':
                continue
            token = token.lstrip('+')
            if any(part in _CIPHER_KEYWORDS or part in names for part in token.split('+')):
                selected = True
        if not selected:
            raise SSLError('No cipher can be selected.')
        self._ciphers = cipherlist

    def get_ciphers(self):
        return [_cipher_dict(c) for c in _CIPHERS]

    def _set_alpn_protocols(self, protos):
        self._alpn = bytes(protos)

    def set_psk_client_callback(self, callback):
        if self._protocol == PROTOCOL_TLS_SERVER:
            raise ValueError('Cannot add PSK client callback to a PROTOCOL_TLS_SERVER context')
        if callback is not None and not callable(callback):
            raise TypeError('callback must be callable')

    def set_psk_server_callback(self, callback, identity_hint=None):
        if self._protocol == PROTOCOL_TLS_CLIENT:
            raise ValueError('Cannot add PSK server callback to a PROTOCOL_TLS_CLIENT context')
        if callback is not None and not callable(callback):
            raise TypeError('callback must be callable')

    def cert_store_stats(self):
        ca = sum(1 for der in self._ca if _is_ca(der))
        return {'x509': len(self._ca), 'crl': 0, 'x509_ca': ca}

    def get_ca_certs(self, binary_form=False):
        out = []
        for der in self._ca:
            if not _is_ca(der):
                continue
            out.append(der if binary_form else _decode_der(der))
        return out

    def session_stats(self):
        return {'number': 0, 'connect': 0, 'connect_good': 0, 'connect_renegotiate': 0, 'accept': 0,
                'accept_good': 0, 'accept_renegotiate': 0, 'hits': 0, 'misses': 0, 'timeouts': 0,
                'cache_full': 0}

    def _wrap_socket(self, sock, server_side, server_hostname=None, *, owner=None, session=None):
        if server_side and self._protocol == PROTOCOL_TLS_CLIENT:
            raise ValueError('Cannot create a server socket with a PROTOCOL_TLS_CLIENT context')
        if not server_side and self._protocol == PROTOCOL_TLS_SERVER:
            raise ValueError('Cannot create a client socket with a PROTOCOL_TLS_SERVER context')
        if server_side and server_hostname is not None:
            raise ValueError('server_hostname can only be specified in client mode')
        if not server_side and self._check_hostname and not server_hostname:
            raise ValueError('check_hostname requires server_hostname')
        return _SSLSocket._make(self, sock, None, None, server_side, server_hostname, owner, session)

    def _wrap_bio(self, incoming, outgoing, server_side, server_hostname=None, *, owner=None, session=None):
        if not server_side and self._check_hostname and not server_hostname:
            raise ValueError('check_hostname requires server_hostname')
        return _SSLSocket._make(self, None, incoming, outgoing, server_side, server_hostname, owner, session)


_SSLContext.__module__ = '_ssl'


# --- conexão --------------------------------------------------------------------------------------

_HTTP_METHODS = (b'GET ', b'POST ', b'HEAD ', b'PUT ')


class _SSLSocket:
    def __new__(cls, *args, **kwargs):
        raise TypeError("cannot create '_ssl._SSLSocket' instances")

    @classmethod
    def _make(cls, context, sock, incoming, outgoing, server_side, server_hostname, owner, session):
        self = object.__new__(cls)
        self._context = context
        self._sock = sock
        self._incoming = incoming
        self._outgoing = outgoing
        self.server_side = bool(server_side)
        if isinstance(server_hostname, bytes):
            server_hostname = server_hostname.decode('ascii')
        self.server_hostname = server_hostname
        self.owner = owner
        self._session = session
        self._sent_hello = False
        self._failed = None
        # Estado do TLS do cliente: bytes crus recebidos, texto claro ainda não lido, mensagens de handshake
        # em remontagem, as chaves de cada sentido e o que o handshake aprendeu do par.
        self._rbuf = bytearray()
        self._plain = bytearray()
        self._hsbuf = bytearray()
        self._stage = 'server_hello'
        self._rkeys = None
        self._wkeys = None
        self._suite = None
        self._transcript = bytearray()
        self._shutdown_seen = False
        self._peer_certs = None
        self._peer_chain = None
        self._alpn_selected = None
        self._cert_requested = False
        return self

    @property
    def context(self):
        return self._context

    @context.setter
    def context(self, ctx):
        if not isinstance(ctx, _SSLContext):
            raise TypeError('The value must be a SSLContext')
        self._context = ctx

    @property
    def session(self):
        return self._session

    @session.setter
    def session(self, value):
        if self.server_side:
            raise ValueError('Cannot set session for server-side SSLSocket.')
        raise ValueError('Cannot set session after handshake.')

    @property
    def session_reused(self):
        return False

    # Entrada e saída cruas: pelo descritor do socket (respeita o timeout) ou pelos MemoryBIO.
    def _send(self, data):
        if self._sock is not None:
            _socket.socket.sendall(self._sock, data)
        else:
            self._outgoing.write(data)

    def _recv_exact(self, count):
        if self._sock is None:
            # MemoryBIO: sem o registro inteiro, nada é consumido e o chamador alimenta mais dados.
            if self._incoming.pending < count and not self._incoming._eof_written:
                raise _want_read()
            return self._incoming.read(count)
        data = b''
        while len(data) < count:
            try:
                chunk = _socket.socket.recv(self._sock, count - len(data))
            except TimeoutError:
                raise TimeoutError('_ssl.c:1012: The handshake operation timed out') from None
            if not chunk:
                break
            data += chunk
        return data

    def _fail(self, err):
        self._failed = err
        raise err

    def do_handshake(self):
        if self._failed is not None:
            raise self._failed
        if self.server_side:
            head = self._recv_exact(5)
            if not head:
                self._fail(_ssl_error(SSLEOFError, 8, 'SSL', 'UNEXPECTED_EOF_WHILE_READING',
                                      'EOF occurred in violation of protocol', 1029))
            if head.startswith(_HTTP_METHODS) or head[:4] in (b'GET ', b'POST', b'HEAD', b'PUT '):
                self._fail(_ssl_error(SSLError, 1, 'SSL', 'HTTP_REQUEST', 'http request', 1029))
            if head.startswith(b'CONNE'):
                self._fail(_ssl_error(SSLError, 1, 'SSL', 'HTTPS_PROXY_REQUEST', 'https proxy request', 1029))
            if head[0] == 0x16 and head[1] == 3:
                self._fail(_ssl_error(SSLError, 1, 'SSL', 'NO_SHARED_CIPHER', 'no shared cipher', 1029))
            self._fail(_ssl_error(SSLError, 1, 'SSL', 'WRONG_VERSION_NUMBER', 'wrong version number', 1029))
        if self._stage != 'done':
            self._handshake_client()

    # -- TLS 1.3 do cliente ----------------------------------------------------------------------
    def _eof_error(self):
        return _ssl_error(SSLEOFError, 8, 'SSL', 'UNEXPECTED_EOF_WHILE_READING',
                          'EOF occurred in violation of protocol', 1029)

    def _fill(self, count, what):
        """Garante `count` bytes crus em `_rbuf` sem consumir nada: sem eles, `WantRead` (ou o prazo vencido)."""
        buf = self._rbuf
        if self._sock is None:
            incoming = self._incoming
            while len(buf) < count:
                chunk = incoming.read(count - len(buf))
                if chunk:
                    buf += chunk
                elif incoming._eof_written:
                    self._fail(self._eof_error())
                else:
                    raise _want_read()
            return
        while len(buf) < count:
            try:
                chunk = _socket.socket.recv(self._sock, 16384)
            except TimeoutError:
                prefix = '_ssl.c:1012: ' if what == 'handshake' else ''
                raise TimeoutError('%sThe %s operation timed out' % (prefix, what)) from None
            except BlockingIOError:
                raise _want_read() from None
            if not chunk:
                self._fail(self._eof_error())
            buf += chunk

    def _read_record(self, what):
        """Lê um registro TLS e devolve `(tipo, conteúdo)`, já decifrado quando há chaves de leitura."""
        self._fill(5, what)
        buf = self._rbuf
        if buf[0] not in (0x14, 0x15, 0x16, 0x17) or buf[1] != 3:
            self._fail(_ssl_error(SSLError, 1, 'SSL', 'WRONG_VERSION_NUMBER', 'wrong version number', 1029))
        length = (buf[3] << 8) | buf[4]
        if length > 16384 + 256:
            self._fail(_ssl_error(SSLError, 1, 'SSL', 'PACKET_LENGTH_TOO_LONG', 'packet length too long', 1029))
        self._fill(5 + length, what)
        header = bytes(buf[:5])
        body = bytes(buf[5:5 + length])
        del buf[:5 + length]
        kind = header[0]
        if kind == 0x17 and self._rkeys is not None:
            inner = self._rkeys.open(body, header)
            if inner is None:
                self._fail(_ssl_error(SSLError, 1, 'SSL', 'DECRYPTION_FAILED_OR_BAD_RECORD_MAC',
                                      'decryption failed or bad record mac', 1029))
            end = len(inner)
            while end > 0 and inner[end - 1] == 0:
                end -= 1
            if end == 0:
                self._fail(_ssl_error(SSLError, 1, 'SSL', 'SSLV3_ALERT_UNEXPECTED_MESSAGE',
                                      'ssl/tls alert unexpected message', 1029))
            return inner[end - 1], inner[:end - 1]
        return kind, body

    def _send_record(self, kind, data):
        if self._wkeys is None:
            self._send(bytes((kind, 3, 3)) + len(data).to_bytes(2, 'big') + data)
            return
        inner = data + bytes((kind,))
        header = b'\x17\x03\x03' + (len(inner) + 16).to_bytes(2, 'big')
        self._send(header + self._wkeys.seal(inner, header))

    def _send_alert(self, description):
        try:
            self._send_record(0x15, bytes((2, description)))
        except OSError:
            pass

    def _on_alert(self, body):
        if len(body) < 2:
            self._fail(_ssl_error(SSLError, 1, 'SSL', 'SSLV3_ALERT_UNEXPECTED_MESSAGE',
                                  'ssl/tls alert unexpected message', 1029))
        if body[1] == 0:
            self._shutdown_seen = True
            return
        known = _ALERTS.get(body[1])
        if known is None:
            self._fail(_ssl_error(SSLError, 1, None, 'SSL', 'unknown error', 1029))
        self._fail(_ssl_error(SSLError, 1, 'SSL', known[0], known[1], 1029))

    def _unexpected(self):
        self._send_alert(10)
        self._fail(_ssl_error(SSLError, 1, 'SSL', 'SSLV3_ALERT_UNEXPECTED_MESSAGE',
                              'ssl/tls alert unexpected message', 1029))

    def _next_handshake_message(self):
        """A próxima mensagem de handshake inteira como `(tipo, corpo, bytes crus)`."""
        buf = self._hsbuf
        while True:
            if len(buf) >= 4:
                size = 4 + int.from_bytes(buf[1:4], 'big')
                if len(buf) >= size:
                    raw = bytes(buf[:size])
                    del buf[:size]
                    return raw[0], raw[4:], raw
            kind, body = self._read_record('handshake')
            if kind == 0x14:
                continue
            if kind == 0x16:
                buf += body
            elif kind == 0x15:
                self._on_alert(body)
                self._fail(self._eof_error())
            else:
                self._unexpected()

    def _transcript_hash(self):
        return _hash(_SUITES[self._suite][1], bytes(self._transcript))

    def _handshake_client(self):
        if not self._sent_hello:
            record, hello, self._private = _client_hello(self.server_hostname, self._context._alpn,
                                                         self._context._options)
            self._transcript = bytearray(hello)
            self._send(record)
            self._sent_hello = True
        try:
            while self._stage != 'done':
                self._handshake_step()
        except SSLError:
            raise
        except (IndexError, ValueError, KeyError, OverflowError, TypeError, AttributeError):
            self._fail(_ssl_error(SSLError, 1, 'SSL', 'SSLV3_ALERT_HANDSHAKE_FAILURE',
                                  'ssl/tls alert handshake failure', 1029))

    def _handshake_step(self):
        kind, body, raw = self._next_handshake_message()
        stage = self._stage
        if stage == 'server_hello':
            if kind != 2:
                self._unexpected()
            self._on_server_hello(body, raw)
        elif stage == 'encrypted_extensions':
            if kind != 8:
                self._unexpected()
            self._transcript += raw
            self._on_encrypted_extensions(body)
            self._stage = 'certificate'
        elif stage == 'certificate':
            if kind == 13 and not self._cert_requested:
                self._transcript += raw
                self._cert_requested = True
            elif kind == 11:
                self._on_certificate(body)
                self._transcript += raw
                self._stage = 'certificate_verify'
            else:
                self._unexpected()
        elif stage == 'certificate_verify':
            if kind != 15:
                self._unexpected()
            self._on_certificate_verify(body)
            self._transcript += raw
            self._stage = 'finished'
        elif stage == 'finished':
            if kind != 20:
                self._unexpected()
            self._on_finished(body, raw)

    def _on_server_hello(self, body, raw):
        if body[2:34] == _HELLO_RETRY_RANDOM:
            self._send_alert(40)
            self._fail(_ssl_error(SSLError, 1, 'SSL', 'SSLV3_ALERT_HANDSHAKE_FAILURE',
                                  'ssl/tls alert handshake failure', 1029))
        pos = 34
        pos += 1 + body[pos]
        suite = int.from_bytes(body[pos:pos + 2], 'big')
        pos += 3
        end = pos + 2 + int.from_bytes(body[pos:pos + 2], 'big')
        pos += 2
        version = share = group = None
        while pos < end:
            kind = int.from_bytes(body[pos:pos + 2], 'big')
            size = int.from_bytes(body[pos + 2:pos + 4], 'big')
            data = body[pos + 4:pos + 4 + size]
            pos += 4 + size
            if kind == 43:
                version = int.from_bytes(data, 'big')
            elif kind == 51:
                group = int.from_bytes(data[:2], 'big')
                share = data[4:4 + int.from_bytes(data[2:4], 'big')]
        if version != 0x0304:
            self._send_alert(70)
            self._fail(_ssl_error(SSLError, 1, 'SSL', 'TLSV1_ALERT_PROTOCOL_VERSION',
                                  'tlsv1 alert protocol version', 1029))
        if suite not in _SUITES or group != 0x001d or share is None or len(share) != 32:
            self._send_alert(40)
            self._fail(_ssl_error(SSLError, 1, 'SSL', 'SSLV3_ALERT_HANDSHAKE_FAILURE',
                                  'ssl/tls alert handshake failure', 1029))
        shared = _x25519(self._private, share)
        if not any(shared):
            self._send_alert(47)
            self._fail(_ssl_error(SSLError, 1, 'SSL', 'SSLV3_ALERT_ILLEGAL_PARAMETER',
                                  'ssl/tls alert illegal parameter', 1029))
        self._suite = suite
        self._transcript += raw
        name = _SUITES[suite][1]
        zeros = bytes(len(_hash(name, b'')))
        empty = _hash(name, b'')
        early = _hmac(name, zeros, zeros)
        handshake = _hmac(name, _expand_label(name, early, b'derived', empty, len(empty)), shared)
        digest = self._transcript_hash()
        self._client_hs_secret = _expand_label(name, handshake, b'c hs traffic', digest, len(empty))
        self._server_hs_secret = _expand_label(name, handshake, b's hs traffic', digest, len(empty))
        self._master = _hmac(name, _expand_label(name, handshake, b'derived', empty, len(empty)), zeros)
        self._rkeys = _Keys(suite, self._server_hs_secret)
        self._wkeys = _Keys(suite, self._client_hs_secret)
        self._stage = 'encrypted_extensions'

    def _on_encrypted_extensions(self, body):
        pos = 2
        end = 2 + int.from_bytes(body[:2], 'big')
        while pos < end:
            kind = int.from_bytes(body[pos:pos + 2], 'big')
            size = int.from_bytes(body[pos + 2:pos + 4], 'big')
            data = body[pos + 4:pos + 4 + size]
            pos += 4 + size
            if kind == 16 and len(data) >= 3:
                self._alpn_selected = data[3:3 + data[2]].decode('ascii')

    def _on_certificate(self, body):
        pos = 1 + body[0]
        end = pos + 3 + int.from_bytes(body[pos:pos + 3], 'big')
        pos += 3
        certs = []
        while pos < end:
            size = int.from_bytes(body[pos:pos + 3], 'big')
            certs.append(bytes(body[pos + 3:pos + 3 + size]))
            pos += 3 + size
            pos += 2 + int.from_bytes(body[pos:pos + 2], 'big')
        self._peer_certs = certs
        context = self._context
        if context._verify_mode == CERT_NONE:
            return
        if not certs:
            self._send_alert(42)
            self._fail(_verify_failure(21, 'unable to verify the first certificate'))
        host = self.server_hostname if context._check_hostname else None
        try:
            path = _verify_chain(context, certs, host)
        except SSLCertVerificationError as err:
            self._send_alert(_VERIFY_ALERTS.get(err.verify_code, 42))
            self._fail(err)
        except (IndexError, ValueError):
            self._send_alert(42)
            self._fail(_ssl_error(SSLError, 1, 'SSL', 'SSLV3_ALERT_BAD_CERTIFICATE',
                                  'ssl/tls alert bad certificate', 1029))
        self._peer_chain = [cert.der for cert in path]

    def _on_certificate_verify(self, body):
        context = self._context
        if context._verify_mode == CERT_NONE or not self._peer_certs:
            return
        spec = _CERT_VERIFY_ALGS.get(int.from_bytes(body[:2], 'big'))
        size = int.from_bytes(body[2:4], 'big')
        signature = body[4:4 + size]
        signed = b' ' * 64 + b'TLS 1.3, server CertificateVerify\x00' + self._transcript_hash()
        if spec is None or not _X509(self._peer_certs[0]).verify(spec[0], spec[1], signed, signature):
            self._send_alert(51)
            self._fail(_ssl_error(SSLError, 1, 'SSL', 'TLSV1_ALERT_DECRYPT_ERROR', 'tlsv1 alert decrypt error', 1029))

    def _on_finished(self, body, raw):
        name = _SUITES[self._suite][1]
        size = len(_hash(name, b''))
        key = _expand_label(name, self._server_hs_secret, b'finished', b'', size)
        if _hmac(name, key, self._transcript_hash()) != body:
            self._send_alert(51)
            self._fail(_ssl_error(SSLError, 1, 'SSL', 'DIGEST_CHECK_FAILED', 'digest check failed', 1029))
        self._transcript += raw
        digest = self._transcript_hash()
        client_app = _expand_label(name, self._master, b'c ap traffic', digest, size)
        server_app = _expand_label(name, self._master, b's ap traffic', digest, size)
        # O CCS de compatibilidade e, se o servidor pediu certificado, a lista vazia (não há certificado de cliente).
        self._send(b'\x14\x03\x03\x00\x01\x01')
        if self._cert_requested:
            empty = b'\x0b\x00\x00\x04\x00\x00\x00\x00'
            self._send_record(0x16, empty)
            self._transcript += empty
        key = _expand_label(name, self._client_hs_secret, b'finished', b'', size)
        verify_data = _hmac(name, key, self._transcript_hash())
        self._send_record(0x16, b'\x14' + len(verify_data).to_bytes(3, 'big') + verify_data)
        self._rkeys = _Keys(self._suite, server_app)
        self._wkeys = _Keys(self._suite, client_app)
        self._hsbuf = bytearray()
        self._stage = 'done'

    def _post_handshake(self, body):
        buf = self._hsbuf
        buf += body
        while len(buf) >= 4:
            size = 4 + int.from_bytes(buf[1:4], 'big')
            if len(buf) < size:
                break
            raw = bytes(buf[:size])
            del buf[:size]
            if raw[0] == 24:
                self._rkeys = self._rkeys.update()
                if raw[4:5] == b'\x01':
                    self._send_record(0x16, b'\x18\x00\x00\x01\x00')
                    self._wkeys = self._wkeys.update()

    def read(self, size=1024, buffer=None):
        if buffer is not None:
            size = len(buffer) if size is None else min(size, len(buffer))
        self.do_handshake()
        if size == 0:
            return 0 if buffer is not None else b''
        while not self._plain:
            if self._shutdown_seen:
                return 0 if buffer is not None else b''
            kind, body = self._read_record('read')
            if kind == 0x17:
                self._plain += body
            elif kind == 0x16:
                self._post_handshake(body)
            elif kind == 0x15:
                self._on_alert(body)
        count = min(size, len(self._plain))
        data = bytes(self._plain[:count])
        del self._plain[:count]
        if buffer is not None:
            buffer[:count] = data
            return count
        return data

    def write(self, data):
        self.do_handshake()
        payload = memoryview(data).tobytes()
        for start in range(0, len(payload), 16384):
            self._send_record(0x17, payload[start:start + 16384])
        return len(payload)

    def pending(self):
        return len(self._plain)

    def getpeercert(self, binary_form=False):
        if self._stage != 'done':
            raise ValueError('handshake not done yet')
        if not self._peer_certs:
            return None
        if binary_form:
            return self._peer_certs[0]
        if self._context._verify_mode == CERT_NONE:
            return {}
        return _decode_der(self._peer_certs[0])

    def _certificates(self, chain):
        out = []
        for der in chain or ():
            cert = object.__new__(Certificate)
            cert._der = der
            out.append(cert)
        return out

    def get_verified_chain(self):
        return self._certificates(self._peer_chain) if self._peer_chain else None

    def get_unverified_chain(self):
        return self._certificates(self._peer_certs) if self._peer_certs else None

    def selected_alpn_protocol(self):
        return self._alpn_selected

    def cipher(self):
        if self._stage != 'done':
            return None
        name, _, _, _, bits = _SUITES[self._suite]
        return (name, 'TLSv1.3', bits)

    def shared_ciphers(self):
        return None

    def compression(self):
        return None

    def version(self):
        return 'TLSv1.3' if self._stage == 'done' else None

    def shutdown(self):
        if self._stage == 'done' and not self._shutdown_seen:
            self._send_alert_close()
        if self._sock is not None:
            return self._sock
        return None

    def _send_alert_close(self):
        try:
            self._send_record(0x15, b'\x01\x00')
        except OSError:
            pass

    def get_channel_binding(self, cb_type='tls-unique'):
        if cb_type != 'tls-unique':
            raise ValueError("Unsupported channel binding type '%s'" % cb_type)
        return None

    def verify_client_post_handshake(self):
        raise _ssl_error(SSLError, 1, 'SSL', 'WRONG_SSL_VERSION', 'wrong ssl version', 1029)


_SSLSocket.__module__ = '_ssl'

# Alertas TLS recebidos: o motivo e o texto que o OpenSSL 3.5.7 dá a cada código (medidos no oráculo).
_ALERTS = {
    10: ('SSLV3_ALERT_UNEXPECTED_MESSAGE', 'ssl/tls alert unexpected message'),
    20: ('SSLV3_ALERT_BAD_RECORD_MAC', 'ssl/tls alert bad record mac'),
    21: ('TLSV1_ALERT_DECRYPTION_FAILED', 'tlsv1 alert decryption failed'),
    22: ('TLSV1_ALERT_RECORD_OVERFLOW', 'tlsv1 alert record overflow'),
    30: ('SSLV3_ALERT_DECOMPRESSION_FAILURE', 'ssl/tls alert decompression failure'),
    40: ('SSLV3_ALERT_HANDSHAKE_FAILURE', 'ssl/tls alert handshake failure'),
    41: ('SSLV3_ALERT_NO_CERTIFICATE', 'ssl/tls alert no certificate'),
    42: ('SSLV3_ALERT_BAD_CERTIFICATE', 'ssl/tls alert bad certificate'),
    43: ('SSLV3_ALERT_UNSUPPORTED_CERTIFICATE', 'ssl/tls alert unsupported certificate'),
    44: ('SSLV3_ALERT_CERTIFICATE_REVOKED', 'ssl/tls alert certificate revoked'),
    45: ('SSLV3_ALERT_CERTIFICATE_EXPIRED', 'ssl/tls alert certificate expired'),
    46: ('SSLV3_ALERT_CERTIFICATE_UNKNOWN', 'ssl/tls alert certificate unknown'),
    47: ('SSLV3_ALERT_ILLEGAL_PARAMETER', 'ssl/tls alert illegal parameter'),
    48: ('TLSV1_ALERT_UNKNOWN_CA', 'tlsv1 alert unknown ca'),
    49: ('TLSV1_ALERT_ACCESS_DENIED', 'tlsv1 alert access denied'),
    50: ('TLSV1_ALERT_DECODE_ERROR', 'tlsv1 alert decode error'),
    51: ('TLSV1_ALERT_DECRYPT_ERROR', 'tlsv1 alert decrypt error'),
    60: ('TLSV1_ALERT_EXPORT_RESTRICTION', 'tlsv1 alert export restriction'),
    70: ('TLSV1_ALERT_PROTOCOL_VERSION', 'tlsv1 alert protocol version'),
    71: ('TLSV1_ALERT_INSUFFICIENT_SECURITY', 'tlsv1 alert insufficient security'),
    80: ('TLSV1_ALERT_INTERNAL_ERROR', 'tlsv1 alert internal error'),
    86: ('TLSV1_ALERT_INAPPROPRIATE_FALLBACK', 'tlsv1 alert inappropriate fallback'),
    90: ('TLSV1_ALERT_USER_CANCELLED', 'tlsv1 alert user cancelled'),
    100: ('TLSV1_ALERT_NO_RENEGOTIATION', 'tlsv1 alert no renegotiation'),
    109: ('TLSV13_ALERT_MISSING_EXTENSION', 'tlsv13 alert missing extension'),
    110: ('TLSV1_UNSUPPORTED_EXTENSION', 'tlsv1 unsupported extension'),
    111: ('TLSV1_CERTIFICATE_UNOBTAINABLE', 'tlsv1 certificate unobtainable'),
    112: ('TLSV1_UNRECOGNIZED_NAME', 'tlsv1 unrecognized name'),
    113: ('TLSV1_BAD_CERTIFICATE_STATUS_RESPONSE', 'tlsv1 bad certificate status response'),
    114: ('TLSV1_BAD_CERTIFICATE_HASH_VALUE', 'tlsv1 bad certificate hash value'),
    115: ('TLSV1_ALERT_UNKNOWN_PSK_IDENTITY', 'tlsv1 alert unknown psk identity'),
    116: ('TLSV13_ALERT_CERTIFICATE_REQUIRED', 'tlsv13 alert certificate required'),
    120: ('TLSV1_ALERT_NO_APPLICATION_PROTOCOL', 'tlsv1 alert no application protocol'),
}


# --- MemoryBIO e sessão ---------------------------------------------------------------------------

class MemoryBIO:
    def __new__(cls, *args, **kwargs):
        if args:
            raise TypeError('MemoryBIO() takes no positional arguments')
        if kwargs:
            raise TypeError('MemoryBIO() takes no keyword arguments')
        self = object.__new__(cls)
        self._buf = bytearray()
        self._eof_written = False
        return self

    @property
    def pending(self):
        """The number of bytes pending in the memory BIO."""
        return len(self._buf)

    @property
    def eof(self):
        """Whether the memory BIO is at EOF."""
        return self._eof_written and not self._buf

    def read(self, size=-1, /):
        """Read up to size bytes from the memory BIO.

If size is not specified, read the entire buffer.
If the return value is an empty bytes instance, this means either
EOF or that no data is available. Use the "eof" property to
distinguish between the two."""
        if size < 0 or size > len(self._buf):
            size = len(self._buf)
        data = bytes(self._buf[:size])
        del self._buf[:size]
        return data

    def write(self, b, /):
        """Writes the bytes b into the memory BIO.

Returns the number of bytes written."""
        data = memoryview(b).tobytes()
        if self._eof_written:
            raise SSLError('cannot write() after write_eof()')
        self._buf += data
        return len(data)

    def write_eof(self):
        """Write an EOF marker to the memory BIO.

When all data has been read, the "eof" property will be True."""
        self._eof_written = True


MemoryBIO.__module__ = '_ssl'


class SSLSession:
    def __new__(cls, *args, **kwargs):
        raise TypeError("cannot create '_ssl.SSLSession' instances")


SSLSession.__module__ = '_ssl'


# --- funções do módulo ----------------------------------------------------------------------------

def RAND_add(string, entropy, /):
    """Mix string into the OpenSSL PRNG state."""
    if not isinstance(string, (str, bytes, bytearray, memoryview)):
        raise TypeError("a bytes-like object is required, not '%s'" % type(string).__name__)
    float(entropy)


def RAND_bytes(n, /):
    """Generate n cryptographically strong pseudo-random bytes."""
    n = _index(n)
    if n < 0:
        raise ValueError('num must be positive')
    return _os.urandom(n)


def RAND_status():
    """Returns True if the OpenSSL PRNG has been seeded with enough data and False if not."""
    return True


def get_default_verify_paths():
    """Return search paths and environment vars that are used by SSLContext's set_default_verify_paths() to load default CAs.

The values are 'cert_file_env', 'cert_file', 'cert_dir_env', 'cert_dir'."""
    return ('SSL_CERT_FILE', _OPENSSLDIR + '/cert.pem', 'SSL_CERT_DIR', _OPENSSLDIR + '/certs')
