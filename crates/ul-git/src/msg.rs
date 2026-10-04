//! Limpeza de mensagens (o `strbuf_stripspace` e os modos de `commit.cleanup`) e trailers.

/// `strbuf_stripspace`: sem espaço no fim das linhas, sem linhas vazias repetidas, no começo ou no
/// fim, e (com `comment`) sem as linhas de comentário. Termina em `\n` se não ficar vazio.
pub fn stripspace(msg: &[u8], comment: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(msg.len() + 1);
    let mut empties = 0;
    for line in msg.split_inclusive(|c| *c == b'\n') {
        if let Some(c) = comment
            && line.starts_with(c)
        {
            continue;
        }
        let body = crate::object::rtrim(line);
        if body.is_empty() {
            empties += 1;
            continue;
        }
        if empties > 0 && !out.is_empty() {
            out.push(b'\n');
        }
        empties = 0;
        out.extend_from_slice(body);
        out.push(b'\n');
    }
    out
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Cleanup {
    Strip,
    Whitespace,
    Verbatim,
    Scissors,
}

impl Cleanup {
    pub fn parse(s: &str) -> Option<Cleanup> {
        Some(match s {
            "strip" => Cleanup::Strip,
            "whitespace" => Cleanup::Whitespace,
            "verbatim" => Cleanup::Verbatim,
            "scissors" => Cleanup::Scissors,
            "default" => return None,
            _ => return None,
        })
    }
}

pub const SCISSORS: &[u8] = b"------------------------ >8 ------------------------";

/// Aplica o modo de limpeza.
pub fn cleanup(msg: &[u8], mode: Cleanup, comment: &[u8]) -> Vec<u8> {
    match mode {
        Cleanup::Verbatim => msg.to_vec(),
        Cleanup::Whitespace => stripspace(msg, None),
        Cleanup::Strip => stripspace(msg, Some(comment)),
        Cleanup::Scissors => {
            let mut cut = msg.to_vec();
            let mut marker = comment.to_vec();
            marker.push(b' ');
            marker.extend_from_slice(SCISSORS);
            if let Some(pos) = find_line(&cut, &marker) {
                cut.truncate(pos);
            }
            stripspace(&cut, None)
        }
    }
}

fn find_line(hay: &[u8], line: &[u8]) -> Option<usize> {
    let mut pos = 0;
    for l in hay.split_inclusive(|c| *c == b'\n') {
        if l.strip_suffix(b"\n").unwrap_or(l) == line {
            return Some(pos);
        }
        pos += l.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip() {
        assert_eq!(stripspace(b"\n\nfirst  \n\n\n\nsecond\n\n", None), b"first\n\nsecond\n");
        assert_eq!(stripspace(b"a\n# c\nb", Some(b"#")), b"a\nb\n");
        assert_eq!(stripspace(b"   \n", None), b"");
    }
}
