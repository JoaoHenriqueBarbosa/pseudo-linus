"""_ssl: a parte em C do módulo `ssl` do CPython 3.13 (OpenSSL 3.5.7 do Debian 13).

O `ssl.py` do CPython roda por cima deste módulo. Contextos, certificados, MemoryBIO e exceções
seguem o `_ssl.c`; o handshake manda pela conexão TCP um ClientHello com a forma do OpenSSL 3.5
(TLS 1.3, key share X25519MLKEM768 + X25519) e trata a resposta do par como ele: um par que não fala
TLS recebe `WRONG_VERSION_NUMBER`, um par que fecha recebe `UNEXPECTED_EOF_WHILE_READING`. A troca
de chaves e a cifragem do TLS ainda não existem aqui: um par que responde TLS de verdade recebe a
recusa de quem não negociou cifra.
"""

import os as _os
import _socket

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

_CIPHER_SUITES = bytes.fromhex(
    '130213031301c02cc030009fcca9cca8ccaac02bc02f009ec024c028006bc023c0270067c00ac0140039c009c013'
    '0033009d009c003d003c0035002f')
_SIGALGS = bytes.fromhex(
    '003409050906090404030503060308070808081a081b081c0809080a080b080408050806040105010601030303010302040205020602')


def _ext(kind, body):
    return kind.to_bytes(2, 'big') + len(body).to_bytes(2, 'big') + body


def _mlkem768_public():
    """Chave pública ML-KEM-768 bem formada: 768 coeficientes de 12 bits menores que q, mais rho."""
    out = bytearray()
    raw = _os.urandom(768 * 2)
    coeffs = [int.from_bytes(raw[i:i + 2], 'little') % 3329 for i in range(0, len(raw), 2)]
    for i in range(0, 768, 2):
        a, b = coeffs[i], coeffs[i + 1]
        out += bytes((a & 0xff, (a >> 8) | ((b & 0xf) << 4), b >> 4))
    return bytes(out) + _os.urandom(32)


def _is_ip(host):
    if ':' in host:
        return True
    parts = host.split('.')
    return len(parts) == 4 and all(p.isdigit() and int(p) < 256 for p in parts)


def _client_hello(server_hostname, alpn, options):
    x25519 = bytearray(_os.urandom(32))
    x25519[31] &= 0x7f
    shares =b'\x11\xec\x04\xc0' + _mlkem768_public() + bytes(x25519) + b'\x00\x1d\x00\x20' + _os.urandom(32)
    extensions = _ext(0xff01, b'\x00')
    if server_hostname and not _is_ip(server_hostname):
        host = server_hostname.encode('ascii')
        entry = b'\x00' + len(host).to_bytes(2, 'big') + host
        extensions += _ext(0, len(entry).to_bytes(2, 'big') + entry)
    extensions += _ext(11, b'\x03\x00\x01\x02')
    extensions += _ext(10, bytes.fromhex('001011ec001d0017001e0018001901000101'))
    if not options & OP_NO_TICKET:
        extensions += _ext(35, b'')
    if alpn:
        extensions += _ext(16, len(alpn).to_bytes(2, 'big') + alpn)
    extensions += _ext(22, b'') + _ext(23, b'') + _ext(13, _SIGALGS)
    extensions += _ext(43, b'\x04\x03\x04\x03\x03') + _ext(45, b'\x01\x01')
    extensions += _ext(51, len(shares).to_bytes(2, 'big') + shares)
    extensions += _ext(27, b'\x04\x00\x01\x00\x03')
    body = (b'\x03\x03' + _os.urandom(32) + b'\x20' + _os.urandom(32)
            + len(_CIPHER_SUITES).to_bytes(2, 'big') + _CIPHER_SUITES + b'\x01\x00'
            + len(extensions).to_bytes(2, 'big') + extensions)
    handshake = b'\x01' + len(body).to_bytes(3, 'big') + body
    return b'\x16\x03\x01' + len(handshake).to_bytes(2, 'big') + handshake


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

    def set_default_verify_paths(self):
        cafile = _os.environ.get('SSL_CERT_FILE', _OPENSSLDIR + '/cert.pem')
        try:
            self._add_certificates(_pem_certificates(self._load_pem_file(cafile)))
        except OSError:
            pass

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
        if not self._sent_hello:
            alpn = self._context._alpn
            self._send(_client_hello(self.server_hostname, alpn, self._context._options))
            self._sent_hello = True
        if self._sock is None and not self._incoming._eof_written:
            # Pelo MemoryBIO, o registro (e o alerta inteiro) precisa estar lá antes de ser consumido.
            buf = self._incoming._buf
            if len(buf) < 5 or (buf[0] == 0x15 and len(buf) < 7):
                raise _want_read()
        head = self._recv_exact(5)
        if len(head) < 5:
            self._fail(_ssl_error(SSLEOFError, 8, 'SSL', 'UNEXPECTED_EOF_WHILE_READING',
                                  'EOF occurred in violation of protocol', 1029))
        if head[0] not in (0x14, 0x15, 0x16, 0x17) or head[1] != 3:
            self._fail(_ssl_error(SSLError, 1, 'SSL', 'WRONG_VERSION_NUMBER', 'wrong version number', 1029))
        if head[0] == 0x15:
            alert = self._recv_exact(2)
            if len(alert) == 2:
                known = _ALERTS.get(alert[1])
                if known is None:
                    self._fail(_ssl_error(SSLError, 1, None, 'SSL', 'unknown error', 1029))
                self._fail(_ssl_error(SSLError, 1, 'SSL', known[0], known[1], 1029))
        # O par fala TLS: sem a troca de chaves, a conexão termina como uma negociação recusada.
        self._fail(_ssl_error(SSLError, 1, 'SSL', 'SSLV3_ALERT_HANDSHAKE_FAILURE', 'ssl/tls alert handshake failure', 1029))

    def read(self, size=1024, buffer=None):
        self.do_handshake()

    def write(self, data):
        self.do_handshake()

    def pending(self):
        return 0

    def getpeercert(self, binary_form=False):
        raise ValueError('handshake not done yet')

    def get_verified_chain(self):
        return None

    def get_unverified_chain(self):
        return None

    def selected_alpn_protocol(self):
        return None

    def cipher(self):
        return None

    def shared_ciphers(self):
        return None

    def compression(self):
        return None

    def version(self):
        return None

    def shutdown(self):
        if self._sock is not None:
            return self._sock
        return None

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
