//! PROVISÓRIO: avaliador aritmético mínimo pra desenvolvimento do núcleo, até chegar o módulo de
//! sala limpa (`src/arith.rs`). Não é entregue.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArithError {
    pub message: String,
    pub from_env: bool,
}

pub trait ArithEnv {
    fn lookup(&mut self, name: &str, subscript: Option<&[u8]>) -> Result<Option<Vec<u8>>, ArithError>;
    fn assign(&mut self, name: &str, subscript: Option<&[u8]>, value: i64) -> Result<(), ArithError>;
}

struct P<'a, 'e> {
    s: &'a [u8],
    i: usize,
    env: &'e mut dyn ArithEnv,
    noeval: u32,
    depth: u32,
    whole: &'a [u8],
}

type R = Result<i64, ArithError>;

pub fn eval(expr: &[u8], env: &mut dyn ArithEnv) -> R {
    eval_depth(expr, env, 0)
}

fn eval_depth(expr: &[u8], env: &mut dyn ArithEnv, depth: u32) -> R {
    if depth > 1024 {
        return Err(ArithError { message: format!("{}: expression recursion level exceeded", String::from_utf8_lossy(expr)), from_env: false });
    }
    let mut p = P { s: expr, i: 0, env, noeval: 0, depth, whole: expr };
    p.ws();
    if p.i >= p.s.len() {
        return Ok(0);
    }
    let v = p.comma()?;
    p.ws();
    if p.i < p.s.len() {
        return Err(p.err("syntax error in expression"));
    }
    Ok(v)
}

impl P<'_, '_> {
    fn err(&self, msg: &str) -> ArithError {
        let whole = String::from_utf8_lossy(self.whole).trim().to_string();
        let tok = String::from_utf8_lossy(&self.s[self.i.min(self.s.len())..]).into_owned();
        ArithError { message: format!("{whole}: {msg} (error token is \"{tok}\")"), from_env: false }
    }

    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n') {
            self.i += 1;
        }
    }

    fn peek(&self, t: &str) -> bool {
        self.s[self.i..].starts_with(t.as_bytes())
    }

    fn eat(&mut self, t: &str) -> bool {
        self.ws();
        if self.peek(t) {
            self.i += t.len();
            true
        } else {
            false
        }
    }

    fn comma(&mut self) -> R {
        let mut v = self.assign()?;
        while self.eat(",") {
            v = self.assign()?;
        }
        Ok(v)
    }

    fn assign(&mut self) -> R {
        self.ws();
        let save = self.i;
        if let Some((name, sub)) = self.lvalue() {
            self.ws();
            for op in ["=", "+=", "-=", "*=", "/=", "%=", "<<=", ">>=", "&=", "|=", "^="] {
                if self.peek(op) && !(op == "=" && self.peek("==")) {
                    self.i += op.len();
                    let rhs = self.assign()?;
                    let v = if op == "=" {
                        rhs
                    } else {
                        let cur = self.var(&name, sub.as_deref())?;
                        self.binop(&op[..op.len() - 1], cur, rhs)?
                    };
                    if self.noeval == 0 {
                        self.env.assign(&name, sub.as_deref(), v)?;
                    }
                    return Ok(v);
                }
            }
        }
        self.i = save;
        self.ternary()
    }

    fn lvalue(&mut self) -> Option<(String, Option<Vec<u8>>)> {
        let start = self.i;
        if self.i < self.s.len() && (self.s[self.i].is_ascii_alphabetic() || self.s[self.i] == b'_') {
            while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') {
                self.i += 1;
            }
            let name = String::from_utf8_lossy(&self.s[start..self.i]).into_owned();
            let mut sub = None;
            if self.i < self.s.len() && self.s[self.i] == b'[' {
                let mut depth = 0;
                let st = self.i + 1;
                while self.i < self.s.len() {
                    match self.s[self.i] {
                        b'[' => depth += 1,
                        b']' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    self.i += 1;
                }
                sub = Some(self.s[st..self.i].to_vec());
                self.i += 1;
            }
            return Some((name, sub));
        }
        None
    }

    fn var(&mut self, name: &str, sub: Option<&[u8]>) -> R {
        if self.noeval > 0 {
            return Ok(0);
        }
        match self.env.lookup(name, sub)? {
            None => Ok(0),
            Some(v) if v.is_empty() => Ok(0),
            Some(v) => eval_depth(&v, self.env, self.depth + 1),
        }
    }

    fn ternary(&mut self) -> R {
        let c = self.lor()?;
        if self.eat("?") {
            if c != 0 {
                let a = self.comma()?;
                if !self.eat(":") {
                    return Err(self.err("`:' expected for conditional expression"));
                }
                self.noeval += 1;
                let _ = self.assign();
                self.noeval -= 1;
                Ok(a)
            } else {
                self.noeval += 1;
                let _ = self.comma();
                self.noeval -= 1;
                if !self.eat(":") {
                    return Err(self.err("`:' expected for conditional expression"));
                }
                self.assign()
            }
        } else {
            Ok(c)
        }
    }

    fn lor(&mut self) -> R {
        let mut v = self.land()?;
        while self.eat("||") {
            if v != 0 {
                self.noeval += 1;
                let _ = self.land();
                self.noeval -= 1;
                v = 1;
            } else {
                v = (self.land()? != 0) as i64;
            }
        }
        Ok(v)
    }

    fn land(&mut self) -> R {
        let mut v = self.bor()?;
        while self.eat("&&") {
            if v == 0 {
                self.noeval += 1;
                let _ = self.bor();
                self.noeval -= 1;
            } else {
                v = (self.bor()? != 0) as i64;
            }
        }
        Ok(v)
    }

    fn level(&mut self, ops: &[&str], next: fn(&mut Self) -> R) -> R {
        let mut v = next(self)?;
        'outer: loop {
            self.ws();
            for op in ops {
                if self.peek(op) {
                    // Não confundir `|` com `||`, `&` com `&&`, `<` com `<<`...
                    let rest = &self.s[self.i + op.len()..];
                    if (*op == "|" && rest.first() == Some(&b'|'))
                        || (*op == "&" && rest.first() == Some(&b'&'))
                        || ((*op == "<" || *op == ">") && matches!(rest.first(), Some(b'<') | Some(b'>') | Some(b'=')))
                        || ((*op == "<<" || *op == ">>") && rest.first() == Some(&b'='))
                        || (rest.first() == Some(&b'=') && matches!(*op, "+" | "-" | "*" | "/" | "%" | "|" | "&" | "^"))
                        || (*op == "*" && rest.first() == Some(&b'*'))
                    {
                        continue;
                    }
                    self.i += op.len();
                    let r = next(self)?;
                    v = self.binop(op, v, r)?;
                    continue 'outer;
                }
            }
            return Ok(v);
        }
    }

    fn bor(&mut self) -> R {
        self.level(&["|"], Self::bxor)
    }
    fn bxor(&mut self) -> R {
        self.level(&["^"], Self::band)
    }
    fn band(&mut self) -> R {
        self.level(&["&"], Self::eq)
    }
    fn eq(&mut self) -> R {
        self.level(&["==", "!="], Self::rel)
    }
    fn rel(&mut self) -> R {
        self.level(&["<=", ">=", "<", ">"], Self::shift)
    }
    fn shift(&mut self) -> R {
        self.level(&["<<", ">>"], Self::add)
    }
    fn add(&mut self) -> R {
        self.level(&["+", "-"], Self::mul)
    }
    fn mul(&mut self) -> R {
        self.level(&["*", "/", "%"], Self::pow)
    }

    fn pow(&mut self) -> R {
        let b = self.unary()?;
        if self.eat("**") {
            let e = self.pow()?;
            return self.binop("**", b, e);
        }
        Ok(b)
    }

    fn binop(&self, op: &str, a: i64, b: i64) -> R {
        Ok(match op {
            "+" => a.wrapping_add(b),
            "-" => a.wrapping_sub(b),
            "*" => a.wrapping_mul(b),
            "/" | "%" => {
                if b == 0 {
                    if self.noeval > 0 {
                        return Ok(0);
                    }
                    let mut e = self.err("division by 0");
                    let tok = String::from_utf8_lossy(&self.s[self.i.saturating_sub(1).min(self.s.len())..]).trim_start().to_string();
                    let _ = tok;
                    e.message = e.message.clone();
                    return Err(e);
                }
                if op == "/" { a.wrapping_div(b) } else { a.wrapping_rem(b) }
            }
            "**" => {
                if b < 0 {
                    return Err(self.err("exponent less than 0"));
                }
                let mut r: i64 = 1;
                for _ in 0..b.min(64) {
                    r = r.wrapping_mul(a);
                }
                if b > 64 {
                    let mut base = a;
                    let mut e = b;
                    r = 1;
                    while e > 0 {
                        if e & 1 == 1 {
                            r = r.wrapping_mul(base);
                        }
                        base = base.wrapping_mul(base);
                        e >>= 1;
                    }
                }
                r
            }
            "<<" => a.wrapping_shl(b as u32),
            ">>" => a.wrapping_shr(b as u32),
            "<" => (a < b) as i64,
            ">" => (a > b) as i64,
            "<=" => (a <= b) as i64,
            ">=" => (a >= b) as i64,
            "==" => (a == b) as i64,
            "!=" => (a != b) as i64,
            "&" => a & b,
            "|" => a | b,
            "^" => a ^ b,
            _ => 0,
        })
    }

    fn unary(&mut self) -> R {
        self.ws();
        if self.peek("++") || self.peek("--") {
            let inc = if self.peek("++") { 1 } else { -1 };
            self.i += 2;
            self.ws();
            let Some((name, sub)) = self.lvalue() else { return Err(self.err("syntax error: operand expected")) };
            let v = self.var(&name, sub.as_deref())?.wrapping_add(inc);
            if self.noeval == 0 {
                self.env.assign(&name, sub.as_deref(), v)?;
            }
            return Ok(v);
        }
        for (op, f) in [("!", 0), ("~", 1), ("-", 2), ("+", 3)] {
            if self.peek(op) {
                self.i += 1;
                let v = self.unary()?;
                return Ok(match f {
                    0 => (v == 0) as i64,
                    1 => !v,
                    2 => v.wrapping_neg(),
                    _ => v,
                });
            }
        }
        self.postfix()
    }

    fn postfix(&mut self) -> R {
        self.ws();
        let save = self.i;
        if let Some((name, sub)) = self.lvalue() {
            self.ws();
            if self.peek("++") || self.peek("--") {
                let inc = if self.peek("++") { 1 } else { -1 };
                self.i += 2;
                let v = self.var(&name, sub.as_deref())?;
                if self.noeval == 0 {
                    self.env.assign(&name, sub.as_deref(), v.wrapping_add(inc))?;
                }
                return Ok(v);
            }
            return self.var(&name, sub.as_deref());
        }
        self.i = save;
        self.primary()
    }

    fn primary(&mut self) -> R {
        self.ws();
        if self.eat("(") {
            let v = self.comma()?;
            if !self.eat(")") {
                return Err(self.err("missing `)'"));
            }
            return Ok(v);
        }
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || matches!(self.s[self.i], b'#' | b'_' | b'@')) {
            self.i += 1;
        }
        if start == self.i {
            return Err(self.err("syntax error: operand expected"));
        }
        let t = String::from_utf8_lossy(&self.s[start..self.i]).into_owned();
        parse_const(&t).ok_or_else(|| {
            let mut e = self.err("value too great for base");
            e.message = format!("{}: value too great for base (error token is \"{t}\")", String::from_utf8_lossy(self.whole).trim());
            e
        })
    }
}

fn parse_const(t: &str) -> Option<i64> {
    let (base, digits) = if let Some((b, d)) = t.split_once('#') {
        (b.parse::<u32>().ok()?, d)
    } else if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        (16, h)
    } else if t.len() > 1 && t.starts_with('0') {
        (8, &t[1..])
    } else {
        (10, t)
    };
    if !(2..=64).contains(&base) || digits.is_empty() {
        return None;
    }
    let mut v: i64 = 0;
    for c in digits.chars() {
        let d = match c {
            '0'..='9' => c as u32 - '0' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 10,
            'A'..='Z' => if base <= 36 { c as u32 - 'A' as u32 + 10 } else { c as u32 - 'A' as u32 + 36 },
            '@' => 62,
            '_' => 63,
            _ => return None,
        };
        if d >= base {
            return None;
        }
        v = v.wrapping_mul(base as i64).wrapping_add(d as i64);
    }
    Some(v)
}
