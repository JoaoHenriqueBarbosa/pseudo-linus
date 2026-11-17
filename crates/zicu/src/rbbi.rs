//! `RuleBasedBreakIterator` (`rbbi.cpp`) e os seus dois caches (`rbbi_cache.cpp`): o de fronteiras,
//! um buffer circular de 128 posições, e o de dicionário, que subdivide os trechos de caracteres
//! que as regras entregam aos motores de linguagem.

use crate::rbbi_data::{RBBI_BOF_REQUIRED, RbbiData};
use crate::utext::{SENTINEL, Utf16Text};

/// `UBRK_DONE`.
pub const DONE: i32 = -1;

const START_STATE: i32 = 1;
const STOP_STATE: i32 = 0;
const ACCEPTING_UNCONDITIONAL: u16 = 1;
const CACHE_SIZE: usize = 128;
const CACHE_NEAR: i32 = 15;

/// Um motor de linguagem (`LanguageBreakEngine`): acha as fronteiras de um trecho de caracteres de
/// dicionário que as regras não sabem quebrar.
pub trait LanguageBreakEngine: std::fmt::Debug {
    /// Se o motor trata o caractere.
    fn handles(&self, c: i32) -> bool;
    /// Acha as fronteiras de `[start, end)`, acrescenta em `found` e deixa o texto depois do trecho
    /// que tratou. Devolve quantas acrescentou.
    fn find_breaks(
        &self,
        text: &mut Utf16Text,
        start: i32,
        end: i32,
        found: &mut Vec<i32>,
        is_phrase_breaking: bool,
    ) -> i32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Update {
    Retain,
    Position,
}

/// `BreakCache`.
#[derive(Clone, Debug)]
struct BreakCache {
    start_buf_idx: usize,
    end_buf_idx: usize,
    text_idx: i32,
    buf_idx: usize,
    boundaries: [i32; CACHE_SIZE],
    statuses: [u16; CACHE_SIZE],
}

fn mod_chunk(i: isize) -> usize {
    (i & (CACHE_SIZE as isize - 1)) as usize
}

impl BreakCache {
    fn new() -> Self {
        let mut cache = Self {
            start_buf_idx: 0,
            end_buf_idx: 0,
            text_idx: 0,
            buf_idx: 0,
            boundaries: [0; CACHE_SIZE],
            statuses: [0; CACHE_SIZE],
        };
        cache.reset(0, 0);
        cache
    }

    fn reset(&mut self, pos: i32, rule_status: i32) {
        self.start_buf_idx = 0;
        self.end_buf_idx = 0;
        self.text_idx = pos;
        self.buf_idx = 0;
        self.boundaries[0] = pos;
        self.statuses[0] = rule_status as u16;
    }

    fn add_following(&mut self, position: i32, status: i32, update: Update) {
        let next = mod_chunk(self.end_buf_idx as isize + 1);
        if next == self.start_buf_idx {
            self.start_buf_idx = mod_chunk(self.start_buf_idx as isize + 6);
        }
        self.boundaries[next] = position;
        self.statuses[next] = status as u16;
        self.end_buf_idx = next;
        if update == Update::Position {
            self.buf_idx = next;
            self.text_idx = position;
        }
    }

    fn add_preceding(&mut self, position: i32, status: i32, update: Update) -> bool {
        let next = mod_chunk(self.start_buf_idx as isize - 1);
        if next == self.end_buf_idx {
            if self.buf_idx == self.end_buf_idx && update == Update::Retain {
                return false;
            }
            self.end_buf_idx = mod_chunk(self.end_buf_idx as isize - 1);
        }
        self.boundaries[next] = position;
        self.statuses[next] = status as u16;
        self.start_buf_idx = next;
        if update == Update::Position {
            self.buf_idx = next;
            self.text_idx = position;
        }
        true
    }

    /// `BreakCache::seek`.
    fn seek(&mut self, pos: i32) -> bool {
        if pos < self.boundaries[self.start_buf_idx] || pos > self.boundaries[self.end_buf_idx] {
            return false;
        }
        if pos == self.boundaries[self.start_buf_idx] {
            self.buf_idx = self.start_buf_idx;
            self.text_idx = self.boundaries[self.buf_idx];
            return true;
        }
        if pos == self.boundaries[self.end_buf_idx] {
            self.buf_idx = self.end_buf_idx;
            self.text_idx = self.boundaries[self.buf_idx];
            return true;
        }
        let mut min = self.start_buf_idx;
        let mut max = self.end_buf_idx;
        while min != max {
            let probe = (min + max + if min > max { CACHE_SIZE } else { 0 }) / 2;
            let probe = mod_chunk(probe as isize);
            if self.boundaries[probe] > pos {
                max = probe;
            } else {
                min = mod_chunk(probe as isize + 1);
            }
        }
        self.buf_idx = mod_chunk(max as isize - 1);
        self.text_idx = self.boundaries[self.buf_idx];
        true
    }
}

/// `DictionaryCache`.
#[derive(Clone, Debug, Default)]
struct DictionaryCache {
    breaks: Vec<i32>,
    position_in_cache: isize,
    start: i32,
    limit: i32,
    first_rule_status_index: i32,
    other_rule_status_index: i32,
}

impl DictionaryCache {
    fn reset(&mut self) {
        self.position_in_cache = -1;
        self.start = 0;
        self.limit = 0;
        self.first_rule_status_index = 0;
        self.other_rule_status_index = 0;
        self.breaks.clear();
    }

    /// `DictionaryCache::following`.
    fn following(&mut self, from: i32) -> Option<(i32, i32)> {
        if from >= self.limit || from < self.start {
            self.position_in_cache = -1;
            return None;
        }
        let len = self.breaks.len() as isize;
        if self.position_in_cache >= 0
            && self.position_in_cache < len
            && self.breaks[self.position_in_cache as usize] == from
        {
            self.position_in_cache += 1;
            if self.position_in_cache >= len {
                self.position_in_cache = -1;
                return None;
            }
            return Some((
                self.breaks[self.position_in_cache as usize],
                self.other_rule_status_index,
            ));
        }
        for (i, &r) in self.breaks.iter().enumerate() {
            if r > from {
                self.position_in_cache = i as isize;
                return Some((r, self.other_rule_status_index));
            }
        }
        self.position_in_cache = len;
        None
    }

    /// `DictionaryCache::preceding`.
    fn preceding(&mut self, from: i32) -> Option<(i32, i32)> {
        if from <= self.start || from > self.limit {
            self.position_in_cache = -1;
            return None;
        }
        let len = self.breaks.len() as isize;
        if from == self.limit {
            self.position_in_cache = len - 1;
        }
        let status = |r: i32, cache: &Self| {
            if r == cache.start {
                cache.first_rule_status_index
            } else {
                cache.other_rule_status_index
            }
        };
        if self.position_in_cache > 0
            && self.position_in_cache < len
            && self.breaks[self.position_in_cache as usize] == from
        {
            self.position_in_cache -= 1;
            let r = self.breaks[self.position_in_cache as usize];
            return Some((r, status(r, self)));
        }
        if self.position_in_cache == 0 {
            self.position_in_cache = -1;
            return None;
        }
        for i in (0..self.breaks.len()).rev() {
            let r = self.breaks[i];
            if r < from {
                self.position_in_cache = i as isize;
                return Some((r, status(r, self)));
            }
        }
        self.position_in_cache = -1;
        None
    }
}

/// O iterador de quebra dirigido por regras, sobre texto UTF-16.
#[derive(Debug)]
pub struct RuleBasedBreakIterator<'d> {
    data: &'d RbbiData,
    engines: Vec<&'d dyn LanguageBreakEngine>,
    text: Utf16Text,
    position: i32,
    rule_status_index: i32,
    done: bool,
    dictionary_char_count: u32,
    look_ahead_matches: Vec<i32>,
    is_phrase_breaking: bool,
    cache: BreakCache,
    dictionary: DictionaryCache,
}

impl<'d> RuleBasedBreakIterator<'d> {
    /// Cria o iterador sobre as tabelas, com os motores de linguagem dos dicionários disponíveis,
    /// e um texto vazio.
    pub fn new(data: &'d RbbiData, engines: Vec<&'d dyn LanguageBreakEngine>) -> Self {
        let mut bi = Self {
            data,
            engines,
            text: Utf16Text::default(),
            position: 0,
            rule_status_index: 0,
            done: false,
            dictionary_char_count: 0,
            look_ahead_matches: vec![0; data.forward.lookahead_results_size as usize],
            is_phrase_breaking: false,
            cache: BreakCache::new(),
            dictionary: DictionaryCache::default(),
        };
        bi.dictionary.reset();
        bi
    }

    /// `setText`: o texto em unidades UTF-16, com o iterador no começo.
    pub fn set_text(&mut self, units: &[u16]) {
        self.cache.reset(0, 0);
        self.dictionary.reset();
        self.text = Utf16Text::new(units);
        self.first();
    }

    /// `first`.
    pub fn first(&mut self) -> i32 {
        if !self.cache.seek(0) {
            self.populate_near(0);
        }
        self.cache_current();
        0
    }

    /// `last`.
    pub fn last(&mut self) -> i32 {
        let end = self.text.len();
        self.is_boundary(end);
        end
    }

    /// `next`. O nome é o do ICU; o iterador não é um `Iterator`, porque devolve [`DONE`] no fim e
    /// também anda para trás.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> i32 {
        self.cache_next();
        if self.done { DONE } else { self.position }
    }

    /// `previous`.
    pub fn previous(&mut self) -> i32 {
        self.cache_previous();
        if self.done { DONE } else { self.position }
    }

    /// `following`.
    pub fn following(&mut self, start: i32) -> i32 {
        if start < 0 {
            return self.first();
        }
        self.text.set_index(start);
        let start = self.text.index();
        if start == self.cache.text_idx || self.cache.seek(start) || self.populate_near(start) {
            self.done = false;
            self.cache_next();
        }
        if self.done { DONE } else { self.position }
    }

    /// `preceding`.
    pub fn preceding(&mut self, offset: i32) -> i32 {
        if offset > self.text.len() {
            return self.last();
        }
        self.text.set_index(offset);
        let adjusted = self.text.index();
        if adjusted == self.cache.text_idx
            || self.cache.seek(adjusted)
            || self.populate_near(adjusted)
        {
            if adjusted == self.cache.text_idx {
                self.cache_previous();
            } else {
                self.cache_current();
            }
        }
        if self.done { DONE } else { self.position }
    }

    /// `isBoundary`.
    pub fn is_boundary(&mut self, offset: i32) -> bool {
        if offset < 0 {
            self.first();
            return false;
        }
        self.text.set_index(offset);
        let adjusted = self.text.index();
        let mut result = false;
        if self.cache.seek(adjusted) || self.populate_near(adjusted) {
            result = self.cache_current() == offset;
        }
        if result && adjusted < offset {
            self.text.set_index(offset);
            if self.text.current32() == SENTINEL {
                return false;
            }
        }
        if !result {
            self.next();
        }
        result
    }

    /// `current`.
    pub fn current(&self) -> i32 {
        self.position
    }

    /// `getRuleStatus`: o maior valor de status da regra que deu a fronteira atual.
    pub fn rule_status(&self) -> i32 {
        let table = &self.data.rule_status_table;
        let idx = self.rule_status_index as usize;
        let count = table.get(idx).copied().unwrap_or(0) as usize;
        table.get(idx + count).copied().unwrap_or(0)
    }

    /// `getRuleStatusVec`.
    pub fn rule_status_vec(&self) -> Vec<i32> {
        let table = &self.data.rule_status_table;
        let idx = self.rule_status_index as usize;
        let count = table.get(idx).copied().unwrap_or(0) as usize;
        table
            .get(idx + 1..idx + 1 + count)
            .map(<[i32]>::to_vec)
            .unwrap_or_default()
    }

    // ---- o cache de fronteiras --------------------------------------------------------------

    fn cache_current(&mut self) -> i32 {
        self.position = self.cache.text_idx;
        self.rule_status_index = i32::from(self.cache.statuses[self.cache.buf_idx]);
        self.done = false;
        self.cache.text_idx
    }

    fn cache_next(&mut self) {
        if self.cache.buf_idx == self.cache.end_buf_idx {
            self.done = !self.populate_following();
            self.position = self.cache.text_idx;
            self.rule_status_index = i32::from(self.cache.statuses[self.cache.buf_idx]);
        } else {
            self.cache.buf_idx = mod_chunk(self.cache.buf_idx as isize + 1);
            self.cache.text_idx = self.cache.boundaries[self.cache.buf_idx];
            self.position = self.cache.text_idx;
            self.rule_status_index = i32::from(self.cache.statuses[self.cache.buf_idx]);
        }
    }

    fn cache_previous(&mut self) {
        let initial = self.cache.buf_idx;
        if self.cache.buf_idx == self.cache.start_buf_idx {
            self.populate_preceding();
        } else {
            self.cache.buf_idx = mod_chunk(self.cache.buf_idx as isize - 1);
            self.cache.text_idx = self.cache.boundaries[self.cache.buf_idx];
        }
        self.done = self.cache.buf_idx == initial;
        self.position = self.cache.text_idx;
        self.rule_status_index = i32::from(self.cache.statuses[self.cache.buf_idx]);
    }

    /// Um `handleNext` a partir de uma posição segura, avançando de novo se ele andou um só ponto
    /// de código: as regras seguras identificam pares.
    fn boundary_after_safe_point(&mut self, backup: i32) -> i32 {
        self.position = backup;
        let mut boundary = self.handle_next();
        if boundary != DONE && boundary <= backup + 4 {
            self.text.set_index(boundary);
            if backup == self.text.previous_index() {
                boundary = self.handle_next();
            }
        }
        boundary
    }

    /// `BreakCache::populateNear`.
    fn populate_near(&mut self, position: i32) -> bool {
        let mut a_boundary = -1;
        let mut rule_status_index = 0;
        let start = self.cache.boundaries[self.cache.start_buf_idx];
        let end = self.cache.boundaries[self.cache.end_buf_idx];
        let retain_cache;
        if position > start - CACHE_NEAR && position < end + CACHE_NEAR {
            retain_cache = true;
        } else if position <= CACHE_NEAR {
            retain_cache = false;
            a_boundary = 0;
        } else {
            let backup = self.handle_safe_previous(position);
            if end < position && end >= backup - CACHE_NEAR {
                retain_cache = true;
            } else if backup < CACHE_NEAR {
                a_boundary = 0;
                retain_cache = start <= position + CACHE_NEAR;
            } else {
                retain_cache = false;
                a_boundary = self.boundary_after_safe_point(backup);
                if a_boundary == DONE {
                    a_boundary = self.text.len();
                }
                rule_status_index = self.rule_status_index;
            }
        }
        if !retain_cache {
            self.cache.reset(a_boundary, rule_status_index);
        }
        if self.cache.boundaries[self.cache.end_buf_idx] < position {
            while self.cache.boundaries[self.cache.end_buf_idx] < position {
                if !self.populate_following() {
                    return false;
                }
            }
            self.cache.buf_idx = self.cache.end_buf_idx;
            self.cache.text_idx = self.cache.boundaries[self.cache.buf_idx];
            while self.cache.text_idx > position {
                self.cache_previous();
            }
            return true;
        }
        if self.cache.boundaries[self.cache.start_buf_idx] > position {
            while self.cache.boundaries[self.cache.start_buf_idx] > position {
                self.populate_preceding();
            }
            self.cache.buf_idx = self.cache.start_buf_idx;
            self.cache.text_idx = self.cache.boundaries[self.cache.buf_idx];
            while self.cache.text_idx < position {
                self.cache_next();
            }
            if self.cache.text_idx > position {
                self.cache_previous();
            }
            return true;
        }
        true
    }

    /// `BreakCache::populateFollowing`.
    fn populate_following(&mut self) -> bool {
        let from = self.cache.boundaries[self.cache.end_buf_idx];
        let from_status = i32::from(self.cache.statuses[self.cache.end_buf_idx]);
        if let Some((pos, status)) = self.dictionary.following(from) {
            self.cache.add_following(pos, status, Update::Position);
            return true;
        }
        self.position = from;
        let pos = self.handle_next();
        if pos == DONE {
            return false;
        }
        let status = self.rule_status_index;
        if self.dictionary_char_count > 0 {
            self.populate_dictionary(from, pos, from_status, status);
            if let Some((pos, status)) = self.dictionary.following(from) {
                self.cache.add_following(pos, status, Update::Position);
                return true;
            }
        }
        self.cache.add_following(pos, status, Update::Position);
        for _ in 0..6 {
            let pos = self.handle_next();
            if pos == DONE || self.dictionary_char_count > 0 {
                break;
            }
            self.cache
                .add_following(pos, self.rule_status_index, Update::Retain);
        }
        true
    }

    /// `BreakCache::populatePreceding`.
    fn populate_preceding(&mut self) -> bool {
        let from = self.cache.boundaries[self.cache.start_buf_idx];
        if from == 0 {
            return false;
        }
        if let Some((pos, status)) = self.dictionary.preceding(from) {
            self.cache.add_preceding(pos, status, Update::Position);
            return true;
        }
        let mut backup = from;
        let mut position;
        let mut position_status;
        loop {
            backup -= 30;
            if backup <= 0 {
                backup = 0;
            } else {
                backup = self.handle_safe_previous(backup);
            }
            if backup == DONE || backup == 0 {
                position = 0;
                position_status = 0;
            } else {
                position = self.boundary_after_safe_point(backup);
                position_status = self.rule_status_index;
            }
            if position < from {
                break;
            }
        }
        let mut side = vec![(position, position_status)];
        loop {
            let mut prev_position = position;
            self.position = position;
            let prev_status = position_status;
            position = self.handle_next();
            position_status = self.rule_status_index;
            if position == DONE {
                break;
            }
            let mut handled = false;
            if self.dictionary_char_count != 0 {
                let segment_end = position;
                self.populate_dictionary(prev_position, segment_end, prev_status, position_status);
                while let Some((p, s)) = self.dictionary.following(prev_position) {
                    position = p;
                    position_status = s;
                    handled = true;
                    if position >= from {
                        break;
                    }
                    side.push((position, position_status));
                    prev_position = position;
                }
            }
            if !handled && position < from {
                side.push((position, position_status));
            }
            if position >= from {
                break;
            }
        }
        let mut success = false;
        if let Some((p, s)) = side.pop() {
            self.cache.add_preceding(p, s, Update::Position);
            success = true;
        }
        while let Some((p, s)) = side.pop() {
            if !self.cache.add_preceding(p, s, Update::Retain) {
                break;
            }
        }
        success
    }

    /// `DictionaryCache::populateDictionary`.
    fn populate_dictionary(&mut self, start: i32, end: i32, first_status: i32, other_status: i32) {
        if end - start <= 1 {
            return;
        }
        self.dictionary.reset();
        self.dictionary.first_rule_status_index = first_status;
        self.dictionary.other_rule_status_index = other_status;
        let dict_start = self.data.forward.dict_categories_start;
        let mut found = 0;
        self.text.set_index(start);
        let mut c = self.text.current32();
        let mut category = self.data.trie.get(c);
        loop {
            let mut current = self.text.index();
            while current < end && category < dict_start {
                self.text.next32();
                c = self.text.current32();
                category = self.data.trie.get(c);
                current = self.text.index();
            }
            if current >= end {
                break;
            }
            let engine = self.engines.iter().copied().find(|e| e.handles(c));
            match engine {
                Some(engine) => {
                    found += engine.find_breaks(
                        &mut self.text,
                        current,
                        end,
                        &mut self.dictionary.breaks,
                        self.is_phrase_breaking,
                    );
                }
                None => self.skip_unhandled(end),
            }
            c = self.text.current32();
            category = self.data.trie.get(c);
        }
        if found > 0 {
            if start < self.dictionary.breaks[0] {
                self.dictionary.breaks.insert(0, start);
            }
            if end > *self.dictionary.breaks.last().unwrap_or(&end) {
                self.dictionary.breaks.push(end);
            }
            self.dictionary.position_in_cache = 0;
            self.dictionary.start = self.dictionary.breaks[0];
            self.dictionary.limit = *self.dictionary.breaks.last().unwrap_or(&end);
        }
    }

    /// `UnhandledEngine::findBreaks`: passa pelos caracteres de dicionário que nenhum motor trata,
    /// sem achar fronteira. O ICU junta à lista recusada o script inteiro do caractere; aqui o
    /// trecho recusado vai até o primeiro caractere que um motor trate ou que não seja de dicionário,
    /// o que dá as mesmas fronteiras, já que o motor recusado não acha nenhuma.
    fn skip_unhandled(&mut self, end: i32) {
        let dict_start = self.data.forward.dict_categories_start;
        loop {
            let c = self.text.current32();
            if self.text.index() >= end
                || self.data.trie.get(c) < dict_start
                || self.engines.iter().any(|e| e.handles(c))
            {
                break;
            }
            self.text.next32();
        }
    }

    // ---- as máquinas de estado --------------------------------------------------------------

    /// `handleNext`.
    fn handle_next(&mut self) -> i32 {
        let table = &self.data.forward;
        let dict_start = table.dict_categories_start;
        self.rule_status_index = 0;
        self.dictionary_char_count = 0;
        let initial = self.position;
        self.text.set_index(initial);
        let mut result = initial;
        let mut c = self.text.next32();
        if c == SENTINEL {
            self.done = true;
            return DONE;
        }
        let mut state = START_STATE;
        let mut category: u32 = 0;
        #[derive(PartialEq)]
        enum Mode {
            Start,
            Run,
            End,
        }
        let mut mode = Mode::Run;
        if table.flags & RBBI_BOF_REQUIRED != 0 {
            category = 2;
            mode = Mode::Start;
        }
        loop {
            if c == SENTINEL {
                if mode == Mode::End {
                    break;
                }
                mode = Mode::End;
                category = 1;
            }
            if mode == Mode::Run {
                category = self.data.trie.fast_get(c);
                self.dictionary_char_count += u32::from(category >= dict_start);
            }
            state = table.next_state(state, category);
            let accepting = table.accepting(state);
            if accepting == ACCEPTING_UNCONDITIONAL {
                if mode != Mode::Start {
                    result = self.text.index();
                }
                self.rule_status_index = i32::from(table.tags_idx(state));
            } else if accepting > ACCEPTING_UNCONDITIONAL {
                let look_ahead_result = self.look_ahead_matches[accepting as usize];
                if look_ahead_result >= 0 {
                    self.rule_status_index = i32::from(table.tags_idx(state));
                    self.position = look_ahead_result;
                    return look_ahead_result;
                }
            }
            let rule = table.look_ahead(state);
            if rule > ACCEPTING_UNCONDITIONAL {
                self.look_ahead_matches[rule as usize] = self.text.index();
            }
            if state == STOP_STATE {
                break;
            }
            if mode == Mode::Run {
                c = self.text.next32();
            } else if mode == Mode::Start {
                mode = Mode::Run;
            }
        }
        if result == initial {
            self.text.set_index(initial);
            self.text.next32();
            result = self.text.index();
            self.rule_status_index = 0;
        }
        self.position = result;
        result
    }

    /// `handleSafePrevious`.
    fn handle_safe_previous(&mut self, from: i32) -> i32 {
        let table = &self.data.reverse;
        self.text.set_index(from);
        if self.text.index() == 0 {
            return DONE;
        }
        let mut c = self.text.previous32();
        let mut state = START_STATE;
        while c != SENTINEL {
            let category = self.data.trie.fast_get(c);
            state = table.next_state(state, category);
            if state == STOP_STATE {
                break;
            }
            c = self.text.previous32();
        }
        self.text.index()
    }
}
