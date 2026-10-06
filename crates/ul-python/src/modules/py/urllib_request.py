"""urllib.request enxuto: `file:` e `data:` funcionam; o resto levanta URLError (sem rede no sandbox)."""

import base64
import io
import os
import sys
import urllib.parse
from urllib.error import URLError, HTTPError, ContentTooShortError
from urllib.parse import urlparse, unquote, quote
from urllib.response import addinfourl

__all__ = ['Request', 'urlopen', 'urlretrieve', 'pathname2url', 'url2pathname', 'build_opener',
           'install_opener', 'OpenerDirector', 'URLError', 'HTTPError', 'ContentTooShortError']


def pathname2url(pathname):
    return quote(pathname)


def url2pathname(pathname):
    return unquote(pathname)


class Request:

    def __init__(self, url, data=None, headers={}, origin_req_host=None, unverifiable=False, method=None):
        self.full_url = url
        self.data = data
        self.headers = {}
        for key, value in headers.items():
            self.add_header(key, value)
        self._method = method

    @property
    def full_url(self):
        return self._full_url

    @full_url.setter
    def full_url(self, url):
        self._full_url = url
        parts = urlparse(url)
        self.type = parts.scheme
        self.host = parts.netloc
        self.selector = parts.path + ('?' + parts.query if parts.query else '')

    def add_header(self, key, val):
        self.headers[key.capitalize()] = val

    def has_header(self, name):
        return name.capitalize() in self.headers

    def get_header(self, name, default=None):
        return self.headers.get(name.capitalize(), default)

    def get_method(self):
        if self._method is not None:
            return self._method
        return 'POST' if self.data is not None else 'GET'

    def get_full_url(self):
        return self._full_url


def _open_local(url):
    parts = urlparse(url)
    path = url2pathname(parts.path)
    if parts.netloc not in ('', 'localhost'):
        raise URLError('file:// URL with a remote host is not supported')
    try:
        f = open(path, 'rb')
    except OSError as e:
        raise URLError(e)
    return addinfourl(f, {'content-length': str(os.path.getsize(path))}, url)


def _open_data(url):
    header, _, payload = url[5:].partition(',')
    if header.endswith(';base64'):
        data = base64.b64decode(unquote(payload))
    else:
        data = unquote(payload).encode('latin-1')
    return addinfourl(io.BytesIO(data), {'content-type': header.split(';')[0] or 'text/plain'}, url)


def urlopen(url, data=None, timeout=None, *, context=None):
    full = url.full_url if isinstance(url, Request) else url
    scheme = urlparse(full).scheme
    if scheme == 'file':
        return _open_local(full)
    if scheme == 'data':
        return _open_data(full)
    if scheme in ('http', 'https', 'ftp'):
        raise URLError('network access is not available in this sandbox')
    raise URLError('unknown url type: %s' % (scheme or full))


class OpenerDirector:
    addheaders = []

    def open(self, fullurl, data=None, timeout=None):
        return urlopen(fullurl, data, timeout)


_opener = None


def build_opener(*handlers):
    return OpenerDirector()


def install_opener(opener):
    global _opener
    _opener = opener


def urlretrieve(url, filename=None, reporthook=None, data=None):
    with urlopen(url) as resp:
        body = resp.read()
    if filename is None:
        import tempfile
        fd, filename = tempfile.mkstemp()
        os.close(fd)
    with open(filename, 'wb') as f:
        f.write(body)
    return filename, resp.info()
