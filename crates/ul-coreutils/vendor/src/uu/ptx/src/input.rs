// Porte pseudo-linus: leitura do texto, divisão em contextos (sentenças ou linhas) e busca das
// palavras-chave. O `ptx` do GNU trabalha em bytes sobre o arquivo inteiro: uma sentença atravessa
// as quebras de linha, e a palavra padrão é uma sequência de letras ASCII.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use regex::bytes::Regex;
use rustc_hash::FxHashSet;
use sysio::fs::File;
use sysio::io::{Read, stdin};
use uucore::display::Quotable;
use uucore::error::{FromIo, UResult};

/// Como o texto se parte em contextos.
pub(crate) enum ContextSplit {
    /// O fim de sentença padrão do GNU (`[.?!][]"')}]*($|\t|  )[ \t\n]*`).
    Sentences,
    /// Um contexto por linha (o padrão com `-r` e com `-G`).
    Lines,
    /// `-S ''`: o arquivo inteiro é um contexto só.
    Whole,
    /// `-S REGEXP`.
    Custom(Regex),
}

/// Como as palavras-chave são reconhecidas.
pub(crate) enum WordMatcher {
    /// Sequências de letras ASCII (o padrão do GNU com extensões).
    Letters,
    /// Qualquer coisa sem espaço, tab ou quebra de linha (o padrão do modo tradicional).
    NonSpace,
    /// `-W REGEXP`, ou os caracteres fora do arquivo de quebras de `-b`.
    Pattern(Regex),
}

/// Um contexto: o texto de uma sentença ou de uma linha, com cada espaço em branco trocado por um
/// espaço comum.
pub(crate) struct Context {
    pub(crate) text: Vec<u8>,
    /// Posição de `text[0]` no arquivo, pra descobrir a linha de cada palavra.
    pub(crate) offset: usize,
    /// Fim da referência lida do começo da linha (`-r`); 0 quando não há.
    pub(crate) ref_end: usize,
    /// Onde começa o texto que se mostra (depois da referência e do branco que a segue).
    pub(crate) text_start: usize,
}

/// O conteúdo de um arquivo de entrada.
pub(crate) struct FileContent {
    pub(crate) name: OsString,
    pub(crate) contexts: Vec<Context>,
    /// Posições dos `\n` do arquivo, em ordem.
    pub(crate) newlines: Vec<usize>,
}

impl FileContent {
    /// A linha (a partir de 1) da posição `context.offset + index`.
    pub(crate) fn line_of(&self, context: &Context, index: usize) -> usize {
        let position = context.offset + index;
        self.newlines.partition_point(|&n| n < position) + 1
    }
}

/// Uma ocorrência de palavra-chave.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Occurrence {
    /// A chave de ordenação (a palavra, em maiúsculas com `-f`).
    pub(crate) key: Vec<u8>,
    pub(crate) file: usize,
    pub(crate) context: usize,
    pub(crate) position: usize,
    pub(crate) len: usize,
}

fn is_blank(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Lê o arquivo (ou a entrada padrão, com `-`) inteiro.
pub(crate) fn read_all(name: &OsStr) -> UResult<Vec<u8>> {
    let mut data = Vec::new();
    if name == OsStr::new("-") {
        stdin()
            .read_to_end(&mut data)
            .map_err_context(|| name.maybe_quote().to_string())?;
    } else {
        let mut file =
            File::open(Path::new(name)).map_err_context(|| name.maybe_quote().to_string())?;
        file.read_to_end(&mut data)
            .map_err_context(|| name.maybe_quote().to_string())?;
    }
    Ok(data)
}

/// O fim da próxima sentença a partir de `from`: o par `(fim da sentença, fim do separador)`.
fn next_sentence_end(data: &[u8], from: usize) -> Option<(usize, usize)> {
    let n = data.len();
    let mut i = from;
    while i < n {
        if matches!(data[i], b'.' | b'?' | b'!') {
            let mut j = i + 1;
            while j < n && matches!(data[j], b']' | b'"' | b'\'' | b')' | b'}') {
                j += 1;
            }
            let boundary = j == n
                || data[j] == b'\n'
                || data[j] == b'\t'
                || (data[j] == b' ' && j + 1 < n && data[j + 1] == b' ');
            if boundary {
                let mut k = j;
                while k < n && matches!(data[k], b' ' | b'\t' | b'\n') {
                    k += 1;
                }
                return Some((j, k));
            }
        }
        i += 1;
    }
    None
}

/// Os intervalos `[início, fim)` dos contextos brutos do texto.
fn split_ranges(data: &[u8], split: &ContextSplit) -> Vec<(usize, usize)> {
    let n = data.len();
    let mut ranges = Vec::new();
    let mut start = 0;
    match split {
        ContextSplit::Whole => ranges.push((0, n)),
        ContextSplit::Lines => {
            while start < n {
                match data[start..].iter().position(|&b| b == b'\n') {
                    Some(rel) => {
                        ranges.push((start, start + rel));
                        start += rel + 1;
                    }
                    None => {
                        ranges.push((start, n));
                        start = n;
                    }
                }
            }
        }
        ContextSplit::Sentences => {
            while start < n {
                match next_sentence_end(data, start) {
                    Some((end, next)) => {
                        ranges.push((start, end));
                        start = next;
                    }
                    None => {
                        ranges.push((start, n));
                        start = n;
                    }
                }
            }
        }
        ContextSplit::Custom(re) => {
            for m in re.find_iter(data) {
                if m.end() == m.start() || m.start() < start {
                    continue;
                }
                ranges.push((start, m.end()));
                start = m.end();
            }
            if start < n {
                ranges.push((start, n));
            }
        }
    }
    ranges
}

/// Parte o arquivo em contextos. Cada contexto perde o branco das pontas e troca cada branco do
/// meio (quebra de linha, tab) por um espaço.
pub(crate) fn read_contexts(
    name: OsString,
    data: &[u8],
    split: &ContextSplit,
    input_ref: bool,
) -> FileContent {
    let newlines: Vec<usize> = data
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'\n')
        .map(|(i, _)| i)
        .collect();

    let mut contexts = Vec::new();
    for (mut start, mut end) in split_ranges(data, split) {
        while start < end && is_blank(data[start]) {
            start += 1;
        }
        while end > start && is_blank(data[end - 1]) {
            end -= 1;
        }
        if start >= end {
            continue;
        }
        let text: Vec<u8> = data[start..end]
            .iter()
            .map(|&b| if is_blank(b) { b' ' } else { b })
            .collect();
        let (ref_end, text_start) = if input_ref {
            let ref_end = text.iter().position(|&b| b == b' ').unwrap_or(text.len());
            let mut text_start = ref_end;
            while text_start < text.len() && text[text_start] == b' ' {
                text_start += 1;
            }
            (ref_end, text_start)
        } else {
            (0, 0)
        };
        contexts.push(Context {
            text,
            offset: start,
            ref_end,
            text_start,
        });
    }

    FileContent {
        name,
        contexts,
        newlines,
    }
}

impl WordMatcher {
    /// Os intervalos `[início, fim)` das palavras de `text` a partir de `from`.
    pub(crate) fn find(&self, text: &[u8], from: usize) -> Vec<(usize, usize)> {
        let mut found = Vec::new();
        match self {
            Self::Letters | Self::NonSpace => {
                let belongs = |b: u8| match self {
                    Self::Letters => b.is_ascii_alphabetic(),
                    _ => !matches!(b, b' ' | b'\t' | b'\n'),
                };
                let mut i = from;
                while i < text.len() {
                    if belongs(text[i]) {
                        let start = i;
                        while i < text.len() && belongs(text[i]) {
                            i += 1;
                        }
                        found.push((start, i));
                    } else {
                        i += 1;
                    }
                }
            }
            Self::Pattern(re) => {
                for m in re.find_iter(&text[from..]) {
                    if m.end() > m.start() {
                        found.push((from + m.start(), from + m.end()));
                    }
                }
            }
        }
        found
    }
}

/// Os filtros `-i` e `-o` sobre as palavras-chave.
pub(crate) struct WordFilter {
    pub(crate) only: Option<FxHashSet<Vec<u8>>>,
    pub(crate) ignore: Option<FxHashSet<Vec<u8>>>,
}

impl WordFilter {
    fn allows(&self, word: &[u8]) -> bool {
        if let Some(only) = &self.only {
            if !only.contains(word) {
                return false;
            }
        }
        if let Some(ignore) = &self.ignore {
            if ignore.contains(word) {
                return false;
            }
        }
        true
    }
}

/// Acha as ocorrências de palavra-chave de todos os arquivos, já ordenadas. Devolve também o
/// comprimento da maior palavra do texto: o GNU usa esse número, de todas as palavras e antes dos
/// filtros, pra dimensionar o campo `head`.
pub(crate) fn find_occurrences(
    files: &[FileContent],
    matcher: &WordMatcher,
    filter: &WordFilter,
    ignore_case: bool,
) -> (Vec<Occurrence>, usize) {
    let mut occurrences = Vec::new();
    let mut max_word = 0;
    for (file_index, file) in files.iter().enumerate() {
        for (context_index, context) in file.contexts.iter().enumerate() {
            for (start, end) in matcher.find(&context.text, context.text_start) {
                max_word = max_word.max(end - start);
                let word = &context.text[start..end];
                if !filter.allows(word) {
                    continue;
                }
                let key = if ignore_case {
                    word.to_ascii_uppercase()
                } else {
                    word.to_vec()
                };
                occurrences.push(Occurrence {
                    key,
                    file: file_index,
                    context: context_index,
                    position: start,
                    len: end - start,
                });
            }
        }
    }
    occurrences.sort();
    (occurrences, max_word)
}
