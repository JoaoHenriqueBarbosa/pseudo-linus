//! Representação de um hunk como o GNU patch guarda: uma seção antiga (o padrão a procurar) e uma
//! nova (a substituição), cada linha com uma marca (` ` contexto, `-` apagada, `+` inserida, `!`
//! trocada). Os três formatos de entrada (unificado, contexto, normal) viram essa forma; a saída pros
//! `.rej` (unificado ou contexto) sai dela também, do jeito que o 2.8 escreve.

/// Formato de origem do hunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Unified,
    Context,
    Normal,
}

/// Uma linha de uma seção. `text` inclui o `\n`, a não ser na linha marcada com
/// "\ No newline at end of file".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PLine {
    pub mark: u8,
    pub text: Vec<u8>,
}

impl PLine {
    pub fn new(mark: u8, text: &[u8]) -> PLine {
        PLine { mark, text: text.to_vec() }
    }
}

/// Operação de um hunk na ordem de aplicação.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op<'a> {
    Context(&'a [u8]),
    Delete(&'a [u8]),
    Insert(&'a [u8]),
}

#[derive(Clone, Debug)]
pub struct Hunk {
    pub format: Format,
    /// Primeira linha (base 1) coberta na seção antiga; numa seção vazia, a linha seguinte ao ponto
    /// de inserção.
    pub old_first: usize,
    pub old: Vec<PLine>,
    pub new_first: usize,
    pub new: Vec<PLine>,
    /// Texto depois do segundo `@@` (com o espaço), sem o fim de linha.
    pub func: Vec<u8>,
    /// Comando do formato normal (`a`, `c`, `d`).
    pub normal_cmd: u8,
}

impl Hunk {
    /// O hunk ao contrário (`-R`).
    pub fn reversed(&self) -> Hunk {
        let flip = |lines: &[PLine]| -> Vec<PLine> {
            lines
                .iter()
                .map(|l| PLine {
                    mark: match l.mark {
                        b'-' => b'+',
                        b'+' => b'-',
                        m => m,
                    },
                    text: l.text.clone(),
                })
                .collect()
        };
        Hunk {
            format: self.format,
            old_first: self.new_first,
            old: flip(&self.new),
            new_first: self.old_first,
            new: flip(&self.old),
            func: self.func.clone(),
            normal_cmd: match self.normal_cmd {
                b'a' => b'd',
                b'd' => b'a',
                c => c,
            },
        }
    }

    /// Linhas de contexto antes da primeira mudança (de qualquer lado).
    pub fn prefix_context(&self) -> usize {
        self.ops().iter().take_while(|o| matches!(o, Op::Context(_))).count()
    }

    /// Linhas de contexto depois da última mudança (de qualquer lado).
    pub fn suffix_context(&self) -> usize {
        self.ops().iter().rev().take_while(|o| matches!(o, Op::Context(_))).count()
    }

    /// O hunk na ordem de aplicação: contexto anda nas duas seções; num bloco trocado (`!`), primeiro
    /// as linhas antigas, depois as novas.
    pub fn ops(&self) -> Vec<Op<'_>> {
        let (old, new) = (&self.old, &self.new);
        let mut out = Vec::with_capacity(old.len() + new.len());
        let (mut i, mut j) = (0usize, 0usize);
        while i < old.len() || j < new.len() {
            if i < old.len() && old[i].mark == b'-' {
                out.push(Op::Delete(&old[i].text));
                i += 1;
            } else if j < new.len() && new[j].mark == b'+' {
                out.push(Op::Insert(&new[j].text));
                j += 1;
            } else if i < old.len() && old[i].mark == b'!' {
                while i < old.len() && old[i].mark == b'!' {
                    out.push(Op::Delete(&old[i].text));
                    i += 1;
                }
                while j < new.len() && new[j].mark == b'!' {
                    out.push(Op::Insert(&new[j].text));
                    j += 1;
                }
            } else if j < new.len() && new[j].mark == b'!' {
                while j < new.len() && new[j].mark == b'!' {
                    out.push(Op::Insert(&new[j].text));
                    j += 1;
                }
            } else if i < old.len() && j < new.len() {
                out.push(Op::Context(&old[i].text));
                i += 1;
                j += 1;
            } else if i < old.len() {
                out.push(Op::Context(&old[i].text));
                i += 1;
            } else {
                out.push(Op::Context(&new[j].text));
                j += 1;
            }
        }
        out
    }

    /// Hunk com as faixas deslocadas (pro `.rej`, que o GNU grava em coordenadas já deslocadas pelo
    /// saldo de linhas dos hunks aplicados antes).
    pub fn shifted(&self, delta: isize) -> Hunk {
        let mut h = self.clone();
        h.old_first = (h.old_first as isize + delta).max(0) as usize;
        h.new_first = (h.new_first as isize + delta).max(0) as usize;
        h
    }

    /// Hunk em formato unificado, do jeito que o GNU patch 2.8 escreve no `.rej`: linhas sem `\n`
    /// final saem sem ele e sem o aviso "\ No newline at end of file".
    pub fn write_unified(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"@@ -");
        out.extend_from_slice(unified_range(self.old_first, self.old.len()).as_bytes());
        out.extend_from_slice(b" +");
        out.extend_from_slice(unified_range(self.new_first, self.new.len()).as_bytes());
        out.extend_from_slice(b" @@");
        out.extend_from_slice(&self.func);
        out.push(b'\n');
        for op in self.ops() {
            let (c, t) = match op {
                Op::Context(t) => (b' ', t),
                Op::Delete(t) => (b'-', t),
                Op::Insert(t) => (b'+', t),
            };
            out.push(c);
            out.extend_from_slice(t);
        }
    }

    /// Hunk em formato de contexto (as duas seções sempre por inteiro). O formato normal tem
    /// cabeçalhos próprios no 2.8: `*** 2` e `--- 2 -----`, e a faixa nova de um `d` sai como `0`.
    pub fn write_context(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"***************\n");
        // No formato normal, uma faixa vazia sai como `0` dos dois lados.
        let normal = self.format == Format::Normal;
        let old_range =
            if normal && self.old.is_empty() { "0".to_string() } else { context_range(self.old_first, self.old.len()) };
        let new_range =
            if normal && self.new.is_empty() { "0".to_string() } else { context_range(self.new_first, self.new.len()) };
        if self.format == Format::Normal {
            out.extend_from_slice(format!("*** {old_range}\n").as_bytes());
        } else {
            out.extend_from_slice(format!("*** {old_range} ****\n").as_bytes());
        }
        for l in &self.old {
            out.push(l.mark);
            out.push(b' ');
            out.extend_from_slice(&l.text);
        }
        if self.format == Format::Normal {
            out.extend_from_slice(format!("--- {new_range} -----\n").as_bytes());
        } else {
            out.extend_from_slice(format!("--- {new_range} ----\n").as_bytes());
        }
        for l in &self.new {
            out.push(l.mark);
            out.push(b' ');
            out.extend_from_slice(&l.text);
        }
    }
}

/// Faixa do cabeçalho `@@`: `a,n`, `a` quando n = 1, `a-1,0` quando vazia.
pub fn unified_range(first: usize, len: usize) -> String {
    match len {
        0 => format!("{},0", first.saturating_sub(1)),
        1 => format!("{first}"),
        n => format!("{first},{n}"),
    }
}

/// Faixa do formato de contexto: `a,b`, `a` quando uma linha, a linha anterior quando vazia.
pub fn context_range(first: usize, len: usize) -> String {
    match len {
        0 => format!("{}", first.saturating_sub(1)),
        1 => format!("{first}"),
        n => format!("{first},{}", first + n - 1),
    }
}

/// Converte as linhas de um hunk unificado (marca e texto) nas duas seções. Um bloco de mudanças
/// entre linhas de contexto que tem apagadas e inseridas vira `!` dos dois lados, como o GNU guarda.
pub fn sections_from_unified(lines: &[PLine]) -> (Vec<PLine>, Vec<PLine>) {
    let mut old = Vec::new();
    let mut new = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].mark == b' ' {
            old.push(PLine::new(b' ', &lines[i].text));
            new.push(PLine::new(b' ', &lines[i].text));
            i += 1;
            continue;
        }
        let start = i;
        while i < lines.len() && lines[i].mark != b' ' {
            i += 1;
        }
        let block = &lines[start..i];
        let mixed = block.iter().any(|l| l.mark == b'-') && block.iter().any(|l| l.mark == b'+');
        for l in block {
            let mark = if mixed { b'!' } else { l.mark };
            if l.mark == b'-' {
                old.push(PLine::new(mark, &l.text));
            } else {
                new.push(PLine::new(mark, &l.text));
            }
        }
    }
    (old, new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[(u8, &str)]) -> Vec<PLine> {
        v.iter().map(|(m, t)| PLine::new(*m, t.as_bytes())).collect()
    }

    #[test]
    fn unified_round_trip_reorders_changed_blocks() {
        let l = lines(&[(b' ', "9\n"), (b'-', "2\n"), (b'+', "A\n"), (b'-', "3\n"), (b'+', "B\n"), (b' ', "4\n")]);
        let (old, new) = sections_from_unified(&l);
        let h = Hunk { format: Format::Unified, old_first: 1, old, new_first: 1, new, func: Vec::new(), normal_cmd: 0 };
        let mut out = Vec::new();
        h.write_unified(&mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "@@ -1,4 +1,4 @@\n 9\n-2\n-3\n+A\n+B\n 4\n");
        let mut out = Vec::new();
        h.write_context(&mut out);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "***************\n*** 1,4 ****\n  9\n! 2\n! 3\n  4\n--- 1,4 ----\n  9\n! A\n! B\n  4\n"
        );
    }

    #[test]
    fn ranges() {
        assert_eq!(unified_range(5, 0), "4,0");
        assert_eq!(unified_range(1, 1), "1");
        assert_eq!(context_range(3, 0), "2");
        assert_eq!(context_range(3, 3), "3,5");
    }
}
