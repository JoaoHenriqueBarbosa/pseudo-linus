//! Port do algoritmo bidi do fribidi 1.0.16 (`fribidi-bidi.c`, `fribidi-run.c`,
//! `fribidi-brackets.c`), o que o raqm chama para dividir o texto em runs de direção.
//!
//! A lista de runs do fribidi é duplamente encadeada e circular, com sentinela; aqui ela vive
//! numa arena de nós indexados, e os nós liberados simplesmente deixam de ser alcançáveis.

use crate::bidi_table::RANGES;

pub const MASK_RTL: u32 = 0x0000_0001;
pub const MASK_ARABIC: u32 = 0x0000_0002;
pub const MASK_STRONG: u32 = 0x0000_0010;
pub const MASK_WEAK: u32 = 0x0000_0020;
pub const MASK_NEUTRAL: u32 = 0x0000_0040;
pub const MASK_SENTINEL: u32 = 0x0000_0080;
pub const MASK_LETTER: u32 = 0x0000_0100;
pub const MASK_NUMBER: u32 = 0x0000_0200;
pub const MASK_NUMSEPTER: u32 = 0x0000_0400;
pub const MASK_SPACE: u32 = 0x0000_0800;
pub const MASK_EXPLICIT: u32 = 0x0000_1000;
pub const MASK_SEPARATOR: u32 = 0x0000_2000;
pub const MASK_OVERRIDE: u32 = 0x0000_4000;
pub const MASK_ISOLATE: u32 = 0x0000_8000;
pub const MASK_ES: u32 = 0x0001_0000;
pub const MASK_ET: u32 = 0x0002_0000;
pub const MASK_CS: u32 = 0x0004_0000;
pub const MASK_NSM: u32 = 0x0008_0000;
pub const MASK_BN: u32 = 0x0010_0000;
pub const MASK_BS: u32 = 0x0020_0000;
pub const MASK_SS: u32 = 0x0040_0000;
pub const MASK_WS: u32 = 0x0080_0000;
pub const MASK_FIRST: u32 = 0x0200_0000;

pub const TYPE_LTR: u32 = MASK_STRONG | MASK_LETTER;
pub const TYPE_RTL: u32 = MASK_STRONG | MASK_LETTER | MASK_RTL;
pub const TYPE_AL: u32 = MASK_STRONG | MASK_LETTER | MASK_RTL | MASK_ARABIC;
pub const TYPE_PDF: u32 = MASK_WEAK | MASK_EXPLICIT;
pub const TYPE_EN: u32 = MASK_WEAK | MASK_NUMBER;
pub const TYPE_AN: u32 = MASK_WEAK | MASK_NUMBER | MASK_ARABIC;
pub const TYPE_CS: u32 = MASK_WEAK | MASK_NUMSEPTER | MASK_CS;
pub const TYPE_ET: u32 = MASK_WEAK | MASK_NUMSEPTER | MASK_ET;
pub const TYPE_NSM: u32 = MASK_WEAK | MASK_NSM;
pub const TYPE_BS: u32 = MASK_NEUTRAL | MASK_SPACE | MASK_SEPARATOR | MASK_BS;
pub const TYPE_ON: u32 = MASK_NEUTRAL;
pub const TYPE_SENTINEL: u32 = MASK_SENTINEL;
pub const TYPE_LRI: u32 = MASK_NEUTRAL | MASK_ISOLATE;
pub const TYPE_RLI: u32 = MASK_NEUTRAL | MASK_ISOLATE | MASK_RTL;
pub const TYPE_FSI: u32 = MASK_NEUTRAL | MASK_ISOLATE | MASK_FIRST;
pub const TYPE_PDI: u32 = MASK_NEUTRAL | MASK_WEAK | MASK_ISOLATE;

pub const PAR_LTR: u32 = TYPE_LTR;
pub const PAR_RTL: u32 = TYPE_RTL;
pub const PAR_ON: u32 = TYPE_ON;

const MAX_EXPLICIT_LEVEL: i32 = 125;
const MAX_RESOLVED_LEVELS: usize = 127;
const MAX_NESTED_BRACKET_PAIRS: usize = 63;
const BRACKET_OPEN_MASK: u32 = 0x8000_0000;
const BRACKET_ID_MASK: u32 = 0x7fff_ffff;
const NO_BRACKET: u32 = 0;
const SENTINEL_LEVEL: i32 = -1;

fn lookup(c: u32) -> (u32, u32) {
    let i = RANGES.partition_point(|r| r.1 < c);
    match RANGES.get(i) {
        Some(r) if r.0 <= c => (r.2, r.3),
        _ => (TYPE_LTR, NO_BRACKET),
    }
}

/// `fribidi_get_bidi_type`.
pub fn bidi_type(c: u32) -> u32 {
    lookup(c).0
}

/// `fribidi_get_bidi_types`.
pub fn bidi_types(text: &[u32]) -> Vec<u32> {
    text.iter().map(|&c| bidi_type(c)).collect()
}

/// `fribidi_get_bracket_types`: só o que é do tipo ON conta como colchete.
pub fn bracket_types(text: &[u32], types: &[u32]) -> Vec<u32> {
    text.iter().zip(types).map(|(&c, &t)| if t == TYPE_ON { lookup(c).1 } else { NO_BRACKET }).collect()
}

fn is_rtl(p: u32) -> bool {
    p & MASK_RTL != 0
}
fn is_strong(p: u32) -> bool {
    p & MASK_STRONG != 0
}
fn is_neutral(p: u32) -> bool {
    p & MASK_NEUTRAL != 0
}
fn is_letter(p: u32) -> bool {
    p & MASK_LETTER != 0
}
fn is_number(p: u32) -> bool {
    p & MASK_NUMBER != 0
}
fn is_isolate(p: u32) -> bool {
    p & MASK_ISOLATE != 0
}
fn level_is_rtl(l: i32) -> bool {
    l & 1 != 0
}
fn level_to_dir(l: i32) -> u32 {
    if level_is_rtl(l) { TYPE_RTL } else { TYPE_LTR }
}
fn dir_to_level(d: u32) -> i32 {
    i32::from(is_rtl(d))
}
fn explicit_to_override_dir(p: u32) -> u32 {
    if p & MASK_OVERRIDE != 0 { level_to_dir(dir_to_level(p)) } else { TYPE_ON }
}
fn change_number_to_rtl(p: u32) -> u32 {
    if is_number(p) { TYPE_RTL } else { p }
}
fn type_an_en_as_rtl(p: u32) -> u32 {
    if p == TYPE_AN || p == TYPE_EN || p == TYPE_RTL { TYPE_RTL } else { p }
}

#[derive(Clone, Copy)]
struct Run {
    prev: usize,
    next: usize,
    pos: i32,
    len: i32,
    ty: u32,
    level: i32,
    isolate_level: i32,
    bracket: u32,
    prev_isolate: Option<usize>,
    next_isolate: Option<usize>,
}

/// O `sentinel` estático do `get_adjacent_run`, sempre o nó 0 da arena.
const STATIC_SENTINEL: usize = 0;

struct Arena {
    n: Vec<Run>,
}

impl Arena {
    fn new() -> Arena {
        let mut a = Arena { n: Vec::new() };
        a.n.push(Run {
            prev: 0,
            next: 0,
            pos: 0,
            len: 0,
            ty: TYPE_SENTINEL,
            level: -1,
            isolate_level: -1,
            bracket: NO_BRACKET,
            prev_isolate: None,
            next_isolate: None,
        });
        a
    }

    /// `new_run`.
    fn new_run(&mut self) -> usize {
        let i = self.n.len();
        self.n.push(Run {
            prev: usize::MAX,
            next: usize::MAX,
            pos: 0,
            len: 0,
            ty: 0,
            level: 0,
            isolate_level: 0,
            bracket: NO_BRACKET,
            prev_isolate: None,
            next_isolate: None,
        });
        i
    }

    /// `new_run_list`.
    fn new_run_list(&mut self) -> usize {
        let i = self.new_run();
        let r = &mut self.n[i];
        r.ty = TYPE_SENTINEL;
        r.level = SENTINEL_LEVEL;
        r.pos = SENTINEL_LEVEL;
        r.len = SENTINEL_LEVEL;
        r.next = i;
        r.prev = i;
        i
    }

    fn delete_node(&mut self, x: usize) {
        let (p, n) = (self.n[x].prev, self.n[x].next);
        self.n[p].next = n;
        self.n[n].prev = p;
    }

    fn insert_node_before(&mut self, x: usize, list: usize) {
        let lp = self.n[list].prev;
        self.n[x].prev = lp;
        self.n[lp].next = x;
        self.n[x].next = list;
        self.n[list].prev = x;
    }

    fn move_node_before(&mut self, x: usize, list: usize) {
        if self.n[x].prev != usize::MAX {
            self.delete_node(x);
        }
        self.insert_node_before(x, list);
    }

    /// `run_list_encode_bidi_types`.
    fn encode(&mut self, types: &[u32], brackets: &[u32]) -> usize {
        let list = self.new_run_list();
        let mut last = list;
        for (i, (&t, &b)) in types.iter().zip(brackets).enumerate() {
            let l = &self.n[last];
            if t != l.ty || b != NO_BRACKET || l.bracket != NO_BRACKET || is_isolate(t) {
                let run = self.new_run();
                self.n[run].ty = t;
                self.n[run].pos = i as i32;
                self.n[last].len = i as i32 - self.n[last].pos;
                self.n[last].next = run;
                self.n[run].prev = last;
                self.n[run].bracket = b;
                last = run;
            }
        }
        self.n[last].len = types.len() as i32 - self.n[last].pos;
        self.n[last].next = list;
        self.n[list].prev = last;
        list
    }

    /// `merge_with_prev`.
    fn merge_with_prev(&mut self, second: usize) -> usize {
        let first = self.n[second].prev;
        let snext = self.n[second].next;
        self.n[first].next = snext;
        self.n[snext].prev = first;
        self.n[first].len += self.n[second].len;
        let (spi, sni) = (self.n[second].prev_isolate, self.n[second].next_isolate);
        if let Some(ni) = sni {
            self.n[ni].prev_isolate = spi;
        } else if self.n[snext].prev_isolate == Some(second) {
            self.n[snext].prev_isolate = spi;
        }
        if let Some(pi) = spi {
            self.n[pi].next_isolate = sni;
        }
        self.n[first].next_isolate = sni;
        first
    }

    /// `compact_list`.
    fn compact_list(&mut self, list: usize) {
        let mut pp = self.n[list].next;
        while self.n[pp].ty != TYPE_SENTINEL {
            let p = self.n[self.n[pp].prev];
            let c = self.n[pp];
            if p.ty == c.ty
                && p.level == c.level
                && p.isolate_level == c.isolate_level
                && c.bracket == NO_BRACKET
                && p.bracket == NO_BRACKET
            {
                pp = self.merge_with_prev(pp);
            }
            pp = self.n[pp].next;
        }
    }

    /// `compact_neutrals`.
    fn compact_neutrals(&mut self, list: usize) {
        let mut pp = self.n[list].next;
        while self.n[pp].ty != TYPE_SENTINEL {
            let p = self.n[self.n[pp].prev];
            let c = self.n[pp];
            if p.level == c.level
                && p.isolate_level == c.isolate_level
                && (p.ty == c.ty || (is_neutral(p.ty) && is_neutral(c.ty)))
                && c.bracket == NO_BRACKET
                && p.bracket == NO_BRACKET
            {
                pp = self.merge_with_prev(pp);
            }
            pp = self.n[pp].next;
        }
    }

    /// `get_adjacent_run`.
    fn adjacent(&self, list: usize, forward: bool, skip_neutral: bool) -> usize {
        let step = |i: usize| if forward { self.n[i].next_isolate } else { self.n[i].prev_isolate };
        let Some(mut ppp) = step(list) else { return STATIC_SENTINEL };
        loop {
            let t = self.n[ppp].ty;
            if t == TYPE_SENTINEL {
                break;
            }
            if self.n[ppp].isolate_level > self.n[list].isolate_level
                || (forward && t == TYPE_PDI)
                || (skip_neutral && !is_strong(t))
            {
                ppp = step(ppp).unwrap_or(STATIC_SENTINEL);
                continue;
            }
            break;
        }
        ppp
    }

    /// `shadow_run_list`: sobrepõe os runs de `over` aos de `base`.
    fn shadow(&mut self, base: usize, over: usize, preserve_length: bool) {
        let mut p = base;
        let mut pos = 0;
        let mut q = self.n[over].next;
        while self.n[q].ty != TYPE_SENTINEL {
            if self.n[q].len == 0 || self.n[q].pos < pos {
                q = self.n[q].next;
                continue;
            }
            pos = self.n[q].pos;
            while self.n[self.n[p].next].ty != TYPE_SENTINEL && self.n[self.n[p].next].pos <= pos {
                p = self.n[p].next;
            }
            let pos2 = pos + self.n[q].len;
            let mut r = p;
            while self.n[self.n[r].next].ty != TYPE_SENTINEL && self.n[self.n[r].next].pos < pos2 {
                r = self.n[r].next;
            }
            if preserve_length {
                self.n[r].len += self.n[q].len;
            }
            if p == r {
                if self.n[p].pos + self.n[p].len > pos2 {
                    let nr = self.new_run();
                    let pn = self.n[p].next;
                    self.n[pn].prev = nr;
                    self.n[nr].next = pn;
                    self.n[nr].level = self.n[p].level;
                    self.n[nr].isolate_level = self.n[p].isolate_level;
                    self.n[nr].ty = self.n[p].ty;
                    self.n[nr].len = self.n[p].pos + self.n[p].len - pos2;
                    self.n[nr].pos = pos2;
                    r = nr;
                } else {
                    r = self.n[r].next;
                }
                if self.n[p].pos + self.n[p].len >= pos {
                    if self.n[p].pos < pos {
                        self.n[p].len = pos - self.n[p].pos;
                    } else {
                        p = self.n[p].prev;
                    }
                }
            } else {
                if self.n[p].pos + self.n[p].len >= pos {
                    if self.n[p].pos < pos {
                        self.n[p].len = pos - self.n[p].pos;
                    } else {
                        p = self.n[p].prev;
                    }
                }
                if self.n[r].pos + self.n[r].len > pos2 {
                    self.n[r].len = self.n[r].pos + self.n[r].len - pos2;
                    self.n[r].pos = pos2;
                } else {
                    r = self.n[r].next;
                }
            }
            let t = q;
            q = self.n[q].prev;
            self.delete_node(t);
            self.n[p].next = t;
            self.n[t].prev = p;
            self.n[t].next = r;
            self.n[r].prev = t;
            q = self.n[q].next;
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Status {
    override_: u32,
    level: i32,
    isolate: i32,
    isolate_level: i32,
}

/// O estado da pilha de embutimento explícito (macros `PUSH_STATUS` e `POP_STATUS`).
struct Stack {
    s: Vec<Status>,
    over_pushed: i32,
    first_interval: i32,
    isolate_overflow: i32,
    level: i32,
    override_: u32,
    isolate: i32,
    isolate_level: i32,
}

impl Stack {
    fn push(&mut self, new_level: i32, new_override: u32) {
        if self.over_pushed == 0 && self.isolate_overflow == 0 && new_level <= MAX_EXPLICIT_LEVEL {
            if self.level == MAX_EXPLICIT_LEVEL - 1 {
                self.first_interval = self.over_pushed;
            }
            self.s.push(Status {
                override_: self.override_,
                level: self.level,
                isolate: self.isolate,
                isolate_level: self.isolate_level,
            });
            self.level = new_level;
            self.override_ = new_override;
        } else if self.isolate_overflow == 0 {
            self.over_pushed += 1;
        }
    }

    fn pop(&mut self) {
        if !self.s.is_empty() {
            if self.over_pushed > self.first_interval {
                self.over_pushed -= 1;
            } else {
                if self.over_pushed == self.first_interval {
                    self.first_interval = 0;
                }
                let st = self.s.pop().unwrap_or_default();
                self.level = st.level;
                self.override_ = st.override_;
                self.isolate = st.isolate;
                self.isolate_level = st.isolate_level;
            }
        }
    }
}

fn prev_type_or_sor(a: &Arena, pp: usize) -> u32 {
    let p = a.n[a.n[pp].prev];
    if p.level == a.n[pp].level { p.ty } else { level_to_dir(p.level.max(a.n[pp].level)) }
}

/// `fribidi_get_par_embedding_levels_ex`: devolve o nível máximo mais um (0 em erro, que aqui
/// não acontece) e preenche `levels`; `base_dir` sai resolvido quando entra fraco.
pub fn par_embedding_levels(types: &[u32], brackets: &[u32], base_dir: &mut u32, levels: &mut [i32]) -> i32 {
    let len = types.len();
    if len == 0 {
        return 1;
    }
    let mut a = Arena::new();
    let main = a.encode(types, brackets);

    let mut base_level = dir_to_level(*base_dir);
    if !is_strong(*base_dir) {
        let mut valid_isolate_count = 0;
        let mut pp = a.n[main].next;
        while a.n[pp].ty != TYPE_SENTINEL {
            let t = a.n[pp].ty;
            if t == TYPE_PDI {
                if valid_isolate_count > 0 {
                    valid_isolate_count -= 1;
                }
            } else if is_isolate(t) {
                valid_isolate_count += 1;
            } else if valid_isolate_count == 0 && is_letter(t) {
                base_level = dir_to_level(t);
                *base_dir = level_to_dir(base_level);
                break;
            }
            pp = a.n[pp].next;
        }
    }
    let base_dir_v = level_to_dir(base_level);

    // X1 a X9: níveis explícitos e isolamentos.
    let explicits = a.new_run_list();
    {
        let mut st = Stack {
            s: Vec::new(),
            over_pushed: 0,
            first_interval: 0,
            isolate_overflow: 0,
            level: base_level,
            override_: TYPE_ON,
            isolate: 0,
            isolate_level: 0,
        };
        let mut valid_isolate_count = 0i32;
        let mut new_level = 0;
        let mut pp = a.n[main].next;
        while a.n[pp].ty != TYPE_SENTINEL {
            let this_type = a.n[pp].ty;
            a.n[pp].isolate_level = st.isolate_level;
            let mut next = a.n[pp].next;
            if this_type & (MASK_EXPLICIT | MASK_BN) != 0 {
                if is_strong(this_type) {
                    let new_override = explicit_to_override_dir(this_type);
                    for _ in 0..a.n[pp].len {
                        new_level = ((st.level + dir_to_level(this_type) + 2) & !1) - dir_to_level(this_type);
                        st.isolate = 0;
                        st.push(new_level, new_override);
                    }
                } else if this_type == TYPE_PDF {
                    for _ in 0..a.n[pp].len {
                        if st.s.last().is_some_and(|s| s.isolate != 0) {
                            break;
                        }
                        st.pop();
                    }
                }
                a.n[pp].level = SENTINEL_LEVEL;
                next = a.n[pp].next;
                a.move_node_before(pp, explicits);
            } else if this_type == TYPE_PDI {
                for _ in 0..a.n[pp].len {
                    if st.isolate_overflow > 0 {
                        st.isolate_overflow -= 1;
                        a.n[pp].level = st.level;
                    } else if valid_isolate_count > 0 {
                        while st.s.last().is_some_and(|s| s.isolate == 0) {
                            st.pop();
                        }
                        st.over_pushed = 0;
                        st.pop();
                        if st.isolate_level > 0 {
                            st.isolate_level -= 1;
                        }
                        valid_isolate_count -= 1;
                        a.n[pp].level = st.level;
                        a.n[pp].isolate_level = st.isolate_level;
                    } else {
                        a.n[pp].ty = TYPE_ON;
                        a.n[pp].level = st.level;
                    }
                }
            } else if is_isolate(this_type) {
                let new_override = TYPE_ON;
                st.isolate = 1;
                let level = st.level;
                if this_type == TYPE_LRI {
                    new_level = level + 2 - (level % 2);
                } else if this_type == TYPE_RLI {
                    new_level = level + 1 + (level % 2);
                } else if this_type == TYPE_FSI {
                    let mut isolate_count = 0;
                    let mut fsi_base_level = 0;
                    let mut f = a.n[pp].next;
                    while a.n[f].ty != TYPE_SENTINEL {
                        let t = a.n[f].ty;
                        if t == TYPE_PDI {
                            isolate_count -= 1;
                            // O fribidi testa a variável errada aqui; reproduzido como é.
                            if valid_isolate_count < 0 {
                                break;
                            }
                        } else if is_isolate(t) {
                            isolate_count += 1;
                        } else if isolate_count == 0 && is_letter(t) {
                            fsi_base_level = dir_to_level(t);
                            break;
                        }
                        f = a.n[f].next;
                    }
                    new_level = if level_is_rtl(fsi_base_level) { level + 1 + (level % 2) } else { level + 2 - (level % 2) };
                }
                a.n[pp].level = level;
                a.n[pp].isolate_level = st.isolate_level;
                if st.isolate_level < MAX_EXPLICIT_LEVEL - 1 {
                    st.isolate_level += 1;
                }
                if !is_neutral(st.override_) {
                    a.n[pp].ty = st.override_;
                }
                if new_level <= MAX_EXPLICIT_LEVEL {
                    valid_isolate_count += 1;
                    st.push(new_level, new_override);
                    st.level = new_level;
                } else {
                    st.isolate_overflow += 1;
                }
            } else if this_type == TYPE_BS {
                break;
            } else {
                a.n[pp].level = st.level;
                if !is_neutral(st.override_) {
                    a.n[pp].ty = st.override_;
                }
            }
            pp = next;
        }

        let mut run_per_isolate_level: [Option<usize>; MAX_RESOLVED_LEVELS] = [None; MAX_RESOLVED_LEVELS];
        let mut prev_isolate_level = 0;
        let mut pp = a.n[main].next;
        while a.n[pp].ty != TYPE_SENTINEL {
            let il = a.n[pp].isolate_level;
            if il < prev_isolate_level {
                for slot in &mut run_per_isolate_level[(il + 1) as usize..=prev_isolate_level as usize] {
                    *slot = None;
                }
            }
            prev_isolate_level = il;
            if let Some(r) = run_per_isolate_level[il as usize] {
                a.n[r].next_isolate = Some(pp);
                a.n[pp].prev_isolate = Some(r);
            }
            run_per_isolate_level[il as usize] = Some(pp);
            pp = a.n[pp].next;
        }
    }

    a.compact_list(main);

    // W1 a W7.
    let mut max_iso_level = 0;
    {
        let mut last_strong = [0u32; MAX_RESOLVED_LEVELS];
        last_strong[0] = base_dir_v;
        let neighbours = |a: &Arena, pp: usize, number_to_rtl: bool| -> (usize, usize, u32, u32) {
            let pv = a.adjacent(pp, false, false);
            let nx = a.adjacent(pp, true, false);
            let f = |t: u32| if number_to_rtl { change_number_to_rtl(t) } else { t };
            let lv = a.n[pp].level;
            let pt = if a.n[pv].level == lv { f(a.n[pv].ty) } else { level_to_dir(a.n[pv].level.max(lv)) };
            let nt = if a.n[nx].level == lv { f(a.n[nx].ty) } else { level_to_dir(a.n[nx].level.max(lv)) };
            (pv, nx, pt, nt)
        };

        let mut pp = a.n[main].next;
        while a.n[pp].ty != TYPE_SENTINEL {
            let (ppp_prev, ppp_next, prev_type, next_type) = neighbours(&a, pp, false);
            let this_type = a.n[pp].ty;
            let iso_level = a.n[pp].isolate_level;
            if iso_level > max_iso_level {
                max_iso_level = iso_level;
            }
            if is_strong(prev_type) {
                last_strong[iso_level as usize] = prev_type;
            }
            if this_type == TYPE_NSM {
                if is_isolate(a.n[a.n[pp].prev].ty) {
                    a.n[pp].ty = TYPE_ON;
                }
                if a.n[ppp_prev].level == a.n[pp].level {
                    if ppp_prev == a.n[pp].prev {
                        pp = a.merge_with_prev(pp);
                    }
                } else {
                    a.n[pp].ty = prev_type;
                }
                if prev_type == next_type && a.n[pp].level == a.n[a.n[pp].next].level && ppp_next == a.n[pp].next {
                    pp = a.merge_with_prev(a.n[pp].next);
                }
                pp = a.n[pp].next;
                continue;
            }
            if this_type == TYPE_EN && last_strong[iso_level as usize] == TYPE_AL {
                a.n[pp].ty = TYPE_AN;
                if next_type == TYPE_NSM {
                    a.n[ppp_next].ty = TYPE_AN;
                }
            }
            pp = a.n[pp].next;
        }

        last_strong[0] = base_dir_v;
        let mut w4 = true;
        let mut prev_type_orig = TYPE_ON;
        let mut pp = a.n[main].next;
        while a.n[pp].ty != TYPE_SENTINEL {
            let (_, _, prev_type, next_type) = neighbours(&a, pp, false);
            let mut this_type = a.n[pp].ty;
            let iso_level = a.n[pp].isolate_level as usize;
            if is_strong(prev_type) {
                last_strong[iso_level] = prev_type;
            }
            if this_type == TYPE_AL {
                a.n[pp].ty = TYPE_RTL;
                w4 = true;
                prev_type_orig = TYPE_ON;
                pp = a.n[pp].next;
                continue;
            }
            if w4
                && a.n[pp].len == 1
                && this_type & (MASK_ES | MASK_CS) != 0
                && is_number(prev_type_orig)
                && prev_type_orig == next_type
                && (prev_type_orig == TYPE_EN || this_type == TYPE_CS)
            {
                a.n[pp].ty = prev_type;
                this_type = prev_type;
            }
            w4 = true;
            if this_type == TYPE_ET && (prev_type_orig == TYPE_EN || next_type == TYPE_EN) {
                a.n[pp].ty = TYPE_EN;
                w4 = false;
                this_type = TYPE_EN;
            }
            if this_type & MASK_NUMSEPTER != 0 {
                a.n[pp].ty = TYPE_ON;
            }
            if this_type == TYPE_EN && last_strong[iso_level] == TYPE_LTR {
                a.n[pp].ty = TYPE_LTR;
                prev_type_orig = if a.n[pp].level == a.n[a.n[pp].next].level { TYPE_EN } else { TYPE_ON };
            } else {
                prev_type_orig = prev_type_or_sor(&a, a.n[pp].next);
            }
            pp = a.n[pp].next;
        }
    }

    a.compact_neutrals(main);

    // N0: pares de colchetes.
    {
        let num_iso_levels = (max_iso_level + 1) as usize;
        let mut stacks: Vec<Vec<usize>> = vec![Vec::new(); num_iso_levels.max(1)];
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        let mut last_level = a.n[main].level;
        let mut last_iso_level = 0;
        let mut pp = a.n[main].next;
        while a.n[pp].ty != TYPE_SENTINEL {
            let level = a.n[pp].level;
            let iso_level = a.n[pp].isolate_level as usize;
            let brack = a.n[pp].bracket;
            if level != last_level && last_iso_level == iso_level {
                stacks[last_iso_level].clear();
            }
            if brack != NO_BRACKET && a.n[pp].ty == TYPE_ON {
                if brack & BRACKET_OPEN_MASK != 0 {
                    if stacks[iso_level].len() == MAX_NESTED_BRACKET_PAIRS {
                        break;
                    }
                    stacks[iso_level].push(pp);
                } else {
                    let st = &mut stacks[iso_level];
                    if let Some(idx) =
                        st.iter().rposition(|&o| a.n[o].bracket & BRACKET_ID_MASK == brack & BRACKET_ID_MASK)
                    {
                        pairs.push((st[idx], pp));
                        st.truncate(idx);
                    }
                }
            }
            last_level = level;
            last_iso_level = iso_level;
            pp = a.n[pp].next;
        }
        pairs.sort_by_key(|&(o, _)| a.n[o].pos);

        for &(open, close) in &pairs {
            let embedding_level = a.n[open].level;
            let mut found = false;
            let mut ppn = open;
            while ppn != close {
                let t = type_an_en_as_rtl(a.n[ppn].ty);
                let l = a.n[ppn].level + (i32::from(level_is_rtl(a.n[ppn].level)) ^ dir_to_level(t));
                if is_strong(t) && l == embedding_level {
                    let d = if l % 2 != 0 { TYPE_RTL } else { TYPE_LTR };
                    a.n[open].ty = d;
                    a.n[close].ty = d;
                    found = true;
                    break;
                }
                ppn = a.n[ppn].next;
            }
            if !found {
                let mut prec_strong_level = embedding_level;
                let iso_level = a.n[open].isolate_level;
                let mut ppn = a.n[open].prev;
                while a.n[ppn].ty != TYPE_SENTINEL {
                    let t = type_an_en_as_rtl(a.n[ppn].ty);
                    if is_strong(t) && a.n[ppn].isolate_level == iso_level {
                        prec_strong_level =
                            a.n[ppn].level + (i32::from(level_is_rtl(a.n[ppn].level)) ^ dir_to_level(t));
                        break;
                    }
                    ppn = a.n[ppn].prev;
                }
                let mut ppn = open;
                while ppn != close {
                    let t = type_an_en_as_rtl(a.n[ppn].ty);
                    if is_strong(t) && a.n[ppn].isolate_level == iso_level {
                        let d = if prec_strong_level % 2 != 0 { TYPE_RTL } else { TYPE_LTR };
                        a.n[open].ty = d;
                        a.n[close].ty = d;
                        break;
                    }
                    ppn = a.n[ppn].next;
                }
            }
        }

        let mut pp = a.n[main].next;
        while a.n[pp].ty != TYPE_SENTINEL {
            a.n[pp].bracket = NO_BRACKET;
            pp = a.n[pp].next;
        }
        a.compact_neutrals(main);
    }

    // N1 e N2.
    let mut pp = a.n[main].next;
    while a.n[pp].ty != TYPE_SENTINEL {
        let pv = a.adjacent(pp, false, false);
        let nx = a.adjacent(pp, true, false);
        let lv = a.n[pp].level;
        let this_type = change_number_to_rtl(a.n[pp].ty);
        let pt = if a.n[pv].level == lv { change_number_to_rtl(a.n[pv].ty) } else { level_to_dir(a.n[pv].level.max(lv)) };
        let nt = if a.n[nx].level == lv { change_number_to_rtl(a.n[nx].ty) } else { level_to_dir(a.n[nx].level.max(lv)) };
        if is_neutral(this_type) {
            a.n[pp].ty = if pt == nt { pt } else { level_to_dir(lv) };
        }
        pp = a.n[pp].next;
    }

    a.compact_list(main);

    // I1 e I2.
    let mut max_level = base_level;
    let mut pp = a.n[main].next;
    while a.n[pp].ty != TYPE_SENTINEL {
        let t = a.n[pp].ty;
        let level = a.n[pp].level;
        a.n[pp].level =
            if is_number(t) { (level + 2) & !1 } else { level + (i32::from(level_is_rtl(level)) ^ dir_to_level(t)) };
        if a.n[pp].level > max_level {
            max_level = a.n[pp].level;
        }
        pp = a.n[pp].next;
    }

    a.compact_list(main);

    // Reinsere os explícitos, que herdam o nível do run anterior.
    if a.n[explicits].next != explicits {
        a.shadow(main, explicits, true);
        let p = a.n[main].next;
        if p != main && a.n[p].level == SENTINEL_LEVEL {
            a.n[p].level = base_level;
        }
        let mut p = a.n[main].next;
        while a.n[p].ty != TYPE_SENTINEL {
            if a.n[p].level == SENTINEL_LEVEL {
                a.n[p].level = a.n[a.n[p].prev].level;
            }
            p = a.n[p].next;
        }
    }

    // L1: separadores e brancos finais voltam ao nível de base.
    {
        let list = a.new_run_list();
        let mut q = list;
        let mut state = true;
        let mut pos = len as i32 - 1;
        let mut j = len as i32 - 1;
        while j >= -1 {
            let ct = if j >= 0 { types[j as usize] } else { TYPE_ON };
            if !state && ct & MASK_SEPARATOR != 0 {
                state = true;
                pos = j;
            } else if state && ct & (MASK_EXPLICIT | MASK_SEPARATOR | MASK_BN | MASK_WS | MASK_ISOLATE) == 0 {
                state = false;
                let p = a.new_run();
                a.n[p].pos = j + 1;
                a.n[p].len = pos - j;
                a.n[p].ty = base_dir_v;
                a.n[p].level = base_level;
                a.move_node_before(p, q);
                q = p;
            }
            j -= 1;
        }
        a.shadow(main, list, false);
    }

    let mut pos = 0usize;
    let mut pp = a.n[main].next;
    while a.n[pp].ty != TYPE_SENTINEL {
        for _ in 0..a.n[pp].len {
            levels[pos] = a.n[pp].level;
            pos += 1;
        }
        pp = a.n[pp].next;
    }
    max_level + 1
}
