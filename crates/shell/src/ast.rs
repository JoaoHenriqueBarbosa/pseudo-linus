//! AST do interpretador.
//!
//! O brush-parser dá a estrutura de comandos com as palavras ainda cruas (`ast::Word { value }`). O
//! [`crate::lower`] converte aquilo pra esta árvore, já com cada palavra quebrada em [`Part`] pelo
//! [`crate::word`] e com o corpo de cada `$(...)` parseado de novo. O interpretador só anda nesta
//! árvore; nada aqui depende dos tipos do brush.

use std::sync::Arc;

/// Número de linha (1-based) no texto de onde o comando veio.
pub type Line = u32;

/// Um programa: a sequência de comandos completos de um trecho de texto.
#[derive(Clone, Debug, Default)]
pub struct Program {
    pub commands: Vec<List>,
}

/// Lista de and-or separados por `;`, `&` ou newline.
#[derive(Clone, Debug, Default)]
pub struct List {
    pub items: Vec<ListItem>,
}

#[derive(Clone, Debug)]
pub struct ListItem {
    pub and_or: AndOr,
    /// Terminado por `&`.
    pub background: bool,
}

#[derive(Clone, Debug)]
pub struct AndOr {
    pub first: Pipeline,
    pub rest: Vec<(Connector, Pipeline)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connector {
    And,
    Or,
}

#[derive(Clone, Debug)]
pub struct Pipeline {
    /// `!` na frente.
    pub negated: bool,
    /// `time` (com `-p` quando `posix` é verdadeiro).
    pub time: Option<TimeSpec>,
    pub commands: Vec<Command>,
    pub line: Line,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeSpec {
    pub posix: bool,
}

#[derive(Clone, Debug)]
pub enum Command {
    Simple(Arc<Simple>),
    Compound(Arc<Compound>, Arc<[Redirect]>),
    FunctionDef(Arc<FunctionDef>),
}

impl Command {
    pub fn line(&self) -> Line {
        match self {
            Command::Simple(s) => s.line,
            Command::Compound(c, _) => c.line,
            Command::FunctionDef(f) => f.line,
        }
    }
}

/// Comando simples: atribuições, palavras e redireções, cada grupo na ordem do texto.
#[derive(Clone, Debug)]
pub struct Simple {
    pub assigns: Vec<Assign>,
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
    pub line: Line,
}

/// `nome=valor`, `nome[i]=valor`, `nome+=valor`, `nome=(a b [k]=v)`.
#[derive(Clone, Debug)]
pub struct Assign {
    pub name: String,
    /// Subscript cru de `nome[...]=`, como palavra (expande como entre aspas duplas).
    pub index: Option<Word>,
    pub append: bool,
    pub value: AssignValue,
    /// Texto original (pra xtrace e `declare -f`).
    pub raw: Arc<str>,
}

#[derive(Clone, Debug)]
pub enum AssignValue {
    Scalar(Word),
    Array(Vec<ArrayElem>),
}

#[derive(Clone, Debug)]
pub struct ArrayElem {
    /// `[chave]=` explícito.
    pub key: Option<Word>,
    /// `[chave]+=valor` (bash 5.1+).
    pub append: bool,
    pub value: Word,
}

/// Uma palavra do shell: o texto original e as partes já reconhecidas.
#[derive(Clone, Debug)]
pub struct Word {
    pub raw: Arc<str>,
    pub parts: Arc<[Part]>,
    /// A palavra tem forma de atribuição (`a=b`, `a[1]=b`, `a=(...)`). Só importa como argumento de
    /// `declare`/`local`/`export`/`readonly`/`typeset`, que expandem esses argumentos como atribuição.
    pub assign: Option<Box<Assign>>,
}

/// Peça de uma palavra.
#[derive(Clone, Debug)]
pub enum Part {
    /// Texto sem aspas (sujeito a glob; vira campo junto com o resto).
    Lit(Vec<u8>),
    /// Texto que estava entre aspas ou escapado com `\`: literal, sem split nem glob.
    Quoted(Vec<u8>),
    /// Corpo cru de `$'...'` (decodificado na expansão, que conhece o locale).
    AnsiC(Vec<u8>),
    /// Trecho entre aspas duplas (só contém `Quoted`, `Param`, `CmdSub`, `Arith`).
    Double(Vec<Part>),
    Param(Box<ParamExp>),
    CmdSub(Arc<CmdSub>),
    Arith(Arc<ArithExp>),
    /// `~` ou `~nome` no início (ou depois de `:`/`=` em atribuição). O texto é o que vem depois do `~`.
    Tilde(Vec<u8>),
    /// `{a,b,c}`: cada alternativa é uma sequência de partes.
    Brace(Vec<Vec<Part>>),
    /// `{1..10..2}`, `{a..e}`, `{01..10}`.
    BraceSeq(BraceSeq),
    /// `<(...)` ou `>(...)`.
    ProcSub(Arc<ProcSub>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BraceSeq {
    /// Números com a largura de preenchimento (zero à esquerda em qualquer ponta: largura máxima).
    Num { start: i64, end: i64, step: i64, width: usize },
    Char { start: u8, end: u8, step: i64 },
}

/// `$(...)` ou crase.
#[derive(Clone, Debug)]
pub struct CmdSub {
    /// Texto do corpo, já sem o desescape da crase.
    pub src: Arc<str>,
    pub program: Program,
    pub backquote: bool,
}

/// `$(( ))` ou `$[ ]`: o texto da expressão, expandido como entre aspas duplas antes de avaliar.
#[derive(Clone, Debug)]
pub struct ArithExp {
    pub parts: Vec<Part>,
    pub raw: Arc<str>,
}

#[derive(Clone, Debug)]
pub struct ProcSub {
    /// `>(...)`: o comando lê do pipe.
    pub write: bool,
    pub body: List,
    pub src: Arc<str>,
}

/// Expansão de parâmetro.
#[derive(Clone, Debug)]
pub struct ParamExp {
    pub name: ParamName,
    pub index: Option<Index>,
    /// `${!nome}`.
    pub indirect: bool,
    pub op: ParamOp,
    /// Forma com chaves (`${x}`); `$x` é `false`.
    pub braced: bool,
    /// Texto entre as chaves (ou do `$x`), pras mensagens de erro (`${x!}: bad substitution`).
    pub raw: Arc<str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamName {
    Var(String),
    Positional(u32),
    /// `@ * # ? - $ ! 0`.
    Special(u8),
}

#[derive(Clone, Debug)]
pub enum Index {
    /// `[@]`
    At,
    /// `[*]`
    Star,
    /// `[expr]`: subscript como palavra (expande como entre aspas duplas; pra array indexado vira
    /// aritmética).
    Expr(Word),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultKind {
    /// `-`
    Use,
    /// `=`
    Assign,
    /// `?`
    Error,
    /// `+`
    Alt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplaceKind {
    First,
    All,
    Prefix,
    Suffix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseOp {
    /// `^` / `^^`
    Upper,
    /// `,` / `,,`
    Lower,
    /// `~` / `~~`
    Toggle,
}

#[derive(Clone, Debug)]
pub enum ParamOp {
    None,
    /// `${#x}`
    Length,
    /// `${x:-w}` e família; `colon` = forma com `:`.
    Default { colon: bool, kind: DefaultKind, word: Vec<Part> },
    /// `${x#p}` / `${x##p}`
    RemovePrefix { longest: bool, pattern: Vec<Part> },
    /// `${x%p}` / `${x%%p}`
    RemoveSuffix { longest: bool, pattern: Vec<Part> },
    /// `${x/p/r}` e família.
    Replace { kind: ReplaceKind, pattern: Vec<Part>, replacement: Option<Vec<Part>> },
    /// `${x:off}` / `${x:off:len}` (texto aritmético).
    Substring { offset: Vec<Part>, length: Option<Vec<Part>> },
    /// `${x^}` `${x^^}` `${x,}` `${x,,}` `${x~}` `${x~~}`, com padrão opcional.
    Case { op: CaseOp, all: bool, pattern: Option<Vec<Part>> },
    /// `${x@Q}` e afins (a letra do operador).
    Transform(u8),
    /// `${!prefixo*}` / `${!prefixo@}`
    Names { prefix: String, star: bool },
    /// `${!a[@]}` / `${!a[*]}`
    Keys { star: bool },
    /// Forma inválida: o bash só reclama ("bad substitution") quando a expansão acontece.
    Bad,
}

/// Comando composto.
#[derive(Clone, Debug)]
pub struct Compound {
    pub kind: CompoundKind,
    pub line: Line,
}

#[derive(Clone, Debug)]
pub enum CompoundKind {
    Brace(List),
    Subshell(List),
    For { var: String, words: Option<Vec<Word>>, body: List },
    Select { var: String, words: Option<Vec<Word>>, body: List },
    ArithFor { init: Option<Arc<ArithExp>>, cond: Option<Arc<ArithExp>>, step: Option<Arc<ArithExp>>, body: List },
    Case { word: Word, items: Vec<CaseItem> },
    If { branches: Vec<(List, List)>, else_body: Option<List> },
    While { cond: List, body: List },
    Until { cond: List, body: List },
    Arith(Arc<ArithExp>),
    Cond(CondExpr),
    Coproc { name: String, body: Box<Command> },
}

#[derive(Clone, Debug)]
pub struct CaseItem {
    pub patterns: Vec<Word>,
    pub body: List,
    pub term: CaseTerm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseTerm {
    /// `;;`
    Break,
    /// `;&`
    FallThrough,
    /// `;;&`
    Continue,
}

/// Expressão de `[[ ]]`.
#[derive(Clone, Debug)]
pub enum CondExpr {
    And(Box<CondExpr>, Box<CondExpr>),
    Or(Box<CondExpr>, Box<CondExpr>),
    Not(Box<CondExpr>),
    Group(Box<CondExpr>),
    /// `-f arq`, `-z s`... (o operador sem o `-`, ex.: "f", "z", "v").
    Unary(String, Word),
    Binary(CondBinOp, Word, Word),
    /// Palavra sozinha: verdade se não vazia.
    Word(Word),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CondBinOp {
    /// `==` / `=` (casamento de padrão)
    Match,
    /// `!=`
    NoMatch,
    /// `=~`
    Regex,
    Less,
    Greater,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Newer,
    Older,
    SameFile,
}

#[derive(Clone, Debug)]
pub struct FunctionDef {
    pub name: String,
    pub body: Compound,
    pub redirects: Vec<Redirect>,
    pub line: Line,
    /// Arquivo de onde veio (pro `BASH_SOURCE`); vazio no `-c`.
    pub source: Arc<str>,
}

#[derive(Clone, Debug)]
pub struct Redirect {
    pub fd: RedirFd,
    pub op: RedirOp,
    pub target: RedirTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RedirFd {
    /// Sem número: o padrão do operador (0 pra leitura, 1 pra escrita).
    Default,
    Num(i32),
    /// `{nome}>...`
    Var(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedirOp {
    /// `<`
    Read,
    /// `>`
    Write,
    /// `>>`
    Append,
    /// `<>`
    ReadWrite,
    /// `>|`
    Clobber,
    /// `<&`
    DupIn,
    /// `>&`
    DupOut,
    /// `<<` / `<<-`
    HereDoc,
    /// `<<<`
    HereString,
    /// `&>` (append `&>>`)
    OutErr { append: bool },
}

#[derive(Clone, Debug)]
pub enum RedirTarget {
    Word(Word),
    HereDoc(Arc<HereDoc>),
    ProcSub(Arc<ProcSub>),
}

#[derive(Clone, Debug)]
pub struct HereDoc {
    /// Corpo (com os tabs iniciais já tirados no `<<-`).
    pub body: Vec<u8>,
    /// Delimitador sem aspas: o corpo expande (`$x`, `$(...)`, `\$`...).
    pub expand: bool,
    /// Partes do corpo quando `expand`.
    pub parts: Vec<Part>,
    pub delimiter: String,
    pub strip_tabs: bool,
}
