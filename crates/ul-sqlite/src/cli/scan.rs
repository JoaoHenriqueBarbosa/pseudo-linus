//! Varredura de texto SQL do CLI: `sqlite3_complete` (complete.c), o `quickscan` do shell.c, a
//! separação de um buffer em comandos e o reconhecimento dos terminadores `go` e `/`.

/// `isspace` da libc no locale C: espaço, \t, \n, \v, \f, \r.
pub fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `IdChar` do tokenizador do SQLite: letra, dígito, `_`, `$` e todo byte >= 0x80.
pub fn id_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tk {
    Semi = 0,
    Ws = 1,
    Other = 2,
    Explain = 3,
    Create = 4,
    Temp = 5,
    Trigger = 6,
    End = 7,
}

const TRANS: [[u8; 8]; 8] = [
    [1, 0, 2, 3, 4, 2, 2, 2],
    [1, 1, 2, 3, 4, 2, 2, 2],
    [1, 2, 2, 2, 2, 2, 2, 2],
    [1, 3, 3, 2, 4, 2, 2, 2],
    [1, 4, 2, 2, 2, 4, 5, 2],
    [6, 5, 5, 5, 5, 5, 5, 5],
    [6, 6, 5, 5, 5, 5, 5, 7],
    [1, 7, 5, 5, 5, 5, 5, 5],
];

/// Resultado da varredura: onde cada comando termina (índice logo depois do `;` que o fecha) e se o
/// texto todo está completo.
pub struct Scan {
    pub boundaries: Vec<usize>,
    pub complete: bool,
}

/// Porte de `sqlite3_complete`, devolvendo também as fronteiras dos comandos. O texto acaba no
/// primeiro NUL, como a string C.
pub fn scan(sql: &[u8]) -> Scan {
    let end = sql.iter().position(|&b| b == 0).unwrap_or(sql.len());
    let z = &sql[..end];
    let mut state: u8 = 0;
    let mut i = 0;
    let mut boundaries = Vec::new();
    while i < z.len() {
        let c = z[i];
        let token = match c {
            b';' => Tk::Semi,
            b' ' | b'\r' | b'\t' | b'\n' | 0x0c => Tk::Ws,
            b'/' => {
                if z.get(i + 1) != Some(&b'*') {
                    Tk::Other
                } else {
                    i += 2;
                    while i < z.len() && !(z[i] == b'*' && z.get(i + 1) == Some(&b'/')) {
                        i += 1;
                    }
                    if i >= z.len() {
                        return Scan { boundaries, complete: false };
                    }
                    i += 1;
                    Tk::Ws
                }
            }
            b'-' => {
                if z.get(i + 1) != Some(&b'-') {
                    Tk::Other
                } else {
                    while i < z.len() && z[i] != b'\n' {
                        i += 1;
                    }
                    if i >= z.len() {
                        return Scan { boundaries, complete: state == 1 };
                    }
                    Tk::Ws
                }
            }
            b'[' => {
                i += 1;
                while i < z.len() && z[i] != b']' {
                    i += 1;
                }
                if i >= z.len() {
                    return Scan { boundaries, complete: false };
                }
                Tk::Other
            }
            b'`' | b'"' | b'\'' => {
                i += 1;
                while i < z.len() && z[i] != c {
                    i += 1;
                }
                if i >= z.len() {
                    return Scan { boundaries, complete: false };
                }
                Tk::Other
            }
            _ if id_char(c) => {
                let mut n = 1;
                while i + n < z.len() && id_char(z[i + n]) {
                    n += 1;
                }
                let w = &z[i..i + n];
                let t = if w.eq_ignore_ascii_case(b"create") {
                    Tk::Create
                } else if w.eq_ignore_ascii_case(b"trigger") {
                    Tk::Trigger
                } else if w.eq_ignore_ascii_case(b"temp") || w.eq_ignore_ascii_case(b"temporary") {
                    Tk::Temp
                } else if w.eq_ignore_ascii_case(b"end") {
                    Tk::End
                } else if w.eq_ignore_ascii_case(b"explain") {
                    Tk::Explain
                } else {
                    Tk::Other
                };
                i += n - 1;
                t
            }
            _ => Tk::Other,
        };
        let next = TRANS[state as usize][token as usize];
        if token == Tk::Semi && next == 1 {
            boundaries.push(i + 1);
        }
        state = next;
        i += 1;
    }
    Scan { boundaries, complete: state == 1 }
}

/// `sqlite3_complete`.
pub fn complete(sql: &[u8]) -> bool {
    scan(sql).complete
}

/// `line_is_complete`: completo depois de acrescentar um `;`.
pub fn line_is_complete(sql: &[u8]) -> bool {
    let mut s = sql.to_vec();
    s.push(b';');
    complete(&s)
}

/// Estado do `quickscan` (resumível entre linhas): o caractere que fecha o lexema aberto (0 = texto
/// simples, `*` = comentário de bloco), se já houve texto "escuro" e se a linha acabou num `;`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Qss {
    pub wait: u8,
    pub has_dark: bool,
    pub ending_semi: bool,
}

impl Qss {
    pub const START: Qss = Qss { wait: 0, has_dark: false, ending_semi: false };

    /// `QSS_INPLAIN`.
    pub fn in_plain(self) -> bool {
        self.wait == 0
    }

    /// `QSS_PLAINWHITE`.
    pub fn plain_white(self) -> bool {
        self.wait == 0 && !self.has_dark
    }

    /// `QSS_SEMITERM`.
    pub fn semi_term(self) -> bool {
        self.wait == 0 && self.ending_semi
    }
}

/// `quickscan` do shell.c.
pub fn quickscan(line: &[u8], mut qss: Qss) -> Qss {
    let z = line;
    let mut i = 0;
    let mut wait = qss.wait;
    'outer: loop {
        if wait == 0 {
            while i < z.len() {
                let c = z[i];
                i += 1;
                if is_space(c) {
                    continue;
                }
                match c {
                    b'-' => {
                        if z.get(i) == Some(&b'-') {
                            while i < z.len() {
                                let cin = z[i];
                                i += 1;
                                if cin == b'\n' {
                                    continue 'outer;
                                }
                            }
                            return qss;
                        }
                    }
                    b';' => {
                        qss.ending_semi = true;
                        continue;
                    }
                    b'/' => {
                        if z.get(i) == Some(&b'*') {
                            i += 1;
                            wait = b'*';
                            qss.wait = b'*';
                            continue 'outer;
                        }
                    }
                    b'[' => {
                        wait = b']';
                        qss = Qss { wait, has_dark: true, ending_semi: false };
                        continue 'outer;
                    }
                    b'`' | b'\'' | b'"' => {
                        wait = c;
                        qss = Qss { wait, has_dark: true, ending_semi: false };
                        continue 'outer;
                    }
                    _ => {}
                }
                qss.ending_semi = false;
                qss.has_dark = true;
            }
            return qss;
        } else {
            while i < z.len() {
                let c = z[i];
                i += 1;
                if c == wait {
                    match wait {
                        b'*' => {
                            if z.get(i) != Some(&b'/') {
                                continue;
                            }
                            i += 1;
                            wait = 0;
                            qss.wait = 0;
                            continue 'outer;
                        }
                        b'`' | b'\'' | b'"' => {
                            if z.get(i) == Some(&wait) {
                                i += 1;
                                continue;
                            }
                            wait = 0;
                            qss.wait = 0;
                            continue 'outer;
                        }
                        _ => {
                            wait = 0;
                            qss.wait = 0;
                            continue 'outer;
                        }
                    }
                }
            }
            return qss;
        }
    }
}

/// `line_is_command_terminator`: a linha é só `/` (Oracle) ou `go` (SQL Server), com espaço e
/// comentário em volta.
pub fn line_is_command_terminator(line: &[u8]) -> bool {
    let mut z = line;
    while let Some(&c) = z.first() {
        if is_space(c) {
            z = &z[1..];
        } else {
            break;
        }
    }
    if z.first() == Some(&b'/') {
        z = &z[1..];
    } else if z.len() >= 2 && z[0].eq_ignore_ascii_case(&b'g') && z[1].eq_ignore_ascii_case(&b'o') {
        z = &z[2..];
    } else {
        return false;
    }
    quickscan(z, Qss::START) == Qss::START
}

/// `true` quando o texto só tem espaço, comentários e `;`.
pub fn is_blank_sql(sql: &[u8]) -> bool {
    let z = sql;
    let mut i = 0;
    while i < z.len() {
        match z[i] {
            b' ' | b'\t' | b'\n' | b'\r' | 0x0c | 0x0b | b';' => i += 1,
            b'-' if z.get(i + 1) == Some(&b'-') => {
                while i < z.len() && z[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if z.get(i + 1) == Some(&b'*') => {
                let mut j = i + 2;
                while j < z.len() && !(z[j] == b'*' && z.get(j + 1) == Some(&b'/')) {
                    j += 1;
                }
                i = j + 2;
            }
            _ => return false,
        }
    }
    true
}

/// Separa um buffer nos comandos que o `sqlite3_prepare` consumiria um a um: cada item é o deslocamento
/// do começo do comando (já sem o espaço inicial, como o shell_exec pula) e o do fim.
pub fn split_statements(buf: &[u8]) -> Vec<(usize, usize)> {
    let s = scan(buf);
    let mut ends = s.boundaries;
    let limit = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    if ends.last().copied() != Some(limit) {
        ends.push(limit);
    }
    let mut out = Vec::new();
    let mut start = 0;
    for end in ends {
        let mut b = start;
        while b < end && is_space(buf[b]) {
            b += 1;
        }
        if b < end {
            out.push((b, end));
        }
        start = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_matches_sqlite() {
        assert!(complete(b"SELECT 1;"));
        assert!(!complete(b"SELECT 1"));
        assert!(!complete(b"SELECT 'a;"));
        assert!(complete(b"SELECT 1; -- x"));
        let trig = b"CREATE TRIGGER t AFTER INSERT ON a BEGIN INSERT INTO b VALUES(1); END;";
        let s = scan(trig);
        assert!(s.complete);
        assert_eq!(s.boundaries, vec![trig.len()]);
        assert_eq!(scan(b"SELECT 1; SELECT 2;").boundaries, vec![9, 19]);
        assert!(is_blank_sql(b"  ;; -- c\n /* x */ ;"));
        assert!(!is_blank_sql(b"-- c\nSELECT 1"));
    }

    #[test]
    fn quickscan_states() {
        let q = quickscan(b"SELECT 1;", Qss::START);
        assert!(q.semi_term());
        let q = quickscan(b"SELECT 'abc", Qss::START);
        assert_eq!(q.wait, b'\'');
        let q = quickscan(b"def';", q);
        assert!(q.semi_term());
        assert!(quickscan(b"   -- only comment", Qss::START).plain_white());
        assert!(line_is_command_terminator(b"  go  "));
        assert!(line_is_command_terminator(b"/"));
        assert!(!line_is_command_terminator(b"gone"));
    }
}
