"""`memoryview` sobre `bytes` e `bytearray`: uma visão (com fatias e passo) que escreve na base."""


class memoryview:
    __module__ = 'builtins'

    def __init__(self, obj):
        if isinstance(obj, memoryview):
            self._base = obj._base
            self._idx = list(obj._idx)
            self.obj = obj.obj
        elif isinstance(obj, (bytes, bytearray)):
            self._base = obj
            self._idx = list(range(len(obj)))
            self.obj = obj
        elif hasattr(type(obj), '__buffer__'):
            # PEP 688: o objeto entrega a própria visão (mmap, classes de usuário).
            view = type(obj).__buffer__(obj, 0)
            self._base = view._base
            self._idx = list(view._idx)
            self.obj = obj
        else:
            raise TypeError("memoryview: a bytes-like object is required, not '%s'" % type(obj).__name__)
        self._released = False

    def _check(self):
        if self._released:
            raise ValueError('operation forbidden on released memoryview object')

    @property
    def readonly(self):
        self._check()
        return not isinstance(self._base, bytearray)

    @property
    def nbytes(self):
        self._check()
        return len(self._idx)

    itemsize = 1
    format = 'B'
    ndim = 1
    c_contiguous = True
    f_contiguous = True
    contiguous = True

    @property
    def shape(self):
        return (len(self._idx),)

    @property
    def strides(self):
        return (1,)

    def __len__(self):
        self._check()
        return len(self._idx)

    def __getitem__(self, key):
        self._check()
        if isinstance(key, slice):
            view = memoryview(self)
            view._idx = self._idx[key]
            return view
        if isinstance(key, int):
            return self._base[self._idx[key]]
        raise TypeError('memoryview: invalid slice key')

    def __setitem__(self, key, value):
        self._check()
        if self.readonly:
            raise TypeError('cannot modify read-only memory')
        if isinstance(key, slice):
            targets = self._idx[key]
            data = bytes(value)
            if len(data) != len(targets):
                raise ValueError('memoryview assignment: lvalue and rvalue have different structures')
            for i, b in zip(targets, data):
                self._base[i] = b
            return
        if not isinstance(key, int):
            raise TypeError('memoryview: invalid slice key')
        self._base[self._idx[key]] = value

    def __iter__(self):
        self._check()
        return iter([self._base[i] for i in self._idx])

    def tobytes(self, order='C'):
        self._check()
        return bytes([self._base[i] for i in self._idx])

    __bytes__ = tobytes

    def tolist(self):
        self._check()
        return [self._base[i] for i in self._idx]

    def hex(self, *args):
        return self.tobytes().hex(*args)

    def toreadonly(self):
        view = memoryview(bytes(self.tobytes()))
        return view

    def release(self):
        self._released = True

    def cast(self, format, shape=None):
        if format not in ('B', 'b', 'c'):
            raise NotImplementedError("memoryview: cast to format %r is not supported" % format)
        return self

    def __enter__(self):
        self._check()
        return self

    def __exit__(self, *exc):
        self.release()

    def __eq__(self, other):
        if isinstance(other, memoryview):
            return self.tobytes() == other.tobytes()
        if isinstance(other, (bytes, bytearray)):
            return self.tobytes() == bytes(other)
        return NotImplemented

    __hash__ = None

    def __repr__(self):
        if self._released:
            return '<released memory at 0x%x>' % id(self)
        return '<memory at 0x%x>' % id(self)
