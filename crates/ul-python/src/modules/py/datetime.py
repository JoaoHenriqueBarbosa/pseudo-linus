"""datetime: date, time, datetime, timedelta, timezone (UTC como fuso local)."""

import time as _time

MINYEAR = 1
MAXYEAR = 9999

_DIM = (31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31)
_DAYNAMES = ('Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun')
_MONTHNAMES = ('Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec')


def _is_leap(y):
    return y % 4 == 0 and (y % 100 != 0 or y % 400 == 0)


def _dby(y):
    y -= 1
    return y * 365 + y // 4 - y // 100 + y // 400


def _dim(y, m):
    return 29 if m == 2 and _is_leap(y) else _DIM[m - 1]


def _ymd2ord(y, m, d):
    n = _dby(y)
    for i in range(1, m):
        n += _dim(y, i)
    return n + d


def _ord2ymd(n):
    n -= 1
    y = 1 + n // 366
    while _dby(y + 1) <= n:
        y += 1
    while _dby(y) > n:
        y -= 1
    n -= _dby(y)
    m = 1
    while n >= _dim(y, m):
        n -= _dim(y, m)
        m += 1
    return y, m, n + 1


def _check_date(y, m, d):
    if not MINYEAR <= y <= MAXYEAR:
        raise ValueError('year %d is out of range' % y)
    if not 1 <= m <= 12:
        raise ValueError('month must be in 1..12')
    if not 1 <= d <= _dim(y, m):
        raise ValueError('day %d must be in range 1..%d' % (d, _dim(y, m)) if False else 'day is out of range for month')


def _check_time(hh, mm, ss, us):
    if not 0 <= hh <= 23:
        raise ValueError('hour must be in 0..23')
    if not 0 <= mm <= 59:
        raise ValueError('minute must be in 0..59')
    if not 0 <= ss <= 59:
        raise ValueError('second must be in 0..59')
    if not 0 <= us <= 999999:
        raise ValueError('microsecond must be in 0..999999')


def _divround(a, b):
    q, r = divmod(a, b)
    r2 = r * 2
    if r2 > b or (r2 == b and q % 2 == 1):
        q += 1
    return q


class timedelta:
    __slots__ = ('_days', '_seconds', '_microseconds')

    def __new__(cls, days=0, seconds=0, microseconds=0, milliseconds=0, minutes=0, hours=0, weeks=0):
        # Cada componente é reduzido a (dias, segundos, microssegundos) sem passar por um total em
        # microssegundos, que estouraria 64 bits perto de timedelta.max.
        d = 0
        s = 0
        us = 0
        for v, per_day, per_sec, per_us in ((days, 1, 0, 0), (seconds, 0, 1, 0), (microseconds, 0, 0, 1),
                                            (milliseconds, 0, 0, 1000), (minutes, 0, 60, 0),
                                            (hours, 0, 3600, 0), (weeks, 7, 0, 0)):
            if isinstance(v, float):
                v = round(v * (per_day * 86400000000 + per_sec * 1000000 + per_us))
                us += v
            elif isinstance(v, int):
                d += v * per_day
                s += v * per_sec
                us += v * per_us
            else:
                raise TypeError("unsupported type for timedelta days component: %s" % type(v).__name__)
        s += us // 1000000
        us = us % 1000000
        d += s // 86400
        s = s % 86400
        if abs(d) > 999999999:
            raise OverflowError('days=%d; must have magnitude <= 999999999' % d)
        self = object.__new__(cls)
        self._days = d
        self._seconds = s
        self._microseconds = us
        return self

    days = property(lambda self: self._days)
    seconds = property(lambda self: self._seconds)
    microseconds = property(lambda self: self._microseconds)

    def _total_us(self):
        return (self._days * 86400 + self._seconds) * 1000000 + self._microseconds

    def total_seconds(self):
        return self._total_us() / 10**6

    def __repr__(self):
        args = []
        if self._days:
            args.append('days=%d' % self._days)
        if self._seconds:
            args.append('seconds=%d' % self._seconds)
        if self._microseconds:
            args.append('microseconds=%d' % self._microseconds)
        if not args:
            args.append('0')
        return 'datetime.timedelta(%s)' % ', '.join(args)

    def __str__(self):
        mm, ss = divmod(self._seconds, 60)
        hh, mm = divmod(mm, 60)
        s = '%d:%02d:%02d' % (hh, mm, ss)
        if self._days:
            s = ('%d day%s, ' % (self._days, '' if abs(self._days) == 1 else 's')) + s
        if self._microseconds:
            s += '.%06d' % self._microseconds
        return s

    def __add__(self, other):
        if isinstance(other, timedelta):
            return timedelta(microseconds=self._total_us() + other._total_us())
        return NotImplemented

    __radd__ = __add__

    def __sub__(self, other):
        if isinstance(other, timedelta):
            return timedelta(microseconds=self._total_us() - other._total_us())
        return NotImplemented

    def __rsub__(self, other):
        if isinstance(other, timedelta):
            return other - self
        return NotImplemented

    def __neg__(self):
        return timedelta(microseconds=-self._total_us())

    def __pos__(self):
        return self

    def __abs__(self):
        return -self if self._days < 0 else self

    def __mul__(self, other):
        if isinstance(other, int):
            return timedelta(microseconds=self._total_us() * other)
        if isinstance(other, float):
            return timedelta(microseconds=_divround(int(round(self._total_us() * other)), 1))
        return NotImplemented

    __rmul__ = __mul__

    def __truediv__(self, other):
        if isinstance(other, timedelta):
            return self._total_us() / other._total_us()
        if isinstance(other, int):
            return timedelta(microseconds=_divround(self._total_us(), other))
        if isinstance(other, float):
            return timedelta(microseconds=int(round(self._total_us() / other)))
        return NotImplemented

    def __floordiv__(self, other):
        if isinstance(other, timedelta):
            return self._total_us() // other._total_us()
        if isinstance(other, int):
            return timedelta(microseconds=self._total_us() // other)
        return NotImplemented

    def __mod__(self, other):
        if isinstance(other, timedelta):
            return timedelta(microseconds=self._total_us() % other._total_us())
        return NotImplemented

    def __divmod__(self, other):
        if isinstance(other, timedelta):
            q, r = divmod(self._total_us(), other._total_us())
            return q, timedelta(microseconds=r)
        return NotImplemented

    def __eq__(self, other):
        if isinstance(other, timedelta):
            return self._total_us() == other._total_us()
        return NotImplemented

    def __lt__(self, other):
        if isinstance(other, timedelta):
            return self._total_us() < other._total_us()
        return NotImplemented

    def __le__(self, other):
        if isinstance(other, timedelta):
            return self._total_us() <= other._total_us()
        return NotImplemented

    def __gt__(self, other):
        if isinstance(other, timedelta):
            return self._total_us() > other._total_us()
        return NotImplemented

    def __ge__(self, other):
        if isinstance(other, timedelta):
            return self._total_us() >= other._total_us()
        return NotImplemented

    def __hash__(self):
        return hash(self._total_us())

    def __bool__(self):
        return self._total_us() != 0


timedelta.min = timedelta(-999999999)
timedelta.max = timedelta(days=999999999, hours=23, minutes=59, seconds=59, microseconds=999999)
timedelta.resolution = timedelta(microseconds=1)


class tzinfo:
    __slots__ = ()

    def tzname(self, dt):
        raise NotImplementedError

    def utcoffset(self, dt):
        raise NotImplementedError

    def dst(self, dt):
        raise NotImplementedError

    def fromutc(self, dt):
        if not isinstance(dt, datetime):
            raise TypeError('fromutc() requires a datetime argument')
        if dt.tzinfo is not self:
            raise ValueError('dt.tzinfo is not self')
        dtoff = dt.utcoffset()
        if dtoff is None:
            raise ValueError('fromutc() requires a non-None utcoffset() result')
        dtdst = dt.dst()
        if dtdst is None:
            raise ValueError('fromutc() requires a non-None dst() result')
        delta = dtoff - dtdst
        if delta:
            dt += delta
            dtdst = dt.dst()
            if dtdst is None:
                raise ValueError('fromutc(): dt.dst gave inconsistent results; cannot convert')
        return dt + dtdst if dtdst else dt


class timezone(tzinfo):
    __slots__ = ('_offset', '_name')

    def __new__(cls, offset, name=None):
        if not isinstance(offset, timedelta):
            raise TypeError('timezone() argument 1 must be datetime.timedelta, not %s' % type(offset).__name__)
        if not timedelta(hours=-24) < offset < timedelta(hours=24):
            raise ValueError('offset must be a timedelta strictly between -timedelta(hours=24) and timedelta(hours=24).')
        self = object.__new__(cls)
        self._offset = offset
        self._name = name
        return self

    def utcoffset(self, dt):
        return self._offset

    def dst(self, dt):
        return None

    def tzname(self, dt):
        if self._name is not None:
            return self._name
        if not self._offset:
            return 'UTC'
        total = self._offset._total_us() // 1000000
        sign = '-' if total < 0 else '+'
        total = abs(total)
        hh, rem = divmod(total, 3600)
        mm, ss = divmod(rem, 60)
        s = 'UTC%s%02d:%02d' % (sign, hh, mm)
        if ss:
            s += ':%02d' % ss
        return s

    def fromutc(self, dt):
        return dt + self._offset

    def __eq__(self, other):
        if isinstance(other, timezone):
            return self._offset == other._offset
        return NotImplemented

    def __hash__(self):
        return hash(self._offset)

    def __repr__(self):
        if self is timezone.utc:
            return 'datetime.timezone.utc'
        if self._name is None:
            return 'datetime.timezone(%r)' % (self._offset,)
        return 'datetime.timezone(%r, %r)' % (self._offset, self._name)

    def __str__(self):
        return self.tzname(None)


timezone.utc = timezone(timedelta(0))
timezone.min = timezone(timedelta(hours=-23, minutes=-59))
timezone.max = timezone(timedelta(hours=23, minutes=59))
UTC = timezone.utc


def _fmt_offset(off, sep=':'):
    total = off._total_us() // 1000000
    sign = '-' if total < 0 else '+'
    total = abs(total)
    hh, rem = divmod(total, 3600)
    mm, ss = divmod(rem, 60)
    s = '%s%02d%s%02d' % (sign, hh, sep, mm)
    if ss:
        s += '%s%02d' % (sep, ss)
    return s


def _wrap_strftime(obj, fmt, tt):
    out = []
    i = 0
    n = len(fmt)
    while i < n:
        c = fmt[i]
        if c == '%' and i + 1 < n:
            k = fmt[i + 1]
            i += 2
            if k == 'f':
                out.append('%06d' % getattr(obj, 'microsecond', 0))
            elif k == 'z':
                off = obj.utcoffset() if hasattr(obj, 'utcoffset') else None
                out.append('' if off is None else _fmt_offset(off, '').replace(':', ''))
            elif k == 'Z':
                nm = obj.tzname() if hasattr(obj, 'tzname') else None
                out.append('' if nm is None else nm.replace('%', '%%'))
            elif k == '%':
                out.append('%%')
            else:
                out.append('%' + k)
        else:
            out.append(c.replace('%', '%%') if c == '%' else c)
            i += 1
    return _time.strftime(''.join(out), tt)


class date:
    __slots__ = ('_year', '_month', '_day')

    def __new__(cls, year, month=None, day=None):
        _check_date(year, month, day)
        self = object.__new__(cls)
        self._year = year
        self._month = month
        self._day = day
        return self

    year = property(lambda self: self._year)
    month = property(lambda self: self._month)
    day = property(lambda self: self._day)

    @classmethod
    def today(cls):
        return cls.fromtimestamp(_time.time())

    @classmethod
    def fromtimestamp(cls, t):
        tt = _time.gmtime(t)
        return cls(tt[0], tt[1], tt[2])

    @classmethod
    def fromordinal(cls, n):
        if not 1 <= n <= 3652059:
            raise ValueError('ordinal must be >= 1')
        return cls(*_ord2ymd(n))

    @classmethod
    def fromisoformat(cls, s):
        try:
            if len(s) == 10 and s[4] == '-' and s[7] == '-':
                return cls(int(s[0:4]), int(s[5:7]), int(s[8:10]))
            if len(s) == 8 and s.isdigit():
                return cls(int(s[0:4]), int(s[4:6]), int(s[6:8]))
        except ValueError:
            pass
        raise ValueError('Invalid isoformat string: %r' % (s,))

    @classmethod
    def fromisocalendar(cls, year, week, day):
        jan4 = cls(year, 1, 4)
        start = jan4.toordinal() - jan4.weekday()
        return cls.fromordinal(start + (week - 1) * 7 + day - 1)

    def toordinal(self):
        return _ymd2ord(self._year, self._month, self._day)

    def weekday(self):
        return (self.toordinal() + 6) % 7

    def isoweekday(self):
        return self.toordinal() % 7 or 7

    def isocalendar(self):
        year = self._year
        week1 = _ymd2ord(year, 1, 4) - ((_ymd2ord(year, 1, 4) + 6) % 7)
        o = self.toordinal()
        if o < week1:
            year -= 1
            week1 = _ymd2ord(year, 1, 4) - ((_ymd2ord(year, 1, 4) + 6) % 7)
        elif year < 9999 and o >= _ymd2ord(year + 1, 1, 4) - ((_ymd2ord(year + 1, 1, 4) + 6) % 7):
            year += 1
            week1 = _ymd2ord(year, 1, 4) - ((_ymd2ord(year, 1, 4) + 6) % 7)
        return _IsoCalendarDate(year, (o - week1) // 7 + 1, self.isoweekday())

    def isoformat(self):
        return '%04d-%02d-%02d' % (self._year, self._month, self._day)

    __str__ = isoformat

    def ctime(self):
        return '%s %s %2d 00:00:00 %d' % (
            _DAYNAMES[self.weekday()], _MONTHNAMES[self._month - 1], self._day, self._year)

    def timetuple(self):
        yday = self.toordinal() - _ymd2ord(self._year, 1, 1) + 1
        return _time.struct_time((self._year, self._month, self._day, 0, 0, 0, self.weekday(), yday, -1))

    def strftime(self, fmt):
        return _wrap_strftime(self, fmt, self.timetuple())

    def __format__(self, fmt):
        if not fmt:
            return str(self)
        return self.strftime(fmt)

    def replace(self, year=None, month=None, day=None):
        return type(self)(self._year if year is None else year,
                          self._month if month is None else month,
                          self._day if day is None else day)

    def __repr__(self):
        return 'datetime.date(%d, %d, %d)' % (self._year, self._month, self._day)

    def _cmpkey(self):
        return (self._year, self._month, self._day)

    def __eq__(self, other):
        if isinstance(other, date) and not isinstance(other, datetime):
            return self._cmpkey() == other._cmpkey()
        return NotImplemented

    def __lt__(self, other):
        if isinstance(other, date) and not isinstance(other, datetime):
            return self._cmpkey() < other._cmpkey()
        return NotImplemented

    def __le__(self, other):
        if isinstance(other, date) and not isinstance(other, datetime):
            return self._cmpkey() <= other._cmpkey()
        return NotImplemented

    def __gt__(self, other):
        if isinstance(other, date) and not isinstance(other, datetime):
            return self._cmpkey() > other._cmpkey()
        return NotImplemented

    def __ge__(self, other):
        if isinstance(other, date) and not isinstance(other, datetime):
            return self._cmpkey() >= other._cmpkey()
        return NotImplemented

    def __hash__(self):
        return hash(self._cmpkey())

    def __add__(self, other):
        if isinstance(other, timedelta):
            return date.fromordinal(self.toordinal() + other.days)
        return NotImplemented

    __radd__ = __add__

    def __sub__(self, other):
        if isinstance(other, timedelta):
            return date.fromordinal(self.toordinal() - other.days)
        if isinstance(other, date):
            return timedelta(self.toordinal() - other.toordinal())
        return NotImplemented


class _IsoCalendarDate(tuple):
    def __new__(cls, year, week, weekday):
        return tuple.__new__(cls, (year, week, weekday))

    year = property(lambda self: self[0])
    week = property(lambda self: self[1])
    weekday = property(lambda self: self[2])

    def __repr__(self):
        return 'datetime.IsoCalendarDate(year=%d, week=%d, weekday=%d)' % tuple(self)


date.min = date(1, 1, 1)
date.max = date(9999, 12, 31)
date.resolution = timedelta(days=1)


class time:
    __slots__ = ('_hour', '_minute', '_second', '_microsecond', '_tzinfo', '_fold')

    def __new__(cls, hour=0, minute=0, second=0, microsecond=0, tzinfo=None, *, fold=0):
        _check_time(hour, minute, second, microsecond)
        self = object.__new__(cls)
        self._hour = hour
        self._minute = minute
        self._second = second
        self._microsecond = microsecond
        self._tzinfo = tzinfo
        self._fold = fold
        return self

    hour = property(lambda self: self._hour)
    minute = property(lambda self: self._minute)
    second = property(lambda self: self._second)
    microsecond = property(lambda self: self._microsecond)
    tzinfo = property(lambda self: self._tzinfo)
    fold = property(lambda self: self._fold)

    def utcoffset(self):
        return None if self._tzinfo is None else self._tzinfo.utcoffset(None)

    def tzname(self):
        return None if self._tzinfo is None else self._tzinfo.tzname(None)

    def dst(self):
        return None if self._tzinfo is None else self._tzinfo.dst(None)

    @classmethod
    def fromisoformat(cls, s):
        try:
            tz = None
            body = s
            if body.endswith('Z'):
                tz = timezone.utc
                body = body[:-1]
            else:
                for sign in '+-':
                    k = body.find(sign)
                    if k > 0:
                        off = body[k + 1:]
                        parts = off.split(':')
                        delta = timedelta(hours=int(parts[0]), minutes=int(parts[1]) if len(parts) > 1 else 0)
                        tz = timezone(delta if sign == '+' else -delta)
                        body = body[:k]
                        break
            frac = 0
            if '.' in body:
                body, f = body.split('.')
                frac = int((f + '000000')[:6])
            parts = body.split(':')
            return cls(int(parts[0]), int(parts[1]) if len(parts) > 1 else 0,
                       int(parts[2]) if len(parts) > 2 else 0, frac, tz)
        except (ValueError, IndexError):
            raise ValueError('Invalid isoformat string: %r' % (s,))

    def isoformat(self, timespec='auto'):
        if timespec == 'auto':
            timespec = 'microseconds' if self._microsecond else 'seconds'
        if timespec == 'hours':
            s = '%02d' % self._hour
        elif timespec == 'minutes':
            s = '%02d:%02d' % (self._hour, self._minute)
        elif timespec == 'seconds':
            s = '%02d:%02d:%02d' % (self._hour, self._minute, self._second)
        elif timespec == 'milliseconds':
            s = '%02d:%02d:%02d.%03d' % (self._hour, self._minute, self._second, self._microsecond // 1000)
        elif timespec == 'microseconds':
            s = '%02d:%02d:%02d.%06d' % (self._hour, self._minute, self._second, self._microsecond)
        else:
            raise ValueError('Unknown timespec value')
        off = self.utcoffset()
        if off is not None:
            s += _fmt_offset(off)
        return s

    __str__ = isoformat

    def strftime(self, fmt):
        tt = _time.struct_time((1900, 1, 1, self._hour, self._minute, self._second, 0, 1, -1))
        return _wrap_strftime(self, fmt, tt)

    def __format__(self, fmt):
        if not fmt:
            return str(self)
        return self.strftime(fmt)

    def replace(self, hour=None, minute=None, second=None, microsecond=None, tzinfo=True, *, fold=None):
        return type(self)(self._hour if hour is None else hour,
                          self._minute if minute is None else minute,
                          self._second if second is None else second,
                          self._microsecond if microsecond is None else microsecond,
                          self._tzinfo if tzinfo is True else tzinfo,
                          fold=self._fold if fold is None else fold)

    def _cmpkey(self):
        off = self.utcoffset()
        base = ((self._hour * 60 + self._minute) * 60 + self._second) * 1000000 + self._microsecond
        if off is not None:
            base -= off._total_us()
        return base

    def __eq__(self, other):
        if isinstance(other, time):
            return self._cmpkey() == other._cmpkey()
        return NotImplemented

    def __lt__(self, other):
        if isinstance(other, time):
            return self._cmpkey() < other._cmpkey()
        return NotImplemented

    def __le__(self, other):
        if isinstance(other, time):
            return self._cmpkey() <= other._cmpkey()
        return NotImplemented

    def __gt__(self, other):
        if isinstance(other, time):
            return self._cmpkey() > other._cmpkey()
        return NotImplemented

    def __ge__(self, other):
        if isinstance(other, time):
            return self._cmpkey() >= other._cmpkey()
        return NotImplemented

    def __hash__(self):
        return hash(self._cmpkey())

    def __bool__(self):
        return True

    def __repr__(self):
        if self._microsecond:
            s = ', %d, %d' % (self._second, self._microsecond)
        elif self._second:
            s = ', %d' % self._second
        else:
            s = ''
        r = 'datetime.time(%d, %d%s)' % (self._hour, self._minute, s)
        if self._tzinfo is not None:
            r = r[:-1] + ', tzinfo=%r)' % (self._tzinfo,)
        return r


time.min = time(0, 0, 0)
time.max = time(23, 59, 59, 999999)
time.resolution = timedelta(microseconds=1)


class datetime(date):
    __slots__ = ('_hour', '_minute', '_second', '_microsecond', '_tzinfo', '_fold')

    def __new__(cls, year, month=None, day=None, hour=0, minute=0, second=0, microsecond=0,
                tzinfo=None, *, fold=0):
        _check_date(year, month, day)
        _check_time(hour, minute, second, microsecond)
        self = object.__new__(cls)
        self._year = year
        self._month = month
        self._day = day
        self._hour = hour
        self._minute = minute
        self._second = second
        self._microsecond = microsecond
        self._tzinfo = tzinfo
        self._fold = fold
        return self

    hour = property(lambda self: self._hour)
    minute = property(lambda self: self._minute)
    second = property(lambda self: self._second)
    microsecond = property(lambda self: self._microsecond)
    tzinfo = property(lambda self: self._tzinfo)
    fold = property(lambda self: self._fold)

    @classmethod
    def fromtimestamp(cls, t, tz=None):
        frac, whole = _modf(t)
        us = round(frac * 1e6)
        if us >= 1000000:
            whole += 1
            us -= 1000000
        elif us < 0:
            whole -= 1
            us += 1000000
        tt = _time.gmtime(whole)
        dt = cls(tt[0], tt[1], tt[2], tt[3], tt[4], tt[5], us)
        if tz is not None:
            dt = tz.fromutc(dt.replace(tzinfo=tz))
        return dt

    @classmethod
    def utcfromtimestamp(cls, t):
        return cls.fromtimestamp(t)

    @classmethod
    def now(cls, tz=None):
        return cls.fromtimestamp(_time.time(), tz)

    @classmethod
    def utcnow(cls):
        return cls.fromtimestamp(_time.time())

    @classmethod
    def today(cls):
        return cls.fromtimestamp(_time.time())

    @classmethod
    def fromordinal(cls, n):
        y, m, d = _ord2ymd(n)
        return cls(y, m, d)

    @classmethod
    def combine(cls, d, t, tzinfo=True):
        return cls(d.year, d.month, d.day, t.hour, t.minute, t.second, t.microsecond,
                   t.tzinfo if tzinfo is True else tzinfo)

    @classmethod
    def fromisoformat(cls, s):
        try:
            if 'T' in s or ' ' in s:
                sep = 'T' if 'T' in s else ' '
                dpart, tpart = s.split(sep, 1)
            else:
                dpart, tpart = s, None
            d = date.fromisoformat(dpart)
            if tpart is None:
                return cls(d.year, d.month, d.day)
            t = time.fromisoformat(tpart)
            return cls.combine(d, t)
        except ValueError:
            raise ValueError('Invalid isoformat string: %r' % (s,))

    @classmethod
    def strptime(cls, text, fmt):
        tt = _time.strptime(text, fmt)
        us = 0
        return cls(tt[0], tt[1], tt[2], tt[3], tt[4], tt[5], us)

    def date(self):
        return date(self._year, self._month, self._day)

    def time(self):
        return time(self._hour, self._minute, self._second, self._microsecond)

    def timetz(self):
        return time(self._hour, self._minute, self._second, self._microsecond, self._tzinfo)

    def utcoffset(self):
        return None if self._tzinfo is None else self._tzinfo.utcoffset(self)

    def tzname(self):
        return None if self._tzinfo is None else self._tzinfo.tzname(self)

    def dst(self):
        return None if self._tzinfo is None else self._tzinfo.dst(self)

    def astimezone(self, tz=None):
        if tz is None:
            tz = timezone.utc
        if self._tzinfo is tz:
            return self
        off = self.utcoffset()
        if off is None:
            off = timedelta(0)
        utc = (self - off).replace(tzinfo=tz)
        return tz.fromutc(utc)

    def timestamp(self):
        off = self.utcoffset() or timedelta(0)
        days = self.toordinal() - 719163
        secs = days * 86400 + self._hour * 3600 + self._minute * 60 + self._second
        return (secs - off.total_seconds()) + self._microsecond / 1e6 if False else \
            (secs * 1000000 + self._microsecond - off._total_us()) / 1e6

    def utctimetuple(self):
        d = self
        off = self.utcoffset()
        if off is not None:
            d = (self - off).replace(tzinfo=None)
        return d.timetuple()

    def timetuple(self):
        yday = self.toordinal() - _ymd2ord(self._year, 1, 1) + 1
        return _time.struct_time((self._year, self._month, self._day, self._hour, self._minute,
                                  self._second, self.weekday(), yday, -1))

    def isoformat(self, sep='T', timespec='auto'):
        s = '%04d-%02d-%02d%s' % (self._year, self._month, self._day, sep)
        s += time(self._hour, self._minute, self._second, self._microsecond).isoformat(timespec)
        off = self.utcoffset()
        if off is not None:
            s += _fmt_offset(off)
        return s

    def __str__(self):
        return self.isoformat(' ')

    def ctime(self):
        return '%s %s %2d %02d:%02d:%02d %d' % (
            _DAYNAMES[self.weekday()], _MONTHNAMES[self._month - 1], self._day,
            self._hour, self._minute, self._second, self._year)

    def strftime(self, fmt):
        return _wrap_strftime(self, fmt, self.timetuple())

    def replace(self, year=None, month=None, day=None, hour=None, minute=None, second=None,
                microsecond=None, tzinfo=True, *, fold=None):
        return type(self)(self._year if year is None else year,
                          self._month if month is None else month,
                          self._day if day is None else day,
                          self._hour if hour is None else hour,
                          self._minute if minute is None else minute,
                          self._second if second is None else second,
                          self._microsecond if microsecond is None else microsecond,
                          self._tzinfo if tzinfo is True else tzinfo,
                          fold=self._fold if fold is None else fold)

    def __repr__(self):
        parts = [self._year, self._month, self._day, self._hour, self._minute, self._second, self._microsecond]
        while parts and parts[-1] == 0 and len(parts) > 5:
            parts.pop()
        r = 'datetime.datetime(%s)' % ', '.join(map(str, parts))
        if self._tzinfo is not None:
            r = r[:-1] + ', tzinfo=%r)' % (self._tzinfo,)
        return r

    def _utc_us(self):
        days = self.toordinal()
        us = (((days * 24 + self._hour) * 60 + self._minute) * 60 + self._second) * 1000000 + self._microsecond
        off = self.utcoffset()
        if off is not None:
            us -= off._total_us()
        return us

    def _cmp_other(self, other):
        if isinstance(other, datetime):
            if (self._tzinfo is None) != (other._tzinfo is None):
                raise TypeError("can't compare offset-naive and offset-aware datetimes")
            return other
        return None

    def __eq__(self, other):
        if not isinstance(other, datetime):
            return NotImplemented
        if (self._tzinfo is None) != (other._tzinfo is None):
            return False
        return self._utc_us() == other._utc_us()

    def __lt__(self, other):
        o = self._cmp_other(other)
        return NotImplemented if o is None else self._utc_us() < o._utc_us()

    def __le__(self, other):
        o = self._cmp_other(other)
        return NotImplemented if o is None else self._utc_us() <= o._utc_us()

    def __gt__(self, other):
        o = self._cmp_other(other)
        return NotImplemented if o is None else self._utc_us() > o._utc_us()

    def __ge__(self, other):
        o = self._cmp_other(other)
        return NotImplemented if o is None else self._utc_us() >= o._utc_us()

    def __hash__(self):
        return hash(self._utc_us())

    def __add__(self, other):
        if not isinstance(other, timedelta):
            return NotImplemented
        total = (((self.toordinal() * 24 + self._hour) * 60 + self._minute) * 60 + self._second) * 1000000 \
            + self._microsecond + other._total_us()
        days, rem = divmod(total, 86400 * 1000000)
        secs, us = divmod(rem, 1000000)
        hh, rem = divmod(secs, 3600)
        mm, ss = divmod(rem, 60)
        if not 1 <= days <= 3652059:
            raise OverflowError('date value out of range')
        y, m, d = _ord2ymd(days)
        return type(self)(y, m, d, hh, mm, ss, us, self._tzinfo)

    __radd__ = __add__

    def __sub__(self, other):
        if isinstance(other, datetime):
            if (self._tzinfo is None) != (other._tzinfo is None):
                raise TypeError("can't subtract offset-naive and offset-aware datetimes")
            return timedelta(microseconds=self._utc_us() - other._utc_us())
        if isinstance(other, timedelta):
            return self + timedelta(microseconds=-other._total_us())
        return NotImplemented


datetime.min = datetime(1, 1, 1)
datetime.max = datetime(9999, 12, 31, 23, 59, 59, 999999)
datetime.resolution = timedelta(microseconds=1)


def _modf(x):
    if isinstance(x, int):
        return 0.0, x
    whole = int(x // 1)
    return x - whole, whole
