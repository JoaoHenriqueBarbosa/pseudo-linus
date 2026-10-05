//! Tipos de token do CPython 3.13, com os mesmos nomes e números de `Lib/token.py` (gerado a partir
//! de `Grammar/Tokens`), e a tabela `EXACT_TOKEN_TYPES` dos operadores.

/// Tipo de token; o discriminante é o número que `token.py` atribui.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum TokenType {
    Endmarker = 0,
    Name = 1,
    Number = 2,
    String = 3,
    Newline = 4,
    Indent = 5,
    Dedent = 6,
    Lpar = 7,
    Rpar = 8,
    Lsqb = 9,
    Rsqb = 10,
    Colon = 11,
    Comma = 12,
    Semi = 13,
    Plus = 14,
    Minus = 15,
    Star = 16,
    Slash = 17,
    Vbar = 18,
    Amper = 19,
    Less = 20,
    Greater = 21,
    Equal = 22,
    Dot = 23,
    Percent = 24,
    Lbrace = 25,
    Rbrace = 26,
    EqEqual = 27,
    NotEqual = 28,
    LessEqual = 29,
    GreaterEqual = 30,
    Tilde = 31,
    Circumflex = 32,
    LeftShift = 33,
    RightShift = 34,
    DoubleStar = 35,
    PlusEqual = 36,
    MinEqual = 37,
    StarEqual = 38,
    SlashEqual = 39,
    PercentEqual = 40,
    AmperEqual = 41,
    VbarEqual = 42,
    CircumflexEqual = 43,
    LeftShiftEqual = 44,
    RightShiftEqual = 45,
    DoubleStarEqual = 46,
    DoubleSlash = 47,
    DoubleSlashEqual = 48,
    At = 49,
    AtEqual = 50,
    Rarrow = 51,
    Ellipsis = 52,
    ColonEqual = 53,
    Exclamation = 54,
    Op = 55,
    TypeIgnore = 56,
    TypeComment = 57,
    SoftKeyword = 58,
    FstringStart = 59,
    FstringMiddle = 60,
    FstringEnd = 61,
    Comment = 62,
    Nl = 63,
    ErrorToken = 64,
    Encoding = 65,
}

/// `N_TOKENS` de `token.py`.
pub const N_TOKENS: u16 = 66;
/// `NT_OFFSET` de `token.py`: números a partir daqui são não-terminais.
pub const NT_OFFSET: u16 = 256;

/// Todos os tipos, na ordem numérica (o índice é o número).
pub const ALL: [TokenType; N_TOKENS as usize] = {
    use TokenType::*;
    [
        Endmarker, Name, Number, String, Newline, Indent, Dedent, Lpar, Rpar, Lsqb, Rsqb, Colon,
        Comma, Semi, Plus, Minus, Star, Slash, Vbar, Amper, Less, Greater, Equal, Dot, Percent,
        Lbrace, Rbrace, EqEqual, NotEqual, LessEqual, GreaterEqual, Tilde, Circumflex, LeftShift,
        RightShift, DoubleStar, PlusEqual, MinEqual, StarEqual, SlashEqual, PercentEqual,
        AmperEqual, VbarEqual, CircumflexEqual, LeftShiftEqual, RightShiftEqual, DoubleStarEqual,
        DoubleSlash, DoubleSlashEqual, At, AtEqual, Rarrow, Ellipsis, ColonEqual, Exclamation, Op,
        TypeIgnore, TypeComment, SoftKeyword, FstringStart, FstringMiddle, FstringEnd, Comment,
        Nl, ErrorToken, Encoding,
    ]
};

/// `EXACT_TOKEN_TYPES` de `token.py`, ordenado pelo texto do operador.
pub const EXACT_TOKEN_TYPES: [(&str, TokenType); 48] = {
    use TokenType::*;
    [
        ("!", Exclamation),
        ("!=", NotEqual),
        ("%", Percent),
        ("%=", PercentEqual),
        ("&", Amper),
        ("&=", AmperEqual),
        ("(", Lpar),
        (")", Rpar),
        ("*", Star),
        ("**", DoubleStar),
        ("**=", DoubleStarEqual),
        ("*=", StarEqual),
        ("+", Plus),
        ("+=", PlusEqual),
        (",", Comma),
        ("-", Minus),
        ("-=", MinEqual),
        ("->", Rarrow),
        (".", Dot),
        ("...", Ellipsis),
        ("/", Slash),
        ("//", DoubleSlash),
        ("//=", DoubleSlashEqual),
        ("/=", SlashEqual),
        (":", Colon),
        (":=", ColonEqual),
        (";", Semi),
        ("<", Less),
        ("<<", LeftShift),
        ("<<=", LeftShiftEqual),
        ("<=", LessEqual),
        ("=", Equal),
        ("==", EqEqual),
        (">", Greater),
        (">=", GreaterEqual),
        (">>", RightShift),
        (">>=", RightShiftEqual),
        ("@", At),
        ("@=", AtEqual),
        ("[", Lsqb),
        ("]", Rsqb),
        ("^", Circumflex),
        ("^=", CircumflexEqual),
        ("{", Lbrace),
        ("|", Vbar),
        ("|=", VbarEqual),
        ("}", Rbrace),
        ("~", Tilde),
    ]
};

impl TokenType {
    /// Número do tipo (o valor da constante em `token.py`).
    pub fn number(self) -> u16 {
        self as u16
    }

    /// Tipo pelo número, se existir.
    pub fn from_number(n: u16) -> Option<TokenType> {
        ALL.get(n as usize).copied()
    }

    /// Nome em `token.tok_name` (ex.: `"NOTEQUAL"`).
    pub fn name(self) -> &'static str {
        TOK_NAME[self as usize]
    }

    /// Tipo pelo nome de `tok_name`.
    pub fn from_name(name: &str) -> Option<TokenType> {
        TOK_NAME.iter().position(|n| *n == name).map(|i| ALL[i])
    }

    /// Texto fixo do operador, quando o tipo tem um só (inverso de `EXACT_TOKEN_TYPES`).
    pub fn exact_text(self) -> Option<&'static str> {
        EXACT_TOKEN_TYPES.iter().find(|(_, t)| *t == self).map(|(s, _)| *s)
    }
}

/// `token.tok_name`, indexado pelo número.
const TOK_NAME: [&str; N_TOKENS as usize] = [
    "ENDMARKER", "NAME", "NUMBER", "STRING", "NEWLINE", "INDENT", "DEDENT", "LPAR", "RPAR",
    "LSQB", "RSQB", "COLON", "COMMA", "SEMI", "PLUS", "MINUS", "STAR", "SLASH", "VBAR", "AMPER",
    "LESS", "GREATER", "EQUAL", "DOT", "PERCENT", "LBRACE", "RBRACE", "EQEQUAL", "NOTEQUAL",
    "LESSEQUAL", "GREATEREQUAL", "TILDE", "CIRCUMFLEX", "LEFTSHIFT", "RIGHTSHIFT", "DOUBLESTAR",
    "PLUSEQUAL", "MINEQUAL", "STAREQUAL", "SLASHEQUAL", "PERCENTEQUAL", "AMPEREQUAL",
    "VBAREQUAL", "CIRCUMFLEXEQUAL", "LEFTSHIFTEQUAL", "RIGHTSHIFTEQUAL", "DOUBLESTAREQUAL",
    "DOUBLESLASH", "DOUBLESLASHEQUAL", "AT", "ATEQUAL", "RARROW", "ELLIPSIS", "COLONEQUAL",
    "EXCLAMATION", "OP", "TYPE_IGNORE", "TYPE_COMMENT", "SOFT_KEYWORD", "FSTRING_START",
    "FSTRING_MIDDLE", "FSTRING_END", "COMMENT", "NL", "ERRORTOKEN", "ENCODING",
];

/// Tipo exato do operador `text` (`EXACT_TOKEN_TYPES[text]`).
pub fn exact_type(text: &str) -> Option<TokenType> {
    EXACT_TOKEN_TYPES.binary_search_by(|(s, _)| s.cmp(&text)).ok().map(|i| EXACT_TOKEN_TYPES[i].1)
}

/// `token.ISTERMINAL`.
pub fn is_terminal(x: u16) -> bool {
    x < NT_OFFSET
}

/// `token.ISNONTERMINAL`.
pub fn is_nonterminal(x: u16) -> bool {
    x >= NT_OFFSET
}

/// `token.ISEOF`.
pub fn is_eof(x: u16) -> bool {
    x == TokenType::Endmarker as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_names_match_token_py() {
        for (i, t) in ALL.iter().enumerate() {
            assert_eq!(t.number() as usize, i);
            assert_eq!(TokenType::from_name(t.name()), Some(*t));
        }
        assert_eq!(TokenType::Op.name(), "OP");
        assert_eq!(TokenType::Op.number(), 55);
        assert_eq!(TokenType::Encoding.number(), 65);
        assert_eq!(TokenType::from_number(N_TOKENS), None);
    }

    #[test]
    fn exact_types_sorted_and_lookup() {
        for w in EXACT_TOKEN_TYPES.windows(2) {
            assert!(w[0].0 < w[1].0);
        }
        assert_eq!(exact_type("**="), Some(TokenType::DoubleStarEqual));
        assert_eq!(exact_type("!"), Some(TokenType::Exclamation));
        assert_eq!(exact_type("<>"), None);
        assert_eq!(TokenType::Rarrow.exact_text(), Some("->"));
        assert_eq!(TokenType::Name.exact_text(), None);
        assert!(is_eof(0) && is_terminal(255) && is_nonterminal(256));
    }
}
