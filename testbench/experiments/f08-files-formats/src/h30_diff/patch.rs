//! `patch` contra o GNU patch 2.8. O front-end (opções, divisão do patch por arquivo, escolha do arquivo
//! alvo com -p, mensagens, `.orig`, `.rej`, exit codes) é nosso e igual pra todos; o que muda é o motor
//! que interpreta os hunks e os aplica:
//!
//! - `diffy` 0.5: `Patch::from_bytes` + `apply_bytes` (estrito, tudo ou nada, sem fuzz, sem posição);
//! - `flickzeug` 0.6: fork do diffy com aplicação parcial e fuzz (`apply_bytes_partial`);
//! - localizador nosso sobre o parser do diffy: procura a posição como o GNU (deslocamento pra frente
//!   antes de pra trás, fuzz cortando contexto das pontas, âncora no começo/fim do arquivo quando o
//!   contexto é assimétrico) e informa posição, deslocamento e fuzz de cada hunk.

use harness::{Candidate, Entry, Invocation, MemTree, Outcome};

/// Um hunk já interpretado: linhas com o marcador (' ', '-', '+') e o conteúdo com `\n` (sem `\n` quando
/// vem com "\ No newline at end of file").
#[derive(Clone, Debug)]
pub struct HunkIr {
    pub old_start: usize,
    pub old_len: usize,
    pub new_start: usize,
    pub new_len: usize,
    pub lines: Vec<(u8, Vec<u8>)>,
}

impl HunkIr {
    pub fn reversed(&self) -> HunkIr {
        HunkIr {
            old_start: self.new_start,
            old_len: self.new_len,
            new_start: self.old_start,
            new_len: self.old_len,
            lines: self
                .lines
                .iter()
                .map(|(k, l)| {
                    let k = match k {
                        b'-' => b'+',
                        b'+' => b'-',
                        o => *o,
                    };
                    (k, l.clone())
                })
                .collect(),
        }
    }

    fn header(&self) -> String {
        let range = |s: usize, l: usize| if l == 1 { format!("{s}") } else { format!("{s},{l}") };
        format!("@@ -{} +{} @@\n", range(self.old_start, self.old_len), range(self.new_start, self.new_len))
    }

    /// Texto do hunk em formato unificado (pro `.rej`).
    pub fn to_unified(&self) -> Vec<u8> {
        let mut out = self.header().into_bytes();
        for (k, l) in &self.lines {
            out.push(*k);
            out.extend_from_slice(l);
            if !l.ends_with(b"\n") {
                out.extend_from_slice(b"\n\\ No newline at end of file\n");
            }
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HunkResult {
    /// Aplicado. Posição (linha do arquivo novo), deslocamento e fuzz quando o motor informa.
    Applied { at: Option<usize>, offset: Option<isize>, fuzz: Option<usize> },
    /// Não aplicou. `at` é onde se esperava; `line_endings` quando só a quebra de linha impede.
    Failed { at: usize, line_endings: bool },
    /// Não tentado (o motor abortou o arquivo inteiro antes).
    Ignored,
}

pub struct FileApply {
    pub content: Vec<u8>,
    pub hunks: Vec<HunkResult>,
}

pub trait PatchEngine: Send + Sync {
    fn name(&self) -> String;
    fn krate(&self) -> (&'static str, &'static str);
    /// Interpreta os hunks de um arquivo (texto unificado com cabeçalho ---/+++).
    fn hunks(&self, chunk: &[u8]) -> Result<Vec<HunkIr>, String>;
    /// Aplica (ou, com `reverse`, desaplica) os hunks de um arquivo.
    fn apply(&self, base: &[u8], chunk: &[u8], reverse: bool, fuzz: usize) -> Result<FileApply, String>;
    /// Detecção de patch invertido ou já aplicado: o primeiro hunk não casa na direção pedida e casa na
    /// oposta.
    fn detect_reversed(&self, base: &[u8], chunk: &[u8], reverse: bool, fuzz: usize) -> bool;
}

fn diffy_to_ir(patch: &diffy::Patch<'_, [u8]>) -> Vec<HunkIr> {
    patch
        .hunks()
        .iter()
        .map(|h| HunkIr {
            old_start: h.old_range().start(),
            old_len: h.old_range().len(),
            new_start: h.new_range().start(),
            new_len: h.new_range().len(),
            lines: h
                .lines()
                .iter()
                .map(|l| match l {
                    diffy::Line::Context(t) => (b' ', t.to_vec()),
                    diffy::Line::Delete(t) => (b'-', t.to_vec()),
                    diffy::Line::Insert(t) => (b'+', t.to_vec()),
                })
                .collect(),
        })
        .collect()
}

/// diffy 0.5, do jeito que vem.
pub struct DiffyStrict;

impl PatchEngine for DiffyStrict {
    fn name(&self) -> String {
        "diffy apply_bytes (estrito)".into()
    }

    fn krate(&self) -> (&'static str, &'static str) {
        ("diffy", "0.5.2")
    }

    fn hunks(&self, chunk: &[u8]) -> Result<Vec<HunkIr>, String> {
        let p = diffy::Patch::from_bytes(chunk).map_err(|e| e.to_string())?;
        Ok(diffy_to_ir(&p))
    }

    fn apply(&self, base: &[u8], chunk: &[u8], reverse: bool, _fuzz: usize) -> Result<FileApply, String> {
        let p = diffy::Patch::from_bytes(chunk).map_err(|e| e.to_string())?;
        let p = if reverse { p.reverse() } else { p };
        let n = p.hunks().len();
        match diffy::apply_bytes(base, &p) {
            Ok(content) => Ok(FileApply {
                content,
                hunks: vec![HunkResult::Applied { at: None, offset: None, fuzz: None }; n],
            }),
            Err(e) => {
                // A crate só diz qual hunk falhou ("error applying hunk #N") e não aplica nenhum.
                let failed: usize =
                    e.to_string().rsplit('#').next().and_then(|s| s.trim().parse().ok()).unwrap_or(1);
                let hunks = diffy_to_ir(&p);
                let results = (1..=n)
                    .map(|i| {
                        if i == failed {
                            HunkResult::Failed { at: hunks[i - 1].old_start, line_endings: false }
                        } else {
                            HunkResult::Ignored
                        }
                    })
                    .collect();
                Ok(FileApply { content: base.to_vec(), hunks: results })
            }
        }
    }

    fn detect_reversed(&self, base: &[u8], chunk: &[u8], reverse: bool, _fuzz: usize) -> bool {
        let Ok(p) = diffy::Patch::from_bytes(chunk) else { return false };
        let (fwd, back) = if reverse { (p.reverse(), p.clone()) } else { (p.clone(), p.reverse()) };
        diffy::apply_bytes(base, &fwd).is_err() && diffy::apply_bytes(base, &back).is_ok()
    }
}

/// flickzeug 0.6: aplicação parcial com fuzz (similaridade 1.0, como o GNU).
pub struct FlickzeugPartial;

fn flick_config(fuzz: usize) -> flickzeug::ApplyConfig {
    flickzeug::ApplyConfig {
        line_end_strategy: flickzeug::LineEndHandling::KeepOriginal,
        fuzzy_config: flickzeug::FuzzyConfig {
            max_fuzz: fuzz,
            ignore_whitespace: false,
            ignore_case: false,
            similarity_threshold: 1.0,
        },
    }
}

impl PatchEngine for FlickzeugPartial {
    fn name(&self) -> String {
        "flickzeug apply_bytes_partial (fuzz)".into()
    }

    fn krate(&self) -> (&'static str, &'static str) {
        ("flickzeug", "0.6.0")
    }

    fn hunks(&self, chunk: &[u8]) -> Result<Vec<HunkIr>, String> {
        // O parser é o do diffy (de onde a crate saiu); a lista de hunks só serve pro .rej e pra contagem.
        DiffyStrict.hunks(chunk)
    }

    fn apply(&self, base: &[u8], chunk: &[u8], reverse: bool, fuzz: usize) -> Result<FileApply, String> {
        let d = flickzeug::Diff::from_bytes(chunk).map_err(|e| e.to_string())?;
        let d = if reverse { d.reverse() } else { d };
        let cfg = flick_config(fuzz);
        let all: Vec<(usize, usize)> =
            d.hunks().iter().map(|h| (h.old_range().start(), h.old_range().len())).collect();
        let partial = flickzeug::apply_bytes_partial(base, &d, &cfg);
        let rejected: Vec<(usize, usize)> =
            partial.rejected.iter().map(|h| (h.old_range().start(), h.old_range().len())).collect();
        let hunks = all
            .iter()
            .map(|r| {
                if rejected.contains(r) {
                    HunkResult::Failed { at: r.0, line_endings: false }
                } else {
                    HunkResult::Applied { at: None, offset: None, fuzz: None }
                }
            })
            .collect();
        Ok(FileApply { content: partial.content, hunks })
    }

    fn detect_reversed(&self, base: &[u8], chunk: &[u8], reverse: bool, fuzz: usize) -> bool {
        let Ok(first) = self.apply(base, chunk, reverse, fuzz) else { return false };
        if !matches!(first.hunks.first(), Some(HunkResult::Failed { .. })) {
            return false;
        }
        let Ok(d) = flickzeug::Diff::from_bytes(chunk) else { return false };
        let d = if reverse { d.reverse() } else { d };
        flickzeug::is_diff_applied_with_config(base, &d, &flick_config(0))
    }
}

/// Parser do diffy + localizador e aplicador nossos, com o comportamento observável do GNU patch.
pub struct OursOnDiffy;

fn strip_cr(l: &[u8]) -> Vec<u8> {
    match l.strip_suffix(b"\r\n") {
        Some(b) => [b, b"\n"].concat(),
        None => l.to_vec(),
    }
}

/// Procura onde o hunk casa, do fuzz 0 até `max_fuzz`. Devolve (linha 1-based da entrada, fuzz).
fn locate(input: &[&[u8]], hunk: &HunkIr, guess: isize, frozen: usize, max_fuzz: usize, crlf_blind: bool) -> Option<(usize, usize)> {
    (0..=max_fuzz).find_map(|f| locate_level(input, hunk, guess, frozen, f, crlf_blind).map(|p| (p, f)))
}

/// Procura onde o hunk casa com exatamente `fuzz` linhas de contexto ignoradas nas pontas.
fn locate_level(input: &[&[u8]], hunk: &HunkIr, guess: isize, frozen: usize, fuzz: usize, crlf_blind: bool) -> Option<usize> {
    let pattern: Vec<&[u8]> = hunk.lines.iter().filter(|(k, _)| *k != b'+').map(|(_, l)| l.as_slice()).collect();
    let n = pattern.len();
    let prefix = hunk.lines.iter().take_while(|(k, _)| *k == b' ').count();
    let suffix = hunk.lines.iter().rev().take_while(|(k, _)| *k == b' ').count();
    let context = prefix.max(suffix);
    let eq = |a: &[u8], b: &[u8]| if crlf_blind { strip_cr(a) == strip_cr(b) } else { a == b };
    let matches = |pos: usize, pf: usize, sf: usize| -> bool {
        // pos é 1-based; compara pattern[pf..n-sf] com input[pos-1+pf..]
        if pos == 0 || pos - 1 + n - sf > input.len() {
            return false;
        }
        (pf..n - sf).all(|k| eq(pattern[k], input[pos - 1 + k]))
    };
    if n == 0 {
        return (fuzz == 0).then_some(guess.max(1) as usize);
    }
    if fuzz > context {
        return None;
    }
    let pf = fuzz as isize + prefix as isize - context as isize;
    let sf = fuzz as isize + suffix as isize - context as isize;
    // Contexto assimétrico: menos contexto antes (hunk no começo do arquivo) só casa no começo; menos
    // contexto depois só casa no fim.
    if pf < 0 && hunk.old_start <= 1 {
        if sf < 0 && n != input.len() {
            return None;
        }
        return (frozen == 0 && matches(1, 0, sf.max(0) as usize)).then_some(1);
    }
    let pf = pf.max(0) as usize;
    if sf < 0 {
        let pos = input.len() as isize - n as isize + 1;
        return (pos >= 1 && pos as usize > frozen && matches(pos as usize, pf, 0)).then_some(pos as usize);
    }
    let sf = sf as usize;
    let max_where = input.len() as isize - (n - sf) as isize + 1;
    let min_where = frozen as isize + 1 - (prefix as isize - pf as isize);
    let max_pos = max_where - guess;
    let mut max_neg = guess - min_where;
    if guess <= max_neg {
        max_neg = guess - 1;
    }
    let max_off = max_pos.max(max_neg);
    let mut off = 0isize;
    while off <= max_off {
        if off <= max_pos && matches((guess + off) as usize, pf, sf) {
            return Some((guess + off) as usize);
        }
        if off > 0 && off <= max_neg && matches((guess - off) as usize, pf, sf) {
            return Some((guess - off) as usize);
        }
        off += 1;
    }
    None
}

impl PatchEngine for OursOnDiffy {
    fn name(&self) -> String {
        "localizador estilo GNU nosso sobre o parser do diffy".into()
    }

    fn krate(&self) -> (&'static str, &'static str) {
        ("diffy", "0.5.2")
    }

    fn hunks(&self, chunk: &[u8]) -> Result<Vec<HunkIr>, String> {
        DiffyStrict.hunks(chunk)
    }

    fn apply(&self, base: &[u8], chunk: &[u8], reverse: bool, fuzz: usize) -> Result<FileApply, String> {
        let mut hunks = self.hunks(chunk)?;
        if reverse {
            hunks = hunks.iter().map(HunkIr::reversed).collect();
        }
        let input: Vec<&[u8]> = base.split_inclusive(|&c| c == b'\n').collect();
        let mut out: Vec<u8> = Vec::new();
        let mut frozen = 0usize; // linhas da entrada já consumidas
        let mut in_offset = 0isize;
        let mut out_delta = 0isize; // linhas a mais na saída em relação à entrada
        let mut results = Vec::new();
        for h in &hunks {
            let first = if h.old_len == 0 { h.old_start + 1 } else { h.old_start };
            let guess = first as isize + in_offset;
            match locate(&input, h, guess, frozen, fuzz, false) {
                Some((pos, used_fuzz)) => {
                    let offset = pos as isize - first as isize;
                    in_offset = offset;
                    for line in &input[frozen..pos - 1] {
                        out.extend_from_slice(line);
                    }
                    let mut k = pos - 1;
                    for (kind, line) in &h.lines {
                        match kind {
                            b' ' => {
                                out.extend_from_slice(input[k]);
                                k += 1;
                            }
                            b'-' => k += 1,
                            _ => out.extend_from_slice(line),
                        }
                    }
                    let at = (pos as isize + out_delta) as usize;
                    out_delta += h.new_len as isize - h.old_len as isize;
                    frozen = k;
                    results.push(HunkResult::Applied { at: Some(at), offset: Some(offset), fuzz: Some(used_fuzz) });
                }
                None => {
                    let line_endings = locate(&input, h, guess, frozen, fuzz, true).is_some();
                    // O GNU informa a posição esperada em coordenadas do arquivo novo, sem o deslocamento.
                    let at = (h.old_start as isize + out_delta).max(1) as usize;
                    results.push(HunkResult::Failed { at, line_endings });
                }
            }
        }
        for line in &input[frozen.min(input.len())..] {
            out.extend_from_slice(line);
        }
        Ok(FileApply { content: out, hunks: results })
    }

    fn detect_reversed(&self, base: &[u8], chunk: &[u8], reverse: bool, fuzz: usize) -> bool {
        let Ok(hunks) = self.hunks(chunk) else { return false };
        let Some(h) = hunks.first() else { return false };
        let (fwd, back) = if reverse { (h.reversed(), h.clone()) } else { (h.clone(), h.reversed()) };
        let input: Vec<&[u8]> = base.split_inclusive(|&c| c == b'\n').collect();
        let start = |x: &HunkIr| (if x.old_len == 0 { x.old_start + 1 } else { x.old_start }) as isize;
        // Como o GNU: em cada nível de fuzz tenta pra frente e, se não casar, ao contrário.
        for f in 0..=fuzz {
            if locate_level(&input, &fwd, start(&fwd), 0, f, false).is_some() {
                return false;
            }
            if locate_level(&input, &back, start(&back), 0, f, false).is_some() {
                return true;
            }
        }
        false
    }
}

pub fn all_engines() -> Vec<Box<dyn PatchEngine>> {
    vec![Box::new(DiffyStrict), Box::new(FlickzeugPartial), Box::new(OursOnDiffy)]
}

// ---------------------------------------------------------------------------------------------------
// Front-end

/// Um arquivo do patch, já em texto unificado (contexto e normal são convertidos na divisão).
#[derive(Clone, Debug)]
struct Chunk {
    /// Linhas antes do primeiro hunk desde o fim do arquivo anterior (pra mensagem de arquivo ausente).
    leading: Vec<Vec<u8>>,
    old: Option<String>,
    new: Option<String>,
    new_mode: Option<u32>,
    /// Texto do arquivo: cabeçalho ---/+++ e hunks.
    text: Vec<u8>,
    /// Linha (1-based) do patch onde começa o primeiro hunk.
    first_hunk_line: usize,
}

enum SplitError {
    Garbage,
    Malformed { line: usize, text: Vec<u8> },
}

fn header_name(line: &[u8], prefix: &[u8]) -> Option<String> {
    let rest = line.strip_prefix(prefix)?;
    let rest = rest.strip_suffix(b"\n").unwrap_or(rest);
    let name = rest.split(|&c| c == b'\t').next().unwrap_or(rest);
    let name = String::from_utf8_lossy(name).trim_end().to_string();
    Some(name)
}

fn parse_range(s: &str) -> Option<(usize, usize)> {
    let (a, b) = match s.split_once(',') {
        Some((a, b)) => (a.parse().ok()?, b.parse().ok()?),
        None => (s.parse().ok()?, 1),
    };
    Some((a, b))
}

fn is_normal_command(line: &[u8]) -> bool {
    let s = String::from_utf8_lossy(line);
    let s = s.trim_end();
    let pos = s.find(['a', 'c', 'd']);
    match pos {
        Some(p) if p > 0 => {
            let (l, r) = (&s[..p], &s[p + 1..]);
            let ok = |x: &str| !x.is_empty() && x.split(',').all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            ok(l) && ok(r)
        }
        _ => false,
    }
}

/// Junta o "\ No newline at end of file" à linha anterior (tira o `\n` dela).
fn push_line(out: &mut Vec<(u8, Vec<u8>)>, kind: u8, body: &[u8]) {
    out.push((kind, body.to_vec()));
}

fn mark_no_newline(out: &mut [(u8, Vec<u8>)]) {
    if let Some((_, l)) = out.last_mut()
        && l.ends_with(b"\n")
    {
        l.pop();
    }
}

fn emit_unified(text: &mut Vec<u8>, old_start: usize, new_start: usize, lines: &[(u8, Vec<u8>)]) {
    let old_len = lines.iter().filter(|(k, _)| *k != b'+').count();
    let new_len = lines.iter().filter(|(k, _)| *k != b'-').count();
    let h = HunkIr { old_start, old_len, new_start, new_len, lines: lines.to_vec() };
    text.extend_from_slice(&h.to_unified());
}

/// Converte os hunks de um diff de contexto (a partir da linha "***************") pra texto unificado.
fn context_to_unified(lines: &[&[u8]], mut i: usize, old: &str, new: &str) -> Result<(Vec<u8>, usize), SplitError> {
    let mut text = format!("--- {old}\n+++ {new}\n").into_bytes();
    let range = |l: &[u8], open: &str, close: &str| -> Option<(usize, usize)> {
        let s = String::from_utf8_lossy(l);
        let inner = s.trim_end().strip_prefix(open)?.strip_suffix(close)?.trim().to_string();
        match inner.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => {
                let a: usize = inner.parse().ok()?;
                Some((a, a))
            }
        }
    };
    let mut any = false;
    while i < lines.len() && lines[i].starts_with(b"***************") {
        any = true;
        i += 1;
        let Some((o_first, _)) = lines.get(i).and_then(|l| range(l, "*** ", " ****")) else {
            return Err(SplitError::Malformed { line: i + 1, text: lines.get(i).map(|l| l.to_vec()).unwrap_or_default() });
        };
        i += 1;
        let mut old_sec: Vec<(u8, Vec<u8>)> = Vec::new();
        while i < lines.len() && !lines[i].starts_with(b"--- ") {
            let l = lines[i];
            if l.starts_with(b"\\") {
                mark_no_newline(&mut old_sec);
            } else if l.len() >= 2 {
                push_line(&mut old_sec, l[0], &l[2..]);
            }
            i += 1;
        }
        let Some((n_first, _)) = lines.get(i).and_then(|l| range(l, "--- ", " ----")) else {
            return Err(SplitError::Malformed { line: i + 1, text: lines.get(i).map(|l| l.to_vec()).unwrap_or_default() });
        };
        i += 1;
        let mut new_sec: Vec<(u8, Vec<u8>)> = Vec::new();
        while i < lines.len()
            && !lines[i].starts_with(b"***************")
            && !(lines[i].starts_with(b"*** ") && lines.get(i + 1).is_some_and(|n| n.starts_with(b"--- ")))
            && (lines[i].len() >= 2 && matches!(lines[i][0], b' ' | b'+' | b'!' | b'-') || lines[i].starts_with(b"\\"))
        {
            let l = lines[i];
            if l.starts_with(b"\\") {
                mark_no_newline(&mut new_sec);
            } else {
                push_line(&mut new_sec, l[0], &l[2..]);
            }
            i += 1;
        }
        // Junta as duas seções: contexto anda nas duas, '-'/'+' numa só, '!' vira '-' no velho e '+' no novo.
        let mut merged: Vec<(u8, Vec<u8>)> = Vec::new();
        if old_sec.is_empty() {
            merged.extend(new_sec.iter().map(|(k, l)| (if *k == b'!' { b'+' } else { *k }, l.clone())));
        } else if new_sec.is_empty() {
            merged.extend(old_sec.iter().map(|(k, l)| (if *k == b'!' { b'-' } else { *k }, l.clone())));
        } else {
            let (mut a, mut b) = (0usize, 0usize);
            while a < old_sec.len() || b < new_sec.len() {
                let ka = old_sec.get(a).map(|x| x.0);
                let kb = new_sec.get(b).map(|x| x.0);
                match (ka, kb) {
                    (Some(b'-'), _) => {
                        merged.push((b'-', old_sec[a].1.clone()));
                        a += 1;
                    }
                    (_, Some(b'+')) => {
                        merged.push((b'+', new_sec[b].1.clone()));
                        b += 1;
                    }
                    (Some(b'!'), Some(b'!')) => {
                        while old_sec.get(a).is_some_and(|x| x.0 == b'!') {
                            merged.push((b'-', old_sec[a].1.clone()));
                            a += 1;
                        }
                        while new_sec.get(b).is_some_and(|x| x.0 == b'!') {
                            merged.push((b'+', new_sec[b].1.clone()));
                            b += 1;
                        }
                    }
                    (Some(_), Some(_)) => {
                        merged.push((b' ', old_sec[a].1.clone()));
                        a += 1;
                        b += 1;
                    }
                    (Some(_), None) => {
                        merged.push((b'-', old_sec[a].1.clone()));
                        a += 1;
                    }
                    (None, Some(_)) => {
                        merged.push((b'+', new_sec[b].1.clone()));
                        b += 1;
                    }
                    (None, None) => break,
                }
            }
        }
        emit_unified(&mut text, o_first, n_first, &merged);
    }
    if !any {
        return Err(SplitError::Garbage);
    }
    Ok((text, i))
}

/// Converte comandos de diff normal ("5c5", "3a4,5", "4,5d3") pra hunks unificados sem contexto.
fn normal_to_unified(lines: &[&[u8]], mut i: usize) -> Result<(Vec<u8>, usize), SplitError> {
    let mut text = b"--- a\n+++ b\n".to_vec();
    while i < lines.len() && is_normal_command(lines[i]) {
        let cmd = String::from_utf8_lossy(lines[i]).trim_end().to_string();
        let p = cmd.find(['a', 'c', 'd']).expect("comando");
        let parse = |s: &str| -> (usize, usize) {
            match s.split_once(',') {
                Some((a, b)) => (a.parse().unwrap_or(0), b.parse().unwrap_or(0)),
                None => {
                    let a = s.parse().unwrap_or(0);
                    (a, a)
                }
            }
        };
        let (l1, _) = parse(&cmd[..p]);
        let (r1, _) = parse(&cmd[p + 1..]);
        i += 1;
        let mut hunk: Vec<(u8, Vec<u8>)> = Vec::new();
        while i < lines.len() && (lines[i].starts_with(b"< ") || lines[i].starts_with(b"> ") || lines[i].starts_with(b"---") || lines[i].starts_with(b"\\")) {
            let l = lines[i];
            if l.starts_with(b"\\") {
                mark_no_newline(&mut hunk);
            } else if l.starts_with(b"< ") {
                push_line(&mut hunk, b'-', &l[2..]);
            } else if l.starts_with(b"> ") {
                push_line(&mut hunk, b'+', &l[2..]);
            }
            i += 1;
        }
        // Nas faixas vazias ("3a4", "4d3") o número já é a linha anterior, igual ao unificado.
        emit_unified(&mut text, l1, r1, &hunk);
    }
    Ok((text, i))
}

/// Divide a entrada em arquivos. Reconhece unificado (com cabeçalhos do git), contexto e normal; os dois
/// últimos são convertidos pra texto unificado.
fn split(input: &[u8]) -> Result<Vec<Chunk>, SplitError> {
    let lines: Vec<&[u8]> = input.split_inclusive(|&c| c == b'\n').collect();
    let mut chunks = Vec::new();
    let mut i = 0;
    let mut leading_start = 0;
    let mut pending_mode: Option<u32> = None;
    while i < lines.len() {
        let l = lines[i];
        if l.starts_with(b"diff --git ") {
            pending_mode = None;
        }
        if let Some(m) = l.strip_prefix(b"new file mode ") {
            pending_mode = u32::from_str_radix(String::from_utf8_lossy(m).trim(), 8).ok().map(|m| m & 0o7777);
        }
        if l.starts_with(b"--- ") && lines.get(i + 1).is_some_and(|n| n.starts_with(b"+++ ")) {
            let old = header_name(l, b"--- ");
            let new = header_name(lines[i + 1], b"+++ ");
            let mut text: Vec<u8> = Vec::new();
            text.extend_from_slice(l);
            text.extend_from_slice(lines[i + 1]);
            let leading: Vec<Vec<u8>> = lines[leading_start..i + 2].iter().map(|x| x.to_vec()).collect();
            let mut j = i + 2;
            let first_hunk_line = j + 1;
            let mut any = false;
            while j < lines.len() && lines[j].starts_with(b"@@ ") {
                any = true;
                let h = String::from_utf8_lossy(lines[j]).to_string();
                let parts: Vec<&str> = h.split_whitespace().collect();
                let old_r = parts.get(1).and_then(|s| s.strip_prefix('-')).and_then(parse_range);
                let new_r = parts.get(2).and_then(|s| s.strip_prefix('+')).and_then(parse_range);
                let (Some((_, mut ol)), Some((_, mut nl))) = (old_r, new_r) else {
                    return Err(SplitError::Malformed { line: j + 1, text: lines[j].to_vec() });
                };
                text.extend_from_slice(lines[j]);
                j += 1;
                while ol > 0 || nl > 0 {
                    let Some(hl) = lines.get(j) else {
                        return Err(SplitError::Malformed { line: j + 1, text: Vec::new() });
                    };
                    match hl.first() {
                        Some(b' ') | Some(b'\n') => {
                            ol = ol.saturating_sub(1);
                            nl = nl.saturating_sub(1);
                        }
                        Some(b'-') => ol = ol.saturating_sub(1),
                        Some(b'+') => nl = nl.saturating_sub(1),
                        Some(b'\\') => {}
                        _ => return Err(SplitError::Malformed { line: j + 1, text: hl.to_vec() }),
                    }
                    text.extend_from_slice(hl);
                    j += 1;
                }
                if lines.get(j).is_some_and(|x| x.starts_with(b"\\")) {
                    text.extend_from_slice(lines[j]);
                    j += 1;
                }
            }
            if any {
                chunks.push(Chunk {
                    leading,
                    old,
                    new,
                    new_mode: pending_mode.take(),
                    text,
                    first_hunk_line,
                });
                leading_start = j;
                i = j;
                continue;
            }
        }
        if l.starts_with(b"*** ")
            && lines.get(i + 1).is_some_and(|n| n.starts_with(b"--- "))
            && lines.get(i + 2).is_some_and(|n| n.starts_with(b"***************"))
        {
            let old = header_name(l, b"*** ");
            let new = header_name(lines[i + 1], b"--- ");
            let leading: Vec<Vec<u8>> = lines[leading_start..i + 2].iter().map(|x| x.to_vec()).collect();
            let (text, next) = context_to_unified(&lines, i + 2, old.as_deref().unwrap_or("a"), new.as_deref().unwrap_or("b"))?;
            chunks.push(Chunk { leading, old, new, new_mode: None, text, first_hunk_line: i + 3 });
            leading_start = next;
            i = next;
            continue;
        }
        if is_normal_command(l) {
            let (text, next) = normal_to_unified(&lines, i)?;
            chunks.push(Chunk {
                leading: Vec::new(),
                old: None,
                new: None,
                new_mode: None,
                text,
                first_hunk_line: i + 1,
            });
            leading_start = next;
            i = next;
            continue;
        }
        i += 1;
    }
    if chunks.is_empty() && !input.iter().all(|c| c.is_ascii_whitespace()) {
        return Err(SplitError::Garbage);
    }
    Ok(chunks)
}

#[derive(Default)]
struct PatchOpts {
    strip: Option<usize>,
    input: Option<String>,
    reverse: bool,
    forward: bool,
    batch: bool,
    force: bool,
    silent: bool,
    dry_run: bool,
    output: Option<String>,
    backup: bool,
    no_backup_if_mismatch: bool,
    fuzz: Option<usize>,
    remove_empty: bool,
    positional: Vec<String>,
}

fn parse_opts(args: &[String]) -> Result<PatchOpts, String> {
    let mut o = PatchOpts::default();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if let Some(long) = a.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let mut val = || -> Result<String, String> {
                if let Some(v) = &value {
                    return Ok(v.clone());
                }
                let v = args.get(i).cloned().ok_or_else(|| format!("opção --{name} sem argumento"))?;
                i += 1;
                Ok(v)
            };
            match name {
                "strip" => o.strip = Some(val()?.parse().map_err(|_| "strip")?),
                "input" => o.input = Some(val()?),
                "reverse" => o.reverse = true,
                "forward" => o.forward = true,
                "batch" => o.batch = true,
                "force" => o.force = true,
                "silent" | "quiet" => o.silent = true,
                "dry-run" => o.dry_run = true,
                "output" => o.output = Some(val()?),
                "backup" => o.backup = true,
                "no-backup-if-mismatch" => o.no_backup_if_mismatch = true,
                "fuzz" => o.fuzz = Some(val()?.parse().map_err(|_| "fuzz")?),
                "remove-empty-files" => o.remove_empty = true,
                _ => return Err(format!("opção --{name} não implementada no front-end")),
            }
            continue;
        }
        if a.len() > 1 && a.starts_with('-') {
            let chars: Vec<char> = a[1..].chars().collect();
            let mut k = 0;
            while k < chars.len() {
                let c = chars[k];
                k += 1;
                let mut val = |k: &mut usize| -> Result<String, String> {
                    if *k < chars.len() {
                        let v: String = chars[*k..].iter().collect();
                        *k = chars.len();
                        Ok(v)
                    } else {
                        let v = args.get(i).cloned().ok_or_else(|| format!("opção -{c} sem argumento"))?;
                        i += 1;
                        Ok(v)
                    }
                };
                match c {
                    'p' => o.strip = Some(val(&mut k)?.parse().map_err(|_| "strip")?),
                    'i' => o.input = Some(val(&mut k)?),
                    'o' => o.output = Some(val(&mut k)?),
                    'F' => o.fuzz = Some(val(&mut k)?.parse().map_err(|_| "fuzz")?),
                    'R' => o.reverse = true,
                    'N' => o.forward = true,
                    't' => o.batch = true,
                    'f' => o.force = true,
                    's' => o.silent = true,
                    'b' => o.backup = true,
                    'E' => o.remove_empty = true,
                    'u' => {}
                    _ => return Err(format!("opção -{c} não implementada no front-end")),
                }
            }
            continue;
        }
        o.positional.push(a.clone());
    }
    Ok(o)
}

fn strip_path(name: &str, strip: Option<usize>) -> String {
    match strip {
        None => name.rsplit('/').next().unwrap_or(name).to_string(),
        Some(n) => {
            let parts: Vec<&str> = name.split('/').collect();
            if n >= parts.len() { parts.last().copied().unwrap_or("").to_string() } else { parts[n..].join("/") }
        }
    }
}

/// Arquivo alvo: o posicional, ou o nome novo numa criação, ou o primeiro dos nomes (velho, novo) que
/// existe, depois do -p.
fn pick_target(o: &PatchOpts, files: &MemTree, old: &Option<String>, new: &Option<String>) -> Option<String> {
    if let Some(p) = o.positional.first() {
        return Some(crate::common::relative(p));
    }
    let names: Vec<String> = [old, new]
        .iter()
        .filter_map(|n| n.as_deref())
        .filter(|n| *n != "/dev/null")
        .map(|n| strip_path(n, o.strip))
        .collect();
    if old.as_deref() == Some("/dev/null") {
        return names.last().cloned();
    }
    names.iter().find(|n| matches!(files.get(n), Some(Entry::File { .. }))).cloned()
}

/// Conteúdo do `.rej`: as duas linhas de cabeçalho do patch como vieram (com data, se houver) e os
/// hunks rejeitados em formato unificado.
fn reject_file<'a>(chunk_text: &[u8], hunks: impl Iterator<Item = &'a HunkIr>) -> Vec<u8> {
    let mut rej: Vec<u8> = Vec::new();
    for l in chunk_text.split_inclusive(|&c| c == b'\n').take(2) {
        rej.extend_from_slice(l);
        if !l.ends_with(b"\n") {
            rej.push(b'\n');
        }
    }
    for h in hunks {
        rej.extend_from_slice(&h.to_unified());
    }
    rej
}

/// Últimos nomes ---/+++ antes da linha `line` (1-based).
fn names_before(text: &[u8], line: usize) -> (Option<String>, Option<String>) {
    let lines: Vec<&[u8]> = text.split_inclusive(|&c| c == b'\n').take(line).collect();
    for k in (0..lines.len().saturating_sub(1)).rev() {
        if lines[k].starts_with(b"--- ") && lines[k + 1].starts_with(b"+++ ") {
            return (header_name(lines[k], b"--- "), header_name(lines[k + 1], b"+++ "));
        }
    }
    (None, None)
}

pub struct PatchCandidate {
    pub engine: Box<dyn PatchEngine>,
}

impl Candidate for PatchCandidate {
    fn name(&self) -> String {
        format!("{} + front-end GNU nosso", self.engine.name())
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        if inv.script.is_some() || inv.program() != Some("patch") {
            return Outcome::unsupported("só casos argv de patch");
        }
        match self.run_inner(inv) {
            Ok(out) => out,
            Err(why) => Outcome::unsupported(why),
        }
    }
}

impl PatchCandidate {
    fn run_inner(&self, inv: &Invocation) -> Result<Outcome, String> {
        let o = parse_opts(inv.args())?;
        let mut files = inv.files.clone();
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let patch_name = o.input.clone().or_else(|| o.positional.get(1).cloned());
        let raw: Vec<u8> = match &patch_name {
            Some(p) => match files.read(&crate::common::relative(p)) {
                Some(d) => d.to_vec(),
                None => {
                    err.extend_from_slice(format!("patch: **** Can't open patch file {p} : No such file or directory\n").as_bytes());
                    return Ok(Outcome::exited(out, err, 2, files));
                }
            },
            None => inv.stdin.clone(),
        };
        // Patch com CRLF: o GNU tira os CRs e avisa.
        let text: Vec<u8> = if raw.windows(2).any(|w| w == b"\r\n") {
            out.extend_from_slice(b"(Stripping trailing CRs from patch; use --binary to disable.)\n");
            raw.split_inclusive(|&c| c == b'\n').flat_map(strip_cr).collect()
        } else {
            raw
        };
        let chunks = match split(&text) {
            Ok(c) => c,
            Err(SplitError::Garbage) => {
                err.extend_from_slice(b"patch: **** Only garbage was found in the patch input.\n");
                return Ok(Outcome::exited(out, err, 2, files));
            }
            Err(SplitError::Malformed { line, text: bad }) => {
                // O GNU já anunciou o arquivo quando acha o hunk quebrado.
                let (old, new) = names_before(&text, line);
                if let Some(t) = pick_target(&o, &files, &old, &new)
                    && !o.silent
                {
                    out.extend_from_slice(format!("patching file {t}\n").as_bytes());
                }
                let shown = if bad.is_empty() { text.split(|&c| c == b'\n').nth(line.saturating_sub(2)).unwrap_or(b"").to_vec() } else { bad };
                let line = line.min(text.split_inclusive(|&c| c == b'\n').count());
                err.extend_from_slice(format!("patch: **** malformed patch at line {line}: ").as_bytes());
                err.extend_from_slice(&shown);
                if !shown.ends_with(b"\n") {
                    err.push(b'\n');
                }
                return Ok(Outcome::exited(out, err, 2, files));
            }
        };
        let mut status = 0;
        for chunk in &chunks {
            let hunks = self.engine.hunks(&chunk.text)?;
            let is_create = chunk.old.as_deref() == Some("/dev/null");
            let is_delete = chunk.new.as_deref() == Some("/dev/null");
            let target = pick_target(&o, &files, &chunk.old, &chunk.new);
            let Some(target) = target else {
                out.extend_from_slice(
                    format!(
                        "can't find file to patch at input line {}\nPerhaps you used the wrong -p or --strip option?\n\
                         The text leading up to this was:\n--------------------------\n",
                        chunk.first_hunk_line
                    )
                    .as_bytes(),
                );
                for l in &chunk.leading {
                    out.push(b'|');
                    out.extend_from_slice(l);
                }
                out.extend_from_slice(b"--------------------------\nFile to patch: \nSkip this patch? [y] \nSkipping patch.\n");
                let n = hunks.len();
                out.extend_from_slice(format!("{n} out of {n} hunk{} ignored\n", if n == 1 { "" } else { "s" }).as_bytes());
                status = status.max(1);
                continue;
            };
            let base: Vec<u8> = files.read(&target).map(|d| d.to_vec()).unwrap_or_default();
            let shown = match &o.output {
                Some(outf) => format!("{outf} (read from {target})"),
                None => target.clone(),
            };
            if !o.silent {
                let verb = if o.dry_run { "checking" } else { "patching" };
                out.extend_from_slice(format!("{verb} file {shown}\n").as_bytes());
            }
            let fuzz = o.fuzz.unwrap_or(2);
            let mut reverse = o.reverse;
            let mut skipped = false;
            let mut mismatch = false;
            if !is_create && !o.force && self.engine.detect_reversed(&base, &chunk.text, reverse, fuzz) {
                let what = if reverse { "Unreversed" } else { "Reversed" };
                if o.forward {
                    out.extend_from_slice(format!("{what} (or previously applied) patch detected!  Skipping patch.\n").as_bytes());
                    skipped = true;
                } else if o.batch || o.force {
                    let assume = if reverse { "Ignoring -R." } else { "Assuming -R." };
                    out.extend_from_slice(format!("{what} (or previously applied) patch detected!  {assume}\n").as_bytes());
                    reverse = !reverse;
                    mismatch = true;
                } else {
                    let assume = if reverse { "Ignore -R?" } else { "Assume -R?" };
                    out.extend_from_slice(
                        format!("{what} (or previously applied) patch detected!  {assume} [n] \nApply anyway? [n] \nSkipping patch.\n")
                            .as_bytes(),
                    );
                    skipped = true;
                }
            }
            let total = hunks.len();
            if skipped {
                out.extend_from_slice(
                    format!(
                        "{total} out of {total} hunk{} ignored -- saving rejects to file {target}.rej\n",
                        if total == 1 { "" } else { "s" }
                    )
                    .as_bytes(),
                );
                if !o.dry_run {
                    let rej = reject_file(&chunk.text, hunks.iter());
                    files.insert(&format!("{target}.rej"), Entry::file(rej, 0o644));
                }
                status = status.max(1);
                continue;
            }
            let first = self.engine.apply(&base, &chunk.text, reverse, fuzz)?;
            let mut failed: Vec<usize> = Vec::new();
            let mut failed_shift: Vec<isize> = Vec::new();
            for (idx, r) in first.hunks.iter().enumerate() {
                let n = idx + 1;
                match r {
                    HunkResult::Applied { at: Some(at), offset: Some(off), fuzz: Some(fz) } => {
                        if *off != 0 || *fz != 0 {
                            mismatch = true;
                            let mut msg = format!("Hunk #{n} succeeded at {at}");
                            if *fz != 0 {
                                msg.push_str(&format!(" with fuzz {fz}"));
                            }
                            if *off != 0 {
                                msg.push_str(&format!(" (offset {off} line{})", if *off == 1 { "" } else { "s" }));
                            }
                            msg.push_str(".\n");
                            out.extend_from_slice(msg.as_bytes());
                        }
                    }
                    HunkResult::Applied { .. } => {}
                    HunkResult::Failed { at, line_endings } => {
                        mismatch = true;
                        failed.push(idx);
                        let declared = hunks.get(idx).map(|h| if reverse { h.new_start } else { h.old_start }).unwrap_or(*at);
                        failed_shift.push(*at as isize - declared as isize);
                        let le = if *line_endings { " (different line endings)" } else { "" };
                        out.extend_from_slice(format!("Hunk #{n} FAILED at {at}{le}.\n").as_bytes());
                    }
                    HunkResult::Ignored => {
                        failed.push(idx);
                        failed_shift.push(0);
                    }
                }
            }
            if !failed.is_empty() {
                status = status.max(1);
                out.extend_from_slice(
                    format!(
                        "{} out of {total} hunk{} FAILED -- saving rejects to file {target}.rej\n",
                        failed.len(),
                        if total == 1 { "" } else { "s" }
                    )
                    .as_bytes(),
                );
            }
            if o.dry_run {
                continue;
            }
            if (o.backup || (mismatch && !o.no_backup_if_mismatch)) && !is_create {
                files.insert(&format!("{target}.orig"), Entry::file(base.clone(), 0o644));
            }
            if !failed.is_empty() {
                // O GNU grava o hunk rejeitado com as faixas deslocadas pelo saldo de linhas já aplicado.
                let shifted: Vec<HunkIr> = failed
                    .iter()
                    .zip(&failed_shift)
                    .filter_map(|(i, d)| {
                        hunks.get(*i).map(|h| {
                            let mut h = h.clone();
                            h.old_start = (h.old_start as isize + d).max(0) as usize;
                            h.new_start = (h.new_start as isize + d).max(0) as usize;
                            h
                        })
                    })
                    .collect();
                let rej = reject_file(&chunk.text, shifted.iter());
                files.insert(&format!("{target}.rej"), Entry::file(rej, 0o644));
            }
            let dest = o.output.clone().map(|p| crate::common::relative(&p)).unwrap_or(target.clone());
            if (is_delete || o.remove_empty) && first.content.is_empty() && o.output.is_none() {
                files.entries.remove(&dest);
            } else {
                // O GNU cria a saída do -o com 0600 (arquivo temporário renomeado).
                let mode = match files.get(&dest) {
                    Some(Entry::File { mode, .. }) => *mode,
                    _ if o.output.is_some() => 0o600,
                    _ => chunk.new_mode.unwrap_or(0o644),
                };
                files.insert(&dest, Entry::file(first.content, mode));
            }
        }
        Ok(Outcome::exited(out, err, status, normalize_tree(files)))
    }
}

/// Recria diretórios intermediários implícitos (MemTree::insert já cria) e mantém a árvore estável.
fn normalize_tree(t: MemTree) -> MemTree {
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunk(old_start: usize, lines: &[(u8, &str)]) -> HunkIr {
        let old_len = lines.iter().filter(|(k, _)| *k != b'+').count();
        let new_len = lines.iter().filter(|(k, _)| *k != b'-').count();
        HunkIr {
            old_start,
            old_len,
            new_start: old_start,
            new_len,
            lines: lines.iter().map(|(k, l)| (*k, l.as_bytes().to_vec())).collect(),
        }
    }

    #[test]
    fn forward_offset_wins_ties() {
        let file = "x\ny\nz\nm\nx\ny\nz\n";
        let input: Vec<&[u8]> = file.as_bytes().split_inclusive(|&c| c == b'\n').collect();
        let h = hunk(3, &[(b' ', "x\n"), (b'-', "y\n"), (b'+', "Y\n"), (b' ', "z\n")]);
        assert_eq!(locate(&input, &h, 3, 0, 2, false), Some((5, 0)));
    }

    #[test]
    fn fuzz_ignores_outer_context() {
        let file = "a\nB\nc\nd\ne\nf\ng\n";
        let input: Vec<&[u8]> = file.as_bytes().split_inclusive(|&c| c == b'\n').collect();
        let h = hunk(2, &[(b' ', "b\n"), (b' ', "c\n"), (b'-', "d\n"), (b'+', "D\n"), (b' ', "e\n"), (b' ', "f\n")]);
        assert_eq!(locate(&input, &h, 2, 0, 2, false), Some((2, 1)));
        assert_eq!(locate(&input, &h, 2, 0, 0, false), None);
    }

    #[test]
    fn strip_and_split() {
        assert_eq!(strip_path("a/b/c.txt", Some(1)), "b/c.txt");
        assert_eq!(strip_path("a/b/c.txt", None), "c.txt");
        assert!(is_normal_command(b"5c5\n"));
        assert!(!is_normal_command(b"abc\n"));
    }
}
