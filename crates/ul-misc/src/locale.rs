//! `locale` da glibc 2.41 (Debian 13), portado de `locale/programs/locale.c`: sem argumento mostra as
//! variáveis de ambiente de locale, com nomes de categoria ou de palavra-chave mostra os valores
//! (`-c`, `-k`), `-a` lista os locales instalados e `-m` os mapas de caracteres.
//!
//! O sandbox só tem os locales embutidos: `C`, `POSIX` (idênticos) e `C.UTF-8`/`C.utf8` (qualquer
//! grafia do codeset que normalize para `utf8`). Para eles todos os valores de todas as categorias
//! vêm das tabelas abaixo (extraídas do oráculo); qualquer outro nome faz o `setlocale` falhar com as
//! mensagens de sempre ("Cannot set LC_CTYPE to default locale: No such file or directory"). O `-a`
//! lê o diretório `/usr/lib/locale` do sandbox como o original (cada subdiretório com um
//! `LC_IDENTIFICATION` regular é um locale), então lista o que a imagem tiver ali.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, FileType, sys};

use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("all-locales", HasArg::No, 'a' as i32),
    LongOpt::new("charmaps", HasArg::No, 'm' as i32),
    LongOpt::new("category-name", HasArg::No, 'c' as i32),
    LongOpt::new("keyword-name", HasArg::No, 'k' as i32),
    LongOpt::new("verbose", HasArg::No, 'v' as i32),
    LongOpt::new("help", HasArg::No, '?' as i32),
    LongOpt::new("usage", HasArg::No, 256),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

const HELP: &str = "Usage: locale [OPTION...] NAME
  or:  locale [OPTION...] [-a|-m]
Get locale-specific information.

 System information:
  -a, --all-locales          Write names of available locales
  -m, --charmaps             Write names of available charmaps

 Modify output format:
  -c, --category-name        Write names of selected categories
  -k, --keyword-name         Write names of selected keywords
  -v, --verbose              Print more information

  -?, --help                 Give this help list
      --usage                Give a short usage message
  -V, --version              Print program version

For bug reporting instructions, please see:
<http://www.debian.org/Bugs/>.
";

const USAGE: &str = "Usage: locale [-ckv?V] [--category-name] [--keyword-name] [--verbose] [--help]
            [--usage] [--version] NAME
  or:  locale [OPTION...] [-a|-m]
";

const VERSION: &str = "locale (Debian GLIBC 2.41-12+deb13u4) 2.41
Copyright (C) 2024 Free Software Foundation, Inc.
This is free software; see the source for copying conditions.  There is NO
warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
Written by Ulrich Drepper.
";

/// Diretório dos locales compilados e dos arquivos de apelidos.
const COMPLOCALEDIR: &str = "/usr/lib/locale";
const LOCALE_ALIAS_PATH: &str = "/usr/share/locale";
const CHARMAP_PATH: &str = "/usr/share/i18n/charmaps";

/// As categorias na ordem de `categories.def` (sem `LC_ALL`).
const CATEGORIES: [&str; 12] = [
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "LC_PAPER",
    "LC_NAME",
    "LC_ADDRESS",
    "LC_TELEPHONE",
    "LC_MEASUREMENT",
    "LC_IDENTIFICATION",
];

const LC_CTYPE: usize = 0;
const LC_COLLATE: usize = 3;
const LC_MESSAGES: usize = 5;

/// Os dados que cada locale embutido dá a uma categoria.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Loc {
    C,
    Utf8,
}

/// Como um item se imprime: `Q` texto entre aspas no `-k`, `L` lista de textos entre aspas (no texto
/// os elementos vêm separados por `;`) e `N` valor numérico ou lista vazia, sempre cru.
type Item = (u8, &'static str, char, &'static str);

/// Itens do locale `C` por categoria, na ordem em que o original imprime a categoria inteira.
const ITEMS: &[Item] = &[
    (0, "ctype-class-names", 'L', "upper;lower;alpha;digit;xdigit;space;print;graph;blank;cntrl;punct;alnum"),
    (0, "ctype-map-names", 'L', "toupper;tolower"),
    (0, "ctype-width", 'N', "7"),
    (0, "ctype-mb-cur-max", 'N', "1"),
    (0, "charmap", 'Q', "ANSI_X3.4-1968"),
    (0, "ctype-class-offset", 'N', "72"),
    (0, "ctype-map-offset", 'N', "84"),
    (0, "ctype-indigits_mb-len", 'N', "1"),
    (0, "ctype-indigits0_mb", 'Q', "0"),
    (0, "ctype-indigits1_mb", 'Q', "1"),
    (0, "ctype-indigits2_mb", 'Q', "2"),
    (0, "ctype-indigits3_mb", 'Q', "3"),
    (0, "ctype-indigits4_mb", 'Q', "4"),
    (0, "ctype-indigits5_mb", 'Q', "5"),
    (0, "ctype-indigits6_mb", 'Q', "6"),
    (0, "ctype-indigits7_mb", 'Q', "7"),
    (0, "ctype-indigits8_mb", 'Q', "8"),
    (0, "ctype-indigits9_mb", 'Q', "9"),
    (0, "ctype-indigits_wc-len", 'N', "1"),
    (0, "ctype-outdigit0_mb", 'Q', "0"),
    (0, "ctype-outdigit1_mb", 'Q', "1"),
    (0, "ctype-outdigit2_mb", 'Q', "2"),
    (0, "ctype-outdigit3_mb", 'Q', "3"),
    (0, "ctype-outdigit4_mb", 'Q', "4"),
    (0, "ctype-outdigit5_mb", 'Q', "5"),
    (0, "ctype-outdigit6_mb", 'Q', "6"),
    (0, "ctype-outdigit7_mb", 'Q', "7"),
    (0, "ctype-outdigit8_mb", 'Q', "8"),
    (0, "ctype-outdigit9_mb", 'Q', "9"),
    (0, "ctype-outdigit0_wc", 'N', "48"),
    (0, "ctype-outdigit1_wc", 'N', "49"),
    (0, "ctype-outdigit2_wc", 'N', "50"),
    (0, "ctype-outdigit3_wc", 'N', "51"),
    (0, "ctype-outdigit4_wc", 'N', "52"),
    (0, "ctype-outdigit5_wc", 'N', "53"),
    (0, "ctype-outdigit6_wc", 'N', "54"),
    (0, "ctype-outdigit7_wc", 'N', "55"),
    (0, "ctype-outdigit8_wc", 'N', "56"),
    (0, "ctype-outdigit9_wc", 'N', "57"),
    (0, "ctype-translit-tab-size", 'N', "1659"),
    (0, "ctype-translit-default-missing-len", 'N', "1"),
    (0, "ctype-translit-ignore-len", 'N', "0"),
    (0, "ctype-translit-ignore", 'Q', ""),
    (0, "map-to-nonascii", 'N', "0"),
    (0, "nonascii-case", 'N', "0"),
    (1, "decimal_point", 'Q', "."),
    (1, "thousands_sep", 'Q', ""),
    (1, "grouping", 'N', "-1"),
    (1, "numeric-decimal-point-wc", 'N', "46"),
    (1, "numeric-thousands-sep-wc", 'N', "0"),
    (1, "numeric-codeset", 'Q', "ANSI_X3.4-1968"),
    (2, "abday", 'Q', "Sun;Mon;Tue;Wed;Thu;Fri;Sat"),
    (2, "day", 'Q', "Sunday;Monday;Tuesday;Wednesday;Thursday;Friday;Saturday"),
    (2, "abmon", 'Q', "Jan;Feb;Mar;Apr;May;Jun;Jul;Aug;Sep;Oct;Nov;Dec"),
    (2, "mon", 'Q', "January;February;March;April;May;June;July;August;September;October;November;December"),
    (2, "am_pm", 'Q', "AM;PM"),
    (2, "d_t_fmt", 'Q', "%a %b %e %H:%M:%S %Y"),
    (2, "d_fmt", 'Q', "%m/%d/%y"),
    (2, "t_fmt", 'Q', "%H:%M:%S"),
    (2, "t_fmt_ampm", 'Q', "%I:%M:%S %p"),
    (2, "era", 'N', ""),
    (2, "era_year", 'Q', ""),
    (2, "era_d_fmt", 'Q', ""),
    (2, "alt_digits", 'N', ""),
    (2, "era_d_t_fmt", 'Q', ""),
    (2, "era_t_fmt", 'Q', ""),
    (2, "time-era-num-entries", 'N', "0"),
    (2, "time-era-entries", 'Q', ""),
    (2, "week-ndays", 'N', "7"),
    (2, "week-1stday", 'N', "19971130"),
    (2, "week-1stweek", 'N', "4"),
    (2, "first_weekday", 'N', "1"),
    (2, "first_workday", 'N', "2"),
    (2, "cal_direction", 'N', "1"),
    (2, "timezone", 'Q', ""),
    (2, "date_fmt", 'Q', "%a %b %e %H:%M:%S %Z %Y"),
    (2, "time-codeset", 'Q', "ANSI_X3.4-1968"),
    (2, "alt_mon", 'Q', "January;February;March;April;May;June;July;August;September;October;November;December"),
    (2, "ab_alt_mon", 'Q', "Jan;Feb;Mar;Apr;May;Jun;Jul;Aug;Sep;Oct;Nov;Dec"),
    (3, "collate-nrules", 'N', "0"),
    (3, "collate-rulesets", 'Q', ""),
    (3, "collate-symb-hash-sizemb", 'N', "0"),
    (3, "collate-codeset", 'Q', "ANSI_X3.4-1968"),
    (4, "int_curr_symbol", 'Q', ""),
    (4, "currency_symbol", 'Q', ""),
    (4, "mon_decimal_point", 'Q', ""),
    (4, "mon_thousands_sep", 'Q', ""),
    (4, "mon_grouping", 'N', "-1"),
    (4, "positive_sign", 'Q', ""),
    (4, "negative_sign", 'Q', ""),
    (4, "int_frac_digits", 'N', "-1"),
    (4, "frac_digits", 'N', "-1"),
    (4, "p_cs_precedes", 'N', "-1"),
    (4, "p_sep_by_space", 'N', "-1"),
    (4, "n_cs_precedes", 'N', "-1"),
    (4, "n_sep_by_space", 'N', "-1"),
    (4, "p_sign_posn", 'N', "-1"),
    (4, "n_sign_posn", 'N', "-1"),
    (4, "crncystr", 'Q', "-"),
    (4, "int_p_cs_precedes", 'N', "-1"),
    (4, "int_p_sep_by_space", 'N', "-1"),
    (4, "int_n_cs_precedes", 'N', "-1"),
    (4, "int_n_sep_by_space", 'N', "-1"),
    (4, "int_p_sign_posn", 'N', "-1"),
    (4, "int_n_sign_posn", 'N', "-1"),
    (4, "duo_int_curr_symbol", 'Q', ""),
    (4, "duo_currency_symbol", 'Q', ""),
    (4, "duo_int_frac_digits", 'N', "-1"),
    (4, "duo_frac_digits", 'N', "-1"),
    (4, "duo_p_cs_precedes", 'N', "-1"),
    (4, "duo_p_sep_by_space", 'N', "-1"),
    (4, "duo_n_cs_precedes", 'N', "-1"),
    (4, "duo_n_sep_by_space", 'N', "-1"),
    (4, "duo_int_p_cs_precedes", 'N', "-1"),
    (4, "duo_int_p_sep_by_space", 'N', "-1"),
    (4, "duo_int_n_cs_precedes", 'N', "-1"),
    (4, "duo_int_n_sep_by_space", 'N', "-1"),
    (4, "duo_p_sign_posn", 'N', "-1"),
    (4, "duo_n_sign_posn", 'N', "-1"),
    (4, "duo_int_p_sign_posn", 'N', "-1"),
    (4, "duo_int_n_sign_posn", 'N', "-1"),
    (4, "uno_valid_from", 'N', "10101"),
    (4, "uno_valid_to", 'N', "99991231"),
    (4, "duo_valid_from", 'N', "10101"),
    (4, "duo_valid_to", 'N', "99991231"),
    (4, "conversion_rate", 'N', "1;1"),
    (4, "monetary-decimal-point-wc", 'N', "0"),
    (4, "monetary-thousands-sep-wc", 'N', "0"),
    (4, "monetary-codeset", 'Q', "ANSI_X3.4-1968"),
    (5, "yesexpr", 'Q', "^[yY]"),
    (5, "noexpr", 'Q', "^[nN]"),
    (5, "yesstr", 'Q', ""),
    (5, "nostr", 'Q', ""),
    (5, "messages-codeset", 'Q', "ANSI_X3.4-1968"),
    (6, "height", 'N', "297"),
    (6, "width", 'N', "210"),
    (6, "paper-codeset", 'Q', "ANSI_X3.4-1968"),
    (7, "name_fmt", 'Q', "%p%t%g%t%m%t%f"),
    (7, "name_gen", 'Q', ""),
    (7, "name_mr", 'Q', ""),
    (7, "name_mrs", 'Q', ""),
    (7, "name_miss", 'Q', ""),
    (7, "name_ms", 'Q', ""),
    (7, "name-codeset", 'Q', "ANSI_X3.4-1968"),
    (8, "postal_fmt", 'Q', "%a%N%f%N%d%N%b%N%s %h %e %r%N%C-%z %T%N%c%N"),
    (8, "country_name", 'Q', ""),
    (8, "country_post", 'Q', ""),
    (8, "country_ab2", 'Q', ""),
    (8, "country_ab3", 'Q', ""),
    (8, "country_car", 'Q', ""),
    (8, "country_num", 'N', "0"),
    (8, "country_isbn", 'Q', ""),
    (8, "lang_name", 'Q', ""),
    (8, "lang_ab", 'Q', ""),
    (8, "lang_term", 'Q', ""),
    (8, "lang_lib", 'Q', ""),
    (8, "address-codeset", 'Q', "ANSI_X3.4-1968"),
    (9, "tel_int_fmt", 'Q', "+%c %a %l"),
    (9, "tel_dom_fmt", 'Q', ""),
    (9, "int_select", 'Q', ""),
    (9, "int_prefix", 'Q', ""),
    (9, "telephone-codeset", 'Q', "ANSI_X3.4-1968"),
    (10, "measurement", 'N', "1"),
    (10, "measurement-codeset", 'Q', "ANSI_X3.4-1968"),
    (11, "title", 'Q', "ISO/IEC 14652 i18n FDCC-set"),
    (11, "source", 'Q', "ISO/IEC JTC1/SC22/WG20 - internationalization"),
    (11, "address", 'Q', "C/o Keld Simonsen, Skt. Jorgens Alle 8, DK-1615 Kobenhavn V"),
    (11, "contact", 'Q', "Keld Simonsen"),
    (11, "email", 'Q', "keld@dkuug.dk"),
    (11, "tel", 'Q', "+45 3122-6543"),
    (11, "fax", 'Q', "+45 3325-6543"),
    (11, "language", 'Q', ""),
    (11, "territory", 'Q', "ISO"),
    (11, "audience", 'Q', ""),
    (11, "application", 'Q', ""),
    (11, "abbreviation", 'Q', ""),
    (11, "revision", 'Q', "1.0"),
    (11, "date", 'Q', "1997-12-20"),
    (11, "category", 'Q', "i18n:1999;ANSI_X3.4-1968;;;;;;;;;;;"),
    (11, "identification-codeset", 'Q', "ANSI_X3.4-1968"),
];

/// O que muda no `C.UTF-8` em relação ao `C`.
const UTF8_OVERRIDES: &[(&str, &str)] = &[
    ("ctype-class-names", "upper;lower;alpha;digit;xdigit;space;print;graph;blank;cntrl;punct;alnum;combining;combining_level3"),
    ("ctype-map-names", "toupper;tolower;totitle"),
    ("ctype-width", "16"),
    ("ctype-mb-cur-max", "6"),
    ("charmap", "UTF-8"),
    ("ctype-map-offset", "86"),
    ("ctype-translit-tab-size", "6492"),
    ("numeric-codeset", "UTF-8"),
    ("time-era-entries", "S"),
    ("time-codeset", "UTF-8"),
    ("collate-codeset", "UTF-8"),
    ("monetary-codeset", "UTF-8"),
    ("messages-codeset", "UTF-8"),
    ("paper-codeset", "UTF-8"),
    ("name-codeset", "UTF-8"),
    ("address-codeset", "UTF-8"),
    ("telephone-codeset", "UTF-8"),
    ("measurement-codeset", "UTF-8"),
    ("title", "C locale"),
    ("source", ""),
    ("address", ""),
    ("contact", ""),
    ("email", "bug-glibc-locales@gnu.org"),
    ("tel", ""),
    ("fax", ""),
    ("territory", ""),
    ("revision", "2.1"),
    ("date", "2022-01-30"),
    ("category", "i18n:2012;UTF-8;;;;;;;;;;;"),
    ("identification-codeset", "UTF-8"),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// O texto de um item no locale `loc`.
fn item_text(item: &Item, loc: Loc) -> &'static str {
    if loc == Loc::Utf8 {
        if let Some((_, v)) = UTF8_OVERRIDES.iter().find(|(n, _)| *n == item.1) {
            return v;
        }
    }
    item.3
}

/// `print_item`: `NOME=valor` com `-k`, só o valor sem ele.
fn print_item(out: &mut dyn Write, item: &Item, loc: Loc, keyword: bool) {
    let text = item_text(item, loc);
    let mut line = String::new();
    match item.2 {
        'Q' => {
            if keyword {
                line.push_str(&format!("{}=\"{}\"", item.1, text));
            } else {
                line.push_str(text);
            }
        }
        'L' => {
            if keyword {
                line.push_str(&format!("{}=\"{}\"", item.1, text.replace(';', "\";\"")));
            } else {
                line.push_str(text);
            }
        }
        _ => {
            if keyword {
                line.push_str(&format!("{}=", item.1));
            }
            line.push_str(text);
        }
    }
    line.push('\n');
    let _ = out.write_all(line.as_bytes());
}

/// `print_assignment`: `NOME=valor`, com aspas duplas ou com barras antes dos caracteres especiais.
fn print_assignment(out: &mut dyn Write, name: &str, val: &[u8], dquote: bool) {
    let special: &[u8] = if dquote { b"$`\"\\" } else { b"~|&;<>()$`\\\"' \t\n" };
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(name.as_bytes());
    buf.push(b'=');
    if dquote {
        buf.push(b'"');
    }
    for &b in val {
        if special.contains(&b) {
            buf.push(b'\\');
        }
        buf.push(b);
    }
    if dquote {
        buf.push(b'"');
    }
    buf.push(b'\n');
    let _ = out.write_all(&buf);
}

fn env(name: &str) -> Option<Vec<u8>> {
    sys::getenv(name)
}

/// O nome de locale que o `setlocale (cat, "")` usa: `LC_ALL`, a variável da categoria, `LANG` ou `C`.
fn env_locale_name(category: &str) -> Vec<u8> {
    for var in ["LC_ALL", category, "LANG"] {
        if let Some(v) = env(var) {
            if !v.is_empty() {
                return v;
            }
        }
    }
    b"C".to_vec()
}

/// O codeset normalizado da glibc (`_nl_normalize_codeset`): só letras e dígitos, em minúsculas.
fn normalize_codeset(s: &[u8]) -> Vec<u8> {
    s.iter().filter(|b| b.is_ascii_alphanumeric()).map(|b| b.to_ascii_lowercase()).collect()
}

const ENOENT_MSG: &str = "No such file or directory";
const EINVAL_MSG: &str = "Invalid argument";

/// `valid_locale_name` do findlocale.c: nome curto, sem `..` como componente e, se tem barra, começando
/// por ela.
fn valid_locale_name(name: &[u8]) -> bool {
    let n = name.len();
    if n > 255 || name.windows(4).any(|w| w == b"/../") {
        return false;
    }
    if n == 2 && name == b".." {
        return false;
    }
    if n >= 3 && (name.starts_with(b"../") || name.ends_with(b"/..")) {
        return false;
    }
    !(name.contains(&b'/') && name[0] != b'/')
}

/// O `strip` do gconv aplicado ao codeset do nome (letras, dígitos e `_-.,:` em maiúsculas): só
/// `UTF-8` e `UTF8` são o mesmo codeset do `C.utf8`.
fn codeset_is_utf8(codeset: &[u8]) -> bool {
    let stripped: Vec<u8> = codeset
        .iter()
        .filter(|b| b.is_ascii_alphanumeric() || matches!(**b, b'_' | b'-' | b'.' | b',' | b':'))
        .map(|b| b.to_ascii_uppercase())
        .collect();
    stripped == b"UTF-8" || stripped == b"UTF8"
}

/// Resolve um nome de locale pros dados embutidos como o `_nl_find_locale` faria com a árvore de
/// `/usr/lib/locale` do sandbox (só `C.utf8` existe lá). `Err` traz a mensagem do `errno`.
///
/// O nome se decompõe em `língua[_território][.codeset][@modificador]`; território e modificador são
/// descartados na busca (o `C_US.UTF-8` e o `C.UTF-8@x` resolvem como `C.UTF-8`), a língua tem de ser
/// `C`, e o codeset, que dá o diretório `C.utf8` pela forma normalizada, tem de ser mesmo o UTF-8.
fn resolve_locale(name: &[u8]) -> Result<Loc, &'static str> {
    if name == b"C" || name == b"POSIX" {
        return Ok(Loc::C);
    }
    if !valid_locale_name(name) {
        return Err(EINVAL_MSG);
    }
    let len = name.len();
    let lang_end = name.iter().position(|b| matches!(*b, b'_' | b'@' | b'.')).unwrap_or(len);
    let language = &name[..lang_end];
    if language.is_empty() {
        return Err(ENOENT_MSG);
    }
    let mut cp = lang_end;
    if name.get(cp) == Some(&b'_') {
        cp += 1;
        while cp < len && name[cp] != b'.' && name[cp] != b'@' {
            cp += 1;
        }
    }
    let mut codeset: &[u8] = b"";
    if name.get(cp) == Some(&b'.') {
        cp += 1;
        let start = cp;
        while cp < len && name[cp] != b'@' {
            cp += 1;
        }
        codeset = &name[start..cp];
    }
    // Um nome absoluto ("/C.utf8") passa pela concatenação com o diretório e vira o mesmo caminho.
    let mut lang = language;
    while let Some(rest) = lang.strip_prefix(b"/") {
        lang = rest;
    }
    if lang != b"C" || codeset.is_empty() || normalize_codeset(codeset) != b"utf8" || !codeset_is_utf8(codeset) {
        return Err(ENOENT_MSG);
    }
    Ok(Loc::Utf8)
}

/// O estado do `setlocale` do processo: o locale de cada categoria e se alguma tentativa falhou.
struct Locales {
    cat: [Loc; 12],
    failed: bool,
}

impl Locales {
    /// `try_setlocale (category, name)` de uma categoria.
    fn try_one(&mut self, idx: usize, argv0: &str) {
        match resolve_locale(&env_locale_name(CATEGORIES[idx])) {
            Ok(loc) => self.cat[idx] = loc,
            Err(msg) => {
                io::eprint(format!("{argv0}: Cannot set {} to default locale: {msg}\n", CATEGORIES[idx]));
                self.failed = true;
            }
        }
    }

    /// `try_setlocale (LC_ALL, "LC_ALL")`: só muda se todas as categorias resolvem.
    fn try_all(&mut self, argv0: &str) {
        let mut new = self.cat;
        for (i, cat) in CATEGORIES.iter().enumerate() {
            match resolve_locale(&env_locale_name(cat)) {
                Ok(loc) => new[i] = loc,
                Err(msg) => {
                    io::eprint(format!("{argv0}: Cannot set LC_ALL to default locale: {msg}\n"));
                    self.failed = true;
                    return;
                }
            }
        }
        self.cat = new;
    }

    /// `setlocale_diagnostics`: depois de uma falha, avisa do `LOCPATH`.
    fn diagnostics(&self) {
        if !self.failed {
            return;
        }
        if let Some(locpath) = env("LOCPATH") {
            io::eprint(format!("warning: The LOCPATH variable is set to \"{}\"\n", quote_string(&locpath)));
        }
    }
}

/// `quote_string`: escapes de C para controles e `\ ' "`, octal de três dígitos para o resto não
/// imprimível.
fn quote_string(input: &[u8]) -> String {
    let mut out = String::new();
    for &ch in input {
        match ch {
            7 => out.push_str("\\a"),
            8 => out.push_str("\\b"),
            0x0c => out.push_str("\\f"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x0b => out.push_str("\\v"),
            b'\\' | b'\'' | b'"' => {
                out.push('\\');
                out.push(char::from(ch));
            }
            c if !(b' '..=b'~').contains(&c) => out.push_str(&format!("\\{c:03o}")),
            c => out.push(char::from(c)),
        }
    }
    out
}

/// `show_locale_vars`.
fn show_locale_vars(out: &mut dyn Write) {
    let lcall = env("LC_ALL").unwrap_or_default();
    let language = env("LANGUAGE").unwrap_or_default();
    let lang = env("LANG").unwrap_or_default();
    print_assignment(out, "LANG", &lang, false);
    if env("POSIXLY_CORRECT").is_none() {
        let mut line = b"LANGUAGE=".to_vec();
        line.extend_from_slice(&language);
        line.push(b'\n');
        let _ = out.write_all(&line);
    }
    for name in CATEGORIES {
        let val = env(name);
        if !lcall.is_empty() || val.is_none() {
            let shown: &[u8] = if !lcall.is_empty() {
                &lcall
            } else if !lang.is_empty() {
                &lang
            } else {
                b"POSIX"
            };
            print_assignment(out, name, shown, true);
        } else {
            print_assignment(out, name, &val.unwrap_or_default(), false);
        }
    }
    print_assignment(out, "LC_ALL", &lcall, false);
}

/// `show_info`: uma categoria inteira ou uma palavra-chave; `Err` é o nome desconhecido.
fn show_info(out: &mut dyn Write, name: &[u8], locales: &Locales, show_category: bool, show_keyword: bool) -> Result<(), ()> {
    for (cat_no, cat_name) in CATEGORIES.iter().enumerate() {
        let loc = locales.cat[cat_no];
        if name == cat_name.as_bytes() {
            if show_category {
                let _ = writeln!(out, "{cat_name}");
            }
            for item in ITEMS.iter().filter(|i| usize::from(i.0) == cat_no) {
                print_item(out, item, loc, show_keyword);
            }
            return Ok(());
        }
        if let Some(item) = ITEMS.iter().find(|i| usize::from(i.0) == cat_no && i.1.as_bytes() == name) {
            if show_category {
                let _ = writeln!(out, "{cat_name}");
            }
            print_item(out, item, loc, show_keyword);
            return Ok(());
        }
    }
    Err(())
}

/// Identificação e codeset de um locale compilado (`print_LC_IDENTIFICATION` e `print_LC_CTYPE`).
fn print_locale_details(out: &mut dyn Write, dir: &str) {
    let word = |d: &[u8], at: usize| -> Option<u32> {
        d.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let string_at = |d: &[u8], index: usize| -> Option<String> {
        let off = word(d, 8 + 4 * index)? as usize;
        let rest = d.get(off..)?;
        let end = rest.iter().position(|b| *b == 0)?;
        Some(String::from_utf8_lossy(&rest[..end]).into_owned())
    };
    if let Ok(d) = sys::read_file(format!("{dir}/LC_IDENTIFICATION").as_bytes()) {
        if let (Some(magic), Some(n)) = (word(&d, 0), word(&d, 4)) {
            if magic == 0x2003_1119 && 8 + n as usize * 4 <= d.len() {
                const NAMES: [&str; 14] = [
                    "title",
                    "source",
                    "address",
                    "contact",
                    "email",
                    "telephone",
                    "fax",
                    "language",
                    "territory",
                    "audience",
                    "application",
                    "abbreviation",
                    "revision",
                    "date",
                ];
                for (i, name) in NAMES.iter().enumerate() {
                    if let Some(s) = string_at(&d, i) {
                        if !s.is_empty() {
                            let _ = writeln!(out, "{name:>9} | {s}");
                        }
                    }
                }
            }
        }
    }
    if let Ok(d) = sys::read_file(format!("{dir}/LC_CTYPE").as_bytes()) {
        if let (Some(magic), Some(n)) = (word(&d, 0), word(&d, 4)) {
            if magic == 0x2009_0720 && 8 + n as usize * 4 <= d.len() {
                // _NL_CTYPE_CODESET_NAME é o item 14 do arquivo.
                if let Some(s) = string_at(&d, 14) {
                    if !s.is_empty() {
                        let _ = writeln!(out, "  codeset | {s}");
                    }
                }
            }
        }
    }
}

/// `write_locales`: os nomes dos locales (`-a`) ou, com `-v`, a descrição de cada um.
fn write_locales(out: &mut dyn Write, verbose: bool) {
    let mut all: BTreeSet<Vec<u8>> = BTreeSet::new();
    all.insert(b"POSIX".to_vec());
    all.insert(b"C".to_vec());
    let mut first_locale = true;
    let linebuf = "-".repeat(79);
    let mut dirs: Vec<Vec<u8>> = Vec::new();
    if let Ok(entries) = sys::read_dir(COMPLOCALEDIR.as_bytes()) {
        for e in entries {
            let is_dir = match e.kind {
                FileType::Directory => true,
                FileType::Symlink => {
                    let mut p = COMPLOCALEDIR.as_bytes().to_vec();
                    p.push(b'/');
                    p.extend_from_slice(&e.name);
                    sys::stat(&p).is_ok_and(|st| st.file_type() == FileType::Directory)
                }
                _ => false,
            };
            if is_dir {
                dirs.push(e.name);
            }
        }
    }
    dirs.sort();
    for name in dirs {
        let mut ident = COMPLOCALEDIR.as_bytes().to_vec();
        ident.push(b'/');
        ident.extend_from_slice(&name);
        let dir_path = String::from_utf8_lossy(&ident).into_owned();
        ident.extend_from_slice(b"/LC_IDENTIFICATION");
        if sys::stat(&ident).is_ok_and(|st| st.file_type() == FileType::Regular) {
            if verbose && !all.contains(&name) {
                if !first_locale {
                    let _ = out.write_all(b"\n");
                }
                first_locale = false;
                let shown: String = String::from_utf8_lossy(&name).chars().take(15).collect();
                let _ = write!(out, "locale: {shown:<15} directory: {dir_path}\n{linebuf}\n");
                print_locale_details(out, &dir_path);
            }
            all.insert(name);
        }
    }
    // Os apelidos de locale.alias só valem pra quem já existe.
    let alias_file = format!("{LOCALE_ALIAS_PATH}/locale.alias");
    if let Ok(data) = sys::read_file(alias_file.as_bytes()) {
        for line in data.split(|b| *b == b'\n') {
            let mut it = line.split(|b| b.is_ascii_whitespace()).filter(|t| !t.is_empty());
            let (Some(alias), Some(value)) = (it.next(), it.next()) else { continue };
            if alias.starts_with(b"#") {
                continue;
            }
            if !verbose && all.contains(value) {
                all.insert(alias.to_vec());
            }
        }
    }
    if !verbose {
        for name in &all {
            let _ = out.write_all(name);
            let _ = out.write_all(b"\n");
        }
    }
}

/// `write_charmaps`: os arquivos regulares de `CHARMAP_PATH`, sem a extensão de compressão.
fn write_charmaps(out: &mut dyn Write, argv0: &str) -> i32 {
    let entries = match sys::read_dir(CHARMAP_PATH.as_bytes()) {
        Ok(e) => e,
        Err(e) => {
            io::eprint(format!("{argv0}: [error] cannot read character map directory `{CHARMAP_PATH}': {}\n", e.message()));
            return 1;
        }
    };
    let mut names: BTreeSet<Vec<u8>> = BTreeSet::new();
    for e in entries {
        let mut p = CHARMAP_PATH.as_bytes().to_vec();
        p.push(b'/');
        p.extend_from_slice(&e.name);
        let regular = match e.kind {
            FileType::Regular => true,
            FileType::Symlink => sys::stat(&p).is_ok_and(|st| st.file_type() == FileType::Regular),
            _ => false,
        };
        if !regular {
            continue;
        }
        let mut name = e.name;
        if name.len() > 3 && name.ends_with(b".gz") {
            name.truncate(name.len() - 3);
        } else if name.len() > 4 && name.ends_with(b".bz2") {
            name.truncate(name.len() - 4);
        }
        names.insert(name);
    }
    for n in &names {
        let _ = out.write_all(n);
        let _ = out.write_all(b"\n");
    }
    0
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut out = io::stdout();
    let mut locales = Locales { cat: [Loc::C; 12], failed: false };
    let (mut show_category, mut show_keyword, mut do_all, mut do_charmaps, mut verbose) = (false, false, false, false, false);

    locales.try_one(LC_CTYPE, &argv0);
    locales.try_one(LC_MESSAGES, &argv0);

    let mut getopt = Getopt::from_env(&argv[1..], "amckv?V", LONGOPTS);
    while let Some(r) = getopt.next_opt() {
        let opt = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!(
                    "{}\nTry `locale --help' or `locale --usage' for more information.\n",
                    e.message(&argv0)
                ));
                return 64;
            }
        };
        match opt.id {
            id if id == 'a' as i32 => do_all = true,
            id if id == 'm' as i32 => do_charmaps = true,
            id if id == 'c' as i32 => show_category = true,
            id if id == 'k' as i32 => show_keyword = true,
            id if id == 'v' as i32 => verbose = true,
            id if id == '?' as i32 => {
                let _ = out.write_all(HELP.as_bytes());
                return 0;
            }
            256 => {
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            id if id == 'V' as i32 => {
                let _ = out.write_all(VERSION.as_bytes());
                return 0;
            }
            _ => {}
        }
    }
    let names = getopt.operands();

    if do_all {
        locales.diagnostics();
        locales.try_one(LC_COLLATE, &argv0);
        write_locales(&mut out, verbose);
        return 0;
    }
    if do_charmaps {
        locales.diagnostics();
        return write_charmaps(&mut out, &argv0);
    }

    locales.try_all(&argv0);
    locales.diagnostics();

    if names.is_empty() && !show_keyword && !show_category {
        show_locale_vars(&mut out);
        return 0;
    }
    for name in &names {
        if show_info(&mut out, name, &locales, show_category, show_keyword).is_err() {
            let _ = out.flush();
            io::eprint(format!("{argv0}: unknown name \"{}\"\n", io::lossy(name)));
            return 1;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysabi::Program;
    use sysabi::testkit::TestKit;

    fn kit() -> TestKit {
        TestKit::new().programs([Program::bin("locale", main)])
    }

    #[test]
    fn table_is_complete() {
        assert_eq!(ITEMS.len(), 180);
        for (name, _) in UTF8_OVERRIDES {
            assert!(ITEMS.iter().any(|i| i.1 == *name), "{name}");
        }
    }

    #[test]
    fn names_and_values() {
        let k = kit().env("LC_ALL", "C.UTF-8");
        let r = k.run(&["locale", "charmap", "decimal_point"], b"");
        assert_eq!(r.stdout_str(), "UTF-8\n.\n");
        let r = k.run(&["locale", "-ck", "LC_NUMERIC"], b"");
        assert!(r.stdout_str().starts_with("LC_NUMERIC\ndecimal_point=\".\"\nthousands_sep=\"\"\ngrouping=-1\n"));
        let r = k.run(&["locale", "bogus"], b"");
        assert_eq!((r.stderr_str().as_str(), r.code()), ("locale: unknown name \"bogus\"\n", 1));
        let r = k.run(&["locale", "-k", "ctype-class-names"], b"");
        assert!(r.stdout_str().starts_with("ctype-class-names=\"upper\";\"lower\""));
    }

    #[test]
    fn resolves_builtin_names() {
        assert!(resolve_locale(b"C").is_ok());
        assert!(resolve_locale(b"POSIX").is_ok());
        assert!(resolve_locale(b"C.UTF-8").is_ok());
        assert!(resolve_locale(b"C.utf8").is_ok());
        assert!(resolve_locale(b"C.UTF8").is_ok());
        assert!(resolve_locale(b"C.utf-8@euro").is_ok());
        assert!(resolve_locale(b"C_US.UTF-8").is_ok());
        assert!(resolve_locale(b"C.UTF-8 ").is_ok());
        assert!(resolve_locale(b"/C.utf8").is_ok());
        assert_eq!(resolve_locale(b"c.utf-8"), Err(ENOENT_MSG));
        assert_eq!(resolve_locale(b"C.UTF_8"), Err(ENOENT_MSG));
        assert_eq!(resolve_locale(b"C.U-T-F-8"), Err(ENOENT_MSG));
        assert_eq!(resolve_locale(b"C@x"), Err(ENOENT_MSG));
        assert_eq!(resolve_locale(b"POSIX.UTF-8"), Err(ENOENT_MSG));
        assert_eq!(resolve_locale(b"en_US.UTF-8"), Err(ENOENT_MSG));
        assert_eq!(resolve_locale(b"../C.utf8"), Err(EINVAL_MSG));
        assert_eq!(resolve_locale(b"C.utf8/"), Err(EINVAL_MSG));
    }
}
