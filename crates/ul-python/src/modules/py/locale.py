"""locale mínimo do sandbox: só a localidade "C" (o que o oráculo expõe sem LANG)."""

LC_CTYPE = 0
LC_NUMERIC = 1
LC_TIME = 2
LC_COLLATE = 3
LC_MONETARY = 4
LC_MESSAGES = 5
LC_ALL = 6
CHAR_MAX = 127


class Error(Exception):
    pass


def getlocale(category=LC_CTYPE):
    return (None, None)


def setlocale(category, locale=None):
    if locale in (None, "", "C", "POSIX"):
        return "C"
    raise Error("unsupported locale setting")


def getpreferredencoding(do_setlocale=True):
    return "UTF-8"


def getencoding():
    return "UTF-8"


def localeconv():
    return {"decimal_point": ".", "thousands_sep": "", "grouping": [], "int_curr_symbol": "",
            "currency_symbol": "", "mon_decimal_point": "", "mon_thousands_sep": "", "mon_grouping": [],
            "positive_sign": "", "negative_sign": "", "int_frac_digits": CHAR_MAX, "frac_digits": CHAR_MAX,
            "p_cs_precedes": CHAR_MAX, "p_sep_by_space": CHAR_MAX, "n_cs_precedes": CHAR_MAX,
            "n_sep_by_space": CHAR_MAX, "p_sign_posn": CHAR_MAX, "n_sign_posn": CHAR_MAX}


def normalize(localename):
    return localename
