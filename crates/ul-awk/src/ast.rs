//! Árvore do programa awk, com nomes já resolvidos pra índices (globais, locais, funções, regexes).

use std::rc::Rc;

pub type Bytes = Rc<[u8]>;

/// Referência a variável: global (índice na tabela de globais) ou local (parâmetro da função corrente).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Var {
    Global(u32),
    Local(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

/// Alvo de atribuição.
#[derive(Clone, Debug)]
pub enum LValue {
    Var(Var),
    Field(Box<Expr>),
    /// `a[i, j][k]`: cada grupo entre colchetes é um nível (arrays de arrays); o último é o elemento.
    Index(Var, Vec<Vec<Expr>>),
}

/// De onde o `getline` lê.
#[derive(Clone, Debug)]
pub enum GetlineSrc {
    /// Entrada principal.
    Main,
    /// `getline < arquivo`.
    File(Box<Expr>),
    /// `cmd | getline`.
    Cmd(Box<Expr>),
    /// `cmd |& getline`.
    Coproc(Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    And,
    Asort,
    Asorti,
    Atan2,
    Bindtextdomain,
    Close,
    Compl,
    Cos,
    Dcgettext,
    Dcngettext,
    Exp,
    Fflush,
    Gensub,
    Gsub,
    Index,
    Int,
    Isarray,
    Length,
    Log,
    Lshift,
    Match,
    Mkbool,
    Mktime,
    Or,
    Patsplit,
    Rand,
    Rshift,
    Sin,
    Split,
    Sprintf,
    Sqrt,
    Srand,
    Strftime,
    Strtonum,
    Sub,
    Substr,
    System,
    Systime,
    Tolower,
    Toupper,
    Typeof,
    Xor,
}

impl Builtin {
    pub fn from_name(name: &str) -> Option<Builtin> {
        use Builtin::*;
        Some(match name {
            "and" => And,
            "asort" => Asort,
            "asorti" => Asorti,
            "atan2" => Atan2,
            "bindtextdomain" => Bindtextdomain,
            "close" => Close,
            "compl" => Compl,
            "cos" => Cos,
            "dcgettext" => Dcgettext,
            "dcngettext" => Dcngettext,
            "exp" => Exp,
            "fflush" => Fflush,
            "gensub" => Gensub,
            "gsub" => Gsub,
            "index" => Index,
            "int" => Int,
            "isarray" => Isarray,
            "length" => Length,
            "log" => Log,
            "lshift" => Lshift,
            "match" => Match,
            "mkbool" => Mkbool,
            "mktime" => Mktime,
            "or" => Or,
            "patsplit" => Patsplit,
            "rand" => Rand,
            "rshift" => Rshift,
            "sin" => Sin,
            "split" => Split,
            "sprintf" => Sprintf,
            "sqrt" => Sqrt,
            "srand" => Srand,
            "strftime" => Strftime,
            "strtonum" => Strtonum,
            "sub" => Sub,
            "substr" => Substr,
            "system" => System,
            "systime" => Systime,
            "tolower" => Tolower,
            "toupper" => Toupper,
            "typeof" => Typeof,
            "xor" => Xor,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        use Builtin::*;
        match self {
            And => "and",
            Asort => "asort",
            Asorti => "asorti",
            Atan2 => "atan2",
            Bindtextdomain => "bindtextdomain",
            Close => "close",
            Compl => "compl",
            Cos => "cos",
            Dcgettext => "dcgettext",
            Dcngettext => "dcngettext",
            Exp => "exp",
            Fflush => "fflush",
            Gensub => "gensub",
            Gsub => "gsub",
            Index => "index",
            Int => "int",
            Isarray => "isarray",
            Length => "length",
            Log => "log",
            Lshift => "lshift",
            Match => "match",
            Mkbool => "mkbool",
            Mktime => "mktime",
            Or => "or",
            Patsplit => "patsplit",
            Rand => "rand",
            Rshift => "rshift",
            Sin => "sin",
            Split => "split",
            Sprintf => "sprintf",
            Sqrt => "sqrt",
            Srand => "srand",
            Strftime => "strftime",
            Strtonum => "strtonum",
            Sub => "sub",
            Substr => "substr",
            System => "system",
            Systime => "systime",
            Tolower => "tolower",
            Toupper => "toupper",
            Typeof => "typeof",
            Xor => "xor",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Expr {
    Num(f64),
    Str(Bytes),
    /// Regex constante em contexto de valor: casa contra `$0`.
    Regex(u32),
    /// `@/.../`.
    TypedRegex(u32),
    Var(Var),
    Field(Box<Expr>),
    /// `a[i, j][k]`: grupos de subscritos, um por nível.
    Index(Var, Vec<Vec<Expr>>),
    /// Expressão entre parênteses (o gawk distingue em alguns contextos, como `print (a)(b) > f`).
    Group(Box<Expr>),
    Assign(Box<LValue>, Box<Expr>),
    AugAssign(BinOp, Box<LValue>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Plus(Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Cmp(CmpOp, Box<Expr>, Box<Expr>),
    /// `lhs ~ re` / `lhs !~ re`; `re` é `Expr::Regex` (constante) ou qualquer expressão (dinâmica).
    Match(bool, Box<Expr>, Box<Expr>),
    Concat(Box<Expr>, Box<Expr>),
    /// `(k) in a[i]`: chave, array e o caminho até o subarray.
    In(Vec<Expr>, Var, Vec<Vec<Expr>>),
    /// Pré ou pós incremento/decremento: (alvo, é pré, delta).
    IncDec(Box<LValue>, bool, f64),
    /// Chamada de função do usuário (índice na tabela de funções).
    Call(u32, Vec<Expr>),
    /// `@nome(args)` ou `@var(args)`: o nome da função vem de uma variável.
    IndirectCall(Var, Vec<Expr>),
    Builtin(Builtin, Vec<Expr>),
    Getline(GetlineSrc, Option<Box<LValue>>),
    /// `(a, b)`: lista entre parênteses. Só existe durante o parse (vira argumentos do `print` ou o
    /// subscrito do `in`); nunca chega ao interpretador.
    List(Vec<Expr>),
}

impl Expr {
    /// É um alvo de atribuição simples (variável, campo ou elemento)?
    pub fn is_lvalue(&self) -> bool {
        matches!(self, Expr::Var(_) | Expr::Field(_) | Expr::Index(..))
    }

    /// Converte em alvo de atribuição, se for.
    pub fn into_lvalue(self) -> Result<LValue, Expr> {
        match self {
            Expr::Var(v) => Ok(LValue::Var(v)),
            Expr::Field(e) => Ok(LValue::Field(e)),
            Expr::Index(v, s) => Ok(LValue::Index(v, s)),
            other => Err(other),
        }
    }
}

/// Redirecionamento de `print`/`printf`.
#[derive(Clone, Debug)]
pub enum Redirect {
    File(Expr),
    Append(Expr),
    Pipe(Expr),
    Coproc(Expr),
}

/// Rótulo de `case`.
#[derive(Clone, Debug)]
pub enum CaseLabel {
    Num(f64),
    Str(Bytes),
    Regex(u32),
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    /// Fonte (índice) e linha, pras mensagens.
    pub src: u16,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Expr(Expr),
    Print(Vec<Expr>, Option<Redirect>),
    Printf(Vec<Expr>, Option<Redirect>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Box<Stmt>>, Box<Stmt>),
    /// `for (k in a[i])`: variável, array, caminho até o subarray, corpo.
    ForIn(Var, Var, Vec<Vec<Expr>>, Box<Stmt>),
    Block(Vec<Stmt>),
    Next,
    NextFile,
    Exit(Option<Expr>),
    Return(Option<Expr>),
    Break,
    Continue,
    /// `delete a[i][j]` (grupos de subscrito) ou `delete a` (sem grupos).
    Delete(Var, Vec<Vec<Expr>>),
    Switch(Expr, Vec<(Option<CaseLabel>, Vec<Stmt>)>),
    Nop,
}

#[derive(Clone, Debug)]
pub enum Pattern {
    All,
    Expr(Expr),
    /// Faixa: início, fim e o índice do estado da faixa.
    Range(Expr, Expr, u32),
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub pattern: Pattern,
    /// `None`: ação padrão (`print $0`).
    pub action: Option<Vec<Stmt>>,
    pub src: u16,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: Rc<str>,
    pub params: Vec<Rc<str>>,
    pub body: Vec<Stmt>,
    pub defined: bool,
    pub src: u16,
    pub line: u32,
}

/// Fonte do programa (`-f arquivo` ou `-e`/linha de comando).
#[derive(Clone, Debug)]
pub struct Source {
    /// Nome que aparece nas mensagens (`cmd. line` ou o caminho do arquivo).
    pub name: String,
    pub text: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub begin: Vec<Vec<Stmt>>,
    pub end: Vec<Vec<Stmt>>,
    pub beginfile: Vec<Vec<Stmt>>,
    pub endfile: Vec<Vec<Stmt>>,
    pub rules: Vec<Rule>,
    pub functions: Vec<Function>,
    /// Nomes das globais (o índice é o `Var::Global`).
    pub globals: Vec<Rc<str>>,
    /// Corpos das regexes constantes (o índice é o de `Expr::Regex`).
    pub regexes: Vec<Bytes>,
    /// Onde cada regex constante aparece primeiro (fonte, linha), pras mensagens de erro.
    pub regex_locs: Vec<(u16, u32)>,
    pub range_count: u32,
    pub sources: Vec<Source>,
}
