//! Os motores de dicionário do Sudeste Asiático (`dictbe.cpp`, com o patch `khmer-dictbe` do
//! Chromium): tailandês, laosiano, birmanês e khmer. Os quatro seguem o mesmo algoritmo, com
//! constantes e conjuntos próprios; o tailandês ainda trata os sufixos PAIYANNOI e MAIYAMOK.

use crate::dictionary::DictionaryMatcher;
use crate::rbbi::LanguageBreakEngine;
use crate::sets;
use crate::utext::Utf16Text;

const POSSIBLE_WORD_LIST_MAX: usize = 20;
const LOOKAHEAD: usize = 3;
const THAI_PAIYANNOI: i32 = 0x0e2f;
const THAI_MAIYAMOK: i32 = 0x0e46;

/// Um conjunto de pontos de código em faixas fechadas, em ordem.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CharSet(Vec<(u32, u32)>);

impl CharSet {
    fn of(ranges: &[(u32, u32)]) -> Self {
        Self(ranges.to_vec())
    }

    fn contains(&self, c: i32) -> bool {
        c >= 0
            && self
                .0
                .iter()
                .any(|&(start, end)| (start..=end).contains(&(c as u32)))
    }

    fn add(&mut self, start: u32, end: u32) {
        self.0.push((start, end));
    }

    /// Tira `[start, end]`, partindo as faixas que o cruzam.
    fn remove(&mut self, start: u32, end: u32) {
        let mut out = Vec::new();
        for &(a, b) in &self.0 {
            if b < start || a > end {
                out.push((a, b));
                continue;
            }
            if a < start {
                out.push((a, start - 1));
            }
            if b > end {
                out.push((end + 1, b));
            }
        }
        self.0 = out;
    }
}

/// `PossibleWord`: as palavras candidatas numa posição do texto.
#[derive(Clone, Debug)]
struct PossibleWord {
    count: i32,
    prefix: i32,
    offset: i32,
    mark: i32,
    current: i32,
    cu_lengths: Vec<i32>,
    cp_lengths: Vec<i32>,
}

impl Default for PossibleWord {
    fn default() -> Self {
        Self {
            count: 0,
            prefix: 0,
            offset: -1,
            mark: 0,
            current: 0,
            cu_lengths: Vec::new(),
            cp_lengths: Vec::new(),
        }
    }
}

impl PossibleWord {
    fn candidates(
        &mut self,
        text: &mut Utf16Text,
        dict: &DictionaryMatcher,
        range_end: i32,
    ) -> i32 {
        let start = text.index();
        if start != self.offset {
            self.offset = start;
            let found = dict.matches(text, range_end - start, POSSIBLE_WORD_LIST_MAX);
            self.count = found.cu_lengths.len() as i32;
            self.cu_lengths = found.cu_lengths;
            self.cp_lengths = found.cp_lengths;
            self.prefix = found.prefix;
            if self.count <= 0 {
                text.set_index(start);
            }
        }
        if self.count > 0 {
            text.set_index(start + self.cu_lengths[(self.count - 1) as usize]);
        }
        self.current = self.count - 1;
        self.mark = self.current;
        self.count
    }

    fn accept_marked(&self, text: &mut Utf16Text) -> i32 {
        let length = self.cu_lengths[self.mark as usize];
        text.set_index(self.offset + length);
        length
    }

    fn back_up(&mut self, text: &mut Utf16Text) -> bool {
        if self.current > 0 {
            self.current -= 1;
            text.set_index(self.offset + self.cu_lengths[self.current as usize]);
            return true;
        }
        false
    }

    fn mark_current(&mut self) {
        self.mark = self.current;
    }

    fn marked_cp_length(&self) -> i32 {
        self.cp_lengths[self.mark as usize]
    }
}

/// Qual dos quatro motores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Thai,
    Lao,
    Burmese,
    Khmer,
}

/// Um motor de dicionário (`ThaiBreakEngine`, `LaoBreakEngine`, `BurmeseBreakEngine`,
/// `KhmerBreakEngine`).
#[derive(Clone, Debug)]
pub struct DictionaryBreakEngine {
    kind: Kind,
    dictionary: DictionaryMatcher,
    set: CharSet,
    mark_set: CharSet,
    end_word_set: CharSet,
    begin_word_set: CharSet,
    suffix_set: CharSet,
    root_combine_threshold: i32,
    prefix_combine_threshold: i32,
}

impl DictionaryBreakEngine {
    fn new(
        kind: Kind,
        dictionary: DictionaryMatcher,
        word: &[(u32, u32)],
        mark: &[(u32, u32)],
    ) -> Self {
        let mut mark_set = CharSet::of(mark);
        mark_set.add(0x20, 0x20);
        Self {
            kind,
            dictionary,
            set: CharSet::of(word),
            mark_set,
            end_word_set: CharSet::of(word),
            begin_word_set: CharSet::default(),
            suffix_set: CharSet::default(),
            root_combine_threshold: 3,
            prefix_combine_threshold: 3,
        }
    }

    /// `ThaiBreakEngine`.
    pub fn thai(dictionary: DictionaryMatcher) -> Self {
        let mut engine = Self::new(Kind::Thai, dictionary, sets::THAI_WORD, sets::THAI_MARK);
        engine.end_word_set.remove(0x0e31, 0x0e31);
        engine.end_word_set.remove(0x0e40, 0x0e44);
        engine.begin_word_set.add(0x0e01, 0x0e2e);
        engine.begin_word_set.add(0x0e40, 0x0e44);
        engine
            .suffix_set
            .add(THAI_PAIYANNOI as u32, THAI_PAIYANNOI as u32);
        engine
            .suffix_set
            .add(THAI_MAIYAMOK as u32, THAI_MAIYAMOK as u32);
        engine
    }

    /// `LaoBreakEngine`.
    pub fn lao(dictionary: DictionaryMatcher) -> Self {
        let mut engine = Self::new(Kind::Lao, dictionary, sets::LAO_WORD, sets::LAO_MARK);
        engine.end_word_set.remove(0x0ec0, 0x0ec4);
        engine.begin_word_set.add(0x0e81, 0x0eae);
        engine.begin_word_set.add(0x0edc, 0x0edd);
        engine.begin_word_set.add(0x0ec0, 0x0ec4);
        engine
    }

    /// `BurmeseBreakEngine`.
    pub fn burmese(dictionary: DictionaryMatcher) -> Self {
        let mut engine = Self::new(
            Kind::Burmese,
            dictionary,
            sets::BURMESE_WORD,
            sets::BURMESE_MARK,
        );
        engine.begin_word_set.add(0x1000, 0x102a);
        engine
    }

    /// `KhmerBreakEngine`, com os limiares do patch do Chromium (10 e 5, contra 3 e 3 no ICU).
    pub fn khmer(dictionary: DictionaryMatcher) -> Self {
        let mut engine = Self::new(Kind::Khmer, dictionary, sets::KHMER_WORD, sets::KHMER_MARK);
        engine.begin_word_set.add(0x1780, 0x17b3);
        engine.end_word_set.remove(0x17d2, 0x17d2);
        engine.root_combine_threshold = 10;
        engine.prefix_combine_threshold = 5;
        engine
    }

    /// Se o trecho é curto demais para duas palavras: o tailandês conta pontos de código, os
    /// outros, unidades.
    fn too_short(&self, text: &mut Utf16Text, start: i32, end: i32) -> bool {
        if self.kind == Kind::Thai {
            text.set_index(start);
            text.move_forward32(4);
            let short = text.index() >= end;
            text.set_index(start);
            short
        } else {
            end - start < 4
        }
    }

    /// Escolhe entre várias candidatas a que leva mais longe em palavras (o laço com `goto
    /// foundBest`).
    fn choose_best(
        &self,
        words: &mut [PossibleWord; LOOKAHEAD],
        found: usize,
        text: &mut Utf16Text,
        end: i32,
    ) {
        let dict = &self.dictionary;
        if text.index() >= end {
            return;
        }
        loop {
            if words[(found + 1) % LOOKAHEAD].candidates(text, dict, end) > 0 {
                words[found % LOOKAHEAD].mark_current();
                if text.index() >= end {
                    return;
                }
                loop {
                    if words[(found + 2) % LOOKAHEAD].candidates(text, dict, end) != 0 {
                        words[found % LOOKAHEAD].mark_current();
                        return;
                    }
                    if !words[(found + 1) % LOOKAHEAD].back_up(text) {
                        break;
                    }
                }
            }
            if !words[found % LOOKAHEAD].back_up(text) {
                return;
            }
        }
    }

    /// A ressincronização depois de uma não-palavra: anda até um par fim-começo plausível que
    /// comece uma palavra do dicionário. Devolve quantas unidades passou.
    fn resync(
        &self,
        words: &mut [PossibleWord; LOOKAHEAD],
        found: usize,
        text: &mut Utf16Text,
        (current, cu_word_length, end): (i32, i32, i32),
    ) -> i32 {
        let mut remaining = end - (current + cu_word_length);
        let mut chars = 0;
        loop {
            let pc_index = text.index();
            let pc = text.next32();
            let pc_size = text.index() - pc_index;
            chars += pc_size;
            remaining -= pc_size;
            if remaining <= 0 {
                break;
            }
            let uc = text.current32();
            if self.end_word_set.contains(pc) && self.begin_word_set.contains(uc) {
                let candidates =
                    words[(found + 1) % LOOKAHEAD].candidates(text, &self.dictionary, end);
                text.set_index(current + cu_word_length + chars);
                if candidates > 0 {
                    break;
                }
            }
        }
        chars
    }

    /// Os sufixos do tailandês, quando não segue uma palavra do dicionário.
    fn thai_suffix(
        &self,
        words: &mut [PossibleWord; LOOKAHEAD],
        found: usize,
        text: &mut Utf16Text,
        (current, cu_word_length, end): (i32, i32, i32),
    ) -> i32 {
        let mut length = cu_word_length;
        let mut uc;
        if words[found % LOOKAHEAD].candidates(text, &self.dictionary, end) <= 0 && {
            uc = text.current32();
            self.suffix_set.contains(uc)
        } {
            if uc == THAI_PAIYANNOI {
                if !self.suffix_set.contains(text.previous32()) {
                    text.next32();
                    let index = text.index();
                    text.next32();
                    length += text.index() - index;
                    uc = text.current32();
                } else {
                    text.next32();
                }
            }
            if uc == THAI_MAIYAMOK {
                if text.previous32() != THAI_MAIYAMOK {
                    text.next32();
                    let index = text.index();
                    text.next32();
                    length += text.index() - index;
                } else {
                    text.next32();
                }
            }
        } else {
            text.set_index(current + cu_word_length);
        }
        length
    }

    /// `divideUpDictionaryRange`.
    fn divide_up_dictionary_range(
        &self,
        text: &mut Utf16Text,
        start: i32,
        end: i32,
        breaks: &mut Vec<i32>,
    ) -> i32 {
        if self.too_short(text, start, end) {
            return 0;
        }
        let dict = &self.dictionary;
        let mut found: usize = 0;
        let mut words: [PossibleWord; LOOKAHEAD] = Default::default();
        text.set_index(start);
        loop {
            let current = text.index();
            if current >= end {
                break;
            }
            let mut cu_word_length = 0;
            let mut cp_word_length = 0;
            let candidates = words[found % LOOKAHEAD].candidates(text, dict, end);
            if candidates == 1 {
                cu_word_length = words[found % LOOKAHEAD].accept_marked(text);
                cp_word_length = words[found % LOOKAHEAD].marked_cp_length();
                found += 1;
            } else if candidates > 1 {
                self.choose_best(&mut words, found, text, end);
                cu_word_length = words[found % LOOKAHEAD].accept_marked(text);
                cp_word_length = words[found % LOOKAHEAD].marked_cp_length();
                found += 1;
            }
            if text.index() < end && cp_word_length < self.root_combine_threshold {
                if words[found % LOOKAHEAD].candidates(text, dict, end) <= 0
                    && (cu_word_length == 0
                        || words[found % LOOKAHEAD].prefix < self.prefix_combine_threshold)
                {
                    let chars =
                        self.resync(&mut words, found, text, (current, cu_word_length, end));
                    if cu_word_length <= 0 {
                        found += 1;
                    }
                    cu_word_length += chars;
                } else {
                    text.set_index(current + cu_word_length);
                }
            }
            loop {
                let pos = text.index();
                if pos >= end || !self.mark_set.contains(text.current32()) {
                    break;
                }
                text.next32();
                cu_word_length += text.index() - pos;
            }
            if self.kind == Kind::Thai && text.index() < end && cu_word_length > 0 {
                cu_word_length =
                    self.thai_suffix(&mut words, found, text, (current, cu_word_length, end));
            }
            if cu_word_length > 0 {
                breaks.push(current + cu_word_length);
            }
        }
        if breaks.last().copied().unwrap_or(0) >= end {
            breaks.pop();
            found = found.wrapping_sub(1);
        }
        found as u32 as i32
    }
}

impl LanguageBreakEngine for DictionaryBreakEngine {
    fn handles(&self, c: i32) -> bool {
        self.set.contains(c)
    }

    /// `DictionaryBreakEngine::findBreaks`.
    fn find_breaks(
        &self,
        text: &mut Utf16Text,
        start: i32,
        end: i32,
        found: &mut Vec<i32>,
        _is_phrase_breaking: bool,
    ) -> i32 {
        text.set_index(start);
        let range_start = text.index();
        let mut current;
        loop {
            current = text.index();
            if current >= end || !self.set.contains(text.current32()) {
                break;
            }
            text.next32();
        }
        let result = self.divide_up_dictionary_range(text, range_start, current, found);
        text.set_index(current);
        result
    }
}
