//! Erros de compilação com as mensagens exatas do glibc 2.41 (`__re_error_msgid`) e os diagnósticos
//! do `dfa.c` que o grep e o sed também emitem.

/// Código de erro do `regcomp`/`re_compile_pattern`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// `REG_BADPAT`
    BadPattern,
    /// `REG_ECOLLATE`
    Collate,
    /// `REG_ECTYPE`
    Ctype,
    /// `REG_EESCAPE`
    Escape,
    /// `REG_ESUBREG`
    Subreg,
    /// `REG_EBRACK`
    Brack,
    /// `REG_EPAREN`
    Paren,
    /// `REG_EBRACE`
    Brace,
    /// `REG_BADBR`
    BadBrace,
    /// `REG_ERANGE`
    Range,
    /// `REG_ESPACE`
    Space,
    /// `REG_BADRPT`
    BadRepeat,
    /// `REG_EEND`
    End,
    /// `REG_ESIZE`
    Size,
    /// `REG_ERPAREN`
    RParen,
}

impl ErrorCode {
    /// Mensagem do glibc (o que `re_compile_pattern` devolve e o grep e o sed imprimem).
    pub fn message(self) -> &'static str {
        match self {
            ErrorCode::BadPattern => "Invalid regular expression",
            ErrorCode::Collate => "Invalid collation character",
            ErrorCode::Ctype => "Invalid character class name",
            ErrorCode::Escape => "Trailing backslash",
            ErrorCode::Subreg => "Invalid back reference",
            ErrorCode::Brack => "Unmatched [, [^, [:, [., or [=",
            ErrorCode::Paren => "Unmatched ( or \\(",
            ErrorCode::Brace => "Unmatched \\{",
            ErrorCode::BadBrace => "Invalid content of \\{\\}",
            ErrorCode::Range => "Invalid range end",
            ErrorCode::Space => "Memory exhausted",
            ErrorCode::BadRepeat => "Invalid preceding regular expression",
            ErrorCode::End => "Premature end of regular expression",
            ErrorCode::Size => "Regular expression too big",
            ErrorCode::RParen => "Unmatched ) or \\)",
        }
    }

    /// Número do `REG_*` (como o `regcomp` devolve).
    pub fn code(self) -> i32 {
        match self {
            ErrorCode::BadPattern => 2,
            ErrorCode::Collate => 3,
            ErrorCode::Ctype => 4,
            ErrorCode::Escape => 5,
            ErrorCode::Subreg => 6,
            ErrorCode::Brack => 7,
            ErrorCode::Paren => 8,
            ErrorCode::Brace => 9,
            ErrorCode::BadBrace => 10,
            ErrorCode::Range => 11,
            ErrorCode::Space => 12,
            ErrorCode::BadRepeat => 13,
            ErrorCode::End => 14,
            ErrorCode::Size => 15,
            ErrorCode::RParen => 16,
        }
    }
}

/// Erro de compilação de uma regex.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Erro do `regcomp` do glibc.
    Syntax(ErrorCode),
    /// `[:space:]` fora de colchetes. No grep é erro (`DFA_CONFUSING_BRACKETS_ERROR`); no sed o
    /// aviso do `dfa.c` vira erro fatal. Só aparece com [`crate::RegexBuilder::confusing_brackets_error`].
    ConfusingBrackets,
}

impl Error {
    pub fn message(&self) -> &'static str {
        match self {
            Error::Syntax(c) => c.message(),
            Error::ConfusingBrackets => CONFUSING_BRACKETS,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for Error {}

/// Mensagem do `dfa.c` pra `[:space:]` sem os colchetes externos.
pub const CONFUSING_BRACKETS: &str = "character class syntax is [[:space:]], not [:space:]";

/// Aviso do `dfa.c` sobre operador de repetição no começo da expressão (só com
/// `CONTEXT_INDEP_OPS`, ou seja, no `grep -E`). O grep imprime `grep: warning: <mensagem>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Warning {
    StarAtStart,
    QuestionAtStart,
    PlusAtStart,
    BraceAtStart,
}

impl Warning {
    pub fn message(self) -> &'static str {
        match self {
            Warning::StarAtStart => "* at start of expression",
            Warning::QuestionAtStart => "? at start of expression",
            Warning::PlusAtStart => "+ at start of expression",
            Warning::BraceAtStart => "{...} at start of expression",
        }
    }
}
