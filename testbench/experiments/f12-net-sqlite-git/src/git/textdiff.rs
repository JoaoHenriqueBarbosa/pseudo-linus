//! Diff de blobs no formato do `git diff`.
//!
//! O algoritmo e o agrupamento em hunks com contexto vêm do `gix-diff` (imara-diff com a heurística de
//! slider do git). A moldura é nossa: o cabeçalho `@@ -a,b +c,d @@` omite `,1` como o git, o contexto
//! de função (`@@ ... @@ j`) segue a regra padrão do xdiff (última linha antes do hunk que começa com
//! letra, `_` ou `$`) e a marca `\ No newline at end of file`.

use gix_diff::blob::unified_diff::{ConsumeHunk, ContextSize, DiffLineKind, HunkHeader};
use gix_diff::blob::{Algorithm, InternedInput, UnifiedDiff, diff_with_slider_heuristics};

struct GitHunks<'a> {
    before: &'a [u8],
    out: Vec<u8>,
}

/// Faixa do cabeçalho de hunk: o git omite `,1` e, com tamanho zero, usa a linha anterior.
fn range(start: u32, len: u32) -> String {
    match len {
        0 => format!("{},0", start.saturating_sub(1)),
        1 => format!("{start}"),
        _ => format!("{start},{len}"),
    }
}

/// Linha de contexto de função: a última linha antes de `line` (1-based) que começa com letra, `_` ou `$`.
fn func_context(before: &[u8], line: u32) -> Option<String> {
    let lines: Vec<&[u8]> = before.split_inclusive(|&b| b == b'\n').collect();
    let upto = (line as usize).saturating_sub(1).min(lines.len());
    for l in lines[..upto].iter().rev() {
        if let Some(&c) = l.first()
            && (c.is_ascii_alphabetic() || c == b'_' || c == b'$')
        {
            let text = String::from_utf8_lossy(l);
            let text = text.trim_end_matches('\n');
            // O xdiff corta em 80 bytes.
            let cut: String = text.chars().take(80).collect();
            return Some(cut.trim_end().to_string());
        }
    }
    None
}

impl ConsumeHunk for GitHunks<'_> {
    type Out = Vec<u8>;

    fn consume_hunk(&mut self, header: HunkHeader, lines: &[(DiffLineKind, &[u8])]) -> std::io::Result<()> {
        let before = range(header.before_hunk_start, header.before_hunk_len);
        let after = range(header.after_hunk_start, header.after_hunk_len);
        let mut head = format!("@@ -{before} +{after} @@");
        if let Some(ctx) = func_context(self.before, header.before_hunk_start) {
            head.push(' ');
            head.push_str(&ctx);
        }
        self.out.extend_from_slice(head.as_bytes());
        self.out.push(b'\n');
        for (kind, content) in lines {
            self.out.push(kind.to_prefix() as u8);
            self.out.extend_from_slice(content);
            if !content.ends_with(b"\n") {
                self.out.extend_from_slice(b"\n\\ No newline at end of file\n");
            }
        }
        Ok(())
    }

    fn finish(self) -> Self::Out {
        self.out
    }
}

/// Hunks do diff entre `old` e `new`, no formato do git (sem as linhas `diff --git`/`---`/`+++`).
pub fn hunks(old: &[u8], new: &[u8]) -> Vec<u8> {
    let input = InternedInput::new(old, new);
    let diff = diff_with_slider_heuristics(Algorithm::Myers, &input);
    let sink = GitHunks { before: old, out: Vec::new() };
    UnifiedDiff::new(&diff, &input, sink, ContextSize::symmetrical(3)).consume().unwrap_or_default()
}

/// (inserções, remoções) em linhas.
pub fn line_stats(old: &[u8], new: &[u8]) -> (u32, u32) {
    let input = InternedInput::new(old, new);
    let diff = diff_with_slider_heuristics(Algorithm::Myers, &input);
    (diff.count_additions(), diff.count_removals())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_style_hunks() {
        let old = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nN\n";
        let new = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\n";
        assert_eq!(String::from_utf8(hunks(old, new)).unwrap(), "@@ -11,4 +11,4 @@ j\n k\n l\n m\n-N\n+n\n");
        let h = hunks(b"no newline\n", b"no newline now");
        assert_eq!(String::from_utf8(h).unwrap(), "@@ -1 +1 @@\n-no newline\n+no newline now\n\\ No newline at end of file\n");
        assert_eq!(String::from_utf8(hunks(b"", b"x\n")).unwrap(), "@@ -0,0 +1 @@\n+x\n");
        assert_eq!(line_stats(b"one\n", b"one\ntwo\nthree\n"), (2, 0));
    }
}
