// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDO) INFTY MULT accum breakwords linebreak linebreaking linebreaks linelen maxlength minlength nchars ostream overlen parasplit plass posn powf punct signum slen sstart tabwidth tlen underlen winfo wlen wordlen

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
// Porte pseudo-linus: o algoritmo de Knuth-Plass do uutils ficou sem uso (o GNU quebra as linhas
// por outro custo); as funções dele continuam no arquivo.
#![allow(dead_code)]

use sysio::io::{BufWriter, Stdout, Write};
use std::mem;

use crate::FmtOptions;
use crate::parasplit::{ParaWords, Paragraph, WordInfo};

struct BreakArgs<'a> {
    opts: &'a FmtOptions,
    init_len: usize,
    indent: &'a [u8],
    indent_len: usize,
    uniform: bool,
    ostream: &'a mut BufWriter<Stdout>,
}

impl BreakArgs<'_> {
    fn compute_width(&self, winfo: &WordInfo, posn: usize, fresh: bool) -> usize {
        if fresh {
            0
        } else {
            let post = winfo.after_tab;
            match winfo.before_tab {
                None => post,
                Some(pre) => {
                    post + ((pre + posn) / self.opts.tabwidth + 1) * self.opts.tabwidth - posn
                }
            }
        }
    }
}

pub fn break_lines(
    para: &Paragraph,
    opts: &FmtOptions,
    ostream: &mut BufWriter<Stdout>,
) -> sysio::io::Result<()> {
    // indent
    let p_indent = &para.indent_str;
    let p_indent_len = para.indent_len;

    // words
    let p_words = ParaWords::new(opts, para);
    let mut p_words_words = p_words.words();

    // the first word will *always* appear on the first line
    // make sure of this here
    let Some(winfo) = p_words_words.next() else {
        return ostream.write_all(b"\n");
    };

    // print the init, if it exists, and get its length
    let p_init_len = winfo.word_nchars
        + if opts.crown || opts.tagged {
            // handle "init" portion
            ostream.write_all(&para.init_str)?;
            para.init_len
        } else if !para.mail_header {
            // for non-(crown, tagged) that's the same as a normal indent
            ostream.write_all(p_indent)?;
            p_indent_len
        } else {
            // except that mail headers get no indent at all
            0
        };

    // write first word after writing init
    ostream.write_all(winfo.word)?;

    // does this paragraph require uniform spacing?
    let uniform = para.mail_header || opts.uniform;

    let mut break_args = BreakArgs {
        opts,
        init_len: p_init_len,
        indent: p_indent,
        indent_len: p_indent_len,
        uniform,
        ostream,
    };

    if opts.quick || para.mail_header {
        break_simple(p_words_words, &mut break_args)
    } else {
        break_knuth_plass(p_words_words, &mut break_args)
    }
}

/// `break_simple` implements a "greedy" breaking algorithm: print words until
/// maxlength would be exceeded, then print a linebreak and indent and continue.
fn break_simple<'a, T: Iterator<Item = &'a WordInfo<'a>>>(
    mut iter: T,
    args: &mut BreakArgs<'a>,
) -> sysio::io::Result<()> {
    iter.try_fold((args.init_len, false), |(l, prev_punct), winfo| {
        accum_words_simple(args, l, prev_punct, winfo)
    })?;
    args.ostream.write_all(b"\n")
}

fn accum_words_simple<'a>(
    args: &mut BreakArgs<'a>,
    l: usize,
    prev_punct: bool,
    winfo: &'a WordInfo<'a>,
) -> sysio::io::Result<(usize, bool)> {
    // compute the length of this word, considering how tabs will expand at this position on the line
    let wlen = winfo.word_nchars + args.compute_width(winfo, l, false);

    let slen = compute_slen(
        args.uniform,
        winfo.new_line,
        winfo.sentence_start,
        prev_punct,
    );

    if l + wlen + slen > args.opts.width {
        write_newline(args.indent, args.ostream)?;
        write_with_spaces(&winfo.word[winfo.word_start..], 0, args.ostream)?;
        Ok((args.indent_len + winfo.word_nchars, winfo.ends_punct))
    } else {
        write_with_spaces(winfo.word, slen, args.ostream)?;
        Ok((l + wlen + slen, winfo.ends_punct))
    }
}

/// `break_knuth_plass` implements an "optimal" breaking algorithm in the style of
/// Knuth, D.E., and Plass, M.F. "Breaking Paragraphs into Lines." in Software,
/// Practice and Experience. Vol. 11, No. 11, November 1981.
/// <http://onlinelibrary.wiley.com/doi/10.1002/spe.4380111102/pdf>
fn break_knuth_plass<'a, T: Clone + Iterator<Item = &'a WordInfo<'a>>>(
    mut iter: T,
    args: &mut BreakArgs<'a>,
) -> sysio::io::Result<()> {
    // run the algorithm to get the breakpoints
    // Porte pseudo-linus: a quebra ótima é a do GNU (ver `find_gnu_breakpoints`).
    let breakpoints = find_gnu_breakpoints(iter.clone(), args);

    // iterate through the breakpoints (note that breakpoints is in reverse break order, so we .rev() it
    let result: sysio::io::Result<(bool, bool)> = breakpoints.iter().rev().try_fold(
        (false, false),
        |(mut prev_punct, mut fresh), &(next_break, break_before)| {
            if fresh {
                write_newline(args.indent, args.ostream)?;
            }
            // at each breakpoint, keep emitting words until we find the word matching this breakpoint
            for winfo in &mut iter {
                let (slen, word) = slice_if_fresh(
                    fresh,
                    winfo.word,
                    winfo.word_start,
                    args.uniform,
                    winfo.new_line,
                    winfo.sentence_start,
                    prev_punct,
                );
                fresh = false;
                prev_punct = winfo.ends_punct;

                // We find identical breakpoints here by comparing addresses of the references.
                // This is OK because the backing vector is not mutating once we are linebreaking.
                if std::ptr::eq(winfo, next_break) {
                    // OK, we found the matching word
                    if break_before {
                        write_newline(args.indent, args.ostream)?;
                        write_with_spaces(&winfo.word[winfo.word_start..], 0, args.ostream)?;
                    } else {
                        // breaking after this word, so that means "fresh" is true for the next iteration
                        write_with_spaces(word, slen, args.ostream)?;
                        fresh = true;
                    }
                    break;
                }
                write_with_spaces(word, slen, args.ostream)?;
            }
            Ok((prev_punct, fresh))
        },
    );
    let (mut prev_punct, mut fresh) = result?;

    // after the last linebreak, write out the rest of the final line.
    for winfo in iter {
        if fresh {
            write_newline(args.indent, args.ostream)?;
        }
        let (slen, word) = slice_if_fresh(
            fresh,
            winfo.word,
            winfo.word_start,
            args.uniform,
            winfo.new_line,
            winfo.sentence_start,
            prev_punct,
        );
        prev_punct = winfo.ends_punct;
        fresh = false;
        write_with_spaces(word, slen, args.ostream)?;
    }
    args.ostream.write_all(b"\n")
}

struct LineBreak<'a> {
    prev: usize,
    linebreak: Option<&'a WordInfo<'a>>,
    break_before: bool,
    demerits: i64,
    prev_rat: f32,
    length: usize,
    fresh: bool,
}

// Porte pseudo-linus: a quebra ótima do `fmt` do GNU 9.7 (programação dinâmica de trás pra frente,
// com custo quadrático em relação à meta, penalidade de irregularidade entre linhas consecutivas e
// bônus e multas por fim de sentença). As constantes foram medidas contra o oráculo, não lidas do
// GNU: a forma é a do GNU, os números são o que reproduz a saída dele.
const SENTENCE_BONUS: i64 = 25;
const NOBREAK_COST: i64 = 600;
const WIDOW_COST_NUM: i64 = 400;
const ORPHAN_COST_NUM: i64 = 300;

/// Custo da linha de comprimento `len` que termina antes da palavra `next` (índice em `0..=total`,
/// onde `total` é o fim do parágrafo e a última linha sai de graça).
fn gnu_line_cost(
    next: usize,
    total: usize,
    len: i64,
    goal: i64,
    next_break: &[usize],
    line_length: &[i64],
) -> i64 {
    if next == total {
        return 0;
    }
    let short = goal - len;
    // Passar da meta custa mais do que ficar aquém dela.
    let mut cost = short * short * if short < 0 { 2 } else { 1 };
    if next_break[next] != total {
        let ragged = len - line_length[next];
        cost += ragged * ragged / 2;
    }
    cost
}

/// Custo de abrir uma linha na palavra `this`, olhando a pontuação em volta.
fn gnu_base_cost(
    this: usize,
    total: usize,
    period: &[bool],
    sentence_end: &[bool],
    length: &[i64],
    next_break: &[usize],
) -> i64 {
    let mut cost = 0;
    if this > 0 {
        if period[this - 1] {
            if sentence_end[this - 1] {
                cost -= SENTENCE_BONUS;
            } else {
                cost += NOBREAK_COST;
            }
        } else if this > 1 && sentence_end[this - 2] {
            cost += WIDOW_COST_NUM / (length[this - 1] + 2);
        }
    }
    if this < total && sentence_end[this] && next_break[this] == this + 1 {
        cost += ORPHAN_COST_NUM / (length[this] + 2);
    }
    cost
}

/// Acha as quebras de linha ótimas: devolve, do último pro primeiro, as palavras que abrem uma
/// linha nova (a primeira linha não entra).
fn find_gnu_breakpoints<'a, T: Iterator<Item = &'a WordInfo<'a>>>(
    iter: T,
    args: &BreakArgs<'a>,
) -> Vec<(&'a WordInfo<'a>, bool)> {
    // `rest[j - 1]` é a palavra `j`; a palavra 0 é a primeira, que o chamador já escreveu.
    let rest: Vec<&'a WordInfo<'a>> = iter.collect();
    if rest.is_empty() {
        return Vec::new();
    }
    let total = rest.len() + 1;
    let goal = args.opts.goal as i64;

    let mut sentence_end = vec![false; total];
    let mut period = vec![false; total];
    let mut length = vec![0_i64; total];
    length[0] = args.init_len.saturating_sub(args.indent_len) as i64;
    for j in 1..total {
        let word = rest[j - 1];
        period[j] = word.ends_punct;
        length[j] = word.word_nchars as i64;
        sentence_end[j] = match rest.get(j) {
            None => true,
            Some(next) => next.sentence_start || (next.new_line && word.ends_punct),
        };
    }
    sentence_end[0] = total == 1;

    // Espaço antes de cada palavra, como o GNU pós-fixa: depende só do fim de sentença anterior.
    let mut slen = vec![0_usize; total];
    for j in 1..total {
        slen[j] = compute_slen(args.uniform, rest[j - 1].new_line, sentence_end[j - 1], false);
    }

    let mut best_cost = vec![0_i64; total + 1];
    let mut next_break = vec![total; total + 1];
    let mut line_length = vec![0_i64; total + 1];

    for start in (0..total).rev() {
        let mut best = i64::MAX;
        let mut len = if start == 0 {
            args.init_len
        } else {
            args.indent_len + rest[start - 1].word_nchars
        };
        let mut w = start;
        loop {
            w += 1;
            // Considera quebrar antes de `w`.
            let cost = gnu_line_cost(w, total, len as i64, goal, &next_break, &line_length)
                + best_cost[w];
            if cost < best {
                best = cost;
                next_break[start] = w;
                line_length[start] = len as i64;
            }
            if w == total {
                break;
            }
            let word = rest[w - 1];
            len += slen[w] + args.compute_width(word, len, false) + word.word_nchars;
            if len >= args.opts.width {
                break;
            }
        }
        best_cost[start] = best
            + gnu_base_cost(start, total, &period, &sentence_end, &length, &next_break);
    }

    let mut breaks = Vec::new();
    let mut i = next_break[0];
    while i < total {
        breaks.push((rest[i - 1], true));
        i = next_break[i];
    }
    breaks.reverse();
    breaks
}

#[allow(clippy::cognitive_complexity)]
fn find_kp_breakpoints<'a, T: Clone + Iterator<Item = &'a WordInfo<'a>>>(
    iter: T,
    args: &BreakArgs<'a>,
) -> Vec<(&'a WordInfo<'a>, bool)> {
    let mut iter = iter.peekable();
    // set up the initial null linebreak
    let mut linebreaks = vec![LineBreak {
        prev: 0,
        linebreak: None,
        break_before: false,
        demerits: 0,
        prev_rat: f32::NAN,
        length: args.init_len,
        fresh: false,
    }];
    // this vec holds the current active linebreaks; next_ holds the breaks that will be active for
    // the next word
    let mut active_breaks = vec![0];
    let mut next_active_breaks = vec![];

    let stretch = args.opts.width - args.opts.goal;
    let minlength = if args.opts.goal <= 10 {
        1
    } else {
        args.opts.goal.max(stretch + 1) - stretch
    };
    let mut new_linebreaks = vec![];
    let mut is_sentence_start = false;
    while let Some(w) = iter.next() {
        let next_word_sentence_final = is_next_word_sentence_final(iter.clone());
        // if this is the last word, we don't add additional demerits for this break
        let (is_last_word, is_sentence_end) = match iter.peek() {
            None => (true, true),
            Some(&&WordInfo {
                sentence_start: st,
                new_line: nl,
                ..
            }) => (false, st || (nl && w.ends_punct)),
        };

        // should we be adding extra space at the beginning of the next sentence?
        let slen = compute_slen(args.uniform, w.new_line, is_sentence_start, false);

        let mut best_active_demerits = i64::MAX;
        let mut ld_idx = 0;
        new_linebreaks.clear();
        let mut best_break_before: Option<LineBreak<'_>> = None;
        let mut best_break_after: Option<LineBreak<'_>> = None;
        next_active_breaks.clear();
        // go through each active break, extending it and possibly adding a new active
        // break if we are above the minimum required length
        #[allow(clippy::explicit_iter_loop)]
        for &i in active_breaks.iter() {
            let active = &mut linebreaks[i];
            if active.demerits < best_active_demerits {
                best_active_demerits = active.demerits;
                ld_idx = i;
            }

            // Also consider a break before this word, so the previous line can end at the
            // prior word when that yields a better global layout.
            if !active.fresh && active.length >= minlength {
                let (mut new_demerits, new_ratio) = compute_demerits(
                    args.opts.goal as isize - active.length as isize,
                    stretch,
                    w.word_nchars,
                    active.prev_rat,
                );
                if is_sentence_end {
                    new_demerits = new_demerits.saturating_add(ORPHAN_BREAK_PENALTY);
                }
                let total_demerits = active.demerits.saturating_add(new_demerits);
                if best_break_before
                    .as_ref()
                    .is_none_or(|best| total_demerits < best.demerits)
                {
                    best_break_before = Some(LineBreak {
                        prev: i,
                        linebreak: Some(w),
                        break_before: true,
                        demerits: total_demerits,
                        prev_rat: new_ratio,
                        length: args.indent_len + w.word_nchars,
                        fresh: false,
                    });
                }
            }

            // get the new length
            let tlen = w.word_nchars
                + args.compute_width(w, active.length, active.fresh)
                + slen
                + active.length;

            // if tlen is longer than args.opts.width, we drop this break from the active list
            // otherwise, we extend the break, and possibly add a new break at this point
            if tlen <= args.opts.width {
                // this break will still be active next time
                next_active_breaks.push(i);
                // we can put this word on this line
                active.fresh = false;
                active.length = tlen;

                // if we're above the minlength, we can also consider breaking here
                if tlen >= minlength {
                    let (mut new_demerits, new_ratio) = if is_last_word {
                        // there is no penalty for the final line's length
                        (0, 0.0)
                    } else {
                        compute_demerits(
                            args.opts.goal as isize - tlen as isize,
                            stretch,
                            w.word_nchars,
                            active.prev_rat,
                        )
                    };

                    if !is_last_word && next_word_sentence_final {
                        new_demerits = new_demerits.saturating_add(ORPHAN_BREAK_PENALTY);
                    }

                    let total_demerits = active.demerits.saturating_add(new_demerits);
                    if best_break_after
                        .as_ref()
                        .is_none_or(|best| total_demerits < best.demerits)
                    {
                        best_break_after = Some(LineBreak {
                            prev: i,
                            linebreak: Some(w),
                            break_before: false,
                            demerits: total_demerits,
                            prev_rat: new_ratio,
                            length: args.indent_len,
                            fresh: true,
                        });
                    }
                }
            }
        }

        if let Some(lb) = best_break_before {
            new_linebreaks.push(lb);
        }
        if let Some(lb) = best_break_after {
            new_linebreaks.push(lb);
        }

        for lb in new_linebreaks.drain(..) {
            next_active_breaks.push(linebreaks.len());
            linebreaks.push(lb);
        }

        if next_active_breaks.is_empty() {
            // every potential linebreak is too long! choose the linebreak with the least demerits, ld_idx
            let new_break =
                restart_active_breaks(args, &linebreaks[ld_idx], ld_idx, w, slen, minlength);
            next_active_breaks.push(linebreaks.len());
            linebreaks.push(new_break);
        }
        // swap in new list of active breaks
        mem::swap(&mut active_breaks, &mut next_active_breaks);
        // If this was the last word in a sentence, the next one must be the first in the next.
        is_sentence_start = is_sentence_end;
    }

    // return the best path
    build_best_path(&linebreaks, &active_breaks)
}

fn build_best_path<'a>(paths: &[LineBreak<'a>], active: &[usize]) -> Vec<(&'a WordInfo<'a>, bool)> {
    // of the active paths, we select the one with the fewest demerits
    active
        .iter()
        .min_by_key(|&&a| paths[a].demerits)
        .map(|&(mut best_idx)| {
            let mut breakwords = vec![];
            // now, chase the pointers back through the break list, recording
            // the words at which we should break
            loop {
                let next_best = &paths[best_idx];
                match next_best.linebreak {
                    None => return breakwords,
                    Some(prev) => {
                        breakwords.push((prev, next_best.break_before));
                        best_idx = next_best.prev;
                    }
                }
            }
        })
        .unwrap_or_default()
}

// badness = BAD_MULT * abs(r) ^ 3
const BAD_MULT: f32 = 200.0;
// DR_MULT is multiplier for delta-R between lines
const DR_MULT: f32 = 600.0;
// DL_MULT is penalty multiplier for short words at end of line
const DL_MULT: f32 = 10.0;
// Penalize breaks that leave the first word on the next line as the sentence-final word.
const ORPHAN_BREAK_PENALTY: i64 = 250_000_000;

fn is_word_sentence_final(current: &WordInfo, next: Option<&WordInfo>) -> bool {
    match next {
        None => true,
        Some(next_word) => next_word.sentence_start || (next_word.new_line && current.ends_punct),
    }
}

fn is_next_word_sentence_final<'a, T: Iterator<Item = &'a WordInfo<'a>>>(mut iter: T) -> bool {
    let Some(next_word) = iter.next() else {
        return false;
    };
    is_word_sentence_final(next_word, iter.next())
}

fn compute_demerits(delta_len: isize, stretch: usize, wlen: usize, prev_rat: f32) -> (i64, f32) {
    // how much stretch are we using?
    let ratio = if delta_len == 0 {
        0.0f32
    } else {
        delta_len as f32 / stretch as f32
    };

    // compute badness given the stretch ratio
    let bad_linelen = (BAD_MULT * ratio.powi(3).abs()) as i64;

    // we penalize lines ending in really short words
    let bad_wordlen = if wlen >= stretch {
        0
    } else {
        (DL_MULT
            * ((stretch - wlen) as f32 / (stretch - 1) as f32)
                .powi(3)
                .abs()) as i64
    };

    // we penalize lines that have very different ratios from previous lines
    let bad_delta_r = if prev_rat.is_nan() {
        0
    } else {
        (DR_MULT * ((ratio - prev_rat) / 2.0).powi(3).abs()) as i64
    };

    let demerits_base = 1_i64
        .saturating_add(bad_linelen)
        .saturating_add(bad_wordlen)
        .saturating_add(bad_delta_r);
    let demerits = demerits_base.saturating_mul(demerits_base);

    (demerits, ratio)
}

fn restart_active_breaks<'a>(
    args: &BreakArgs<'a>,
    active: &LineBreak<'a>,
    act_idx: usize,
    w: &'a WordInfo<'a>,
    slen: usize,
    min: usize,
) -> LineBreak<'a> {
    let (break_before, line_length) = if active.fresh {
        // never break before a word if that word would be the first on a line
        (false, args.indent_len)
    } else {
        // choose the lesser evil: breaking too early, or breaking too late
        let wlen = w.word_nchars + args.compute_width(w, active.length, active.fresh);
        let underlen = min as isize - active.length as isize;
        let overlen = (wlen + slen + active.length) as isize - args.opts.width as isize;
        if overlen > underlen {
            // break early, put this word on the next line
            (true, args.indent_len + w.word_nchars)
        } else {
            (false, args.indent_len)
        }
    };

    // restart the linebreak. This will be our only active path.
    LineBreak {
        prev: act_idx,
        linebreak: Some(w),
        break_before,
        demerits: 0, // this is the only active break, so we can reset the demerit count
        prev_rat: if break_before { 1.0 } else { -1.0 },
        length: line_length,
        fresh: !break_before,
    }
}

/// Number of spaces to add before a word, based on mode, newline, sentence start.
fn compute_slen(uniform: bool, newline: bool, start: bool, punct: bool) -> usize {
    if uniform || newline {
        if start || (newline && punct) { 2 } else { 1 }
    } else {
        0
    }
}

/// If we're on a fresh line, `slen=0` and we slice off leading whitespace.
/// Otherwise, compute `slen` and leave whitespace alone.
fn slice_if_fresh(
    fresh: bool,
    word: &[u8],
    start: usize,
    uniform: bool,
    newline: bool,
    sstart: bool,
    punct: bool,
) -> (usize, &[u8]) {
    if fresh {
        (0, &word[start..])
    } else {
        (compute_slen(uniform, newline, sstart, punct), word)
    }
}

/// Write a newline and add the indent.
fn write_newline(indent: &[u8], ostream: &mut BufWriter<Stdout>) -> sysio::io::Result<()> {
    ostream.write_all(b"\n")?;
    ostream.write_all(indent)
}

/// Write the word, along with slen spaces.
fn write_with_spaces(
    word: &[u8],
    slen: usize,
    ostream: &mut BufWriter<Stdout>,
) -> sysio::io::Result<()> {
    if slen == 2 {
        ostream.write_all(b"  ")?;
    } else if slen == 1 {
        ostream.write_all(b" ")?;
    }
    ostream.write_all(word)
}
