//! Bits de sintaxe do GNU regex (`reg_syntax_t`, os `RE_*` do `regex.h` do glibc 2.41) e as sintaxes
//! nomeadas que grep, sed, gawk, find e companhia usam.
//!
//! Os valores numéricos são os do glibc, então dá pra conferir um a um contra o `regex.h`.

use bitflags::bitflags;

bitflags! {
    /// `reg_syntax_t`. Cada bit muda um detalhe do dialeto, exatamente como no glibc.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct Syntax: u64 {
        /// `\` dentro de colchetes escapa o caractere seguinte (awk).
        const BACKSLASH_ESCAPE_IN_LISTS = 1;
        /// `\+` e `\?` são operadores; `+` e `?` são literais (BRE).
        const BK_PLUS_QM = 1 << 1;
        /// `[[:alpha:]]` e afins são classes.
        const CHAR_CLASSES = 1 << 2;
        /// `^` e `$` são âncoras em qualquer posição (ERE).
        const CONTEXT_INDEP_ANCHORS = 1 << 3;
        /// `*`, `+`, `?` e `{` são operadores em qualquer posição; no começo são ignorados.
        const CONTEXT_INDEP_OPS = 1 << 4;
        /// Operador de repetição no começo é erro (ERE POSIX).
        const CONTEXT_INVALID_OPS = 1 << 5;
        /// `.` casa o newline.
        const DOT_NEWLINE = 1 << 6;
        /// `.` não casa o NUL.
        const DOT_NOT_NULL = 1 << 7;
        /// `[^...]` não casa o newline.
        const HAT_LISTS_NOT_NEWLINE = 1 << 8;
        /// `{n,m}` (ou `\{n,m\}`) é intervalo.
        const INTERVALS = 1 << 9;
        /// Sem `+`, `?` e `|`.
        const LIMITED_OPS = 1 << 10;
        /// Newline no padrão é alternação (grep).
        const NEWLINE_ALT = 1 << 11;
        /// `{` é intervalo e `\{` é literal.
        const NO_BK_BRACES = 1 << 12;
        /// `(` agrupa e `\(` é literal.
        const NO_BK_PARENS = 1 << 13;
        /// `\1` não é referência.
        const NO_BK_REFS = 1 << 14;
        /// `|` alterna e `\|` é literal.
        const NO_BK_VBAR = 1 << 15;
        /// `[z-a]` é erro (sem o bit, a faixa vazia é ignorada).
        const NO_EMPTY_RANGES = 1 << 16;
        /// `)` sem par é literal.
        const UNMATCHED_RIGHT_PAREN_ORD = 1 << 17;
        /// Sem efeito no glibc moderno (aceito pra compatibilidade).
        const NO_POSIX_BACKTRACKING = 1 << 18;
        /// Sem os operadores GNU (`\w \W \s \S \b \B \< \> \` \'`).
        const NO_GNU_OPS = 1 << 19;
        /// Sem efeito.
        const DEBUG = 1 << 20;
        /// `{` que não forma intervalo válido vira literal (egrep, gawk).
        const INVALID_INTERVAL_ORD = 1 << 21;
        /// Sem distinção de caixa.
        const ICASE = 1 << 22;
        /// `^` é âncora aqui (uso interno do parser, como no glibc).
        const CARET_ANCHORS_HERE = 1 << 23;
        /// Repetição depois de `\{...\}` ou no começo é erro (BRE POSIX).
        const CONTEXT_INVALID_DUP = 1 << 24;
        /// Não reportar submatches.
        const NO_SUB = 1 << 25;
    }
}

impl Syntax {
    /// `RE_SYNTAX_EMACS` (0): o padrão do `find -regex`.
    pub const EMACS: Syntax = Syntax::empty();

    /// `_RE_SYNTAX_POSIX_COMMON`.
    pub const POSIX_COMMON: Syntax = Syntax::CHAR_CLASSES
        .union(Syntax::DOT_NEWLINE)
        .union(Syntax::DOT_NOT_NULL)
        .union(Syntax::INTERVALS)
        .union(Syntax::NO_EMPTY_RANGES);

    /// `RE_SYNTAX_POSIX_BASIC` (também `RE_SYNTAX_SED` e `RE_SYNTAX_ED`).
    pub const POSIX_BASIC: Syntax =
        Syntax::POSIX_COMMON.union(Syntax::BK_PLUS_QM).union(Syntax::CONTEXT_INVALID_DUP);

    /// `RE_SYNTAX_POSIX_MINIMAL_BASIC`.
    pub const POSIX_MINIMAL_BASIC: Syntax = Syntax::POSIX_COMMON.union(Syntax::LIMITED_OPS);

    /// `RE_SYNTAX_POSIX_EXTENDED`.
    pub const POSIX_EXTENDED: Syntax = Syntax::POSIX_COMMON
        .union(Syntax::CONTEXT_INDEP_ANCHORS)
        .union(Syntax::CONTEXT_INDEP_OPS)
        .union(Syntax::NO_BK_BRACES)
        .union(Syntax::NO_BK_PARENS)
        .union(Syntax::NO_BK_VBAR)
        .union(Syntax::CONTEXT_INVALID_OPS)
        .union(Syntax::UNMATCHED_RIGHT_PAREN_ORD);

    /// `RE_SYNTAX_POSIX_MINIMAL_EXTENDED`.
    pub const POSIX_MINIMAL_EXTENDED: Syntax = Syntax::POSIX_COMMON
        .union(Syntax::CONTEXT_INDEP_ANCHORS)
        .union(Syntax::CONTEXT_INVALID_OPS)
        .union(Syntax::NO_BK_BRACES)
        .union(Syntax::NO_BK_PARENS)
        .union(Syntax::NO_BK_REFS)
        .union(Syntax::NO_BK_VBAR)
        .union(Syntax::UNMATCHED_RIGHT_PAREN_ORD);

    /// `RE_SYNTAX_SED`.
    pub const SED: Syntax = Syntax::POSIX_BASIC;

    /// `RE_SYNTAX_ED`.
    pub const ED: Syntax = Syntax::POSIX_BASIC;

    /// `RE_SYNTAX_GREP`: o `grep -G`.
    pub const GREP: Syntax = Syntax::POSIX_BASIC
        .union(Syntax::NEWLINE_ALT)
        .difference(Syntax::CONTEXT_INVALID_DUP.union(Syntax::DOT_NOT_NULL));

    /// `RE_SYNTAX_EGREP`: o `grep -E`.
    pub const EGREP: Syntax = Syntax::POSIX_EXTENDED
        .union(Syntax::INVALID_INTERVAL_ORD)
        .union(Syntax::NEWLINE_ALT)
        .difference(Syntax::CONTEXT_INVALID_OPS.union(Syntax::DOT_NOT_NULL));

    /// `RE_SYNTAX_POSIX_EGREP` (igual ao `EGREP` no glibc atual).
    pub const POSIX_EGREP: Syntax = Syntax::EGREP;

    /// `RE_SYNTAX_AWK` (awk tradicional).
    pub const AWK: Syntax = Syntax::BACKSLASH_ESCAPE_IN_LISTS
        .union(Syntax::DOT_NOT_NULL)
        .union(Syntax::NO_BK_PARENS)
        .union(Syntax::NO_BK_REFS)
        .union(Syntax::NO_BK_VBAR)
        .union(Syntax::NO_EMPTY_RANGES)
        .union(Syntax::DOT_NEWLINE)
        .union(Syntax::CONTEXT_INDEP_ANCHORS)
        .union(Syntax::CHAR_CLASSES)
        .union(Syntax::UNMATCHED_RIGHT_PAREN_ORD)
        .union(Syntax::NO_GNU_OPS);

    /// `RE_SYNTAX_GNU_AWK`: a do gawk.
    pub const GNU_AWK: Syntax = Syntax::POSIX_EXTENDED
        .union(Syntax::BACKSLASH_ESCAPE_IN_LISTS)
        .union(Syntax::INVALID_INTERVAL_ORD)
        .difference(Syntax::DOT_NOT_NULL.union(Syntax::CONTEXT_INDEP_OPS).union(Syntax::CONTEXT_INVALID_OPS));

    /// `RE_SYNTAX_POSIX_AWK`: `gawk --posix`.
    pub const POSIX_AWK: Syntax = Syntax::POSIX_EXTENDED
        .union(Syntax::BACKSLASH_ESCAPE_IN_LISTS)
        .union(Syntax::INTERVALS)
        .union(Syntax::NO_GNU_OPS)
        .union(Syntax::INVALID_INTERVAL_ORD);

    /// O `grep -E`, `sed` e `sed -E` compilam com os bits abaixo (ver `sed/regexp.c` e
    /// `grep/dfasearch.c`); estes atalhos já trazem os ajustes de cada programa.
    ///
    /// `sed` sem `-E`: `POSIX_BASIC` sem `DOT_NOT_NULL`, com `NO_POSIX_BACKTRACKING` e sem
    /// `UNMATCHED_RIGHT_PAREN_ORD` (o modo padrão, `POSIXLY_EXTENDED`).
    pub const SED_BASIC: Syntax = Syntax::POSIX_BASIC
        .difference(Syntax::DOT_NOT_NULL.union(Syntax::UNMATCHED_RIGHT_PAREN_ORD))
        .union(Syntax::NO_POSIX_BACKTRACKING);

    /// `sed -E`.
    pub const SED_EXTENDED: Syntax = Syntax::POSIX_EXTENDED
        .difference(Syntax::DOT_NOT_NULL.union(Syntax::UNMATCHED_RIGHT_PAREN_ORD))
        .union(Syntax::NO_POSIX_BACKTRACKING);

    /// Sintaxe pelo nome que o `find -regextype` aceita (findutils 4.10, `lib/regextype.c`).
    ///
    /// Devolve `None` pra nome desconhecido; a mensagem de erro é responsabilidade do `find`.
    pub fn from_regextype(name: &str) -> Option<Syntax> {
        Some(match name {
            "findutils-default" => Syntax::EMACS.union(Syntax::DOT_NEWLINE),
            "ed" => Syntax::ED,
            "emacs" => Syntax::EMACS,
            "gnu-awk" => Syntax::GNU_AWK,
            "grep" => Syntax::GREP,
            "posix-awk" => Syntax::POSIX_AWK,
            "awk" => Syntax::AWK,
            "posix-basic" => Syntax::POSIX_BASIC,
            "posix-egrep" => Syntax::POSIX_EGREP,
            "egrep" => Syntax::EGREP,
            "posix-extended" => Syntax::POSIX_EXTENDED,
            "posix-minimal-basic" => Syntax::POSIX_MINIMAL_BASIC,
            "sed" => Syntax::SED,
            _ => return None,
        })
    }

    /// Nomes aceitos por [`Syntax::from_regextype`], na ordem do `find -regextype help`.
    pub const REGEXTYPE_NAMES: &'static [&'static str] = &[
        "findutils-default",
        "ed",
        "emacs",
        "gnu-awk",
        "grep",
        "posix-awk",
        "awk",
        "posix-basic",
        "posix-egrep",
        "egrep",
        "posix-extended",
        "posix-minimal-basic",
        "sed",
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Valores numéricos conferidos com o `regex.h` (os bits são deslocamentos de 1).
    #[test]
    fn bits_match_glibc() {
        assert_eq!(Syntax::NO_SUB.bits(), 1 << 25);
        assert_eq!(Syntax::POSIX_BASIC.bits(), 0x102c4 | 0x1000000 | 0x2);
        // RE_SYNTAX_GREP do glibc: (POSIX_BASIC | NEWLINE_ALT) & ~(CONTEXT_INVALID_DUP | DOT_NOT_NULL).
        assert!(Syntax::GREP.contains(Syntax::NEWLINE_ALT | Syntax::BK_PLUS_QM | Syntax::INTERVALS));
        assert!(!Syntax::GREP.intersects(Syntax::CONTEXT_INVALID_DUP | Syntax::DOT_NOT_NULL));
        assert!(Syntax::EGREP.contains(Syntax::CONTEXT_INDEP_OPS | Syntax::INVALID_INTERVAL_ORD));
        assert!(!Syntax::EGREP.contains(Syntax::CONTEXT_INVALID_OPS));
        assert!(!Syntax::GNU_AWK.intersects(Syntax::CONTEXT_INDEP_OPS | Syntax::DOT_NOT_NULL));
        assert_eq!(Syntax::from_regextype("emacs"), Some(Syntax::EMACS));
        assert_eq!(Syntax::from_regextype("nope"), None);
    }
}
