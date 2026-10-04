//! `getopt_long` com a semântica do glibc 2.41, escrito a partir do getopt(3) e do comportamento
//! observado no oráculo: permutação (operandos podem vir antes das opções), `--` encerra as opções,
//! `-` sozinho é operando, opções curtas agrupadas (`-cvzf`), longas com `--nome=valor` ou valor no
//! argumento seguinte, abreviação única de opção longa, e as mensagens de erro exatas.
//!
//! É um iterador: o programa trata cada opção na ordem em que aparece e para no primeiro erro, como os
//! programas GNU fazem com o `?` do getopt.

/// Se a opção leva argumento.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HasArg {
    No,
    Required,
    /// Só na forma colada: `-xVALOR` ou `--nome=VALOR`.
    Optional,
}

/// Uma opção longa. A ordem da tabela importa: é a ordem em que o glibc lista as possibilidades de uma
/// abreviação ambígua, então cada programa deve declarar a tabela na mesma ordem do programa GNU.
#[derive(Clone, Copy, Debug)]
pub struct LongOpt {
    pub name: &'static str,
    pub has_arg: HasArg,
    /// Identificador devolvido em [`Opt::id`]. Pode ser o caractere da curta equivalente.
    pub id: u32,
}

impl LongOpt {
    pub const fn new(name: &'static str, has_arg: HasArg, id: u32) -> LongOpt {
        LongOpt { name, has_arg, id }
    }
}

/// Uma opção reconhecida.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opt {
    /// Caractere da curta (`'u' as u32`) ou o `id` da longa.
    pub id: u32,
    pub arg: Option<Vec<u8>>,
    /// Índice em argv do elemento em que a opção apareceu (o argumento separado, quando há, está em
    /// `index + 1`).
    pub index: usize,
    /// Nome da longa como está na tabela; `None` pra curta.
    pub long: Option<&'static str>,
}

/// Item devolvido pelo iterador.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Opt(Opt),
    Operand(Vec<u8>),
}

/// Erro de linha de comando, com a mensagem do glibc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// `invalid option -- 'c'`.
    InvalidShort(u8),
    /// `option requires an argument -- 'c'`.
    ShortNeedsArg(u8),
    /// `unrecognized option '--texto'` (o texto inclui `=valor`, como o glibc mostra).
    UnrecognizedLong(Vec<u8>),
    /// `option '--nome' requires an argument`.
    LongNeedsArg(&'static str),
    /// `option '--nome' doesn't allow an argument`.
    LongNoArg(&'static str),
    /// `option '--texto' is ambiguous; possibilities: '--a' '--b'`.
    Ambiguous { given: Vec<u8>, candidates: Vec<&'static str> },
}

impl Error {
    /// Linha de erro completa (com `argv0: ` na frente e `\n` no fim), como o glibc escreve.
    pub fn message(&self, argv0: &str) -> String {
        match self {
            Error::InvalidShort(c) => format!("{argv0}: invalid option -- '{}'\n", byte_char(*c)),
            Error::ShortNeedsArg(c) => format!("{argv0}: option requires an argument -- '{}'\n", byte_char(*c)),
            Error::UnrecognizedLong(t) => {
                format!("{argv0}: unrecognized option '--{}'\n", String::from_utf8_lossy(t))
            }
            Error::LongNeedsArg(n) => format!("{argv0}: option '--{n}' requires an argument\n"),
            Error::LongNoArg(n) => format!("{argv0}: option '--{n}' doesn't allow an argument\n"),
            Error::Ambiguous { given, candidates } => {
                let mut s =
                    format!("{argv0}: option '--{}' is ambiguous; possibilities:", String::from_utf8_lossy(given));
                for c in candidates {
                    s.push_str(&format!(" '--{c}'"));
                }
                s.push('\n');
                s
            }
        }
    }

    /// Mesma mensagem em bytes (o caractere inválido pode não ser UTF-8).
    pub fn message_bytes(&self, argv0: &str) -> Vec<u8> {
        match self {
            Error::InvalidShort(c) => {
                let mut v = format!("{argv0}: invalid option -- '").into_bytes();
                v.push(*c);
                v.extend_from_slice(b"'\n");
                v
            }
            Error::ShortNeedsArg(c) => {
                let mut v = format!("{argv0}: option requires an argument -- '").into_bytes();
                v.push(*c);
                v.extend_from_slice(b"'\n");
                v
            }
            Error::UnrecognizedLong(t) => {
                let mut v = format!("{argv0}: unrecognized option '--").into_bytes();
                v.extend_from_slice(t);
                v.extend_from_slice(b"'\n");
                v
            }
            other => other.message(argv0).into_bytes(),
        }
    }
}

fn byte_char(c: u8) -> char {
    if c.is_ascii() { c as char } else { char::REPLACEMENT_CHARACTER }
}

#[derive(Clone, Copy, Debug)]
enum ShortKind {
    Flag,
    Required,
    Optional,
}

/// O analisador. Construa com [`Getopt::new`] e itere.
pub struct Getopt<'a> {
    args: &'a [Vec<u8>],
    shorts: Vec<(u8, ShortKind)>,
    longs: &'a [LongOpt],
    /// Para no primeiro operando (`+` no começo de `shortopts`, ou `POSIXLY_CORRECT`).
    stop_at_operand: bool,
    /// Próximo elemento de argv a examinar.
    next: usize,
    /// Dentro de um grupo de curtas: (índice do elemento, posição do próximo caractere).
    cluster: Option<(usize, usize)>,
    /// Depois de `--` (ou do primeiro operando com `stop_at_operand`): tudo é operando.
    only_operands: bool,
    failed: bool,
}

impl<'a> Getopt<'a> {
    /// `args` é o argv completo (argv[0] incluído). `shortopts` segue o getopt(3): `"ab:c::"`; um `+`
    /// no começo desliga a permutação. `posixly_correct` deve vir de `POSIXLY_CORRECT` no ambiente.
    pub fn new(args: &'a [Vec<u8>], shortopts: &str, longs: &'a [LongOpt], posixly_correct: bool) -> Getopt<'a> {
        let mut spec = shortopts.as_bytes();
        let mut stop_at_operand = posixly_correct;
        while let Some((&c, rest)) = spec.split_first() {
            match c {
                b'+' => stop_at_operand = true,
                b'-' | b':' => {}
                _ => break,
            }
            spec = rest;
        }
        let mut shorts = Vec::new();
        let mut i = 0;
        while i < spec.len() {
            let c = spec[i];
            let mut kind = ShortKind::Flag;
            if spec.get(i + 1) == Some(&b':') {
                kind = ShortKind::Required;
                i += 1;
                if spec.get(i + 1) == Some(&b':') {
                    kind = ShortKind::Optional;
                    i += 1;
                }
            }
            shorts.push((c, kind));
            i += 1;
        }
        Getopt { args, shorts, longs, stop_at_operand, next: 1, cluster: None, only_operands: false, failed: false }
    }

    /// Como [`Getopt::new`], lendo `POSIXLY_CORRECT` do ambiente do processo corrente.
    pub fn from_env(args: &'a [Vec<u8>], shortopts: &str, longs: &'a [LongOpt]) -> Getopt<'a> {
        let posix = sysabi::sys::try_current().is_some_and(|s| s.getenv(b"POSIXLY_CORRECT").is_some());
        Getopt::new(args, shortopts, longs, posix)
    }

    /// Índice do próximo elemento de argv ainda não consumido (o `optind`, sem a permutação).
    pub fn optind(&self) -> usize {
        self.next
    }

    fn short_kind(&self, c: u8) -> Option<ShortKind> {
        if c == b':' {
            return None;
        }
        self.shorts.iter().find(|(s, _)| *s == c).map(|(_, k)| *k)
    }

    fn long(&mut self, index: usize) -> Result<Item, Error> {
        let body = &self.args[index][2..];
        let (name, value) = match body.iter().position(|&b| b == b'=') {
            Some(p) => (&body[..p], Some(body[p + 1..].to_vec())),
            None => (body, None),
        };
        let exact = self.longs.iter().find(|l| l.name.as_bytes() == name);
        let found = match exact {
            Some(l) => *l,
            None => {
                let mut first: Option<LongOpt> = None;
                let mut ambiguous = Vec::<usize>::new();
                let mut first_idx = 0;
                for (k, l) in self.longs.iter().enumerate() {
                    if !l.name.as_bytes().starts_with(name) {
                        continue;
                    }
                    match first {
                        None => {
                            first = Some(*l);
                            first_idx = k;
                        }
                        Some(f) => {
                            if f.has_arg != l.has_arg || f.id != l.id {
                                if ambiguous.is_empty() {
                                    ambiguous.push(first_idx);
                                }
                                ambiguous.push(k);
                            }
                        }
                    }
                }
                if !ambiguous.is_empty() {
                    let candidates = ambiguous.iter().map(|&k| self.longs[k].name).collect();
                    return Err(Error::Ambiguous { given: body.to_vec(), candidates });
                }
                match first {
                    Some(f) if !name.is_empty() => f,
                    _ => return Err(Error::UnrecognizedLong(body.to_vec())),
                }
            }
        };
        let arg = match (found.has_arg, value) {
            (HasArg::No, Some(_)) => return Err(Error::LongNoArg(found.name)),
            (HasArg::No, None) => None,
            (HasArg::Optional, v) => v,
            (HasArg::Required, Some(v)) => Some(v),
            (HasArg::Required, None) => match self.args.get(self.next) {
                Some(v) => {
                    self.next += 1;
                    Some(v.clone())
                }
                None => return Err(Error::LongNeedsArg(found.name)),
            },
        };
        Ok(Item::Opt(Opt { id: found.id, arg, index, long: Some(found.name) }))
    }

    fn short(&mut self, index: usize, pos: usize) -> Result<Item, Error> {
        let arg = &self.args[index];
        let c = arg[pos];
        let rest_start = pos + 1;
        let has_rest = rest_start < arg.len();
        self.cluster = if has_rest { Some((index, rest_start)) } else { None };
        let Some(kind) = self.short_kind(c) else {
            return Err(Error::InvalidShort(c));
        };
        let value = match kind {
            ShortKind::Flag => None,
            ShortKind::Optional => {
                self.cluster = None;
                has_rest.then(|| arg[rest_start..].to_vec())
            }
            ShortKind::Required => {
                self.cluster = None;
                if has_rest {
                    Some(arg[rest_start..].to_vec())
                } else {
                    match self.args.get(self.next) {
                        Some(v) => {
                            self.next += 1;
                            Some(v.clone())
                        }
                        None => return Err(Error::ShortNeedsArg(c)),
                    }
                }
            }
        };
        Ok(Item::Opt(Opt { id: c as u32, arg: value, index, long: None }))
    }
}

impl Iterator for Getopt<'_> {
    type Item = Result<Item, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        let r = if let Some((index, pos)) = self.cluster {
            self.short(index, pos)
        } else {
            let index = self.next;
            let arg = self.args.get(index)?;
            self.next += 1;
            if self.only_operands {
                return Some(Ok(Item::Operand(arg.clone())));
            }
            if arg.as_slice() == b"--" {
                self.only_operands = true;
                return self.next();
            }
            if arg.len() > 2 && arg.starts_with(b"--") {
                self.long(index)
            } else if arg.len() > 1 && arg[0] == b'-' {
                self.short(index, 1)
            } else {
                if self.stop_at_operand {
                    self.only_operands = true;
                }
                return Some(Ok(Item::Operand(arg.clone())));
            }
        };
        if r.is_err() {
            self.failed = true;
        }
        Some(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(v: &[&str]) -> Vec<Vec<u8>> {
        v.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    const LONGS: &[LongOpt] = &[
        LongOpt::new("brief", HasArg::No, b'q' as u32),
        LongOpt::new("label", HasArg::Required, 300),
        LongOpt::new("side-by-side", HasArg::No, b'y' as u32),
        LongOpt::new("speed-large-files", HasArg::No, 301),
        LongOpt::new("unified", HasArg::Optional, 302),
    ];

    fn collect(v: &[&str]) -> Vec<Result<Item, Error>> {
        let a = argv(v);
        Getopt::new(&a, "quU:y", LONGS, false).collect()
    }

    #[test]
    fn permutes_clusters_and_values() {
        let r = collect(&["diff", "a", "-quU5", "--label=x", "--label", "y", "b", "--", "-c"]);
        let ids: Vec<String> = r
            .iter()
            .map(|i| match i {
                Ok(Item::Opt(o)) => format!("{}:{:?}", o.id, o.arg.as_ref().map(|a| String::from_utf8_lossy(a).into_owned())),
                Ok(Item::Operand(o)) => format!("op:{}", String::from_utf8_lossy(o)),
                Err(e) => format!("err:{e:?}"),
            })
            .collect();
        assert_eq!(
            ids,
            vec!["op:a", "113:None", "117:None", "85:Some(\"5\")", "300:Some(\"x\")", "300:Some(\"y\")", "op:b", "op:-c"]
        );
    }

    #[test]
    fn abbreviation_and_ambiguity() {
        let r = collect(&["diff", "--br", "--s", "x"]);
        assert!(matches!(&r[0], Ok(Item::Opt(o)) if o.id == b'q' as u32));
        let e = r[1].clone().unwrap_err();
        assert_eq!(
            e.message("diff"),
            "diff: option '--s' is ambiguous; possibilities: '--side-by-side' '--speed-large-files'\n"
        );
        assert_eq!(r.len(), 2, "para depois do erro");
    }

    #[test]
    fn glibc_messages() {
        let one = |v: &[&str]| collect(v).into_iter().find_map(|r| r.err()).unwrap().message("diff");
        assert_eq!(one(&["diff", "--brief=3"]), "diff: option '--brief' doesn't allow an argument\n");
        assert_eq!(one(&["diff", "--label"]), "diff: option '--label' requires an argument\n");
        assert_eq!(one(&["diff", "-U"]), "diff: option requires an argument -- 'U'\n");
        assert_eq!(one(&["diff", "-k"]), "diff: invalid option -- 'k'\n");
        assert_eq!(one(&["diff", "--bogus=1"]), "diff: unrecognized option '--bogus=1'\n");
    }

    #[test]
    fn optional_argument_only_attached() {
        let r = collect(&["diff", "--unified", "3", "--unified=4"]);
        assert!(matches!(&r[0], Ok(Item::Opt(o)) if o.arg.is_none()));
        assert!(matches!(&r[1], Ok(Item::Operand(o)) if o == b"3"));
        assert!(matches!(&r[2], Ok(Item::Opt(o)) if o.arg.as_deref() == Some(&b"4"[..])));
    }

    #[test]
    fn posix_stops_at_first_operand() {
        let a = argv(&["x", "a", "-q"]);
        let r: Vec<_> = Getopt::new(&a, "q", LONGS, true).collect();
        assert!(matches!(&r[1], Ok(Item::Operand(o)) if o == b"-q"));
        let r: Vec<_> = Getopt::new(&a, "+q", LONGS, false).collect();
        assert!(matches!(&r[1], Ok(Item::Operand(o)) if o == b"-q"));
    }
}
