//! Análise do script do sed: comandos, endereços, `s`, `y`, textos de `a/i/c`, rótulos e blocos.
//!
//! O script chega em pedaços (cada `-e`, cada `-f`, ou o operando), analisados em sequência sobre o
//! mesmo programa: um bloco aberto num `-e` fecha em outro, e `a\` no fim de um `-e` continua no
//! seguinte. A leitura é de um byte por vez com devolução, e o erro sai com a quantidade de bytes já
//! consumida (`-e expression #N, char M`) ou com a linha (`file F line L`), como o sed 4.9 mostra.

use std::sync::Arc;

use regex_posix::{Regex, RegexBuilder, Syntax};

use super::escape::{Context, convert};

/// De onde veio um pedaço do script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// `-e` (ou o operando), numerado a partir de 1.
    Expr(usize),
    File(Vec<u8>),
}

/// Erro de compilação já formatado (sem o prefixo do programa) e o código de saída.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileError {
    pub msg: Vec<u8>,
    pub status: i32,
}

/// Uma regex do script, com o que o `s` precisa saber.
#[derive(Debug)]
pub struct SedRegex {
    pub re: Regex,
    pub nsub: usize,
}

/// `None` é a regex vazia (`//`): usa a última aplicada.
pub type RegexRef = Option<Arc<SedRegex>>;

#[derive(Debug)]
pub enum Addr {
    Line(u64),
    Last,
    Re(RegexRef),
    /// `first~step`.
    Step(u64, u64),
    /// `0` (só em `0,/re/`).
    Zero,
}

#[derive(Debug)]
pub enum Addr2 {
    Line(u64),
    Last,
    Re(RegexRef),
    /// `addr1,+N`.
    Plus(u64),
    /// `addr1,~N`.
    Mult(u64),
    Step(u64, u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    Append,
    Insert,
    Change,
}

/// Parte da substituição.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repl {
    Lit(Vec<u8>),
    /// `&` (0) ou `\1` a `\9`.
    Group(usize),
    Case(CaseOp),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseOp {
    Upper,
    Lower,
    UpperOne,
    LowerOne,
    End,
}

#[derive(Debug)]
pub struct Subst {
    pub re: RegexRef,
    pub repl: Vec<Repl>,
    /// Maior referência usada (`\N`).
    pub max_ref: usize,
    pub global: bool,
    /// N-ésima ocorrência (1 se não dada).
    pub nth: u64,
    /// Bit 1: `p` antes do `e`; bit 2: `p` depois do `e`.
    pub print: u8,
    pub eval: bool,
    pub write: Option<usize>,
}

#[derive(Debug)]
pub enum Kind {
    /// `{`; `end` é o índice do `}`.
    Block { end: usize },
    BlockEnd,
    Label,
    /// `b`, `t`, `T`: alvo resolvido no fim (índice do rótulo, ou `None` = fim do script).
    Branch(Option<usize>),
    BranchSub(Option<usize>),
    BranchNoSub(Option<usize>),
    Text { kind: TextKind, text: Vec<u8> },
    Exec(Option<Vec<u8>>),
    LineNumber,
    Delete,
    DeleteFirst,
    FileName,
    Get,
    GetAppend,
    Hold,
    HoldAppend,
    List(Option<usize>),
    /// `L`: aceito na análise, aborta na execução (como o sed 4.9).
    BadL,
    Next,
    NextAppend,
    Print,
    PrintFirst,
    Quit(i32),
    QuitSilent(i32),
    ReadFile { path: Vec<u8>, prepend: bool },
    ReadLine(usize),
    Subst(Box<Subst>),
    /// Pares (origem, destino), cada um um caractere (ou byte solto).
    Translit(Vec<(Vec<u8>, Vec<u8>)>),
    Write(usize),
    WriteFirst(usize),
    Exchange,
    Zap,
    Nop,
}

#[derive(Debug)]
pub struct Cmd {
    pub a1: Option<Addr>,
    pub a2: Option<Addr2>,
    pub negate: bool,
    pub kind: Kind,
    /// Onde o comando começa (pras mensagens de execução).
    pub origin: Origin,
}

/// Arquivo de saída de `w`, `W` ou `s///w`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutTarget {
    Stdout,
    Stderr,
    File(Vec<u8>),
}

/// Opções que mudam a análise.
#[derive(Clone, Copy, Debug, Default)]
pub struct ParseOptions {
    pub extended: bool,
    /// `--posix`.
    pub posix: bool,
    /// `POSIXLY_CORRECT` no ambiente.
    pub posixly_correct: bool,
    pub sandbox: bool,
    /// `-z`.
    pub null_data: bool,
}

/// O programa compilado.
#[derive(Debug, Default)]
pub struct Program {
    pub cmds: Vec<Cmd>,
    /// Saídas de `w` na ordem em que apareceram.
    pub outputs: Vec<OutTarget>,
    /// Arquivos lidos por `R`.
    pub readers: Vec<Vec<u8>>,
    /// `#n` na primeira linha do script.
    pub quiet: bool,
    /// Último `Origin` analisado (as mensagens de execução usam o número da última expressão).
    pub last_origin: Option<Origin>,
}

/// Erro de análise ainda sem a localização.
enum Fail {
    /// Mensagem com a posição corrente do leitor.
    Here(String),
    /// Já formatado (erro sem posição: rótulo, `[:space:]`).
    Done(CompileError),
}

impl From<String> for Fail {
    fn from(s: String) -> Fail {
        Fail::Here(s)
    }
}

impl From<&str> for Fail {
    fn from(s: &str) -> Fail {
        Fail::Here(s.to_string())
    }
}

/// Leitor de um pedaço do script, com devolução de um byte.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    /// Newlines consumidos.
    lines: usize,
}

impl Reader<'_> {
    fn get(&mut self) -> Option<u8> {
        let c = self.data.get(self.pos).copied();
        if let Some(ch) = c {
            self.pos += 1;
            if ch == b'\n' {
                self.lines += 1;
            }
        }
        c
    }

    fn unget(&mut self, c: Option<u8>) {
        if let Some(ch) = c {
            self.pos -= 1;
            if ch == b'\n' {
                self.lines -= 1;
            }
        }
    }

    fn nonblank(&mut self) -> Option<u8> {
        loop {
            let c = self.get();
            if !matches!(c, Some(b' ' | b'\t')) {
                return c;
            }
        }
    }

    /// Inteiro a partir de `first` (já lido); devolve o primeiro byte que não é dígito.
    fn integer(&mut self, first: Option<u8>) -> u64 {
        let mut n: u64 = 0;
        let mut c = first;
        while let Some(d @ b'0'..=b'9') = c {
            n = n.saturating_mul(10).saturating_add((d - b'0') as u64);
            c = self.get();
        }
        self.unget(c);
        n
    }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Estado da análise entre pedaços.
pub struct Parser {
    pub prog: Program,
    opts: ParseOptions,
    /// Blocos abertos: índice do `{` e onde ele foi aberto.
    blocks: Vec<(usize, Origin, usize, usize)>,
    /// `a\` no fim de um pedaço: o texto continua no próximo.
    pending_text: Option<usize>,
    labels: Vec<(Vec<u8>, usize)>,
    jumps: Vec<(usize, Vec<u8>)>,
    first_chunk: bool,
    expr_count: usize,
    hook: Arc<dyn Fn() + Send + Sync>,
}

impl Parser {
    pub fn new(opts: ParseOptions, hook: Arc<dyn Fn() + Send + Sync>) -> Parser {
        Parser {
            prog: Program::default(),
            opts,
            blocks: Vec::new(),
            pending_text: None,
            labels: Vec::new(),
            jumps: Vec::new(),
            first_chunk: true,
            expr_count: 0,
            hook,
        }
    }

    /// Número da próxima expressão `-e`.
    pub fn next_expr(&mut self) -> Origin {
        self.expr_count += 1;
        Origin::Expr(self.expr_count)
    }

    fn locate(origin: &Origin, r: &Reader<'_>, msg: &str) -> CompileError {
        let text = match origin {
            Origin::Expr(n) => format!("-e expression #{n}, char {}: {msg}", r.pos),
            Origin::File(name) => {
                let mut v = b"file ".to_vec();
                v.extend_from_slice(name);
                v.extend_from_slice(format!(" line {}: {msg}", r.lines + 1).as_bytes());
                return CompileError { msg: v, status: 1 };
            }
        };
        CompileError { msg: text.into_bytes(), status: 1 }
    }

    /// Analisa um pedaço do script.
    pub fn chunk(&mut self, origin: Origin, data: &[u8]) -> Result<(), CompileError> {
        let mut r = Reader { data, pos: 0, lines: 0 };
        if self.first_chunk {
            self.first_chunk = false;
            if data.starts_with(b"#n") {
                self.prog.quiet = true;
            }
        }
        self.prog.last_origin = Some(origin.clone());
        match self.commands(&origin, &mut r) {
            Ok(()) => Ok(()),
            Err(Fail::Here(msg)) => Err(Parser::locate(&origin, &r, &msg)),
            Err(Fail::Done(e)) => Err(e),
        }
    }

    /// Fim do script: blocos abertos e rótulos.
    pub fn finish(mut self) -> Result<Program, CompileError> {
        if let Some((_, origin, _, lines)) = self.blocks.pop() {
            // O sed relata o `{` sem par na linha onde ele abriu (arquivo) ou com `char 0` (-e).
            let r = Reader { data: &[], pos: 0, lines };
            return Err(Parser::locate(&origin, &r, "unmatched `{'"));
        }
        for (idx, name) in std::mem::take(&mut self.jumps) {
            let target = if name.is_empty() {
                None
            } else {
                match self.labels.iter().find(|(l, _)| *l == name) {
                    Some((_, i)) => Some(*i),
                    None => {
                        let mut msg = b"can't find label for jump to `".to_vec();
                        msg.extend_from_slice(&name);
                        msg.push(b'\'');
                        return Err(CompileError { msg, status: 4 });
                    }
                }
            };
            match &mut self.prog.cmds[idx].kind {
                Kind::Branch(t) | Kind::BranchSub(t) | Kind::BranchNoSub(t) => *t = target,
                _ => {}
            }
        }
        Ok(self.prog)
    }

    fn commands(&mut self, origin: &Origin, r: &mut Reader<'_>) -> Result<(), Fail> {
        if let Some(idx) = self.pending_text.take() {
            let text = self.read_text(r, Lead::Newline, idx)?;
            if let Some(t) = text {
                self.set_text(idx, t);
            }
        }
        loop {
            (self.hook)();
            let mut c = r.get();
            while matches!(c, Some(ch) if ch == b';' || is_space(ch)) {
                c = r.get();
            }
            let Some(_) = c else { return Ok(()) };
            self.command(origin, r, c)?;
        }
    }

    fn set_text(&mut self, idx: usize, t: Vec<u8>) {
        if let Kind::Text { text, .. } | Kind::Exec(Some(text)) = &mut self.prog.cmds[idx].kind {
            *text = t;
        }
    }

    fn gnu(&self) -> bool {
        !self.opts.posix
    }

    /// Endereço começando em `c`; `None` se `c` não começa endereço (nada é consumido além de `c`).
    fn address(&mut self, r: &mut Reader<'_>, c: Option<u8>) -> Result<Option<AddrRaw>, Fail> {
        match c {
            Some(b'/') | Some(b'\\') => {
                let delim = if c == Some(b'\\') { r.get() } else { c };
                let Some(delim) = delim else { return Err("unterminated address regex".into()) };
                let Some(text) = self.delimited(r, delim, true)? else {
                    return Err("unterminated address regex".into());
                };
                let (mut icase, mut multi) = (false, false);
                loop {
                    let f = r.nonblank();
                    match f {
                        Some(b'I') if self.gnu() => icase = true,
                        Some(b'M') if self.gnu() => multi = true,
                        _ => {
                            r.unget(f);
                            break;
                        }
                    }
                }
                let re = self.compile_regex(&text, icase, multi, 0)?;
                Ok(Some(AddrRaw::Re(re)))
            }
            Some(d @ b'0'..=b'9') => {
                let n = r.integer(Some(d));
                let c2 = r.nonblank();
                if c2 == Some(b'~') && self.gnu() {
                    let first = r.nonblank();
                    let step = r.integer(first);
                    if step > 0 {
                        return Ok(Some(AddrRaw::Step(n, step)));
                    }
                    return Ok(Some(AddrRaw::Line(n)));
                }
                r.unget(c2);
                Ok(Some(AddrRaw::Line(n)))
            }
            Some(op @ (b'+' | b'~')) if self.gnu() => {
                let first = r.nonblank();
                let n = r.integer(first);
                Ok(Some(if op == b'+' { AddrRaw::Plus(n) } else { AddrRaw::Mult(n) }))
            }
            Some(b'$') => Ok(Some(AddrRaw::Last)),
            _ => Ok(None),
        }
    }

    fn command(&mut self, origin: &Origin, r: &mut Reader<'_>, first: Option<u8>) -> Result<(), Fail> {
        let mut a1 = None;
        let mut a2 = None;
        let mut c = first;
        if let Some(raw) = self.address(r, first)? {
            if matches!(raw, AddrRaw::Plus(_) | AddrRaw::Mult(_)) {
                return Err("invalid usage of +N or ~N as first address".into());
            }
            a1 = Some(raw);
            c = r.nonblank();
            if c == Some(b',') {
                let n = r.nonblank();
                match self.address(r, n)? {
                    Some(x) => a2 = Some(x),
                    None => return Err("unexpected `,'".into()),
                }
                c = r.nonblank();
            }
            let zero = matches!(a1, Some(AddrRaw::Line(0)));
            if zero
                && ((a2.is_none() && c != Some(b'r'))
                    || (a2.is_some() && !matches!(a2, Some(AddrRaw::Re(_))))
                    || self.opts.posix)
            {
                return Err("invalid usage of line address 0".into());
            }
        }
        let mut negate = false;
        if c == Some(b'!') {
            negate = true;
            c = r.nonblank();
            if c == Some(b'!') {
                return Err("multiple `!'s".into());
            }
        }
        if self.opts.posix {
            match c {
                Some(b'e' | b'F' | b'v' | b'z' | b'L' | b'Q' | b'T' | b'R' | b'W') => {
                    return Err(format!("unknown command: `{}'", c.unwrap_or(b'?') as char).into());
                }
                Some(b'a' | b'i' | b'l' | b'=' | b'r') if a2.is_some() => {
                    return Err("command only uses one address".into());
                }
                _ => {}
            }
        }
        let has_a1 = a1.is_some();
        let a1 = a1.map(|x| x.into_addr1());
        let a2 = a2.map(|x| x.into_addr2());
        let idx = self.prog.cmds.len();
        let kind = match c {
            None => return Err("missing command".into()),
            Some(b'#') => {
                if has_a1 {
                    return Err("comments don't accept any addresses".into());
                }
                loop {
                    match r.get() {
                        None | Some(b'\n') => break,
                        _ => {}
                    }
                }
                return Ok(());
            }
            Some(b'v') => {
                let v = self.label(r);
                let wanted = if v.is_empty() { b"4.0".to_vec() } else { v };
                if version_cmp(&wanted, b"4.9") == std::cmp::Ordering::Greater {
                    return Err("expected newer version of sed".into());
                }
                return Ok(());
            }
            Some(b'{') => {
                self.blocks.push((idx, origin.clone(), r.pos, r.lines));
                Kind::Block { end: usize::MAX }
            }
            Some(b'}') => {
                let Some((open, ..)) = self.blocks.pop() else {
                    return Err("unexpected `}'".into());
                };
                if has_a1 {
                    return Err("`}' doesn't want any addresses".into());
                }
                self.end_of_cmd(r)?;
                if let Kind::Block { end } = &mut self.prog.cmds[open].kind {
                    *end = idx;
                }
                Kind::BlockEnd
            }
            Some(b'e') => {
                if self.opts.sandbox {
                    return Err("e/r/w commands disabled in sandbox mode".into());
                }
                let c2 = r.nonblank();
                if matches!(c2, None | Some(b'\n')) {
                    Kind::Exec(None)
                } else {
                    let lead = self.text_lead(r, c2)?;
                    self.push(a1, a2, negate, Kind::Exec(Some(Vec::new())), origin);
                    if let Some(t) = self.read_text(r, lead, idx)? {
                        self.set_text(idx, t);
                    }
                    return Ok(());
                }
            }
            Some(ch @ (b'a' | b'i' | b'c')) => {
                let c2 = r.nonblank();
                if c2.is_none() {
                    return Err("expected \\ after `a', `c' or `i'".into());
                }
                let lead = self.text_lead(r, c2)?;
                let kind = match ch {
                    b'a' => TextKind::Append,
                    b'i' => TextKind::Insert,
                    _ => TextKind::Change,
                };
                self.push(a1, a2, negate, Kind::Text { kind, text: Vec::new() }, origin);
                if let Some(t) = self.read_text(r, lead, idx)? {
                    self.set_text(idx, t);
                }
                return Ok(());
            }
            Some(b':') => {
                if has_a1 {
                    return Err(": doesn't want any addresses".into());
                }
                let label = self.label(r);
                if label.is_empty() {
                    return Err("\":\" lacks a label".into());
                }
                self.labels.push((label, idx));
                Kind::Label
            }
            Some(ch @ (b'b' | b't' | b'T')) => {
                let label = self.label(r);
                self.jumps.push((idx, label));
                match ch {
                    b'b' => Kind::Branch(None),
                    b't' => Kind::BranchSub(None),
                    _ => Kind::BranchNoSub(None),
                }
            }
            Some(ch @ (b'q' | b'Q' | b'l' | b'L')) => {
                if matches!(ch, b'q' | b'Q') && a2.is_some() {
                    return Err("command only uses one address".into());
                }
                let c2 = r.nonblank();
                let n = match c2 {
                    Some(d @ b'0'..=b'9') if self.gnu() => Some(r.integer(Some(d))),
                    _ => {
                        r.unget(c2);
                        None
                    }
                };
                self.end_of_cmd(r)?;
                match ch {
                    b'q' => Kind::Quit(n.unwrap_or(0) as i32),
                    b'Q' => Kind::QuitSilent(n.unwrap_or(0) as i32),
                    b'l' => Kind::List(n.map(|v| v as usize)),
                    _ => Kind::BadL,
                }
            }
            Some(ch @ (b'=' | b'd' | b'D' | b'F' | b'g' | b'G' | b'h' | b'H' | b'n' | b'N' | b'p' | b'P' | b'z' | b'x')) => {
                self.end_of_cmd(r)?;
                match ch {
                    b'=' => Kind::LineNumber,
                    b'd' => Kind::Delete,
                    b'D' => Kind::DeleteFirst,
                    b'F' => Kind::FileName,
                    b'g' => Kind::Get,
                    b'G' => Kind::GetAppend,
                    b'h' => Kind::Hold,
                    b'H' => Kind::HoldAppend,
                    b'n' => Kind::Next,
                    b'N' => Kind::NextAppend,
                    b'p' => Kind::Print,
                    b'P' => Kind::PrintFirst,
                    b'z' => Kind::Zap,
                    _ => Kind::Exchange,
                }
            }
            Some(b'r') => {
                let path = self.filename(r)?;
                // `0r` insere antes da primeira linha.
                let prepend = matches!(a1, Some(Addr::Zero)) && a2.is_none();
                let a1 = if prepend { Some(Addr::Line(1)) } else { a1 };
                self.push(a1, a2, negate, Kind::ReadFile { path, prepend }, origin);
                return Ok(());
            }
            Some(b'R') => {
                let path = self.filename(r)?;
                let id = match self.prog.readers.iter().position(|p| *p == path) {
                    Some(i) => i,
                    None => {
                        self.prog.readers.push(path);
                        self.prog.readers.len() - 1
                    }
                };
                Kind::ReadLine(id)
            }
            Some(ch @ (b'w' | b'W')) => {
                let id = self.output(r)?;
                if ch == b'w' { Kind::Write(id) } else { Kind::WriteFirst(id) }
            }
            Some(b's') => Kind::Subst(Box::new(self.subst(r)?)),
            Some(b'y') => Kind::Translit(self.translit(r)?),
            Some(other) => return Err(format!("unknown command: `{}'", printable(other)).into()),
        };
        self.push(a1, a2, negate, kind, origin);
        Ok(())
    }

    fn push(&mut self, a1: Option<Addr>, a2: Option<Addr2>, negate: bool, kind: Kind, origin: &Origin) {
        self.prog.cmds.push(Cmd { a1, a2, negate, kind, origin: origin.clone() });
    }

    /// Fim de comando: `;`, newline, fim; `}` e `#` ficam pro próximo.
    fn end_of_cmd(&mut self, r: &mut Reader<'_>) -> Result<(), Fail> {
        let c = r.nonblank();
        match c {
            Some(b'}') | Some(b'#') => {
                r.unget(c);
                Ok(())
            }
            None | Some(b'\n') | Some(b';') => Ok(()),
            _ => Err("extra characters after command".into()),
        }
    }

    /// Rótulo de `:`, `b`, `t`, `T` (e versão do `v`): até espaço, `;`, `}` ou fim de linha.
    fn label(&mut self, r: &mut Reader<'_>) -> Vec<u8> {
        let mut out = Vec::new();
        let mut c = r.nonblank();
        while let Some(ch) = c {
            if ch == b'\n' || ch == b' ' || ch == b'\t' || ch == b';' || ch == b'}' {
                break;
            }
            out.push(ch);
            c = r.get();
        }
        r.unget(c);
        out
    }

    /// Nome de arquivo de `r`, `R`, `w`, `W`: o resto da linha.
    fn filename(&mut self, r: &mut Reader<'_>) -> Result<Vec<u8>, Fail> {
        if self.opts.sandbox {
            return Err("e/r/w commands disabled in sandbox mode".into());
        }
        let mut out = Vec::new();
        let mut c = r.nonblank();
        while let Some(ch) = c {
            if ch == b'\n' {
                break;
            }
            out.push(ch);
            c = r.get();
        }
        if out.is_empty() {
            return Err("missing filename in r/R/w/W commands".into());
        }
        Ok(out)
    }

    /// Saída de `w`/`W`/`s///w`: o arquivo é criado (truncado) já na análise.
    fn output(&mut self, r: &mut Reader<'_>) -> Result<usize, Fail> {
        let path = self.filename(r)?;
        let target = match path.as_slice() {
            b"/dev/stdout" if self.gnu() => OutTarget::Stdout,
            b"/dev/stderr" if self.gnu() => OutTarget::Stderr,
            _ => OutTarget::File(path),
        };
        if let Some(i) = self.prog.outputs.iter().position(|t| *t == target) {
            return Ok(i);
        }
        self.prog.outputs.push(target);
        Ok(self.prog.outputs.len() - 1)
    }

    /// Como o texto de `a`, `i`, `c`, `e` começa, a partir do primeiro byte não branco `c2`.
    fn text_lead(&mut self, r: &mut Reader<'_>, c2: Option<u8>) -> Result<Lead, Fail> {
        if c2 == Some(b'\\') {
            return Ok(match r.get() {
                None => Lead::Eof,
                Some(b'\n') => Lead::Newline,
                Some(x) => Lead::Byte(x),
            });
        }
        if self.opts.posix {
            return Err("expected \\ after `a', `c' or `i'".into());
        }
        r.unget(c2);
        Ok(Lead::Newline)
    }

    /// Lê o texto de `a`, `i`, `c` ou `e`. `None`: o texto continua no próximo pedaço.
    fn read_text(&mut self, r: &mut Reader<'_>, lead: Lead, idx: usize) -> Result<Option<Vec<u8>>, Fail> {
        let mut buf = Vec::new();
        match lead {
            Lead::Eof => {
                self.pending_text = Some(idx);
                return Ok(None);
            }
            Lead::Byte(b) => buf.push(b),
            Lead::Newline => {}
        }
        loop {
            match r.get() {
                None | Some(b'\n') => break,
                Some(b'\\') => match r.get() {
                    // Barra no fim do script: some e o texto acaba.
                    None => break,
                    Some(x) => {
                        buf.push(b'\\');
                        buf.push(x);
                    }
                },
                Some(x) => buf.push(x),
            }
        }
        let mut text = convert(&buf, Context::Text);
        text.push(b'\n');
        Ok(Some(text))
    }

    /// Lê até o delimitador. Na regex, colchetes protegem o delimitador; `\delim` vira o
    /// delimitador; `\n` vira newline; barra seguida de newline vira newline. `None`: não fechou.
    fn delimited(&mut self, r: &mut Reader<'_>, delim: u8, regex: bool) -> Result<Option<Vec<u8>>, Fail> {
        let mut out = Vec::new();
        loop {
            let c = r.get();
            match c {
                None => return Ok(None),
                Some(b'\n') => {
                    r.unget(c);
                    return Ok(None);
                }
                Some(ch) if ch == delim => return Ok(Some(out)),
                Some(b'\\') => {
                    let c2 = r.get();
                    match c2 {
                        None => return Ok(None),
                        Some(x) if x == delim => {
                            if !regex && x == b'&' {
                                out.push(b'\\');
                            }
                            out.push(x);
                        }
                        Some(b'\n') => out.push(b'\n'),
                        Some(b'n') if regex => out.push(b'\n'),
                        Some(x) => {
                            out.push(b'\\');
                            out.push(x);
                        }
                    }
                }
                Some(b'[') if regex => {
                    out.push(b'[');
                    if !self.bracket(r, &mut out) {
                        return Ok(None);
                    }
                }
                Some(ch) => out.push(ch),
            }
        }
    }

    /// Copia uma expressão de colchetes até o `]` que a fecha (o primeiro `]`, depois de `^`
    /// opcional, é literal; `[:...:]`, `[.  .]` e `[= =]` são atravessados). `false` se chegou
    /// ao fim da linha ou do script.
    fn bracket(&mut self, r: &mut Reader<'_>, out: &mut Vec<u8>) -> bool {
        let mut c = r.get();
        if c == Some(b'^') {
            out.push(b'^');
            c = r.get();
        }
        if c == Some(b']') {
            out.push(b']');
            c = r.get();
        }
        loop {
            match c {
                None | Some(b'\n') => {
                    r.unget(c);
                    return false;
                }
                Some(b']') => {
                    out.push(b']');
                    return true;
                }
                Some(b'[') => {
                    out.push(b'[');
                    let d = r.get();
                    match d {
                        Some(delim @ (b':' | b'.' | b'=')) => {
                            out.push(delim);
                            // Até `delim]`.
                            loop {
                                let x = r.get();
                                match x {
                                    None | Some(b'\n') => {
                                        r.unget(x);
                                        return false;
                                    }
                                    Some(ch) => {
                                        out.push(ch);
                                        if ch == delim {
                                            let y = r.get();
                                            if y == Some(b']') {
                                                out.push(b']');
                                                break;
                                            }
                                            r.unget(y);
                                        }
                                    }
                                }
                            }
                            c = r.get();
                            continue;
                        }
                        _ => {
                            c = d;
                            continue;
                        }
                    }
                }
                Some(ch) => {
                    out.push(ch);
                    c = r.get();
                }
            }
        }
    }

    /// Compila uma regex do script. Texto vazio é a regex vazia (a última usada).
    fn compile_regex(&mut self, text: &[u8], icase: bool, multi: bool, refs: usize) -> Result<RegexRef, Fail> {
        if text.is_empty() {
            if icase || multi {
                return Err("cannot specify modifiers on empty regexp".into());
            }
            return Ok(None);
        }
        let pattern = convert(text, Context::Regex);
        let mut syntax = if self.opts.extended { Syntax::SED_EXTENDED } else { Syntax::SED_BASIC };
        if self.opts.posixly_correct || self.opts.posix {
            syntax |= Syntax::UNMATCHED_RIGHT_PAREN_ORD;
        }
        if self.opts.posix {
            syntax |= Syntax::NO_GNU_OPS;
            if !self.opts.extended {
                syntax |= Syntax::LIMITED_OPS;
            }
        }
        if multi {
            syntax = syntax.difference(Syntax::DOT_NEWLINE) | Syntax::HAT_LISTS_NOT_NEWLINE;
        }
        let mut b = RegexBuilder::new(syntax)
            .icase(icase)
            .confusing_brackets_error(true)
            .checkpoint(self.hook.clone());
        if multi {
            b = if self.opts.null_data { b.line_separator(Some(0)) } else { b.newline_anchor(true) };
        }
        let re = match b.build(&pattern) {
            Ok(re) => re,
            Err(regex_posix::Error::ConfusingBrackets) => {
                return Err(Fail::Done(CompileError { msg: regex_posix::CONFUSING_BRACKETS.as_bytes().to_vec(), status: 4 }));
            }
            Err(e) => return Err(e.message().into()),
        };
        let nsub = re.group_count();
        if refs > nsub {
            return Err(format!("invalid reference \\{refs} on `s' command's RHS").into());
        }
        Ok(Some(Arc::new(SedRegex { re, nsub })))
    }

    fn subst(&mut self, r: &mut Reader<'_>) -> Result<Subst, Fail> {
        let Some(delim) = r.get() else { return Err("unterminated `s' command".into()) };
        let Some(re_text) = self.delimited(r, delim, true)? else {
            return Err("unterminated `s' command".into());
        };
        let Some(rep_text) = self.delimited(r, delim, false)? else {
            return Err("unterminated `s' command".into());
        };
        let (repl, max_ref) = self.replacement(&rep_text);
        let mut s = Subst { re: None, repl, max_ref, global: false, nth: 0, print: 0, eval: false, write: None };
        let (mut icase, mut multi) = (false, false);
        loop {
            let c = r.nonblank();
            match c {
                Some(b'i' | b'I') if self.gnu() => icase = true,
                Some(b'm' | b'M') if self.gnu() => multi = true,
                Some(b'e') if self.gnu() => {
                    if self.opts.sandbox {
                        return Err("e/r/w commands disabled in sandbox mode".into());
                    }
                    s.eval = true;
                }
                Some(b'p') => {
                    if s.print != 0 {
                        return Err("multiple `p' options to `s' command".into());
                    }
                    s.print = if s.eval { 2 } else { 1 };
                }
                Some(b'g') => {
                    if s.global {
                        return Err("multiple `g' options to `s' command".into());
                    }
                    s.global = true;
                }
                Some(b'w') => {
                    s.write = Some(self.output(r)?);
                    break;
                }
                Some(d @ b'0'..=b'9') => {
                    if s.nth != 0 {
                        return Err("multiple number options to `s' command".into());
                    }
                    s.nth = r.integer(Some(d));
                    if s.nth == 0 {
                        return Err("number option to `s' command may not be zero".into());
                    }
                }
                Some(b'}') | Some(b'#') => {
                    r.unget(c);
                    break;
                }
                None | Some(b'\n') | Some(b';') => break,
                Some(b'\r') => {
                    if r.get() == Some(b'\n') {
                        break;
                    }
                    return Err("unknown option to `s'".into());
                }
                _ => return Err("unknown option to `s'".into()),
            }
        }
        if s.nth == 0 {
            s.nth = 1;
        }
        s.re = self.compile_regex(&re_text, icase, multi, max_ref)?;
        Ok(s)
    }

    /// Analisa a substituição: `&`, `\0`..`\9`, `\L \U \E \l \u`, `\&`, `\\`, `\X` = X.
    fn replacement(&self, raw: &[u8]) -> (Vec<Repl>, usize) {
        let text = convert(raw, Context::Replacement);
        let mut parts = Vec::new();
        let mut lit = Vec::new();
        let mut max_ref = 0;
        let flush = |lit: &mut Vec<u8>, parts: &mut Vec<Repl>| {
            if !lit.is_empty() {
                parts.push(Repl::Lit(std::mem::take(lit)));
            }
        };
        let mut i = 0;
        while i < text.len() {
            let c = text[i];
            if c == b'\\' {
                if i + 1 >= text.len() {
                    lit.push(b'\\');
                    i += 1;
                    continue;
                }
                let e = text[i + 1];
                i += 2;
                match e {
                    b'0'..=b'9' => {
                        flush(&mut lit, &mut parts);
                        let n = (e - b'0') as usize;
                        max_ref = max_ref.max(n);
                        parts.push(Repl::Group(n));
                    }
                    b'L' | b'U' | b'E' | b'l' | b'u' if self.gnu() => {
                        flush(&mut lit, &mut parts);
                        parts.push(Repl::Case(match e {
                            b'L' => CaseOp::Lower,
                            b'U' => CaseOp::Upper,
                            b'E' => CaseOp::End,
                            b'l' => CaseOp::LowerOne,
                            _ => CaseOp::UpperOne,
                        }));
                    }
                    other => lit.push(other),
                }
            } else if c == b'&' {
                flush(&mut lit, &mut parts);
                parts.push(Repl::Group(0));
                i += 1;
            } else {
                lit.push(c);
                i += 1;
            }
        }
        flush(&mut lit, &mut parts);
        (parts, max_ref)
    }

    fn translit(&mut self, r: &mut Reader<'_>) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Fail> {
        let Some(delim) = r.get() else { return Err("unterminated `y' command".into()) };
        let Some(src) = self.delimited(r, delim, false)? else {
            return Err("unterminated `y' command".into());
        };
        let Some(dst) = self.delimited(r, delim, false)? else {
            return Err("unterminated `y' command".into());
        };
        let src = split_chars(&convert(&src, Context::Translit));
        let dst = split_chars(&convert(&dst, Context::Translit));
        if src.len() != dst.len() {
            return Err("strings for `y' command are different lengths".into());
        }
        self.end_of_cmd(r)?;
        Ok(src.into_iter().zip(dst).collect())
    }
}

/// Divide em caracteres UTF-8 (byte inválido conta como um caractere, como no sed).
pub fn split_chars(s: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let len = match regex_posix::nfa::decode_at(s, i) {
            Some((_, l)) => l,
            None => 1,
        };
        out.push(s[i..i + len].to_vec());
        i += len;
    }
    out
}

fn printable(c: u8) -> String {
    (c as char).to_string()
}

/// Comparação de versões no estilo do `strverscmp` (só números e pontos importam aqui).
fn version_cmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let parts = |s: &[u8]| -> Vec<u64> {
        String::from_utf8_lossy(s).split('.').map(|p| p.trim().parse::<u64>().unwrap_or(0)).collect()
    };
    let (pa, pb) = (parts(a), parts(b));
    for i in 0..pa.len().max(pb.len()) {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        if x != y {
            return x.cmp(&y);
        }
    }
    std::cmp::Ordering::Equal
}

#[derive(Clone, Copy)]
enum Lead {
    /// `a\` no fim do pedaço.
    Eof,
    Newline,
    Byte(u8),
}

/// Endereço antes de saber se é o primeiro ou o segundo.
enum AddrRaw {
    Line(u64),
    Last,
    Re(RegexRef),
    Step(u64, u64),
    Plus(u64),
    Mult(u64),
}

impl AddrRaw {
    fn into_addr1(self) -> Addr {
        match self {
            AddrRaw::Line(0) => Addr::Zero,
            AddrRaw::Line(n) => Addr::Line(n),
            AddrRaw::Last => Addr::Last,
            AddrRaw::Re(r) => Addr::Re(r),
            AddrRaw::Step(a, b) => Addr::Step(a, b),
            AddrRaw::Plus(_) | AddrRaw::Mult(_) => Addr::Line(u64::MAX),
        }
    }

    fn into_addr2(self) -> Addr2 {
        match self {
            AddrRaw::Line(n) => Addr2::Line(n),
            AddrRaw::Last => Addr2::Last,
            AddrRaw::Re(r) => Addr2::Re(r),
            AddrRaw::Step(a, b) => Addr2::Step(a, b),
            AddrRaw::Plus(n) => Addr2::Plus(n),
            AddrRaw::Mult(n) => Addr2::Mult(n),
        }
    }
}
