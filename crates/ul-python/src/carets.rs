//! Linha de fonte e carets (`~~~^^^`) de um quadro de traceback: porta de
//! `StackSummary.format_frame_summary`, `_should_show_carets` e `_extract_caret_anchors_from_line_segment`
//! do `traceback.py` do CPython 3.13, que é o que também desenha os erros não capturados.

use icu_properties::props::EastAsianWidth;
use icu_properties::CodePointMapData;

use crate::ast::{ExprKind, Mod, StmtKind};
use crate::compile::Span;

/// Posições de âncora dentro do segmento: onde o operador (ou os colchetes) começa e termina.
struct Anchors {
    left_end_lineno: usize,
    left_end_offset: usize,
    right_start_lineno: usize,
    right_start_offset: usize,
    primary: char,
    secondary: char,
}

/// `textwrap.dedent`: tira a margem comum; linhas só de espaço viram vazias.
fn dedent(text: &str) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut margin: Option<String> = None;
    for line in &lines {
        let body = line.trim_end_matches('\n');
        if body.chars().all(|c| c == ' ' || c == '\t') {
            continue;
        }
        let indent: String = body.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        margin = Some(match margin {
            None => indent,
            Some(m) if indent.starts_with(&m) => m,
            Some(m) if m.starts_with(&indent) => indent,
            Some(m) => m.chars().zip(indent.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a).collect(),
        });
    }
    let margin = margin.unwrap_or_default();
    let mut out = String::with_capacity(text.len());
    for line in lines {
        let body = line.trim_end_matches('\n');
        let nl = &line[body.len()..];
        if body.chars().all(|c| c == ' ' || c == '\t') {
            out.push_str(nl);
        } else {
            out.push_str(body.strip_prefix(margin.as_str()).unwrap_or(body));
            out.push_str(nl);
        }
    }
    out
}

/// `_byte_offset_to_character_offset`.
fn char_offset(line: &str, byte_offset: usize) -> usize {
    let bytes = line.as_bytes();
    let end = byte_offset.min(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).chars().count()
}

/// `_display_width`: caracteres largos (leste asiático) ocupam duas colunas.
fn display_width(line: &str, offset: usize) -> usize {
    if line.is_ascii() {
        return offset;
    }
    let map = CodePointMapData::<EastAsianWidth>::new();
    line.chars()
        .take(offset)
        .map(|c| if matches!(map.get(c), EastAsianWidth::Wide | EastAsianWidth::Fullwidth) { 2 } else { 1 })
        .sum()
}

fn parse_body(src: &str) -> Option<Vec<crate::ast::Stmt>> {
    match crate::parser::parse_module(src) {
        Ok(Mod::Module { body, .. }) => Some(body),
        _ => None,
    }
}

/// `_extract_caret_anchors_from_line_segment`.
fn extract_anchors(segment: &str) -> Option<Anchors> {
    let body = parse_body(&format!("(\n{segment}\n)"))?;
    if body.len() != 1 {
        return None;
    }
    let lines: Vec<Vec<char>> = segment.lines().map(|l| l.chars().collect()).collect();
    let raw: Vec<&str> = segment.lines().collect();
    let normalize = |lineno: usize, offset: usize| -> Option<usize> { Some(char_offset(raw.get(lineno)?, offset)) };
    let next_valid = |mut lineno: usize, mut col: usize| -> Option<(usize, usize)> {
        while lineno < lines.len() && col >= lines[lineno].len() {
            col = 0;
            lineno += 1;
        }
        (lineno < lines.len()).then_some((lineno, col))
    };
    let increment = |lineno: usize, col: usize| next_valid(lineno, col + 1);
    let nextline = |lineno: usize, _col: usize| next_valid(lineno + 1, 0);
    let increment_until = |mut lineno: usize, mut col: usize, stop: &dyn Fn(char) -> bool| -> Option<(usize, usize)> {
        loop {
            let ch = *lines.get(lineno)?.get(col)?;
            if ch == '\\' || ch == '#' {
                (lineno, col) = nextline(lineno, col)?;
            } else if !stop(ch) {
                (lineno, col) = increment(lineno, col)?;
            } else {
                return Some((lineno, col));
            }
        }
    };
    let setup = |e: &crate::ast::Expr, force_valid: bool| -> Option<(usize, usize)> {
        let lineno = e.pos.end_lineno?.checked_sub(2)?;
        let col = normalize(lineno, e.pos.end_col_offset?)?;
        if force_valid { next_valid(lineno, col) } else { Some((lineno, col)) }
    };
    let StmtKind::Expr { value } = &body[0].kind else { return None };
    match &value.kind {
        ExprKind::BinOp { left, right, .. } => {
            let (lineno, col) = setup(left, true)?;
            let (lineno, col) = increment_until(lineno, col, &|x| !x.is_whitespace() && x != ')')?;
            let mut right_col = col + 1;
            let line = lines.get(lineno)?;
            let right_line = right.pos.lineno.checked_sub(2)?;
            if right_col < line.len()
                && (right_line > lineno || right_col < normalize(right_line, right.pos.col_offset)?)
                && !line[right_col].is_whitespace()
                && line[right_col] != '\\'
                && line[right_col] != '#'
            {
                right_col += 1;
            }
            Some(Anchors { left_end_lineno: lineno, left_end_offset: col, right_start_lineno: lineno, right_start_offset: right_col, primary: '~', secondary: '^' })
        }
        ExprKind::Subscript { value: inner, .. } | ExprKind::Call { func: inner, .. } => {
            let open = if matches!(value.kind, ExprKind::Subscript { .. }) { '[' } else { '(' };
            let (left_lineno, left_col) = setup(inner, true)?;
            let (left_lineno, left_col) = increment_until(left_lineno, left_col, &|x| x == open)?;
            let (right_lineno, right_col) = setup(value, false)?;
            Some(Anchors { left_end_lineno: left_lineno, left_end_offset: left_col, right_start_lineno: right_lineno, right_start_offset: right_col, primary: '~', secondary: '^' })
        }
        _ => None,
    }
}

/// `_should_show_carets`.
fn should_show_carets(start: usize, end: usize, all_lines: &[String], anchors: &Option<Anchors>) -> bool {
    let text = all_lines.join("\n");
    if let Some(body) = parse_body(&text) {
        let Some(statement) = body.first() else { return false };
        let value = match &statement.kind {
            StmtKind::Return { value: Some(v) } => match &v.kind {
                ExprKind::Call { func, .. } if matches!(func.kind, ExprKind::Name { .. }) => Some(v),
                _ => None,
            },
            StmtKind::Assign { targets, value, .. } => match &value.kind {
                ExprKind::Call { .. } if targets.len() == 1 && matches!(targets[0].kind, ExprKind::Name { .. }) => Some(value),
                _ => None,
            },
            _ => None,
        };
        if let Some(v) = value {
            if v.pos.lineno == 1
                && v.pos.end_lineno == Some(all_lines.len())
                && v.pos.col_offset == start
                && v.pos.end_col_offset == Some(end)
            {
                return false;
            }
        }
    }
    if anchors.is_some() {
        return true;
    }
    let first: String = all_lines[0].chars().take(start).collect();
    let last: String = all_lines[all_lines.len() - 1].chars().skip(end).collect();
    !first.trim_start().is_empty() || !last.trim_end().is_empty()
}

/// O que vem depois da linha `File "...", line N, in f` de um quadro: a linha de fonte (as do intervalo,
/// quando multilinha) indentada em 4 espaços, com os carets embaixo. `lines` são as linhas de `span.lineno` a
/// `span.end_lineno` (sem o fim de linha); sem `span`, só a primeira. Vazio se não há fonte.
pub fn frame_body(lines: &[String], span: Option<Span>) -> String {
    let original: String = lines.iter().map(|l| format!("{}\n", l.trim_end())).collect();
    let dedented = dedent(&original);
    if dedented.trim().is_empty() {
        return String::new();
    }
    let Some(span) = span else {
        let first = original.lines().next().unwrap_or("").trim();
        return format!("    {first}\n");
    };
    let originals: Vec<&str> = original.lines().collect();
    let first_line = originals[0];
    let last_idx = (span.end_lineno - span.lineno) as usize;
    let Some(last_line) = originals.get(last_idx) else {
        return format!("    {}\n", originals[0].trim());
    };
    let mut start_offset = char_offset(first_line, span.col as usize);
    let mut end_offset = char_offset(last_line, span.end_col as usize);
    let all_lines: Vec<String> = dedented.lines().take(last_idx + 1).map(str::to_string).collect();
    if all_lines.len() != last_idx + 1 {
        return format!("    {}\n", originals[0].trim());
    }
    let dedent_chars = first_line.chars().count() - all_lines[0].chars().count();
    start_offset = start_offset.saturating_sub(dedent_chars);
    end_offset = end_offset.saturating_sub(dedent_chars);
    let dp_start = display_width(&all_lines[0], start_offset);
    let dp_end = display_width(&all_lines[all_lines.len() - 1], end_offset);

    let joined = all_lines.join("\n");
    let joined_chars: Vec<char> = joined.chars().collect();
    let tail = all_lines[all_lines.len() - 1].chars().count().saturating_sub(end_offset);
    let seg_end = joined_chars.len().saturating_sub(tail);
    let segment: String = joined_chars.get(start_offset..seg_end.max(start_offset)).unwrap_or(&[]).iter().collect();

    let anchors = extract_anchors(&segment);
    let show_carets = should_show_carets(start_offset, end_offset, &all_lines, &anchors);

    let mut significant: std::collections::BTreeSet<i64> = [0, all_lines.len() as i64 - 1].into_iter().collect();
    let (mut left_end, mut right_start) = (0usize, 0usize);
    let (mut primary, mut secondary) = ('^', '^');
    if let Some(a) = &anchors {
        left_end = a.left_end_offset;
        right_start = a.right_start_offset;
        if a.left_end_lineno == 0 {
            left_end += start_offset;
        }
        if a.right_start_lineno == 0 {
            right_start += start_offset;
        }
        left_end = display_width(all_lines.get(a.left_end_lineno).map_or("", String::as_str), left_end);
        right_start = display_width(all_lines.get(a.right_start_lineno).map_or("", String::as_str), right_start);
        primary = a.primary;
        secondary = a.secondary;
        for l in (a.left_end_lineno as i64 - 1)..=(a.left_end_lineno as i64 + 1) {
            significant.insert(l);
        }
        for l in (a.right_start_lineno as i64 - 1)..=(a.right_start_lineno as i64 + 1) {
            significant.insert(l);
        }
    }
    significant.remove(&-1);
    significant.remove(&(all_lines.len() as i64));

    let mut result = String::new();
    let output_line = |lineno: usize, result: &mut String| {
        let line = &all_lines[lineno];
        result.push_str(line);
        result.push('\n');
        if !show_carets {
            return;
        }
        let num_spaces = line.chars().count() - line.trim_start().chars().count();
        let num_carets = if lineno == all_lines.len() - 1 { dp_end } else { display_width(line, line.chars().count()) };
        for col in 0..num_carets {
            let c = if col < num_spaces || (lineno == 0 && col < dp_start) {
                ' '
            } else if let Some(a) = &anchors {
                let after_left = lineno > a.left_end_lineno || (lineno == a.left_end_lineno && col >= left_end);
                let before_right = lineno < a.right_start_lineno || (lineno == a.right_start_lineno && col < right_start);
                if after_left && before_right { secondary } else { primary }
            } else {
                primary
            };
            result.push(c);
        }
        result.push('\n');
    };
    let sig: Vec<i64> = significant.into_iter().collect();
    for (i, lineno) in sig.iter().enumerate() {
        if i > 0 {
            let diff = lineno - sig[i - 1];
            if diff == 2 {
                output_line((lineno - 1) as usize, &mut result);
            } else if diff > 2 {
                result.push_str(&format!("...<{} lines>...\n", diff - 1));
            }
        }
        output_line(*lineno as usize, &mut result);
    }
    dedent(&result).split_inclusive('\n').map(|l| format!("    {l}")).collect()
}
