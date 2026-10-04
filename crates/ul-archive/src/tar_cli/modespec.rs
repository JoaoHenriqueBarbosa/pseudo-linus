//! `--mode=CHANGES`: modo octal ou cláusulas simbólicas do chmod (`u+x,go-w`, `a=rX`, `g=u`), escrito a
//! partir da especificação do POSIX `chmod`.

#[derive(Clone, Debug)]
enum Clause {
    Octal(u32),
    Sym { who: u32, ops: Vec<(u8, Perm)> },
}

#[derive(Clone, Debug)]
enum Perm {
    Bits(u32, bool),
    CopyFrom(u32),
}

#[derive(Clone, Debug)]
pub struct ModeSpec {
    clauses: Vec<Clause>,
}

const U: u32 = 0o4700;
const G: u32 = 0o2070;
const O: u32 = 0o1007;

impl ModeSpec {
    pub fn parse(s: &[u8]) -> Option<ModeSpec> {
        if s.is_empty() {
            return None;
        }
        if s.iter().all(|c| (b'0'..=b'7').contains(c)) {
            let v = u32::from_str_radix(std::str::from_utf8(s).ok()?, 8).ok()?;
            if v > 0o7777 {
                return None;
            }
            return Some(ModeSpec { clauses: vec![Clause::Octal(v)] });
        }
        let mut clauses = Vec::new();
        for part in s.split(|&c| c == b',') {
            let mut i = 0;
            let mut who = 0u32;
            while i < part.len() && matches!(part[i], b'u' | b'g' | b'o' | b'a') {
                who |= match part[i] {
                    b'u' => U,
                    b'g' => G,
                    b'o' => O,
                    _ => U | G | O,
                };
                i += 1;
            }
            let mut ops = Vec::new();
            if i >= part.len() {
                return None;
            }
            while i < part.len() {
                let op = part[i];
                if !matches!(op, b'+' | b'-' | b'=') {
                    return None;
                }
                i += 1;
                if i < part.len() && matches!(part[i], b'u' | b'g' | b'o') {
                    let src = match part[i] {
                        b'u' => U,
                        b'g' => G,
                        _ => O,
                    };
                    ops.push((op, Perm::CopyFrom(src)));
                    i += 1;
                    continue;
                }
                let mut bits = 0u32;
                let mut cap_x = false;
                while i < part.len() && matches!(part[i], b'r' | b'w' | b'x' | b'X' | b's' | b't') {
                    match part[i] {
                        b'r' => bits |= 0o444,
                        b'w' => bits |= 0o222,
                        b'x' => bits |= 0o111,
                        b'X' => cap_x = true,
                        b's' => bits |= 0o6000,
                        _ => bits |= 0o1000,
                    }
                    i += 1;
                }
                ops.push((op, Perm::Bits(bits, cap_x)));
            }
            clauses.push(Clause::Sym { who, ops });
        }
        Some(ModeSpec { clauses })
    }

    /// Aplica ao modo (bits 07777) de um arquivo; `is_dir` pro `X`. A umask entra quando `who` é vazio.
    pub fn apply(&self, mode: u32, is_dir: bool, umask: u32) -> u32 {
        let mut m = mode & 0o7777;
        for c in &self.clauses {
            match c {
                Clause::Octal(v) => m = *v,
                Clause::Sym { who, ops } => {
                    let (mask, honor_umask) = if *who == 0 { (U | G | O, true) } else { (*who, false) };
                    for (op, perm) in ops {
                        let mut bits = match perm {
                            Perm::Bits(b, cap_x) => {
                                let mut b = *b;
                                if *cap_x && (is_dir || m & 0o111 != 0) {
                                    b |= 0o111;
                                }
                                b
                            }
                            Perm::CopyFrom(src) => {
                                let v = m & src & 0o777;
                                let v = match *src {
                                    U => v >> 6,
                                    G => v >> 3,
                                    _ => v,
                                } & 0o7;
                                v * 0o111
                            }
                        };
                        bits &= mask;
                        if honor_umask {
                            bits &= !umask;
                        }
                        match op {
                            b'+' => m |= bits,
                            b'-' => m &= !bits,
                            _ => m = (m & !(mask & 0o7777)) | bits,
                        }
                    }
                }
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbolic_and_octal() {
        assert_eq!(ModeSpec::parse(b"go-r,u+x").unwrap().apply(0o644, false, 0o022), 0o700);
        assert_eq!(ModeSpec::parse(b"600").unwrap().apply(0o644, false, 0o022), 0o600);
        assert_eq!(ModeSpec::parse(b"a=rX").unwrap().apply(0o751, false, 0), 0o555);
        assert_eq!(ModeSpec::parse(b"a=rX").unwrap().apply(0o640, false, 0), 0o444);
        assert!(ModeSpec::parse(b"zz").is_none());
    }
}
