//! `BytesTrie` (`bytestrie.cpp`, `bytestrie.h`): só a descida byte a byte que o casamento com
//! dicionário usa.

const MAX_BRANCH_LINEAR_SUB_NODE_LENGTH: i32 = 5;
const MIN_LINEAR_MATCH: i32 = 0x10;
const MIN_VALUE_LEAD: i32 = 0x20;
const VALUE_IS_FINAL: i32 = 1;
const MIN_ONE_BYTE_VALUE_LEAD: i32 = 0x10;
const MIN_TWO_BYTE_VALUE_LEAD: i32 = 0x51;
const MIN_THREE_BYTE_VALUE_LEAD: i32 = 0x6c;
const FOUR_BYTE_VALUE_LEAD: i32 = 0x7e;
const MIN_TWO_BYTE_DELTA_LEAD: i32 = 0xc0;
const MIN_THREE_BYTE_DELTA_LEAD: i32 = 0xf0;
const FOUR_BYTE_DELTA_LEAD: i32 = 0xfe;

/// `UStringTrieResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrieResult {
    NoMatch,
    NoValue,
    FinalValue,
    IntermediateValue,
}

impl TrieResult {
    /// `USTRINGTRIE_HAS_VALUE`.
    pub fn has_value(self) -> bool {
        self >= Self::FinalValue
    }
}

fn value_result(node: i32) -> TrieResult {
    if node & VALUE_IS_FINAL != 0 {
        TrieResult::FinalValue
    } else {
        TrieResult::IntermediateValue
    }
}

#[derive(Clone, Debug)]
pub struct BytesTrie<'a> {
    bytes: &'a [u8],
    pos: Option<usize>,
    remaining_match_length: i32,
}

impl<'a> BytesTrie<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: Some(0),
            remaining_match_length: -1,
        }
    }

    fn at(&self, i: usize) -> i32 {
        i32::from(self.bytes.get(i).copied().unwrap_or(0))
    }

    fn stop(&mut self) {
        self.pos = None;
    }

    fn skip_value(&self, mut pos: usize, lead: i32) -> usize {
        if lead >= (MIN_TWO_BYTE_VALUE_LEAD << 1) {
            if lead < (MIN_THREE_BYTE_VALUE_LEAD << 1) {
                pos += 1;
            } else if lead < (FOUR_BYTE_VALUE_LEAD << 1) {
                pos += 2;
            } else {
                pos += 3 + ((lead >> 1) & 1) as usize;
            }
        }
        pos
    }

    fn skip_value_at(&self, pos: usize) -> usize {
        let lead = self.at(pos);
        self.skip_value(pos + 1, lead)
    }

    fn skip_delta(&self, mut pos: usize) -> usize {
        let delta = self.at(pos);
        pos += 1;
        if delta >= MIN_TWO_BYTE_DELTA_LEAD {
            if delta < MIN_THREE_BYTE_DELTA_LEAD {
                pos += 1;
            } else if delta < FOUR_BYTE_DELTA_LEAD {
                pos += 2;
            } else {
                pos += 3 + (delta & 1) as usize;
            }
        }
        pos
    }

    fn jump_by_delta(&self, mut pos: usize) -> usize {
        let mut delta = self.at(pos);
        pos += 1;
        if delta < MIN_TWO_BYTE_DELTA_LEAD {
        } else if delta < MIN_THREE_BYTE_DELTA_LEAD {
            delta = ((delta - MIN_TWO_BYTE_DELTA_LEAD) << 8) | self.at(pos);
            pos += 1;
        } else if delta < FOUR_BYTE_DELTA_LEAD {
            delta = ((delta - MIN_THREE_BYTE_DELTA_LEAD) << 16)
                | (self.at(pos) << 8)
                | self.at(pos + 1);
            pos += 2;
        } else if delta == FOUR_BYTE_DELTA_LEAD {
            delta = (self.at(pos) << 16) | (self.at(pos + 1) << 8) | self.at(pos + 2);
            pos += 3;
        } else {
            delta = (self.at(pos) << 24)
                | (self.at(pos + 1) << 16)
                | (self.at(pos + 2) << 8)
                | self.at(pos + 3);
            pos += 4;
        }
        (pos as i64 + i64::from(delta)) as usize
    }

    /// `BytesTrie::first`.
    pub fn first(&mut self, byte: i32) -> TrieResult {
        self.remaining_match_length = -1;
        self.pos = Some(0);
        let byte = if byte < 0 { byte + 0x100 } else { byte };
        self.next_impl(0, byte)
    }

    /// `BytesTrie::next`.
    pub fn next(&mut self, byte: i32) -> TrieResult {
        let Some(mut pos) = self.pos else {
            return TrieResult::NoMatch;
        };
        let byte = if byte < 0 { byte + 0x100 } else { byte };
        let mut length = self.remaining_match_length;
        if length >= 0 {
            if byte == self.at(pos) {
                pos += 1;
                length -= 1;
                self.remaining_match_length = length;
                self.pos = Some(pos);
                let node = self.at(pos);
                return if length < 0 && node >= MIN_VALUE_LEAD {
                    value_result(node)
                } else {
                    TrieResult::NoValue
                };
            }
            self.stop();
            return TrieResult::NoMatch;
        }
        self.next_impl(pos, byte)
    }

    fn next_impl(&mut self, mut pos: usize, byte: i32) -> TrieResult {
        loop {
            let node = self.at(pos);
            pos += 1;
            if node < MIN_LINEAR_MATCH {
                return self.branch_next(pos, node, byte);
            } else if node < MIN_VALUE_LEAD {
                let mut length = node - MIN_LINEAR_MATCH;
                if byte == self.at(pos) {
                    pos += 1;
                    length -= 1;
                    self.remaining_match_length = length;
                    self.pos = Some(pos);
                    let node = self.at(pos);
                    return if length < 0 && node >= MIN_VALUE_LEAD {
                        value_result(node)
                    } else {
                        TrieResult::NoValue
                    };
                }
                break;
            } else if node & VALUE_IS_FINAL != 0 {
                break;
            } else {
                pos = self.skip_value(pos, node);
            }
        }
        self.stop();
        TrieResult::NoMatch
    }

    fn branch_next(&mut self, mut pos: usize, mut length: i32, byte: i32) -> TrieResult {
        if length == 0 {
            length = self.at(pos);
            pos += 1;
        }
        length += 1;
        while length > MAX_BRANCH_LINEAR_SUB_NODE_LENGTH {
            let b = self.at(pos);
            pos += 1;
            if byte < b {
                length >>= 1;
                pos = self.jump_by_delta(pos);
            } else {
                length -= length >> 1;
                pos = self.skip_delta(pos);
            }
        }
        loop {
            let b = self.at(pos);
            pos += 1;
            if byte == b {
                let mut node = self.at(pos);
                let result = if node & VALUE_IS_FINAL != 0 {
                    TrieResult::FinalValue
                } else {
                    pos += 1;
                    node >>= 1;
                    let delta;
                    if node < MIN_TWO_BYTE_VALUE_LEAD {
                        delta = node - MIN_ONE_BYTE_VALUE_LEAD;
                    } else if node < MIN_THREE_BYTE_VALUE_LEAD {
                        delta = ((node - MIN_TWO_BYTE_VALUE_LEAD) << 8) | self.at(pos);
                        pos += 1;
                    } else if node < FOUR_BYTE_VALUE_LEAD {
                        delta = ((node - MIN_THREE_BYTE_VALUE_LEAD) << 16)
                            | (self.at(pos) << 8)
                            | self.at(pos + 1);
                        pos += 2;
                    } else if node == FOUR_BYTE_VALUE_LEAD {
                        delta = (self.at(pos) << 16) | (self.at(pos + 1) << 8) | self.at(pos + 2);
                        pos += 3;
                    } else {
                        delta = (self.at(pos) << 24)
                            | (self.at(pos + 1) << 16)
                            | (self.at(pos + 2) << 8)
                            | self.at(pos + 3);
                        pos += 4;
                    }
                    pos = (pos as i64 + i64::from(delta)) as usize;
                    let node = self.at(pos);
                    if node >= MIN_VALUE_LEAD {
                        value_result(node)
                    } else {
                        TrieResult::NoValue
                    }
                };
                self.pos = Some(pos);
                return result;
            }
            length -= 1;
            pos = self.skip_value_at(pos);
            if length <= 1 {
                break;
            }
        }
        if byte == self.at(pos) {
            pos += 1;
            self.pos = Some(pos);
            let node = self.at(pos);
            return if node >= MIN_VALUE_LEAD {
                value_result(node)
            } else {
                TrieResult::NoValue
            };
        }
        self.stop();
        TrieResult::NoMatch
    }
}
