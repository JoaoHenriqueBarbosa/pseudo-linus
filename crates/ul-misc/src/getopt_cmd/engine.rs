//! Porte fiel do `_getopt_internal_r` da glibc 2.41 (`posix/getopt.c`), com permutação do argv,
//! `getopt_long` e `getopt_long_only`, o `W;` do POSIX e as mesmas mensagens no stderr.
//!
//! O `getopt(1)` precisa disso por inteiro (e não do `Getopt` de `util`) porque a tabela de opções
//! longas é montada em tempo de execução, o primeiro caractere da especificação curta pode ser `-`
//! (modo RETURN_IN_ORDER), `+` ou `:`, e a saída depende do argv permutado depois da varredura.

use crate::util::io;

/// Uma opção longa: o `struct option` com `flag != NULL` representado por um booleano.
#[derive(Clone, Debug)]
pub struct LongDef {
    pub name: Vec<u8>,
    /// 0 sem argumento, 1 obrigatório, 2 opcional.
    pub has_arg: u8,
    pub val: i32,
    /// `flag` não nulo: a varredura devolve 0 em vez de `val`.
    pub flag: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Ordering {
    RequireOrder,
    Permute,
    ReturnInOrder,
}

/// O estado do `getopt` (as variáveis globais `optind`, `optarg`... e o `struct _getopt_data`).
pub struct Engine {
    pub argv: Vec<Vec<u8>>,
    pub optind: usize,
    pub optarg: Option<Vec<u8>>,
    pub optopt: i32,
    pub opterr: bool,
    /// Índice da última opção longa reconhecida (o `longind`).
    pub longind: usize,
    first_nonopt: usize,
    last_nonopt: usize,
    nextchar: Option<Vec<u8>>,
    nc_pos: usize,
    ordering: Ordering,
    initialized: bool,
    posixly_correct_env: bool,
}

fn eprint_bytes(parts: &[&[u8]]) {
    let mut m: Vec<u8> = Vec::new();
    for p in parts {
        m.extend_from_slice(p);
    }
    io::eprint(m);
}

impl Engine {
    /// `optind` começa em 1 como a variável global (o `generate_output` do getopt(1) a zera pra
    /// reiniciar a varredura; a reinicialização é a mesma de um estado novo com 0).
    pub fn new(argv: Vec<Vec<u8>>, optind: usize, posixly_correct_env: bool) -> Engine {
        Engine {
            argv,
            optind,
            optarg: None,
            optopt: b'?' as i32,
            opterr: true,
            longind: 0,
            first_nonopt: 0,
            last_nonopt: 0,
            nextchar: None,
            nc_pos: 0,
            ordering: Ordering::Permute,
            initialized: false,
            posixly_correct_env,
        }
    }

    fn nc_empty(&self) -> bool {
        match &self.nextchar {
            None => true,
            Some(v) => self.nc_pos >= v.len(),
        }
    }

    fn nc_rest(&self) -> Vec<u8> {
        match &self.nextchar {
            None => Vec::new(),
            Some(v) => v[self.nc_pos.min(v.len())..].to_vec(),
        }
    }

    fn set_nextchar(&mut self, v: Vec<u8>) {
        self.nextchar = Some(v);
        self.nc_pos = 0;
    }

    fn nonoption(&self, i: usize) -> bool {
        let a = &self.argv[i];
        a.first() != Some(&b'-') || a.len() < 2
    }

    /// `exchange`: troca os segmentos `[first_nonopt, last_nonopt)` e `[last_nonopt, optind)`.
    fn exchange(&mut self) {
        let mut bottom = self.first_nonopt;
        let middle = self.last_nonopt;
        let mut top = self.optind;
        while top > middle && middle > bottom {
            if top - middle > middle - bottom {
                let len = middle - bottom;
                for i in 0..len {
                    self.argv.swap(bottom + i, top - (middle - bottom) + i);
                }
                top -= len;
            } else {
                let len = top - middle;
                for i in 0..len {
                    self.argv.swap(bottom + i, middle + i);
                }
                bottom += len;
            }
        }
        self.first_nonopt += self.optind - self.last_nonopt;
        self.last_nonopt = self.optind;
    }

    /// `_getopt_initialize`: devolve a especificação sem o `-`/`+` do começo.
    fn initialize<'s>(&mut self, optstring: &'s [u8]) -> &'s [u8] {
        if self.optind == 0 {
            self.optind = 1;
        }
        self.first_nonopt = self.optind;
        self.last_nonopt = self.optind;
        self.nextchar = None;
        self.nc_pos = 0;
        let mut os = optstring;
        if os.first() == Some(&b'-') {
            self.ordering = Ordering::ReturnInOrder;
            os = &os[1..];
        } else if os.first() == Some(&b'+') {
            self.ordering = Ordering::RequireOrder;
            os = &os[1..];
        } else if self.posixly_correct_env {
            self.ordering = Ordering::RequireOrder;
        } else {
            self.ordering = Ordering::Permute;
        }
        self.initialized = true;
        os
    }

    /// `process_long_option`. `nextchar` aponta pro texto depois do `--` (ou do `-`, ou do `-W `).
    fn process_long(&mut self, optstring: &[u8], longopts: &[LongDef], long_only: bool, print_errors: bool, prefix: &[u8]) -> i32 {
        let argc = self.argv.len();
        let nc = self.nc_rest();
        let namelen = nc.iter().position(|&b| b == b'=').unwrap_or(nc.len());
        let name = &nc[..namelen];

        let mut found: Option<usize> = longopts.iter().position(|p| p.name == name);

        if found.is_none() {
            let matches: Vec<usize> =
                longopts.iter().enumerate().filter(|(_, p)| p.name.starts_with(name)).map(|(i, _)| i).collect();
            if let Some(&first) = matches.first() {
                let pf = &longopts[first];
                let mut set: Vec<usize> = Vec::new();
                for &i in matches.iter().skip(1) {
                    let p = &longopts[i];
                    if long_only || pf.has_arg != p.has_arg || pf.flag != p.flag || pf.val != p.val {
                        if set.is_empty() {
                            set.push(first);
                        }
                        set.push(i);
                    }
                }
                if !set.is_empty() {
                    if print_errors {
                        let mut m: Vec<u8> = Vec::new();
                        m.extend_from_slice(&self.argv[0]);
                        m.extend_from_slice(b": option '");
                        m.extend_from_slice(prefix);
                        m.extend_from_slice(&nc);
                        m.extend_from_slice(b"' is ambiguous; possibilities:");
                        for &i in &set {
                            m.extend_from_slice(b" '");
                            m.extend_from_slice(prefix);
                            m.extend_from_slice(&longopts[i].name);
                            m.push(b'\'');
                        }
                        m.push(b'\n');
                        io::eprint(m);
                    }
                    self.nc_pos = self.nextchar.as_ref().map_or(0, Vec::len);
                    self.optind += 1;
                    self.optopt = 0;
                    return b'?' as i32;
                }
                found = Some(first);
            }
        }

        let Some(idx) = found else {
            // Não é opção longa. Sem long_only, ou com "--", ou se não é uma curta válida: erro.
            let short_ok = match nc.first() {
                Some(b) => optstring.contains(b),
                None => true,
            };
            let second_dash = self.argv.get(self.optind).is_some_and(|a| a.get(1) == Some(&b'-'));
            if !long_only || second_dash || !short_ok {
                if print_errors {
                    eprint_bytes(&[&self.argv[0], b": unrecognized option '", prefix, &nc, b"'\n"]);
                }
                self.nextchar = None;
                self.nc_pos = 0;
                self.optind += 1;
                self.optopt = 0;
                return b'?' as i32;
            }
            return -1;
        };
        let pfound = longopts[idx].clone();

        self.optind += 1;
        self.nextchar = None;
        self.nc_pos = 0;
        if namelen < nc.len() {
            if pfound.has_arg != 0 {
                self.optarg = Some(nc[namelen + 1..].to_vec());
            } else {
                if print_errors {
                    eprint_bytes(&[&self.argv[0], b": option '", prefix, &pfound.name, b"' doesn't allow an argument\n"]);
                }
                self.optopt = pfound.val;
                return b'?' as i32;
            }
        } else if pfound.has_arg == 1 {
            if self.optind < argc {
                self.optarg = Some(self.argv[self.optind].clone());
                self.optind += 1;
            } else {
                if print_errors {
                    eprint_bytes(&[&self.argv[0], b": option '", prefix, &pfound.name, b"' requires an argument\n"]);
                }
                self.optopt = pfound.val;
                return if optstring.first() == Some(&b':') { b':' as i32 } else { b'?' as i32 };
            }
        }
        self.longind = idx;
        if pfound.flag {
            return 0;
        }
        pfound.val
    }

    /// `getopt_long` (ou `getopt_long_only`): o próximo caractere de opção, 0 pra opção longa com
    /// `flag`, 1 pra operando em RETURN_IN_ORDER, `?`/`:` em erro e -1 no fim.
    pub fn getopt(&mut self, optstring_in: &[u8], longopts: &[LongDef], long_only: bool) -> i32 {
        let mut print_errors = self.opterr;
        let argc = self.argv.len();
        if argc < 1 {
            return -1;
        }
        self.optarg = None;

        let mut optstring = optstring_in;
        if self.optind == 0 || !self.initialized {
            optstring = self.initialize(optstring);
        } else if optstring.first() == Some(&b'-') || optstring.first() == Some(&b'+') {
            optstring = &optstring[1..];
        }
        if optstring.first() == Some(&b':') {
            print_errors = false;
        }

        if self.nc_empty() {
            if self.last_nonopt > self.optind {
                self.last_nonopt = self.optind;
            }
            if self.first_nonopt > self.optind {
                self.first_nonopt = self.optind;
            }

            if self.ordering == Ordering::Permute {
                if self.first_nonopt != self.last_nonopt && self.last_nonopt != self.optind {
                    self.exchange();
                } else if self.last_nonopt != self.optind {
                    self.first_nonopt = self.optind;
                }
                while self.optind < argc && self.nonoption(self.optind) {
                    self.optind += 1;
                }
                self.last_nonopt = self.optind;
            }

            if self.optind != argc && self.argv[self.optind] == b"--" {
                self.optind += 1;
                if self.first_nonopt != self.last_nonopt && self.last_nonopt != self.optind {
                    self.exchange();
                } else if self.first_nonopt == self.last_nonopt {
                    self.first_nonopt = self.optind;
                }
                self.last_nonopt = argc;
                self.optind = argc;
            }

            if self.optind == argc {
                if self.first_nonopt != self.last_nonopt {
                    self.optind = self.first_nonopt;
                }
                return -1;
            }

            if self.nonoption(self.optind) {
                if self.ordering == Ordering::RequireOrder {
                    return -1;
                }
                self.optarg = Some(self.argv[self.optind].clone());
                self.optind += 1;
                return 1;
            }

            // Pode ser uma opção longa. A tabela sempre existe no getopt(1), então não há o caso
            // `longopts == NULL` da glibc.
            let cur = self.argv[self.optind].clone();
            if cur[1] == b'-' {
                self.set_nextchar(cur[2..].to_vec());
                return self.process_long(optstring, longopts, long_only, print_errors, b"--");
            }
            if long_only && (cur.len() > 2 || !optstring.contains(&cur[1])) {
                self.set_nextchar(cur[1..].to_vec());
                let code = self.process_long(optstring, longopts, long_only, print_errors, b"-");
                if code != -1 {
                    return code;
                }
            }
            self.set_nextchar(cur[1..].to_vec());
        }

        // Próximo caractere de opção curta.
        let c = {
            let v = self.nextchar.as_ref().expect("nextchar definido");
            let b = v[self.nc_pos];
            self.nc_pos += 1;
            b
        };
        let temp = optstring.iter().position(|&b| b == c);
        if self.nc_empty() {
            self.optind += 1;
        }

        let Some(temp) = temp.filter(|_| c != b':' && c != b';') else {
            if print_errors {
                eprint_bytes(&[&self.argv[0], b": invalid option -- '", &[c], b"'\n"]);
            }
            self.optopt = i32::from(c);
            return b'?' as i32;
        };

        let after = |k: usize| optstring.get(temp + k).copied();

        // -W foo equivale a --foo (POSIX).
        if optstring[temp] == b'W' && after(1) == Some(b';') {
            if !self.nc_empty() {
                self.optarg = Some(self.nc_rest());
            } else if self.optind == argc {
                if print_errors {
                    eprint_bytes(&[&self.argv[0], b": option requires an argument -- '", &[c], b"'\n"]);
                }
                self.optopt = i32::from(c);
                return if optstring.first() == Some(&b':') { b':' as i32 } else { b'?' as i32 };
            } else {
                self.optarg = Some(self.argv[self.optind].clone());
            }
            let next = self.optarg.take().unwrap_or_default();
            self.set_nextchar(next);
            return self.process_long(optstring, longopts, false, print_errors, b"-W ");
        }

        // `char` com sinal da glibc: o byte 0xff devolvido vira -1, o EOF (o getopt(1) herda isso).
        let mut ret = i32::from(c as i8);
        if after(1) == Some(b':') {
            if after(2) == Some(b':') {
                if !self.nc_empty() {
                    self.optarg = Some(self.nc_rest());
                    self.optind += 1;
                } else {
                    self.optarg = None;
                }
                self.nextchar = None;
                self.nc_pos = 0;
            } else {
                if !self.nc_empty() {
                    self.optarg = Some(self.nc_rest());
                    self.optind += 1;
                } else if self.optind == argc {
                    if print_errors {
                        eprint_bytes(&[&self.argv[0], b": option requires an argument -- '", &[c], b"'\n"]);
                    }
                    self.optopt = i32::from(c);
                    ret = if optstring.first() == Some(&b':') { b':' as i32 } else { b'?' as i32 };
                } else {
                    self.optarg = Some(self.argv[self.optind].clone());
                    self.optind += 1;
                }
                self.nextchar = None;
                self.nc_pos = 0;
            }
        }
        ret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<Vec<u8>> {
        a.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    fn scan(args: &[&str], spec: &str) -> (Vec<i32>, Vec<String>) {
        let mut e = Engine::new(v(args), 0, false);
        let mut ids = Vec::new();
        loop {
            let r = e.getopt(spec.as_bytes(), &[], false);
            if r == -1 {
                break;
            }
            ids.push(r);
        }
        let rest = e.argv[e.optind..].iter().map(|a| String::from_utf8_lossy(a).into_owned()).collect();
        (ids, rest)
    }

    #[test]
    fn permutes_operands_to_the_end() {
        let (ids, rest) = scan(&["p", "x", "-a", "y", "-b", "--", "-z"], "ab");
        assert_eq!(ids, vec![i32::from(b'a'), i32::from(b'b')]);
        assert_eq!(rest, vec!["x", "y", "-z"]);
    }

    #[test]
    fn return_in_order_reports_operands() {
        let (ids, rest) = scan(&["p", "x", "-a"], "-a");
        assert_eq!(ids, vec![1, i32::from(b'a')]);
        assert!(rest.is_empty());
    }

    #[test]
    fn long_options_with_flag_return_zero() {
        let longs = vec![
            LongDef { name: b"alpha".to_vec(), has_arg: 0, val: 0, flag: true },
            LongDef { name: b"beta".to_vec(), has_arg: 1, val: 1, flag: true },
        ];
        let mut e = Engine::new(v(&["p", "--be", "x", "--alpha"]), 0, false);
        assert_eq!(e.getopt(b"", &longs, false), 0);
        assert_eq!(e.longind, 1);
        assert_eq!(e.optarg.as_deref(), Some(&b"x"[..]));
        assert_eq!(e.getopt(b"", &longs, false), 0);
        assert_eq!(e.longind, 0);
        assert_eq!(e.getopt(b"", &longs, false), -1);
    }
}
