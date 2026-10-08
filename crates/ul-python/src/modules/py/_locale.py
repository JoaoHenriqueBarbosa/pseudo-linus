"""Módulo `_locale` do CPython sobre a única localidade que o sandbox tem, a "C".

Os valores das constantes são os do `langinfo.h` do glibc (`_NL_ITEM(categoria, índice)` é `categoria << 16 | índice`).
Os nomes de apoio começam com sublinhado e não existem para o `dir()` do módulo."""

CHAR_MAX = 127
LC_CTYPE = 0
LC_NUMERIC = 1
LC_TIME = 2
LC_COLLATE = 3
LC_MONETARY = 4
LC_MESSAGES = 5
LC_ALL = 6

CODESET = 14
RADIXCHAR = 0x10000
THOUSEP = 0x10001
CRNCYSTR = 0x4000F
YESEXPR = 0x50000
NOEXPR = 0x50001

AM_STR = 0x20026
PM_STR = 0x20027
D_T_FMT = 0x20028
D_FMT = 0x20029
T_FMT = 0x2002A
T_FMT_AMPM = 0x2002B
ERA = 0x2002C
ERA_D_FMT = 0x2002E
ALT_DIGITS = 0x2002F
ERA_D_T_FMT = 0x20030
ERA_T_FMT = 0x20031
_DATE_FMT = 0x2006C

_DAYS = ("Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday")
_MONTHS = ("January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
           "November", "December")

# O que `nl_langinfo` devolve na localidade "C", pela constante.
_C_LANGINFO = {
    CODESET: "UTF-8", RADIXCHAR: ".", THOUSEP: "", CRNCYSTR: "-", YESEXPR: "^[yY]", NOEXPR: "^[nN]",
    AM_STR: "AM", PM_STR: "PM", D_T_FMT: "%a %b %e %H:%M:%S %Y", D_FMT: "%m/%d/%y", T_FMT: "%H:%M:%S",
    T_FMT_AMPM: "%I:%M:%S %p", ERA: "", ERA_D_FMT: "", ALT_DIGITS: "", ERA_D_T_FMT: "", ERA_T_FMT: "",
    _DATE_FMT: "%a %b %e %H:%M:%S %Z %Y",
}

for _i in range(7):
    globals()["ABDAY_%d" % (_i + 1)] = 0x20000 + _i
    _C_LANGINFO[0x20000 + _i] = _DAYS[_i][:3]
    globals()["DAY_%d" % (_i + 1)] = 0x20007 + _i
    _C_LANGINFO[0x20007 + _i] = _DAYS[_i]
for _i in range(12):
    globals()["ABMON_%d" % (_i + 1)] = 0x2000E + _i
    _C_LANGINFO[0x2000E + _i] = _MONTHS[_i][:3]
    globals()["MON_%d" % (_i + 1)] = 0x2001A + _i
    _C_LANGINFO[0x2001A + _i] = _MONTHS[_i]
del _i

_DEFAULT_LOCALEDIR = "/usr/share/locale"
_text_domain = "messages"
_domain_dirs = {}
_domain_codesets = {}


class Error(Exception):
    pass


Error.__module__ = "locale"


def _check_str(func, position, value, none_ok=False):
    """O erro de argumento que o `clinic` do CPython dá quando `value` não é `str` (ou `None`, se `none_ok`)."""
    if isinstance(value, str):
        if "\0" in value:
            raise ValueError("embedded null character")
    elif not (none_ok and value is None):
        raise TypeError("%s() argument %d must be %s, not %s"
                        % (func, position, "str or None" if none_ok else "str", type(value).__name__))


def _check_domain(func, domain):
    """O domínio de `bindtextdomain` e `bind_textdomain_codeset`: texto não vazio."""
    _check_str(func, 1, domain)
    if not domain:
        raise ValueError("domain must be a non-empty string")


def setlocale(category, locale=None, /):
    if not isinstance(category, int):
        raise TypeError("'%s' object cannot be interpreted as an integer" % type(category).__name__)
    _check_str("setlocale", 2, locale, none_ok=True)
    valid = 0 <= category <= 12
    if locale is None:
        if not valid:
            raise Error("locale query failed")
        return "C"
    if not valid or locale not in ("", "C", "POSIX"):
        raise Error("unsupported locale setting")
    return "C"


def localeconv():
    return {"decimal_point": ".", "thousands_sep": "", "grouping": [], "int_curr_symbol": "",
            "currency_symbol": "", "mon_decimal_point": "", "mon_thousands_sep": "", "mon_grouping": [],
            "positive_sign": "", "negative_sign": "", "int_frac_digits": CHAR_MAX, "frac_digits": CHAR_MAX,
            "p_cs_precedes": CHAR_MAX, "p_sep_by_space": CHAR_MAX, "n_cs_precedes": CHAR_MAX,
            "n_sep_by_space": CHAR_MAX, "p_sign_posn": CHAR_MAX, "n_sign_posn": CHAR_MAX}


def getencoding():
    return _C_LANGINFO[CODESET]


def nl_langinfo(key, /):
    if not isinstance(key, int):
        raise TypeError("'%s' object cannot be interpreted as an integer" % type(key).__name__)
    if key not in _C_LANGINFO:
        raise ValueError("unsupported langinfo constant")
    return _C_LANGINFO[key]


def strcoll(os1, os2, /):
    _check_str("strcoll", 1, os1)
    _check_str("strcoll", 2, os2)
    return (os1 > os2) - (os1 < os2)


def strxfrm(string, /):
    _check_str("strxfrm", 1, string)
    return string


def gettext(msg, /):
    _check_str("gettext", 1, msg)
    return msg


def dgettext(domain, msg, /):
    _check_str("dgettext", 1, domain, none_ok=True)
    _check_str("dgettext", 2, msg)
    return msg


def dcgettext(domain, msg, category, /):
    _check_str("dcgettext", 1, domain, none_ok=True)
    _check_str("dcgettext", 2, msg)
    return msg


def textdomain(domain, /):
    global _text_domain
    _check_str("textdomain", 1, domain, none_ok=True)
    if domain is not None:
        _text_domain = domain
    return _text_domain


def bindtextdomain(domain, dir, /):
    _check_domain("bindtextdomain", domain)
    _check_str("bindtextdomain", 2, dir, none_ok=True)
    if dir is not None:
        _domain_dirs[domain] = dir
    return _domain_dirs.get(domain, _DEFAULT_LOCALEDIR)


def bind_textdomain_codeset(domain, codeset, /):
    _check_domain("bind_textdomain_codeset", domain)
    _check_str("bind_textdomain_codeset", 2, codeset, none_ok=True)
    if codeset is not None:
        _domain_codesets[domain] = codeset
    return _domain_codesets.get(domain)
