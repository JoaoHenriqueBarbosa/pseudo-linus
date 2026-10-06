"""zipfile: leitura e escrita de arquivos ZIP (deflate e armazenado), sem ZIP64 nem criptografia."""

import io
import os
import stat
import struct
import time
import zlib
from binascii import crc32

__all__ = ['BadZipFile', 'BadZipfile', 'error', 'ZIP_STORED', 'ZIP_DEFLATED', 'is_zipfile', 'ZipInfo',
           'ZipFile', 'LargeZipFile', 'Path']


class BadZipFile(Exception):
    pass


class LargeZipFile(Exception):
    pass


error = BadZipfile = BadZipFile

ZIP64_LIMIT = (1 << 31) - 1
ZIP_FILECOUNT_LIMIT = (1 << 16) - 1
ZIP_STORED = 0
ZIP_DEFLATED = 8
ZIP_BZIP2 = 12
ZIP_LZMA = 14
DEFAULT_VERSION = 20
ZIP64_VERSION = 45

_FH = '<4s2B4HL2L2H'
_FH_SIZE = struct.calcsize(_FH)
_CD = '<4s4B4HL2L5H2L'
_CD_SIZE = struct.calcsize(_CD)
_EOCD = '<4s4H2LH'
_EOCD_SIZE = struct.calcsize(_EOCD)
_SIG_FH = b'PK\003\004'
_SIG_CD = b'PK\001\002'
_SIG_EOCD = b'PK\005\006'
_SIG_EOCD64 = b'PK\006\006'
_SIG_EOCD64_LOC = b'PK\006\007'

_MASK_UTF_FILENAME = 1 << 11


def _extract_system():
    return 3


class ZipInfo:
    __slots__ = ('orig_filename', 'filename', 'date_time', 'compress_type', 'comment', 'extra',
                 'create_system', 'create_version', 'extract_version', 'reserved', 'flag_bits',
                 'volume', 'internal_attr', 'external_attr', 'header_offset', 'CRC', 'compress_size',
                 'file_size', '_compresslevel')

    def __init__(self, filename='NoName', date_time=(1980, 1, 1, 0, 0, 0)):
        self.orig_filename = filename
        null = filename.find(chr(0))
        if null >= 0:
            filename = filename[:null]
        if os.sep != '/' and os.sep in filename:
            filename = filename.replace(os.sep, '/')
        self.filename = filename
        self.date_time = date_time
        if date_time[0] < 1980:
            raise ValueError('ZIP does not support timestamps before 1980')
        self.compress_type = ZIP_STORED
        self.comment = b''
        self.extra = b''
        self.create_system = 3
        self.create_version = DEFAULT_VERSION
        self.extract_version = DEFAULT_VERSION
        self.reserved = 0
        self.flag_bits = 0
        self.volume = 0
        self.internal_attr = 0
        self.external_attr = 0
        self.header_offset = 0
        self.CRC = 0
        self.compress_size = 0
        self.file_size = 0
        self._compresslevel = None

    def __repr__(self):
        result = ['<%s filename=%r' % (self.__class__.__name__, self.filename)]
        if self.compress_type != ZIP_STORED:
            result.append(' compress_type=%s' % ({8: 'deflate', 12: 'bzip2', 14: 'lzma'}.get(
                self.compress_type, self.compress_type)))
        hi = self.external_attr >> 16
        lo = self.external_attr & 0xFFFF
        if hi:
            result.append(' filemode=%r' % stat.filemode(hi))
        if lo:
            result.append(' external_attr=%#x' % lo)
        isdir = self.is_dir()
        if not isdir or self.file_size:
            result.append(' file_size=%r' % self.file_size)
        if (not isdir or self.compress_size) and (self.compress_type != ZIP_STORED or
                                                    self.file_size != self.compress_size):
            result.append(' compress_size=%r' % self.compress_size)
        result.append('>')
        return ''.join(result)

    def is_dir(self):
        return self.filename[-1:] == '/'

    def _encodeFilenameFlags(self):
        try:
            return self.filename.encode('ascii'), self.flag_bits
        except UnicodeEncodeError:
            return self.filename.encode('utf-8'), self.flag_bits | _MASK_UTF_FILENAME

    def FileHeader(self):
        dt = self.date_time
        dosdate = (dt[0] - 1980) << 9 | dt[1] << 5 | dt[2]
        dostime = dt[3] << 11 | dt[4] << 5 | (dt[5] // 2)
        min_version = DEFAULT_VERSION if self.compress_type == ZIP_DEFLATED else 0
        extract_version = max(min_version, self.extract_version)
        create_version = max(min_version, self.create_version)
        filename, flag_bits = self._encodeFilenameFlags()
        return struct.pack(_FH, _SIG_FH, extract_version, self.reserved, flag_bits, self.compress_type,
                           dostime, dosdate, self.CRC, self.compress_size, self.file_size,
                           len(filename), len(self.extra)) + filename + self.extra

    def _central(self):
        dt = self.date_time
        dosdate = (dt[0] - 1980) << 9 | dt[1] << 5 | dt[2]
        dostime = dt[3] << 11 | dt[4] << 5 | (dt[5] // 2)
        min_version = DEFAULT_VERSION if self.compress_type == ZIP_DEFLATED else 0
        extract_version = max(min_version, self.extract_version)
        create_version = max(min_version, self.create_version)
        filename, flag_bits = self._encodeFilenameFlags()
        return struct.pack(_CD, _SIG_CD, create_version, self.create_system, extract_version,
                           self.reserved, flag_bits, self.compress_type, dostime, dosdate, self.CRC,
                           self.compress_size, self.file_size, len(filename), len(self.extra),
                           len(self.comment), self.volume, self.internal_attr, self.external_attr,
                           self.header_offset) + filename + self.extra + self.comment

    @classmethod
    def from_file(cls, filename, arcname=None, *, strict_timestamps=True):
        if isinstance(filename, os.PathLike):
            filename = os.fspath(filename)
        st = os.stat(filename)
        isdir = stat.S_ISDIR(st.st_mode)
        mtime = time.gmtime(st.st_mtime)
        date_time = mtime[0:6]
        if not strict_timestamps and date_time[0] < 1980:
            date_time = (1980, 1, 1, 0, 0, 0)
        if arcname is None:
            arcname = filename
        arcname = os.path.normpath(os.path.splitdrive(arcname)[1])
        while arcname[0] in (os.sep, os.altsep):
            arcname = arcname[1:]
        if isdir:
            arcname += '/'
        zinfo = cls(arcname, date_time)
        zinfo.external_attr = (st.st_mode & 0xFFFF) << 16
        if isdir:
            zinfo.file_size = 0
            zinfo.external_attr |= 0x10
        else:
            zinfo.file_size = st.st_size
        return zinfo

    def _for_str_or_bytes(self):
        pass


def _check_zipfile(fp):
    try:
        if _EndRecData(fp):
            return True
    except OSError:
        pass
    return False


def is_zipfile(filename):
    result = False
    try:
        if hasattr(filename, 'read'):
            result = _check_zipfile(filename)
        else:
            with open(filename, 'rb') as fp:
                result = _check_zipfile(fp)
    except OSError:
        pass
    return result


def _EndRecData(fpin):
    """Devolve (entradas, tamanho do diretório, offset do diretório, comentário) ou None."""
    fpin.seek(0, 2)
    filesize = fpin.tell()
    try:
        fpin.seek(-_EOCD_SIZE, 2)
    except OSError:
        return None
    data = fpin.read(_EOCD_SIZE)
    if len(data) == _EOCD_SIZE and data[0:4] == _SIG_EOCD and data[-2:] == b'\000\000':
        rec = struct.unpack(_EOCD, data)
        return rec[4], rec[5], rec[6], b''
    maxcomment = (1 << 16) - 1
    start = max(filesize - maxcomment - _EOCD_SIZE, 0)
    fpin.seek(start, 0)
    data = fpin.read()
    k = data.rfind(_SIG_EOCD)
    if k < 0:
        return None
    rec = struct.unpack(_EOCD, data[k:k + _EOCD_SIZE])
    comment = data[k + _EOCD_SIZE:k + _EOCD_SIZE + rec[7]]
    return rec[4], rec[5], rec[6], comment


class ZipExtFile(io.BytesIO):
    """Fluxo de leitura de um membro já descompactado."""

    def __init__(self, data, zipinfo):
        io.BytesIO.__init__(self, data)
        self.name = zipinfo.filename
        self._zinfo = zipinfo

    def peek(self, n=1):
        pos = self.tell()
        data = self.read(n)
        self.seek(pos)
        return data


class _ZipWriteFile(io.BytesIO):
    def __init__(self, zf, zinfo):
        io.BytesIO.__init__(self)
        self._zf = zf
        self._zinfo = zinfo
        self._done = False

    def close(self):
        if not self._done:
            self._done = True
            self._zf._finish_member(self._zinfo, self.getvalue())
            self._zf._writing = False
        io.BytesIO.close(self)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


class ZipFile:
    fp = None
    _windows_illegal_name_trans_table = None

    def __init__(self, file, mode='r', compression=ZIP_STORED, allowZip64=True, compresslevel=None,
                 *, strict_timestamps=True, metadata_encoding=None):
        if mode not in ('r', 'w', 'x', 'a'):
            raise ValueError("ZipFile requires mode 'r', 'w', 'x', or 'a'")
        if compression not in (ZIP_STORED, ZIP_DEFLATED):
            raise NotImplementedError('That compression method is not supported')
        self._allowZip64 = allowZip64
        self._didModify = False
        self.debug = 0
        self.NameToInfo = {}
        self.filelist = []
        self.compression = compression
        self.compresslevel = compresslevel
        self.mode = mode
        self.pwd = None
        self._comment = b''
        self._strict_timestamps = strict_timestamps
        self._writing = False
        self._closed_fp = False
        if isinstance(file, os.PathLike):
            file = os.fspath(file)
        if isinstance(file, str):
            self._filePassed = 0
            self.filename = file
            modeDict = {'r': 'rb', 'w': 'w+b', 'x': 'x+b', 'a': 'r+b', 'r+b': 'w+b', 'w+b': 'wb', 'x+b': 'xb'}
            filemode = modeDict[mode]
            while True:
                try:
                    self.fp = io.open(file, filemode)
                except OSError:
                    if filemode in modeDict:
                        filemode = modeDict[filemode]
                        continue
                    raise
                break
        else:
            self._filePassed = 1
            self.fp = file
            self.filename = getattr(file, 'name', None)
        try:
            if mode == 'r':
                self._RealGetContents()
            elif mode in ('w', 'x'):
                self._didModify = True
                try:
                    self.start_dir = self.fp.tell()
                except (AttributeError, OSError):
                    self.start_dir = 0
            elif mode == 'a':
                try:
                    self._RealGetContents()
                    self.fp.seek(self.start_dir)
                except BadZipFile:
                    self.fp.seek(0, 2)
                    self._didModify = True
                    self.start_dir = self.fp.tell()
            else:
                raise ValueError("Mode must be 'r', 'w', 'x', or 'a'")
        except BaseException:
            fp = self.fp
            self.fp = None
            if not self._filePassed:
                fp.close()
            raise

    def __enter__(self):
        return self

    def __exit__(self, type, value, traceback):
        self.close()

    def __repr__(self):
        result = ['<%s.%s' % (self.__class__.__module__, self.__class__.__qualname__)]
        if self.fp is not None:
            if self._filePassed:
                result.append(' file=%r' % self.fp)
            elif self.filename is not None:
                result.append(' filename=%r' % self.filename)
            result.append(' mode=%r' % self.mode)
        else:
            result.append(' [closed]')
        result.append('>')
        return ''.join(result)

    def _RealGetContents(self):
        fp = self.fp
        try:
            endrec = _EndRecData(fp)
        except OSError:
            raise BadZipFile('File is not a zip file')
        if not endrec:
            raise BadZipFile('File is not a zip file')
        total, size_cd, offset_cd, comment = endrec
        self._comment = comment
        concat = 0
        fp.seek(0, 2)
        filesize = fp.tell()
        # O arquivo pode ter dados na frente (autoextraível): o offset real do diretório central
        # é onde o fim do arquivo aponta menos o tamanho do diretório.
        eocd_pos = filesize - _EOCD_SIZE - len(comment)
        concat = eocd_pos - size_cd - offset_cd
        if concat < 0:
            concat = 0
        self.start_dir = offset_cd + concat
        fp.seek(self.start_dir, 0)
        data = fp.read(size_cd)
        pos = 0
        for _ in range(total):
            centdir = data[pos:pos + _CD_SIZE]
            if len(centdir) != _CD_SIZE:
                raise BadZipFile('Truncated central directory')
            centdir = struct.unpack(_CD, centdir)
            if centdir[0] != _SIG_CD:
                raise BadZipFile('Bad magic number for central directory')
            filename = data[pos + _CD_SIZE:pos + _CD_SIZE + centdir[12]]
            flags = centdir[5]
            if flags & _MASK_UTF_FILENAME:
                filename = filename.decode('utf-8')
            else:
                filename = filename.decode('cp437')
            x = ZipInfo(filename)
            x.extra = data[pos + _CD_SIZE + centdir[12]:pos + _CD_SIZE + centdir[12] + centdir[13]]
            x.comment = data[pos + _CD_SIZE + centdir[12] + centdir[13]:
                             pos + _CD_SIZE + centdir[12] + centdir[13] + centdir[14]]
            x.create_version = centdir[1]
            x.create_system = centdir[2]
            x.extract_version = centdir[3]
            x.reserved = centdir[4]
            x.flag_bits = flags
            x.compress_type = centdir[6]
            t, d = centdir[7], centdir[8]
            x.volume, x.internal_attr, x.external_attr = centdir[15:18]
            x.header_offset = centdir[18]
            x.CRC = centdir[9]
            x.compress_size = centdir[10]
            x.file_size = centdir[11]
            x.date_time = ((d >> 9) + 1980, (d >> 5) & 0xF, d & 0x1F, t >> 11, (t >> 5) & 0x3F, (t & 0x1F) * 2)
            x.header_offset = x.header_offset + concat
            self.filelist.append(x)
            self.NameToInfo[x.filename] = x
            pos += _CD_SIZE + centdir[12] + centdir[13] + centdir[14]

    def namelist(self):
        return [data.filename for data in self.filelist]

    def infolist(self):
        return self.filelist

    def printdir(self, file=None):
        import sys
        print('%-46s %19s %12s' % ('File Name', 'Modified    ', 'Size'), file=file or sys.stdout)
        for zinfo in self.filelist:
            date = '%d-%02d-%02d %02d:%02d:%02d' % zinfo.date_time[:6]
            print('%-46s %s %12d' % (zinfo.filename, date, zinfo.file_size), file=file or sys.stdout)

    def testzip(self):
        for zinfo in self.filelist:
            try:
                self.read(zinfo.filename)
            except BadZipFile:
                return zinfo.filename
        return None

    def getinfo(self, name):
        info = self.NameToInfo.get(name)
        if info is None:
            raise KeyError('There is no item named %r in the archive' % name)
        return info

    def setpassword(self, pwd):
        if pwd and not isinstance(pwd, bytes):
            raise TypeError('pwd: expected bytes, got %s' % type(pwd).__name__)
        self.pwd = pwd or None

    @property
    def comment(self):
        return self._comment

    @comment.setter
    def comment(self, comment):
        if not isinstance(comment, bytes):
            raise TypeError('comment: expected bytes, got %s' % type(comment).__name__)
        if len(comment) > (1 << 16) - 1:
            comment = comment[:(1 << 16) - 1]
        self._comment = comment
        self._didModify = True

    def read(self, name, pwd=None):
        with self.open(name, 'r', pwd) as fp:
            return fp.read()

    def _read_member(self, zinfo):
        if self.fp is None:
            raise ValueError('Attempt to use ZIP archive that was already closed')
        if zinfo.flag_bits & 0x1:
            raise NotImplementedError('encrypted ZIP members are not supported')
        fp = self.fp
        fp.seek(zinfo.header_offset)
        fheader = fp.read(_FH_SIZE)
        if len(fheader) != _FH_SIZE:
            raise BadZipFile('Truncated file header')
        fheader = struct.unpack(_FH, fheader)
        if fheader[0] != _SIG_FH:
            raise BadZipFile('Bad magic number for file header')
        fp.read(fheader[10])
        if fheader[11]:
            fp.read(fheader[11])
        raw = fp.read(zinfo.compress_size)
        if zinfo.compress_type == ZIP_STORED:
            data = raw
        elif zinfo.compress_type == ZIP_DEFLATED:
            try:
                data = zlib.decompress(raw, -15)
            except zlib.error as e:
                raise BadZipFile('Error -3 while decompressing data: %s' % e) from None
        else:
            raise NotImplementedError('compression type %d is not supported' % zinfo.compress_type)
        if crc32(data) & 0xffffffff != zinfo.CRC:
            raise BadZipFile('Bad CRC-32 for file %r' % zinfo.orig_filename)
        return data

    def open(self, name, mode='r', pwd=None, *, force_zip64=False):
        if mode not in {'r', 'w'}:
            raise ValueError('open() requires mode "r" or "w"')
        if pwd and mode == 'w':
            raise ValueError('pwd is only supported for reading files')
        if not self.fp:
            raise ValueError('Attempt to use ZIP archive that was already closed')
        if isinstance(name, ZipInfo):
            zinfo = name
        elif mode == 'w':
            zinfo = ZipInfo(name)
            zinfo.compress_type = self.compression
            zinfo._compresslevel = self.compresslevel
        else:
            zinfo = self.getinfo(name)
        if mode == 'w':
            return self._open_to_write(zinfo, force_zip64=force_zip64)
        return ZipExtFile(self._read_member(zinfo), zinfo)

    def _open_to_write(self, zinfo, force_zip64=False):
        if self._writing:
            raise ValueError("Can't write to the ZIP file while there is another write handle open on it. "
                             "Close the first handle before opening another.")
        zinfo.compress_size = 0
        zinfo.CRC = 0
        zinfo.flag_bits = 0x00
        zinfo.external_attr = zinfo.external_attr or (0o600 << 16)
        self._writing = True
        return _ZipWriteFile(self, zinfo)

    def _finish_member(self, zinfo, data):
        """Comprime e grava o membro no ponto de escrita atual (antes do diretório central)."""
        zinfo.file_size = len(data)
        zinfo.CRC = crc32(data) & 0xffffffff
        if zinfo.compress_type == ZIP_DEFLATED:
            level = zinfo._compresslevel
            co = zlib.compressobj(-1 if level is None else level, zlib.DEFLATED, -15)
            payload = co.compress(data) + co.flush()
        else:
            payload = data
        zinfo.compress_size = len(payload)
        if zinfo.file_size > ZIP64_LIMIT or zinfo.compress_size > ZIP64_LIMIT:
            raise LargeZipFile('File size would require ZIP64 extensions')
        if self.fp is None:
            raise ValueError('Attempt to write to ZIP archive that was already closed')
        self.fp.seek(self.start_dir)
        zinfo.header_offset = self.fp.tell()
        self._didModify = True
        self.fp.write(zinfo.FileHeader())
        self.fp.write(payload)
        self.start_dir = self.fp.tell()
        self.filelist.append(zinfo)
        self.NameToInfo[zinfo.filename] = zinfo

    def extract(self, member, path=None, pwd=None):
        if not isinstance(member, ZipInfo):
            member = self.getinfo(member)
        if path is None:
            path = os.getcwd()
        else:
            path = os.fspath(path)
        return self._extract_member(member, path, pwd)

    def extractall(self, path=None, members=None, pwd=None):
        if members is None:
            members = self.namelist()
        if path is None:
            path = os.getcwd()
        else:
            path = os.fspath(path)
        for zipinfo in members:
            self._extract_member(zipinfo, path, pwd)

    @classmethod
    def _sanitize_windows_name(cls, arcname, pathsep):
        return arcname

    def _extract_member(self, member, targetpath, pwd):
        if not isinstance(member, ZipInfo):
            member = self.getinfo(member)
        arcname = member.filename.replace('/', os.path.sep)
        arcname = os.path.splitdrive(arcname)[1]
        invalid_path_parts = ('', os.path.curdir, os.path.pardir)
        arcname = os.path.sep.join(x for x in arcname.split(os.path.sep) if x not in invalid_path_parts)
        targetpath = os.path.join(targetpath, arcname)
        targetpath = os.path.normpath(targetpath)
        upperdirs = os.path.dirname(targetpath)
        if upperdirs and not os.path.exists(upperdirs):
            os.makedirs(upperdirs, exist_ok=True)
        if member.is_dir():
            if not os.path.isdir(targetpath):
                os.mkdir(targetpath)
            return targetpath
        with self.open(member, pwd=pwd) as source, open(targetpath, 'wb') as target:
            target.write(source.read())
        mode = member.external_attr >> 16
        if mode & 0o7777 and member.create_system == 3:
            try:
                os.chmod(targetpath, mode & 0o7777)
            except OSError:
                pass
        return targetpath

    def _writecheck(self, zinfo):
        if zinfo.filename in self.NameToInfo:
            import warnings
            warnings.warn('Duplicate name: %r' % zinfo.filename, stacklevel=3)
        if self.mode not in ('w', 'x', 'a'):
            raise ValueError("write() requires mode 'w', 'x', or 'a'")
        if not self.fp:
            raise ValueError('Attempt to write ZIP archive that was already closed')

    def write(self, filename, arcname=None, compress_type=None, compresslevel=None):
        if not self.fp:
            raise ValueError('Attempt to write to ZIP archive that was already closed')
        if self._writing:
            raise ValueError("Can't write to ZIP archive while an open writing handle exists")
        zinfo = ZipInfo.from_file(filename, arcname, strict_timestamps=self._strict_timestamps)
        if zinfo.is_dir():
            zinfo.compress_size = 0
            zinfo.CRC = 0
            zinfo.compress_type = ZIP_STORED
            self.mkdir(zinfo)
            return
        zinfo.compress_type = self.compression if compress_type is None else compress_type
        zinfo._compresslevel = self.compresslevel if compresslevel is None else compresslevel
        self._writecheck(zinfo)
        with open(filename, 'rb') as src:
            data = src.read()
        self._finish_member(zinfo, data)

    def writestr(self, zinfo_or_arcname, data, compress_type=None, compresslevel=None):
        if isinstance(data, str):
            data = data.encode('utf-8')
        if not isinstance(zinfo_or_arcname, ZipInfo):
            zinfo = ZipInfo(filename=zinfo_or_arcname, date_time=time.localtime(time.time())[:6])
            zinfo.compress_type = self.compression
            zinfo._compresslevel = self.compresslevel
            if zinfo.filename[-1] == '/':
                zinfo.external_attr = 0o40775 << 16
                zinfo.external_attr |= 0x10
            else:
                zinfo.external_attr = 0o600 << 16
        else:
            zinfo = zinfo_or_arcname
        if not self.fp:
            raise ValueError('Attempt to write to ZIP archive that was already closed')
        if self._writing:
            raise ValueError("Can't write to ZIP archive while an open writing handle exists.")
        if compress_type is not None:
            zinfo.compress_type = compress_type
        if compresslevel is not None:
            zinfo._compresslevel = compresslevel
        self._writecheck(zinfo)
        self._finish_member(zinfo, bytes(data))

    def mkdir(self, zinfo_or_directory_name, mode=511):
        if isinstance(zinfo_or_directory_name, ZipInfo):
            zinfo = zinfo_or_directory_name
            if not zinfo.is_dir():
                raise ValueError('The given ZipInfo does not describe a directory')
        elif isinstance(zinfo_or_directory_name, str):
            directory_name = zinfo_or_directory_name
            if not directory_name.endswith('/'):
                directory_name += '/'
            zinfo = ZipInfo(directory_name)
            zinfo.compress_size = 0
            zinfo.CRC = 0
            zinfo.external_attr = ((0o40000 | mode) & 0xFFFF) << 16
            zinfo.file_size = 0
            zinfo.external_attr |= 0x10
        else:
            raise TypeError('Expected type str or ZipInfo')
        self._finish_member(zinfo, b'')

    def __del__(self):
        self.close()

    def close(self):
        if self.fp is None:
            return
        if self._writing:
            raise ValueError("Can't close the ZIP file while there is an open writing handle on it. "
                             "Close the writing handle before closing the zip.")
        try:
            if self.mode in ('w', 'x', 'a') and self._didModify:
                self.fp.seek(self.start_dir)
                cd_start = self.fp.tell()
                chunks = [zinfo._central() for zinfo in self.filelist]
                cd = b''.join(chunks)
                self.fp.write(cd)
                end = struct.pack(_EOCD, _SIG_EOCD, 0, 0, len(self.filelist), len(self.filelist),
                                  len(cd), cd_start, len(self._comment))
                self.fp.write(end + self._comment)
                if hasattr(self.fp, 'truncate'):
                    self.fp.truncate()
                self.fp.flush()
        finally:
            fp = self.fp
            self.fp = None
            if not self._filePassed:
                fp.close()


class Path:
    def __init__(self, root, at=''):
        self.root = root if isinstance(root, ZipFile) else ZipFile(root)
        self.at = at

    @property
    def name(self):
        return self.at.rstrip('/').rsplit('/', 1)[-1]

    def is_dir(self):
        return not self.at or self.at.endswith('/')

    def is_file(self):
        return self.exists() and not self.is_dir()

    def exists(self):
        return self.at in self.root.namelist() or any(n.startswith(self.at) for n in self.root.namelist())

    def read_text(self, encoding='utf-8'):
        return self.root.read(self.at).decode(encoding)

    def read_bytes(self):
        return self.root.read(self.at)

    def iterdir(self):
        prefix = self.at
        seen = set()
        for n in self.root.namelist():
            if n.startswith(prefix) and n != prefix:
                rest = n[len(prefix):]
                head = rest.split('/', 1)[0]
                child = prefix + head + ('/' if '/' in rest else '')
                if child not in seen:
                    seen.add(child)
                    yield Path(self.root, child)

    def __truediv__(self, other):
        return Path(self.root, self.at + other)

    def __str__(self):
        return self.root.filename + '/' + self.at if self.root.filename else self.at
