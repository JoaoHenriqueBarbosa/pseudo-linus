"""time: relógio do sandbox (fuso sempre UTC)."""

import _os
import _sys
timezone = 0
altzone = 0
daylight = 0
tzname = ('UTC', 'UTC')

_DAYS = ('Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun')
_DAYS_LONG = ('Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday')
_MONTHS = ('Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec')
_MONTHS_LONG = ('January', 'February', 'March', 'April', 'May', 'June', 'July', 'August',
                'September', 'October', 'November', 'December')


def _secs(kind):
    s, ns = _os.clock(kind)
    return s + ns / 1e9


def time():
    return _secs(0)


def time_ns():
    s, ns = _os.clock(0)
    return s * 1000000000 + ns


def monotonic():
    return _secs(1)


def monotonic_ns():
    s, ns = _os.clock(1)
    return s * 1000000000 + ns


def perf_counter():
    return _secs(1)


def perf_counter_ns():
    return monotonic_ns()


def process_time():
    return _secs(2)


def process_time_ns():
    s, ns = _os.clock(2)
    return s * 1000000000 + ns


def thread_time():
    return _secs(2)


def thread_time_ns():
    return process_time_ns()


# Os relógios do Linux (`clock_gettime(2)`) que o sandbox sabe servir: `_os.clock` tem o real, o monotônico e o de CPU.
CLOCK_REALTIME = 0
CLOCK_MONOTONIC = 1
CLOCK_PROCESS_CPUTIME_ID = 2
CLOCK_THREAD_CPUTIME_ID = 3
CLOCK_MONOTONIC_RAW = 4
CLOCK_BOOTTIME = 7
CLOCK_TAI = 11
_STRUCT_TM_ITEMS = 11


def _clock_kind(clk_id):
    """O `kind` do `_os.clock` para o relógio `clk_id`; EINVAL para o que o Linux não conhece."""
    if not isinstance(clk_id, int):
        raise TypeError("'%s' object cannot be interpreted as an integer" % type(clk_id).__name__)
    if clk_id in (CLOCK_REALTIME, CLOCK_TAI):
        return 0
    if clk_id in (CLOCK_MONOTONIC, CLOCK_MONOTONIC_RAW, CLOCK_BOOTTIME):
        return 1
    if clk_id in (CLOCK_PROCESS_CPUTIME_ID, CLOCK_THREAD_CPUTIME_ID) or clk_id < 0:
        return 2
    raise OSError(22, 'Invalid argument')


def clock_gettime(clk_id, /):
    """Return the time of the specified clock clk_id as a float."""
    return _secs(_clock_kind(clk_id))


def clock_gettime_ns(clk_id, /):
    """Return the time of the specified clock clk_id as nanoseconds (int)."""
    s, ns = _os.clock(_clock_kind(clk_id))
    return s * 1000000000 + ns


def clock_getres(clk_id, /):
    """Return the resolution (precision) of the specified clock clk_id."""
    _clock_kind(clk_id)
    return 1e-09


def clock_settime(clk_id, time, /):
    """Set the time of the specified clock clk_id."""
    if _clock_kind(clk_id) != 0 or clk_id == CLOCK_TAI:
        raise OSError(22, 'Invalid argument')
    raise PermissionError(1, 'Operation not permitted')


def clock_settime_ns(clk_id, time, /):
    """Set the time of the specified clock clk_id with nanoseconds."""
    if _clock_kind(clk_id) != 0 or clk_id == CLOCK_TAI:
        raise OSError(22, 'Invalid argument')
    raise PermissionError(1, 'Operation not permitted')


def pthread_getcpuclockid(thread_id, /):
    """Return the clk_id of a thread's CPU time clock."""
    if not isinstance(thread_id, int):
        raise TypeError("'%s' object cannot be interpreted as an integer" % type(thread_id).__name__)
    # `CPUCLOCK_PERTHREAD(tid, CPUCLOCK_SCHED)` do kernel: `(~tid << 3) | 6`.
    return (~thread_id << 3) | 6


def tzset():
    """Initialize, or reinitialize, the local timezone to the value stored in os.environ['TZ']."""
    return None


_sleep_hooks = []


def sleep(secs):
    for hook in _sleep_hooks:
        secs = hook(secs)
    if secs < 0:
        raise ValueError('sleep length must be non-negative')
    if secs > 0:
        _os.sleep(secs)


def _is_leap(y):
    return y % 4 == 0 and (y % 100 != 0 or y % 400 == 0)


def _days_before_year(y):
    y -= 1
    return y * 365 + y // 4 - y // 100 + y // 400


_DIM = (31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31)


def _days_in_month(y, m):
    return 29 if m == 2 and _is_leap(y) else _DIM[m - 1]


class struct_time(tuple):
    __slots__ = ()
    n_fields = 11
    n_sequence_fields = 9
    n_unnamed_fields = 0
    _names = ('tm_year', 'tm_mon', 'tm_mday', 'tm_hour', 'tm_min', 'tm_sec', 'tm_wday', 'tm_yday', 'tm_isdst')

    def __new__(cls, seq):
        seq = tuple(seq)
        if len(seq) < 9:
            raise TypeError('time.struct_time() takes an at least 9-sequence (%d-sequence given)' % len(seq))
        return tuple.__new__(cls, seq[:9])

    @property
    def tm_year(self):
        return self[0]

    @property
    def tm_mon(self):
        return self[1]

    @property
    def tm_mday(self):
        return self[2]

    @property
    def tm_hour(self):
        return self[3]

    @property
    def tm_min(self):
        return self[4]

    @property
    def tm_sec(self):
        return self[5]

    @property
    def tm_wday(self):
        return self[6]

    @property
    def tm_yday(self):
        return self[7]

    @property
    def tm_isdst(self):
        return self[8]

    @property
    def tm_zone(self):
        return 'UTC'

    @property
    def tm_gmtoff(self):
        return 0

    def __repr__(self):
        return 'time.struct_time(' + ', '.join(
            '%s=%r' % (n, v) for n, v in zip(self._names, self)) + ')'


def gmtime(secs=None):
    if secs is None:
        secs = time()
    secs = int(secs // 1)
    days, rem = divmod(secs, 86400)
    hour, rem = divmod(rem, 3600)
    minute, sec = divmod(rem, 60)
    wday = (days + 3) % 7
    # dias desde 1970-01-01 para ano/mês/dia
    n = days + 719162  # dias desde 0001-01-01 (ordinal - 1)
    y = 1 + n // 366
    while _days_before_year(y + 1) <= n:
        y += 1
    while _days_before_year(y) > n:
        y -= 1
    yday = n - _days_before_year(y)
    d = yday
    m = 1
    while d >= _days_in_month(y, m):
        d -= _days_in_month(y, m)
        m += 1
    return struct_time((y, m, d + 1, hour, minute, sec, wday, yday + 1, 0))


def localtime(secs=None):
    return gmtime(secs)


def _timegm(t):
    y, m, d, hh, mm, ss = t[0], t[1], t[2], t[3], t[4], t[5]
    days = _days_before_year(y) - 719162
    for i in range(1, m):
        days += _days_in_month(y, i)
    days += d - 1
    return days * 86400 + hh * 3600 + mm * 60 + ss


def mktime(t):
    return float(_timegm(t))


def asctime(t=None):
    if t is None:
        t = localtime()
    return '%s %s %2d %02d:%02d:%02d %d' % (
        _DAYS[t[6]], _MONTHS[t[1] - 1], t[2], t[3], t[4], t[5], t[0])


def ctime(secs=None):
    return asctime(localtime(secs))


def strftime(fmt, t=None):
    if t is None:
        t = localtime()
    out = []
    i = 0
    n = len(fmt)
    while i < n:
        c = fmt[i]
        if c != '%' or i + 1 >= n:
            out.append(c)
            i += 1
            continue
        i += 1
        k = fmt[i]
        i += 1
        if k == 'Y':
            out.append('%d' % t[0])
        elif k == 'y':
            out.append('%02d' % (t[0] % 100))
        elif k == 'm':
            out.append('%02d' % t[1])
        elif k == 'd':
            out.append('%02d' % t[2])
        elif k == 'e':
            out.append('%2d' % t[2])
        elif k == 'H':
            out.append('%02d' % t[3])
        elif k == 'I':
            out.append('%02d' % ((t[3] % 12) or 12))
        elif k == 'M':
            out.append('%02d' % t[4])
        elif k == 'S':
            out.append('%02d' % t[5])
        elif k == 'p':
            out.append('AM' if t[3] < 12 else 'PM')
        elif k == 'a':
            out.append(_DAYS[t[6]])
        elif k == 'A':
            out.append(_DAYS_LONG[t[6]])
        elif k in 'bh':
            out.append(_MONTHS[t[1] - 1])
        elif k == 'B':
            out.append(_MONTHS_LONG[t[1] - 1])
        elif k == 'j':
            out.append('%03d' % t[7])
        elif k == 'w':
            out.append('%d' % ((t[6] + 1) % 7))
        elif k == 'u':
            out.append('%d' % (t[6] + 1))
        elif k == 'U':
            out.append('%02d' % ((t[7] - 1 + 7 - ((t[6] + 1) % 7)) // 7))
        elif k == 'W':
            out.append('%02d' % ((t[7] - 1 + 7 - t[6]) // 7))
        elif k in 'VGg':
            import datetime
            iso = datetime.date(t[0], t[1], t[2]).isocalendar()
            out.append('%02d' % iso[1] if k == 'V' else ('%d' % iso[0] if k == 'G' else '%02d' % (iso[0] % 100)))
        elif k == 'Z':
            out.append('UTC')
        elif k == 'z':
            out.append('+0000')
        elif k == 'F':
            out.append('%d-%02d-%02d' % (t[0], t[1], t[2]))
        elif k == 'T':
            out.append('%02d:%02d:%02d' % (t[3], t[4], t[5]))
        elif k == 'R':
            out.append('%02d:%02d' % (t[3], t[4]))
        elif k == 'D':
            out.append('%02d/%02d/%02d' % (t[1], t[2], t[0] % 100))
        elif k == 'c':
            out.append(asctime(t))
        elif k == 'x':
            out.append('%02d/%02d/%02d' % (t[1], t[2], t[0] % 100))
        elif k == 'X':
            out.append('%02d:%02d:%02d' % (t[3], t[4], t[5]))
        elif k == 's':
            out.append('%d' % _timegm(t))
        elif k == 'n':
            out.append('\n')
        elif k == 't':
            out.append('\t')
        elif k == '%':
            out.append('%')
        else:
            out.append('%' + k)
    return ''.join(out)


def strptime(text, fmt='%a %b %d %H:%M:%S %Y'):
    import re
    spec = {
        'Y': r'(?P<Y>\d{4})', 'y': r'(?P<y>\d\d)', 'm': r'(?P<m>1[0-2]|0[1-9]|[1-9])',
        'd': r'(?P<d>3[01]|[12]\d|0[1-9]|[1-9]| [1-9])', 'H': r'(?P<H>2[0-3]|[0-1]\d|\d)',
        'M': r'(?P<M>[0-5]\d|\d)', 'S': r'(?P<S>6[0-1]|[0-5]\d|\d)', 'j': r'(?P<j>\d{1,3})',
        'b': r'(?P<b>[A-Za-z]{3})', 'B': r'(?P<B>[A-Za-z]+)', 'a': r'(?P<a>[A-Za-z]{3})',
        'A': r'(?P<A>[A-Za-z]+)', 'p': r'(?P<p>[AaPp][Mm])', 'I': r'(?P<I>1[0-2]|0[1-9]|[1-9])',
        'Z': r'(?P<Z>[A-Za-z]+)', '%': '%',
    }
    pattern = []
    i = 0
    while i < len(fmt):
        c = fmt[i]
        if c == '%' and i + 1 < len(fmt):
            k = fmt[i + 1]
            i += 2
            if k == 'F':
                pattern.append(spec['Y'] + '-' + spec['m'] + '-' + spec['d'])
            elif k == 'T':
                pattern.append(spec['H'] + ':' + spec['M'] + ':' + spec['S'])
            elif k in spec:
                pattern.append(spec[k])
            else:
                raise ValueError("'%s' is a bad directive in format '%s'" % (k, fmt))
        else:
            pattern.append(re.escape(c) if not c.isspace() else r'\s+')
            i += 1
    m = re.match(''.join(pattern) + '$', text)
    if m is None:
        raise ValueError("time data %r does not match format %r" % (text, fmt))
    g = m.groupdict()
    year, mon, day, hh, mi, ss, pm = 1900, 1, 1, 0, 0, 0, None
    if g.get('Y'):
        year = int(g['Y'])
    elif g.get('y'):
        yy = int(g['y'])
        year = 2000 + yy if yy < 69 else 1900 + yy
    if g.get('m'):
        mon = int(g['m'])
    elif g.get('b'):
        mon = [x.lower() for x in _MONTHS].index(g['b'].lower()) + 1
    elif g.get('B'):
        mon = [x.lower() for x in _MONTHS_LONG].index(g['B'].lower()) + 1
    if g.get('d'):
        day = int(g['d'])
    if g.get('H'):
        hh = int(g['H'])
    if g.get('I'):
        hh = int(g['I']) % 12
        if g.get('p') and g['p'].lower() == 'pm':
            hh += 12
    elif g.get('p') and g['p'].lower() == 'pm' and hh < 12:
        hh += 12
    if g.get('M'):
        mi = int(g['M'])
    if g.get('S'):
        ss = int(g['S'])
    days = _days_before_year(year) - 719162
    for k in range(1, mon):
        days += _days_in_month(year, k)
    days += day - 1
    wday = (days + 3) % 7
    yday = days - (_days_before_year(year) - 719162) + 1
    return struct_time((year, mon, day, hh, mi, ss, wday, yday, -1))


def get_clock_info(name):
    class SimpleNamespaceLike:
        pass
    o = SimpleNamespaceLike()
    o.implementation = 'clock_gettime(CLOCK_REALTIME)'
    o.monotonic = name != 'time'
    o.adjustable = name == 'time'
    o.resolution = 1e-09
    return o


# No CPython estas funções são embutidas (C): guardadas num atributo de classe não viram método ligado.
time = _sys._builtin(time)
time_ns = _sys._builtin(time_ns)
monotonic = _sys._builtin(monotonic)
monotonic_ns = _sys._builtin(monotonic_ns)
perf_counter = _sys._builtin(perf_counter)
perf_counter_ns = _sys._builtin(perf_counter_ns)
process_time = _sys._builtin(process_time)
process_time_ns = _sys._builtin(process_time_ns)
thread_time = _sys._builtin(thread_time)
thread_time_ns = _sys._builtin(thread_time_ns)
clock_gettime = _sys._builtin(clock_gettime)
clock_gettime_ns = _sys._builtin(clock_gettime_ns)
clock_getres = _sys._builtin(clock_getres)
clock_settime = _sys._builtin(clock_settime)
clock_settime_ns = _sys._builtin(clock_settime_ns)
pthread_getcpuclockid = _sys._builtin(pthread_getcpuclockid)
tzset = _sys._builtin(tzset)
sleep = _sys._builtin(sleep)
gmtime = _sys._builtin(gmtime)
localtime = _sys._builtin(localtime)
mktime = _sys._builtin(mktime)
asctime = _sys._builtin(asctime)
ctime = _sys._builtin(ctime)
strftime = _sys._builtin(strftime)
strptime = _sys._builtin(strptime)
get_clock_info = _sys._builtin(get_clock_info)
