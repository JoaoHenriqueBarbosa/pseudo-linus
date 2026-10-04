//! Casamento de expressão regular estendida (ERE POSIX, sintaxe do `regcomp` do glibc com
//! `REG_EXTENDED`) pro `pgrep`, `pkill`, `pidwait` e `killall -r`.
//!
//! A interface é pequena de propósito ([`compile`] e [`Matcher::is_match`]): o motor definitivo é o
//! `regex-posix` do projeto, que ainda está em construção. Enquanto isso, o backend provisório
//! traduz a ERE pra sintaxe da crate `regex` 1.x, validando antes com as mesmas mensagens do
//! `regerror` do glibc (`Unmatched ( or \(`, `Invalid preceding regular expression`...). A troca
//! pro `regex-posix` fica pendente (ver STATUS.md). Diferença conhecida do backend provisório:
//! retrovisor (`(a)\1`) válido no glibc vira erro `Invalid back reference`.

/// Uma ERE compilada.
#[derive(Debug)]
pub struct Matcher {
    re: regex::bytes::Regex,
}

impl Matcher {
    /// Há casamento em algum lugar de `s` (o `regexec` com `REG_NOSUB`)?
    pub fn is_match(&self, s: &[u8]) -> bool {
        self.re.is_match(s)
    }
}

/// Compila uma ERE. O erro é o texto do `regerror` do glibc.
pub fn compile(pattern: &[u8], icase: bool) -> Result<Matcher, String> {
    let text = String::from_utf8_lossy(pattern);
    let translated = translate(&text)?;
    let full = format!("(?s{}){translated}", if icase { "i" } else { "" });
    match regex::bytes::RegexBuilder::new(&full).unicode(true).size_limit(1 << 24).build() {
        Ok(re) => Ok(Matcher { re }),
        Err(regex::Error::CompiledTooBig(_)) => Err("Regular expression too big".to_string()),
        Err(_) => Err("Invalid regular expression".to_string()),
    }
}

const CLASSES: [&str; 12] = ["alpha", "digit", "alnum", "upper", "lower", "space", "blank", "punct", "print", "graph", "cntrl", "xdigit"];
const RE_DUP_MAX: u32 = 0x7fff;

fn lit(c: char) -> String {
    regex::escape(c.encode_utf8(&mut [0u8; 4]))
}

fn class_lit(c: char) -> String {
    match c {
        '\\' | '[' | ']' | '^' | '-' | '&' | '~' => format!("\\{c}"),
        _ => c.to_string(),
    }
}

/// Lê uma expressão entre colchetes a partir de `i` (logo depois do `[`); devolve a classe na
/// sintaxe da crate e o índice depois do `]`.
fn bracket(chars: &[char], mut i: usize) -> Result<(String, usize), String> {
    const UNMATCHED: &str = "Unmatched [, [^, [:, [., or [=";
    let mut out = String::from("[");
    if chars.get(i) == Some(&'^') {
        out.push('^');
        i += 1;
    }
    let mut first = true;
    let mut items: Vec<(Option<char>, String)> = Vec::new();
    loop {
        let Some(&c) = chars.get(i) else { return Err(UNMATCHED.to_string()) };
        if c == ']' && !first {
            i += 1;
            break;
        }
        first = false;
        if c == '[' && matches!(chars.get(i + 1), Some(':') | Some('=') | Some('.')) {
            let kind = chars[i + 1];
            let start = i + 2;
            let mut j = start;
            while j + 1 < chars.len() && !(chars[j] == kind && chars[j + 1] == ']') {
                j += 1;
            }
            if j + 1 >= chars.len() {
                return Err(UNMATCHED.to_string());
            }
            let name: String = chars[start..j].iter().collect();
            i = j + 2;
            match kind {
                ':' => {
                    if !CLASSES.contains(&name.as_str()) {
                        return Err("Invalid character class name".to_string());
                    }
                    items.push((None, format!("[:{name}:]")));
                }
                _ => {
                    let mut it = name.chars();
                    match (it.next(), it.next()) {
                        (Some(one), None) => items.push((Some(one), class_lit(one))),
                        _ => return Err("Invalid collation character".to_string()),
                    }
                }
            }
            continue;
        }
        // Faixa a-z (o '-' no fim ou no começo é literal).
        if chars.get(i + 1) == Some(&'-') && chars.get(i + 2).is_some_and(|n| *n != ']') {
            let end = chars[i + 2];
            if end < c {
                return Err("Invalid range end".to_string());
            }
            items.push((None, format!("{}-{}", class_lit(c), class_lit(end))));
            i += 3;
            continue;
        }
        items.push((Some(c), class_lit(c)));
        i += 1;
    }
    for (_, s) in &items {
        out.push_str(s);
    }
    out.push(']');
    Ok((out, i))
}

/// Traduz a ERE pra sintaxe da crate `regex`, com as validações do glibc.
fn translate(p: &str) -> Result<String, String> {
    const BADRPT: &str = "Invalid preceding regular expression";
    let chars: Vec<char> = p.chars().collect();
    let mut out = String::new();
    // Início, na saída, do último átomo (pra embrulhar quantificador repetido).
    let mut atom_start: Option<usize> = None;
    let mut quantified = false;
    let mut open: Vec<usize> = Vec::new();
    let mut closed_groups = 0usize;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '(' => {
                open.push(out.len());
                out.push('(');
                atom_start = None;
                quantified = false;
                i += 1;
            }
            ')' if !open.is_empty() => {
                let start = open.pop().expect("grupo aberto");
                out.push(')');
                closed_groups += 1;
                atom_start = Some(start);
                quantified = false;
                i += 1;
            }
            '|' => {
                out.push('|');
                atom_start = None;
                quantified = false;
                i += 1;
            }
            '^' | '$' => {
                out.push(c);
                atom_start = None;
                quantified = false;
                i += 1;
            }
            '*' | '+' | '?' | '{' => {
                let Some(start) = atom_start else { return Err(BADRPT.to_string()) };
                let quant = if c == '{' {
                    let mut j = i + 1;
                    let mut body = String::new();
                    while j < chars.len() && chars[j] != '}' {
                        body.push(chars[j]);
                        j += 1;
                    }
                    if j >= chars.len() {
                        return Err("Unmatched \\{".to_string());
                    }
                    let (lo, hi) = match body.split_once(',') {
                        Some((a, b)) => (a.to_string(), Some(b.to_string())),
                        None => (body.clone(), None),
                    };
                    let parse = |s: &str| -> Result<Option<u32>, String> {
                        if s.is_empty() {
                            return Ok(None);
                        }
                        if !s.chars().all(|d| d.is_ascii_digit()) {
                            return Err("Invalid content of \\{\\}".to_string());
                        }
                        let v: u32 = s.parse().map_err(|_| "Regular expression too big".to_string())?;
                        if v > RE_DUP_MAX {
                            return Err("Regular expression too big".to_string());
                        }
                        Ok(Some(v))
                    };
                    let lo_v = parse(&lo)?.unwrap_or(0);
                    let q = match hi {
                        None => {
                            if lo.is_empty() {
                                return Err("Invalid content of \\{\\}".to_string());
                            }
                            format!("{{{lo_v}}}")
                        }
                        Some(h) => match parse(&h)? {
                            Some(hv) if hv < lo_v => return Err("Invalid content of \\{\\}".to_string()),
                            Some(hv) => format!("{{{lo_v},{hv}}}"),
                            None => format!("{{{lo_v},}}"),
                        },
                    };
                    i = j + 1;
                    q
                } else {
                    i += 1;
                    c.to_string()
                };
                if quantified {
                    out.insert_str(start, "(?:");
                    out.push(')');
                }
                out.push_str(&quant);
                quantified = true;
                atom_start = Some(start);
            }
            '[' => {
                if i + 1 >= chars.len() {
                    return Err("Invalid regular expression".to_string());
                }
                let (cls, next) = bracket(&chars, i + 1)?;
                atom_start = Some(out.len());
                out.push_str(&cls);
                quantified = false;
                i = next;
            }
            '\\' => {
                let Some(&n) = chars.get(i + 1) else { return Err("Trailing backslash".to_string()) };
                let start = out.len();
                match n {
                    '1'..='9' => {
                        let k = n as usize - '0' as usize;
                        // O backend provisório não tem retrovisor: qualquer um vira erro (no glibc
                        // só o que aponta pra grupo inexistente).
                        let _ = k <= closed_groups;
                        return Err("Invalid back reference".to_string());
                    }
                    '<' => out.push_str("\\b{start}"),
                    '>' => out.push_str("\\b{end}"),
                    'b' => out.push_str("\\b"),
                    'B' => out.push_str("\\B"),
                    'w' => out.push_str("[[:word:]]"),
                    'W' => out.push_str("[^[:word:]]"),
                    's' => out.push_str("[[:space:]]"),
                    'S' => out.push_str("[^[:space:]]"),
                    '`' => out.push_str("\\A"),
                    '\'' => out.push_str("\\z"),
                    other => out.push_str(&lit(other)),
                }
                let is_anchor = matches!(n, '<' | '>' | 'b' | 'B' | '`' | '\'');
                atom_start = if is_anchor { None } else { Some(start) };
                quantified = false;
                i += 2;
            }
            '.' => {
                atom_start = Some(out.len());
                out.push('.');
                quantified = false;
                i += 1;
            }
            other => {
                atom_start = Some(out.len());
                out.push_str(&lit(other));
                quantified = false;
                i += 1;
            }
        }
    }
    if !open.is_empty() {
        return Err("Unmatched ( or \\(".to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(p: &str) -> String {
        compile(p.as_bytes(), false).err().unwrap_or_default()
    }

    fn m(p: &str, s: &str) -> bool {
        compile(p.as_bytes(), false).unwrap().is_match(s.as_bytes())
    }

    #[test]
    fn glibc_error_messages() {
        assert_eq!(err("("), "Unmatched ( or \\(");
        assert_eq!(err("["), "Invalid regular expression");
        assert_eq!(err("[a"), "Unmatched [, [^, [:, [., or [=");
        assert_eq!(err("[]"), "Unmatched [, [^, [:, [., or [=");
        assert_eq!(err("a{"), "Unmatched \\{");
        assert_eq!(err("a{2,1}"), "Invalid content of \\{\\}");
        assert_eq!(err("*a"), "Invalid preceding regular expression");
        assert_eq!(err("a|*"), "Invalid preceding regular expression");
        assert_eq!(err("{1}"), "Invalid preceding regular expression");
        assert_eq!(err("\\"), "Trailing backslash");
        assert_eq!(err("[[:foo:]]"), "Invalid character class name");
        assert_eq!(err("[b-a]"), "Invalid range end");
        assert_eq!(err("\\1"), "Invalid back reference");
    }

    #[test]
    fn posix_constructs() {
        assert!(m(")", "a)b"));
        assert!(m("a**", "aaa"));
        assert!(m("[]a]", "x]"));
        assert!(m("[[:alpha:]]+$", "bash"));
        assert!(m("a\\{1\\}", "a{1}"));
        assert!(m("\\<ps\\>", "ps aux"));
        assert!(!m("\\<ps\\>", "ops"));
        assert!(m("[\\d]", "\\"));
        assert!(m("x\\) \\(y", "x) (y"));
        assert!(m("a{1,2}{3}", "aaa"));
        assert!(compile(b"WORKER", true).unwrap().is_match(b"kworker"));
    }
}
