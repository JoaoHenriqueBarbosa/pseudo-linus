//! `getopt_long` do glibc 2.41: permutação (opções depois de operandos), opções curtas agrupadas,
//! argumento grudado ou no elemento seguinte, prefixo único de opção longa, `--`, e as mensagens
//! de erro exatas (o chamador prefixa com o nome do programa).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HasArg {
    No,
    Required,
    Optional,
}

/// Opção longa: nome, se recebe argumento e o valor que o `getopt` devolve.
#[derive(Clone, Copy, Debug)]
pub struct LongOpt {
    pub name: &'static str,
    pub has_arg: HasArg,
    pub val: i32,
}

pub const fn long(name: &'static str, has_arg: HasArg, val: i32) -> LongOpt {
    LongOpt { name, has_arg, val }
}

/// Uma opção reconhecida: o valor (o caractere, nas curtas) e o argumento.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opt {
    pub val: i32,
    pub arg: Option<Vec<u8>>,
    /// Índice do elemento do argv onde a opção começou (pra regras como a do `-NUM` do grep).
    pub index: usize,
}

/// Iterador de opções sobre o argv (sem o `argv[0]`). Os operandos ficam em [`Getopt::operands`].
pub struct Getopt<'a> {
    args: &'a [Vec<u8>],
    short: Vec<(u8, HasArg)>,
    long: &'a [LongOpt],
    /// Para no primeiro operando (`POSIXLY_CORRECT` ou optstring com `+`).
    stop_at_operand: bool,
    i: usize,
    /// Resto do agrupamento curto corrente.
    cluster: Option<(usize, usize)>,
    pub operands: Vec<Vec<u8>>,
    done: bool,
}

impl<'a> Getopt<'a> {
    pub fn new(args: &'a [Vec<u8>], optstring: &str, long: &'a [LongOpt]) -> Getopt<'a> {
        let bytes = optstring.as_bytes();
        let stop = bytes.first() == Some(&b'+');
        let mut short = Vec::new();
        let mut k = if stop { 1 } else { 0 };
        while k < bytes.len() {
            let c = bytes[k];
            let mut has = HasArg::No;
            if bytes.get(k + 1) == Some(&b':') {
                has = HasArg::Required;
                k += 1;
                if bytes.get(k + 1) == Some(&b':') {
                    has = HasArg::Optional;
                    k += 1;
                }
            }
            short.push((c, has));
            k += 1;
        }
        Getopt { args, short, long, stop_at_operand: stop, i: 0, cluster: None, operands: Vec::new(), done: false }
    }

    /// Liga o comportamento do `POSIXLY_CORRECT`.
    pub fn posixly_correct(mut self, yes: bool) -> Getopt<'a> {
        self.stop_at_operand |= yes;
        self
    }

    fn short_spec(&self, c: u8) -> Option<HasArg> {
        self.short.iter().find(|(x, _)| *x == c).map(|(_, h)| *h)
    }

    /// Próxima opção; `Err` com a mensagem (sem o prefixo do programa) em opção inválida.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Result<Opt, String>> {
        if let Some((idx, pos)) = self.cluster.take() {
            return Some(self.short_at(idx, pos));
        }
        loop {
            if self.done || self.i >= self.args.len() {
                if !self.done {
                    self.done = true;
                }
                return None;
            }
            let idx = self.i;
            let a = &self.args[idx];
            if a == b"--" {
                self.operands.extend(self.args[idx + 1..].iter().cloned());
                self.i = self.args.len();
                self.done = true;
                return None;
            }
            if a.len() > 2 && a.starts_with(b"--") {
                self.i += 1;
                return Some(self.long_opt(idx));
            }
            if a.len() > 1 && a[0] == b'-' {
                self.i += 1;
                return Some(self.short_at(idx, 1));
            }
            self.i += 1;
            if self.stop_at_operand {
                self.operands.extend(self.args[idx..].iter().cloned());
                self.i = self.args.len();
                self.done = true;
                return None;
            }
            self.operands.push(a.clone());
        }
    }

    /// Processa todas as opções restantes e devolve os operandos (útil depois de um erro tratado).
    pub fn rest(mut self) -> Vec<Vec<u8>> {
        while let Some(r) = self.next() {
            let _ = r;
        }
        self.operands
    }

    fn short_at(&mut self, idx: usize, pos: usize) -> Result<Opt, String> {
        let a = &self.args[idx];
        let c = a[pos];
        let more = pos + 1 < a.len();
        let Some(has) = self.short_spec(c).filter(|_| c != b':') else {
            if more {
                self.cluster = Some((idx, pos + 1));
            }
            return Err(format!("invalid option -- '{}'", char_of(c)));
        };
        match has {
            HasArg::No => {
                if more {
                    self.cluster = Some((idx, pos + 1));
                }
                Ok(Opt { val: c as i32, arg: None, index: idx })
            }
            HasArg::Optional => {
                let arg = more.then(|| a[pos + 1..].to_vec());
                Ok(Opt { val: c as i32, arg, index: idx })
            }
            HasArg::Required => {
                if more {
                    return Ok(Opt { val: c as i32, arg: Some(a[pos + 1..].to_vec()), index: idx });
                }
                if self.i < self.args.len() {
                    let v = self.args[self.i].clone();
                    self.i += 1;
                    return Ok(Opt { val: c as i32, arg: Some(v), index: idx });
                }
                Err(format!("option requires an argument -- '{}'", char_of(c)))
            }
        }
    }

    fn long_opt(&mut self, idx: usize) -> Result<Opt, String> {
        let a = &self.args[idx];
        let body = &a[2..];
        let (name, value) = match body.iter().position(|&b| b == b'=') {
            Some(p) => (&body[..p], Some(body[p + 1..].to_vec())),
            None => (body, None),
        };
        let exact = self.long.iter().find(|o| o.name.as_bytes() == name);
        let found = match exact {
            Some(o) => *o,
            None => {
                let cands: Vec<&LongOpt> = self.long.iter().filter(|o| o.name.as_bytes().starts_with(name)).collect();
                match cands.as_slice() {
                    [] => return Err(format!("unrecognized option '{}'", String::from_utf8_lossy(a))),
                    [one] => **one,
                    many => {
                        // Como o glibc: ambíguo só se algum candidato difere do primeiro; a lista
                        // traz o primeiro e os que diferem dele, na ordem da tabela.
                        let first = many[0];
                        let differs = |o: &LongOpt| o.has_arg != first.has_arg || o.val != first.val;
                        if !many.iter().any(|o| differs(o)) {
                            *first
                        } else {
                            let list: Vec<String> = many
                                .iter()
                                .enumerate()
                                .filter(|(k, o)| *k == 0 || differs(o))
                                .map(|(_, o)| format!(" '--{}'", o.name))
                                .collect();
                            return Err(format!(
                                "option '--{}' is ambiguous; possibilities:{}",
                                String::from_utf8_lossy(body),
                                list.concat()
                            ));
                        }
                    }
                }
            }
        };
        match (found.has_arg, value) {
            (HasArg::No, Some(_)) => Err(format!("option '--{}' doesn't allow an argument", found.name)),
            (HasArg::No, None) => Ok(Opt { val: found.val, arg: None, index: idx }),
            (HasArg::Optional, v) => Ok(Opt { val: found.val, arg: v, index: idx }),
            (HasArg::Required, Some(v)) => Ok(Opt { val: found.val, arg: Some(v), index: idx }),
            (HasArg::Required, None) => {
                if self.i < self.args.len() {
                    let v = self.args[self.i].clone();
                    self.i += 1;
                    Ok(Opt { val: found.val, arg: Some(v), index: idx })
                } else {
                    Err(format!("option '--{}' requires an argument", found.name))
                }
            }
        }
    }
}

fn char_of(c: u8) -> String {
    if c.is_ascii() { (c as char).to_string() } else { String::from_utf8_lossy(&[c]).into_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(v: &[&str]) -> Vec<Vec<u8>> {
        v.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    const LONGS: &[LongOpt] = &[
        long("count", HasArg::No, b'c' as i32),
        long("color", HasArg::Optional, 1000),
        long("colour", HasArg::Optional, 1000),
        long("context", HasArg::Required, b'C' as i32),
    ];

    #[test]
    fn permutes_and_clusters() {
        let a = argv(&["-ne", "x", "file", "-c", "--", "-v"]);
        let mut g = Getopt::new(&a, "ne:cv", LONGS);
        let mut got = Vec::new();
        while let Some(o) = g.next() {
            got.push(o.unwrap());
        }
        assert_eq!(got.iter().map(|o| o.val as u8 as char).collect::<String>(), "nec");
        assert_eq!(got[1].arg.as_deref(), Some(&b"x"[..]));
        assert_eq!(g.operands, argv(&["file", "-v"]));
    }

    #[test]
    fn long_prefixes_and_errors() {
        let a = argv(&["--cou", "--col=always", "--con", "3"]);
        let mut g = Getopt::new(&a, "", LONGS);
        assert_eq!(g.next().unwrap().unwrap().val, b'c' as i32);
        assert_eq!(g.next().unwrap().unwrap().arg.as_deref(), Some(&b"always"[..]));
        assert_eq!(g.next().unwrap().unwrap().arg.as_deref(), Some(&b"3"[..]));
        let a = argv(&["--co"]);
        let mut g = Getopt::new(&a, "", LONGS);
        assert_eq!(
            g.next().unwrap().unwrap_err(),
            "option '--co' is ambiguous; possibilities: '--count' '--color' '--colour' '--context'"
        );
        let a = argv(&["--count=1", "-k"]);
        let mut g = Getopt::new(&a, "c", LONGS);
        assert_eq!(g.next().unwrap().unwrap_err(), "option '--count' doesn't allow an argument");
        assert_eq!(g.next().unwrap().unwrap_err(), "invalid option -- 'k'");
    }
}
