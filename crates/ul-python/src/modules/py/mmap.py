"""mmap sem memória compartilhada: o conteúdo mora num bytearray e, nos mapeamentos de arquivo
com escrita (ACCESS_WRITE ou ACCESS_DEFAULT com PROT_WRITE), cada alteração é gravada de volta
no descritor (write-through). Mapeamentos anônimos e ACCESS_COPY só alteram a memória."""

import os

ACCESS_DEFAULT = 0
ACCESS_READ = 1
ACCESS_WRITE = 2
ACCESS_COPY = 3

PROT_READ = 1
PROT_WRITE = 2
PROT_EXEC = 4

MAP_SHARED = 1
MAP_PRIVATE = 2
MAP_DENYWRITE = 2048
MAP_EXECUTABLE = 4096
MAP_ANONYMOUS = 32
MAP_ANON = 32
MAP_POPULATE = 32768
MAP_STACK = 131072
MAP_NORESERVE = 16384
MAP_32BIT = 64

MADV_NORMAL = 0
MADV_RANDOM = 1
MADV_SEQUENTIAL = 2
MADV_WILLNEED = 3
MADV_DONTNEED = 4
MADV_REMOVE = 9
MADV_DONTFORK = 10
MADV_DOFORK = 11
MADV_MERGEABLE = 12
MADV_UNMERGEABLE = 13
MADV_HUGEPAGE = 14
MADV_NOHUGEPAGE = 15
MADV_DONTDUMP = 16
MADV_DODUMP = 17
MADV_FREE = 8
MADV_HWPOISON = 100

PAGESIZE = 4096
ALLOCATIONGRANULARITY = 4096

error = OSError


class mmap:
    def __init__(self, fileno, length, flags=MAP_SHARED, prot=PROT_WRITE | PROT_READ,
                 access=ACCESS_DEFAULT, offset=0, *, trackfd=True):
        if access not in (ACCESS_DEFAULT, ACCESS_READ, ACCESS_WRITE, ACCESS_COPY):
            raise ValueError("mmap invalid access parameter.")
        if access != ACCESS_DEFAULT and (flags != MAP_SHARED or prot != (PROT_WRITE | PROT_READ)):
            raise ValueError("mmap can't specify both access and flags, prot.")
        if access == ACCESS_READ:
            self._writable = False
        elif access == ACCESS_DEFAULT:
            self._writable = bool(prot & PROT_WRITE)
        else:
            self._writable = True
        self._persist = (access == ACCESS_WRITE or
                         (access == ACCESS_DEFAULT and bool(prot & PROT_WRITE) and bool(flags & MAP_SHARED)))
        if offset < 0:
            raise OverflowError("memory mapped offset must be positive")
        if length < 0:
            raise OverflowError("memory mapped length must be positive")
        self._fd = None
        self._offset = offset
        self._pos = 0
        self._closed = False
        if fileno == -1:
            if length == 0:
                raise ValueError("cannot mmap an empty file")
            self._persist = False
            self._data = bytearray(length)
            return
        size = os.fstat(fileno).st_size
        if length == 0:
            if size == 0:
                raise ValueError("cannot mmap an empty file")
            if offset >= size:
                raise ValueError("mmap offset is greater than file size")
            length = size - offset
        elif offset + length > size:
            raise ValueError("mmap length is greater than file size")
        self._fd = fileno
        here = os.lseek(fileno, 0, 1)
        os.lseek(fileno, offset, 0)
        data = bytearray()
        while len(data) < length:
            chunk = os.read(fileno, length - len(data))
            if not chunk:
                break
            data += chunk
        os.lseek(fileno, here, 0)
        self._data = data

    def _check(self):
        if self._closed:
            raise ValueError("mmap closed or invalid")

    def _check_write(self):
        self._check()
        if not self._writable:
            raise TypeError("mmap can't modify a read-only memory map.")

    def _store(self, start, chunk):
        if self._persist and self._fd is not None:
            here = os.lseek(self._fd, 0, 1)
            os.lseek(self._fd, self._offset + start, 0)
            os.write(self._fd, bytes(chunk))
            os.lseek(self._fd, here, 0)

    def close(self):
        self._closed = True

    @property
    def closed(self):
        return self._closed

    def __enter__(self):
        self._check()
        return self

    def __exit__(self, *exc):
        self.close()

    def __len__(self):
        self._check()
        return len(self._data)

    def __buffer__(self, flags):
        # A visão olha a memória do mapeamento; no ACCESS_READ ela é somente leitura. Escrita por
        # ela não passa pelo write-through do _store.
        self._check()
        return memoryview(self._data if self._writable else bytes(self._data))

    def size(self):
        self._check()
        if self._fd is None:
            raise OSError(9, 'Bad file descriptor')
        return os.fstat(self._fd).st_size

    def tell(self):
        self._check()
        return self._pos

    def seekable(self):
        return True

    def seek(self, pos, whence=0):
        self._check()
        n = len(self._data)
        if whence == 0:
            new = pos
        elif whence == 1:
            new = self._pos + pos
        elif whence == 2:
            new = n + pos
        else:
            raise ValueError("unknown seek type")
        if not 0 <= new <= n:
            raise ValueError("seek out of range")
        self._pos = new
        return None

    def read(self, n=None):
        self._check()
        rest = len(self._data) - self._pos
        if n is None or n < 0 or n > rest:
            n = rest
        out = bytes(self._data[self._pos:self._pos + n])
        self._pos += n
        return out

    def read_byte(self):
        self._check()
        if self._pos >= len(self._data):
            raise ValueError("read byte out of range")
        b = self._data[self._pos]
        self._pos += 1
        return b

    def readline(self):
        self._check()
        i = self._data.find(b"\n", self._pos)
        end = len(self._data) if i < 0 else i + 1
        out = bytes(self._data[self._pos:end])
        self._pos = end
        return out

    def write(self, data):
        self._check_write()
        data = bytes(data)
        if self._pos + len(data) > len(self._data):
            raise ValueError("data out of range")
        self._data[self._pos:self._pos + len(data)] = data
        self._store(self._pos, data)
        self._pos += len(data)
        return len(data)

    def write_byte(self, byte):
        self._check_write()
        if not isinstance(byte, int):
            raise TypeError("write_byte() argument must be int, not " + type(byte).__name__)
        if self._pos >= len(self._data):
            raise ValueError("write byte out of range")
        self._data[self._pos] = byte
        self._store(self._pos, bytes([byte]))
        self._pos += 1

    def _bounds(self, start, end):
        n = len(self._data)
        if start is None:
            start = 0
        if end is None:
            end = n
        if start < 0:
            start = max(start + n, 0)
        if end < 0:
            end = max(end + n, 0)
        return min(start, n), min(end, n)

    def find(self, sub, start=None, end=None):
        self._check()
        if isinstance(sub, int):
            sub = bytes([sub])
        start, end = self._bounds(start, end)
        return self._data.find(bytes(sub), start, end)

    def rfind(self, sub, start=None, end=None):
        self._check()
        if isinstance(sub, int):
            sub = bytes([sub])
        start, end = self._bounds(start, end)
        return self._data.rfind(bytes(sub), start, end)

    def flush(self, offset=0, size=None):
        self._check()
        return None

    def madvise(self, option, start=0, length=None):
        self._check()
        return None

    def move(self, dest, src, count):
        self._check_write()
        n = len(self._data)
        if (min(src, dest) < 0 or count < 0 or src + count > n or dest + count > n):
            raise ValueError("source, destination, or count out of range")
        chunk = bytes(self._data[src:src + count])
        self._data[dest:dest + count] = chunk
        self._store(dest, chunk)

    def resize(self, newsize):
        self._check()
        if self._fd is None or not self._persist:
            raise SystemError("mmap: resizing not available--no mremap()")
        os.ftruncate(self._fd, self._offset + newsize)
        if newsize < len(self._data):
            del self._data[newsize:]
        else:
            self._data.extend(bytes(newsize - len(self._data)))
        self._pos = min(self._pos, newsize)

    def __getitem__(self, key):
        self._check()
        if isinstance(key, slice):
            return bytes(self._data[key])
        if not isinstance(key, int):
            raise TypeError("mmap indices must be integers")
        n = len(self._data)
        if key < 0:
            key += n
        if not 0 <= key < n:
            raise IndexError("mmap index out of range")
        return self._data[key]

    def __setitem__(self, key, value):
        self._check_write()
        n = len(self._data)
        if isinstance(key, slice):
            start, stop, step = key.indices(n)
            idx = range(start, stop, step)
            value = bytes(value)
            if len(value) != len(idx):
                raise IndexError("mmap slice assignment is wrong size")
            self._data[key] = value
            if step == 1:
                self._store(start, value)
            else:
                for i, b in zip(idx, value):
                    self._store(i, bytes([b]))
            return
        if not isinstance(key, int):
            raise TypeError("mmap indices must be integers")
        if key < 0:
            key += n
        if not 0 <= key < n:
            raise IndexError("mmap index out of range")
        if not isinstance(value, int):
            raise TypeError("mmap assignment must be integer")
        if not 0 <= value < 256:
            raise ValueError("mmap item value must be in range(0, 256)")
        self._data[key] = value
        self._store(key, bytes([value]))

    def __delitem__(self, key):
        raise TypeError("mmap object doesn't support item deletion")

    def __iter__(self):
        self._check()
        return iter(bytes(self._data))

    def __contains__(self, item):
        self._check()
        if isinstance(item, int):
            return item in self._data
        return bytes(item) in self._data

    def __repr__(self):
        if self._closed:
            return "<mmap.mmap closed=True>"
        return ("<mmap.mmap closed=False, access=%s, length=%d, pos=%d, offset=%d>"
                % ("ACCESS_WRITE" if self._writable else "ACCESS_READ", len(self._data), self._pos, self._offset))
