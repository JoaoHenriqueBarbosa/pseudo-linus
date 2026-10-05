//! `test` e `[`: as regras do POSIX por número de argumentos e, acima de 4, a gramática com
//! `!`, `-a`, `-o` e parênteses.

use crate::cond::{file_compare, unary_test};
use crate::shell::{Exec, Shell};

const UNARY: &[&str] = &[
    "-a", "-b", "-c", "-d", "-e", "-f", "-g", "-h", "-k", "-p", "-r", "-s", "-t", "-u", "-w", "-x", "-G", "-L", "-N", "-O",
    "-S", "-z", "-n", "-o", "-v", "-R",
];
const BINARY: &[&str] = &["=", "==", "!=", "<", ">", "-eq", "-ne", "-lt", "-le", "-gt", "-ge", "-nt", "-ot", "-ef", "-a", "-o"];

struct Test<'a> {
    sh: &'a mut Shell,
    pos: usize,
}

type R = Result<bool, String>;

fn is_unary(a: &[u8]) -> bool {
    UNARY.iter().any(|u| u.as_bytes() == a)
}

fn is_binary(a: &[u8]) -> bool {
    BINARY.iter().any(|u| u.as_bytes() == a)
}

/// Inteiro do `test`: brancos em volta, sinal opcional, só dígitos.
fn test_int(v: &[u8]) -> Result<i64, String> {
    let s = String::from_utf8_lossy(v);
    let t = s.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\n');
    let (neg, d) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if d.is_empty() || !d.bytes().all(|c| c.is_ascii_digit()) {
        return Err(format!("{s}: integer expression expected"));
    }
    let mut n: i64 = 0;
    for c in d.bytes() {
        n = n.wrapping_mul(10).wrapping_add((c - b'0') as i64);
    }
    Ok(if neg { n.wrapping_neg() } else { n })
}

impl Test<'_> {
    fn unary(&mut self, op: &[u8], arg: &[u8]) -> R {
        let o = String::from_utf8_lossy(&op[1..]).into_owned();
        Ok(unary_test(self.sh, &o, arg).unwrap_or(false))
    }

    fn binary(&mut self, a: &[u8], op: &[u8], b: &[u8]) -> R {
        Ok(match op {
            b"=" | b"==" => a == b,
            b"!=" => a != b,
            b"<" => a < b,
            b">" => a > b,
            b"-a" => !a.is_empty() && !b.is_empty(),
            b"-o" => !a.is_empty() || !b.is_empty(),
            b"-nt" | b"-ot" | b"-ef" => file_compare(&String::from_utf8_lossy(&op[1..]), a, b).unwrap_or(false),
            _ => {
                let x = test_int(a)?;
                let y = test_int(b)?;
                match op {
                    b"-eq" => x == y,
                    b"-ne" => x != y,
                    b"-lt" => x < y,
                    b"-le" => x <= y,
                    b"-gt" => x > y,
                    _ => x >= y,
                }
            }
        })
    }

    /// Avaliação pelo número de argumentos (POSIX).
    fn by_count(&mut self, a: &[Vec<u8>]) -> R {
        match a.len() {
            0 => Ok(false),
            1 => Ok(!a[0].is_empty()),
            2 => {
                if a[0] == b"!" {
                    return Ok(a[1].is_empty());
                }
                if is_unary(&a[0]) {
                    return self.unary(&a[0], &a[1]);
                }
                Err(format!("{}: unary operator expected", String::from_utf8_lossy(&a[0])))
            }
            3 => {
                if is_binary(&a[1]) {
                    return self.binary(&a[0], &a[1], &a[2]);
                }
                if a[0] == b"!" {
                    return Ok(!self.by_count(&a[1..])?);
                }
                if a[0] == b"(" && a[2] == b")" {
                    return Ok(!a[1].is_empty());
                }
                Err(format!("{}: binary operator expected", String::from_utf8_lossy(&a[1])))
            }
            4 => {
                if a[0] == b"!" {
                    return Ok(!self.by_count(&a[1..])?);
                }
                if a[0] == b"(" && a[3] == b")" {
                    return self.by_count(&a[1..3]);
                }
                self.general(a)
            }
            _ => self.general(a),
        }
    }

    fn general(&mut self, a: &[Vec<u8>]) -> R {
        let saved = self.pos;
        self.pos = 0;
        let owned: Vec<Vec<u8>> = a.to_vec();
        let r = self.or_expr(&owned);
        let end = self.pos;
        self.pos = saved;
        let v = r?;
        if end < owned.len() {
            return Err("too many arguments".to_string());
        }
        Ok(v)
    }

    fn or_expr(&mut self, a: &[Vec<u8>]) -> R {
        let mut v = self.and_expr(a)?;
        while self.pos < a.len() && a[self.pos] == b"-o" {
            self.pos += 1;
            let r = self.and_expr(a)?;
            v = v || r;
        }
        Ok(v)
    }

    fn and_expr(&mut self, a: &[Vec<u8>]) -> R {
        let mut v = self.not_expr(a)?;
        while self.pos < a.len() && a[self.pos] == b"-a" {
            self.pos += 1;
            let r = self.not_expr(a)?;
            v = v && r;
        }
        Ok(v)
    }

    fn not_expr(&mut self, a: &[Vec<u8>]) -> R {
        if self.pos < a.len() && a[self.pos] == b"!" {
            self.pos += 1;
            return Ok(!self.not_expr(a)?);
        }
        self.primary(a)
    }

    fn primary(&mut self, a: &[Vec<u8>]) -> R {
        if self.pos >= a.len() {
            return Err("argument expected".to_string());
        }
        if a[self.pos] == b"(" {
            self.pos += 1;
            let v = self.or_expr(a)?;
            if self.pos >= a.len() || a[self.pos] != b")" {
                return Err("`)' expected".to_string());
            }
            self.pos += 1;
            return Ok(v);
        }
        // Binário: arg op arg.
        if self.pos + 2 < a.len() && is_binary(&a[self.pos + 1]) && a[self.pos + 1] != b"-a" && a[self.pos + 1] != b"-o" {
            let (x, op, y) = (a[self.pos].clone(), a[self.pos + 1].clone(), a[self.pos + 2].clone());
            self.pos += 3;
            return self.binary(&x, &op, &y);
        }
        if is_unary(&a[self.pos]) && self.pos + 1 < a.len() {
            let (op, x) = (a[self.pos].clone(), a[self.pos + 1].clone());
            self.pos += 2;
            return self.unary(&op, &x);
        }
        let v = !a[self.pos].is_empty();
        self.pos += 1;
        Ok(v)
    }
}

pub fn run(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let name = String::from_utf8_lossy(&argv[0]).into_owned();
    let mut args: &[Vec<u8>] = &argv[1..];
    if name == "[" {
        match args.last() {
            Some(l) if l == b"]" => args = &args[..args.len() - 1],
            _ => {
                sh.builtin_error("[", "missing `]'");
                return Ok(2);
            }
        }
    }
    let mut t = Test { sh, pos: 0 };
    let r = t.by_count(args);
    match r {
        Ok(true) => Ok(0),
        Ok(false) => Ok(1),
        Err(msg) => {
            sh.builtin_error(&name, msg);
            Ok(2)
        }
    }
}
