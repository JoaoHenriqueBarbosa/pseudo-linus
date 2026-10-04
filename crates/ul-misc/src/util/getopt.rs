//! `getopt_long` da glibc 2.41, com as mesmas regras e as mesmas mensagens.
//!
//! - Permutação por padrão: operandos podem vir antes, entre ou depois das opções e voltam na ordem
//!   original. Com `+` no começo da especificação curta, ou com `POSIXLY_CORRECT` no ambiente, a
//!   varredura para no primeiro operando (REQUIRE_ORDER).
//! - `--` encerra as opções; `-` sozinho é operando.
//! - Opções curtas agrupadas (`-bL`), argumento colado (`-F:`) ou no próximo argv (`-F :`); `::`
//!   marca argumento opcional, que só vale colado.
//! - Opções longas por prefixo único; prefixo de várias opções equivalentes (mesmo `has_arg` e mesmo
//!   id) escolhe a primeira, como a glibc; senão é ambíguo e a mensagem lista as possibilidades na
//!   ordem da tabela.
//! - As mensagens usam o `argv[0]` exatamente como veio (a glibc não tira o diretório).

use std::fmt;

/// Se a opção longa leva argumento.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HasArg {
    No,
    Required,
    Optional,
}

/// Uma entrada da tabela de opções longas.
#[derive(Copy, Clone, Debug)]
pub struct LongOpt {
    pub name: &'static str,
    pub has_arg: HasArg,
    /// O que [`Getopt::next`] devolve quando a opção casa: o código de uma opção curta
    /// (`'b' as i32`) ou um número próprio acima de 255 pra opções só longas.
    pub id: i32,
}

impl LongOpt {
    pub const fn new(name: &'static str, has_arg: HasArg, id: i32) -> LongOpt {
        LongOpt { name, has_arg, id }
    }
}

/// Uma opção reconhecida.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opt {
    pub id: i32,
    pub arg: Option<Vec<u8>>,
    /// Como a opção apareceu (`-b`, `--brief`), pra mensagens dos programas.
    pub spelled: String,
}

impl Opt {
    /// O id como `char`, quando é uma opção curta.
    pub fn short(&self) -> Option<char> {
        u8::try_from(self.id).ok().map(char::from)
    }

    pub fn arg_str(&self) -> String {
        self.arg.as_deref().map(|a| String::from_utf8_lossy(a).into_owned()).unwrap_or_default()
    }
}

/// Erro de `getopt`, com o texto da glibc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GetoptError {
    /// `invalid option -- 'x'`
    Invalid(char),
    /// `unrecognized option '--foo=bar'` (o texto inteiro, com `=valor`).
    Unrecognized(String),
    /// `option requires an argument -- 'x'`
    MissingShort(char),
    /// `option '--foo' requires an argument` (com o nome completo).
    MissingLong(String),
    /// `option '--foo' doesn't allow an argument` (com o nome completo).
    NoArgAllowed(String),
    /// `option '--fo' is ambiguous; possibilities: '--foo' '--fob'`
    Ambiguous { given: String, candidates: Vec<String> },
}

impl GetoptError {
    /// A linha que a glibc escreve no stderr, sem o `\n`.
    pub fn message(&self, argv0: &str) -> String {
        match self {
            GetoptError::Invalid(c) => format!("{argv0}: invalid option -- '{c}'"),
            GetoptError::Unrecognized(s) => format!("{argv0}: unrecognized option '{s}'"),
            GetoptError::MissingShort(c) => format!("{argv0}: option requires an argument -- '{c}'"),
            GetoptError::MissingLong(s) => format!("{argv0}: option '{s}' requires an argument"),
            GetoptError::NoArgAllowed(s) => format!("{argv0}: option '{s}' doesn't allow an argument"),
            GetoptError::Ambiguous { given, candidates } => {
                let mut m = format!("{argv0}: option '{given}' is ambiguous; possibilities:");
                for c in candidates {
                    m.push_str(&format!(" '{c}'"));
                }
                m
            }
        }
    }

    /// A opção curta envolvida (o `optopt` da glibc), quando há.
    pub fn optopt(&self) -> Option<char> {
        match self {
            GetoptError::Invalid(c) | GetoptError::MissingShort(c) => Some(*c),
            _ => None,
        }
    }
}

impl fmt::Display for GetoptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message("getopt"))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ShortArg {
    No,
    Required,
    Optional,
}

/// Varredura de `argv` no estilo `getopt_long`.
pub struct Getopt<'a> {
    args: Vec<Vec<u8>>,
    shorts: Vec<(char, ShortArg)>,
    longs: &'a [LongOpt],
    require_order: bool,
    /// Próximo índice de `args` a examinar.
    idx: usize,
    /// Resto de um agrupamento de curtas em curso (`-abc` depois do `a`).
    pending: Vec<u8>,
    operands: Vec<Vec<u8>>,
    done: bool,
}

impl<'a> Getopt<'a> {
    /// `args` sem o `argv[0]`. `spec` é a string de opções curtas do `getopt` (`"bF:i::"`, com `+`
    /// opcional no começo); `posixly_correct` liga o modo REQUIRE_ORDER como a variável de ambiente.
    pub fn new(args: &[Vec<u8>], spec: &str, longs: &'a [LongOpt], posixly_correct: bool) -> Getopt<'a> {
        let mut spec = spec;
        let mut require_order = posixly_correct;
        if let Some(rest) = spec.strip_prefix('+') {
            require_order = true;
            spec = rest;
        } else if let Some(rest) = spec.strip_prefix('-') {
            spec = rest;
        }
        let chars: Vec<char> = spec.chars().collect();
        let mut shorts = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            i += 1;
            if c == ':' {
                continue;
            }
            let mut kind = ShortArg::No;
            if chars.get(i) == Some(&':') {
                kind = ShortArg::Required;
                i += 1;
                if chars.get(i) == Some(&':') {
                    kind = ShortArg::Optional;
                    i += 1;
                }
            }
            shorts.push((c, kind));
        }
        Getopt {
            args: args.to_vec(),
            shorts,
            longs,
            require_order,
            idx: 0,
            pending: Vec::new(),
            operands: Vec::new(),
            done: false,
        }
    }

    /// Atalho que lê `POSIXLY_CORRECT` do ambiente do processo corrente.
    pub fn from_env(args: &[Vec<u8>], spec: &str, longs: &'a [LongOpt]) -> Getopt<'a> {
        let posix = sysabi::sys::try_current().is_some_and(|s| s.getenv(b"POSIXLY_CORRECT").is_some());
        Getopt::new(args, spec, longs, posix)
    }

    fn short_kind(&self, c: char) -> Option<ShortArg> {
        self.shorts.iter().find(|(s, _)| *s == c).map(|(_, k)| *k)
    }

    /// Próxima opção; `None` quando acabaram (aí [`Getopt::operands`] tem os operandos).
    pub fn next_opt(&mut self) -> Option<Result<Opt, GetoptError>> {
        if !self.pending.is_empty() {
            return Some(self.short_from_pending());
        }
        if self.done {
            return None;
        }
        loop {
            let Some(arg) = self.args.get(self.idx).cloned() else {
                self.done = true;
                return None;
            };
            if arg == b"--" {
                self.idx += 1;
                self.operands.extend(self.args[self.idx..].iter().cloned());
                self.idx = self.args.len();
                self.done = true;
                return None;
            }
            if arg.len() < 2 || arg[0] != b'-' {
                if self.require_order {
                    self.operands.extend(self.args[self.idx..].iter().cloned());
                    self.idx = self.args.len();
                    self.done = true;
                    return None;
                }
                self.operands.push(arg);
                self.idx += 1;
                continue;
            }
            self.idx += 1;
            if arg.starts_with(b"--") {
                return Some(self.long(&arg[2..]));
            }
            self.pending = arg[1..].to_vec();
            return Some(self.short_from_pending());
        }
    }

    fn short_from_pending(&mut self) -> Result<Opt, GetoptError> {
        // Opção curta pode ser UTF-8 de vários bytes; a glibc trabalha por byte, mas nenhuma tabela
        // nossa tem opção fora do ASCII, então um byte alto é sempre "invalid option".
        let b = self.pending.remove(0);
        let c = char::from(b);
        let Some(kind) = (if b.is_ascii() { self.short_kind(c) } else { None }) else {
            return Err(GetoptError::Invalid(c));
        };
        let spelled = format!("-{c}");
        match kind {
            ShortArg::No => Ok(Opt { id: i32::from(b), arg: None, spelled }),
            ShortArg::Optional => {
                let arg = (!self.pending.is_empty()).then(|| std::mem::take(&mut self.pending));
                Ok(Opt { id: i32::from(b), arg, spelled })
            }
            ShortArg::Required => {
                if !self.pending.is_empty() {
                    return Ok(Opt { id: i32::from(b), arg: Some(std::mem::take(&mut self.pending)), spelled });
                }
                match self.args.get(self.idx).cloned() {
                    Some(a) => {
                        self.idx += 1;
                        Ok(Opt { id: i32::from(b), arg: Some(a), spelled })
                    }
                    None => Err(GetoptError::MissingShort(c)),
                }
            }
        }
    }

    fn long(&mut self, body: &[u8]) -> Result<Opt, GetoptError> {
        let given = format!("--{}", String::from_utf8_lossy(body));
        let (name, value) = match body.iter().position(|b| *b == b'=') {
            Some(p) => (&body[..p], Some(body[p + 1..].to_vec())),
            None => (body, None),
        };
        let name = String::from_utf8_lossy(name).into_owned();
        let found = match self.longs.iter().find(|l| l.name == name) {
            Some(l) => *l,
            None => {
                let matches: Vec<&LongOpt> = self.longs.iter().filter(|l| l.name.starts_with(name.as_str())).collect();
                match matches.as_slice() {
                    [] => return Err(GetoptError::Unrecognized(given)),
                    [one] => **one,
                    [first, rest @ ..] => {
                        if rest.iter().all(|l| l.has_arg == first.has_arg && l.id == first.id) {
                            **first
                        } else {
                            let candidates = matches.iter().map(|l| format!("--{}", l.name)).collect();
                            return Err(GetoptError::Ambiguous { given, candidates });
                        }
                    }
                }
            }
        };
        let spelled = format!("--{}", found.name);
        match found.has_arg {
            HasArg::No => {
                if value.is_some() {
                    return Err(GetoptError::NoArgAllowed(spelled));
                }
                Ok(Opt { id: found.id, arg: None, spelled })
            }
            HasArg::Optional => Ok(Opt { id: found.id, arg: value, spelled }),
            HasArg::Required => {
                if value.is_some() {
                    return Ok(Opt { id: found.id, arg: value, spelled });
                }
                match self.args.get(self.idx).cloned() {
                    Some(a) => {
                        self.idx += 1;
                        Ok(Opt { id: found.id, arg: Some(a), spelled })
                    }
                    None => Err(GetoptError::MissingLong(spelled)),
                }
            }
        }
    }

    /// Operandos na ordem original. Chame depois que [`Getopt::next_opt`] devolver `None`; se a
    /// varredura parou num erro, inclui o que ainda não foi examinado.
    pub fn operands(&self) -> Vec<Vec<u8>> {
        let mut out = self.operands.clone();
        if !self.done {
            out.extend(self.args[self.idx.min(self.args.len())..].iter().cloned());
        }
        out
    }

    /// Índice (em `args`) do próximo argumento ainda não consumido: o `optind - 1` da glibc.
    pub fn index(&self) -> usize {
        self.idx
    }
}

impl Iterator for Getopt<'_> {
    type Item = Result<Opt, GetoptError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_opt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<Vec<u8>> {
        args.iter().map(|a| a.as_bytes().to_vec()).collect()
    }

    const LONGS: &[LongOpt] = &[
        LongOpt::new("mime", HasArg::No, 'i' as i32),
        LongOpt::new("mime-type", HasArg::No, 300),
        LongOpt::new("mime-encoding", HasArg::No, 301),
        LongOpt::new("separator", HasArg::Required, 'F' as i32),
        LongOpt::new("brief", HasArg::No, 'b' as i32),
        LongOpt::new("color", HasArg::Optional, 302),
    ];

    fn collect(args: &[&str], spec: &str) -> (Vec<Result<Opt, GetoptError>>, Vec<Vec<u8>>) {
        let a = v(args);
        let mut g = Getopt::new(&a, spec, LONGS, false);
        let mut out = Vec::new();
        for r in g.by_ref() {
            let stop = r.is_err();
            out.push(r);
            if stop {
                break;
            }
        }
        let ops = g.operands();
        (out, ops)
    }

    #[test]
    fn permutes_and_clusters() {
        let (opts, ops) = collect(&["x", "-bF:", "y", "--brief", "--", "-z"], "bF:");
        let ids: Vec<i32> = opts.iter().map(|o| o.as_ref().unwrap().id).collect();
        assert_eq!(ids, vec!['b' as i32, 'F' as i32, 'b' as i32]);
        assert_eq!(opts[1].as_ref().unwrap().arg.as_deref(), Some(&b":"[..]));
        assert_eq!(ops, v(&["x", "y", "-z"]));
    }

    #[test]
    fn require_order_stops_at_first_operand() {
        let (opts, ops) = collect(&["-b", "x", "-b"], "+b");
        assert_eq!(opts.len(), 1);
        assert_eq!(ops, v(&["x", "-b"]));
    }

    #[test]
    fn long_prefixes_and_errors() {
        let (opts, _) = collect(&["--sep", ","], "");
        assert_eq!(opts[0].as_ref().unwrap().arg.as_deref(), Some(&b","[..]));
        let (opts, _) = collect(&["--mim"], "");
        assert_eq!(
            opts[0].as_ref().unwrap_err().message("file"),
            "file: option '--mim' is ambiguous; possibilities: '--mime' '--mime-type' '--mime-encoding'"
        );
        let (opts, _) = collect(&["--bogus=1"], "");
        assert_eq!(opts[0].as_ref().unwrap_err().message("x"), "x: unrecognized option '--bogus=1'");
        let (opts, _) = collect(&["--separator"], "");
        assert_eq!(opts[0].as_ref().unwrap_err().message("x"), "x: option '--separator' requires an argument");
        let (opts, _) = collect(&["--br=1"], "");
        assert_eq!(opts[0].as_ref().unwrap_err().message("x"), "x: option '--brief' doesn't allow an argument");
        let (opts, _) = collect(&["-Z"], "b");
        assert_eq!(opts[0].as_ref().unwrap_err().message("x"), "x: invalid option -- 'Z'");
        let (opts, _) = collect(&["-F"], "F:");
        assert_eq!(opts[0].as_ref().unwrap_err().message("x"), "x: option requires an argument -- 'F'");
    }

    #[test]
    fn optional_arguments_only_attached() {
        let (opts, ops) = collect(&["--color", "x", "--color=never", "-ifoo"], "i::");
        assert_eq!(opts[0].as_ref().unwrap().arg, None);
        assert_eq!(opts[1].as_ref().unwrap().arg.as_deref(), Some(&b"never"[..]));
        assert_eq!(opts[2].as_ref().unwrap().arg.as_deref(), Some(&b"foo"[..]));
        assert_eq!(ops, v(&["x"]));
    }

    #[test]
    fn dash_alone_is_operand() {
        let (opts, ops) = collect(&["-", "-b"], "b");
        assert_eq!(opts.len(), 1);
        assert_eq!(ops, v(&["-"]));
    }
}
