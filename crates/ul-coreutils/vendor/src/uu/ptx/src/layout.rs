// Porte pseudo-linus: os campos de cada linha de saída e as três formas de escrevê-los (terminal,
// roff e TeX). As larguras seguem o `ptx` do GNU 9.7, medidas no oráculo.

use std::cmp;

use crate::window::Line;

/// As larguras que decidem quanto de cada campo cabe.
pub(crate) struct Dims<'a> {
    /// Metade da largura útil da linha (já sem a referência, quando ela fica à esquerda).
    pub(crate) half: usize,
    /// Espaço mínimo entre os campos.
    pub(crate) gap: usize,
    /// A marca de truncamento (`-F`).
    pub(crate) trunc: &'a [u8],
    /// A saída em TeX não imprime a marca, mas as larguras contam com ela do mesmo jeito.
    pub(crate) flags: bool,
    /// Comprimento da maior palavra do texto todo.
    pub(crate) max_word: usize,
}

/// Os cinco campos de uma ocorrência, na ordem `tail before KEYWORD after head`: `before` e `after`
/// são o contexto colado ao keyword; `tail` e `head` recebem o texto que dá a volta pelas pontas da
/// linha quando o keyword fica perto de uma delas.
pub(crate) struct Fields {
    pub(crate) tail: Vec<u8>,
    pub(crate) before: Vec<u8>,
    pub(crate) keyword: Vec<u8>,
    pub(crate) after: Vec<u8>,
    pub(crate) head: Vec<u8>,
    /// O campo `before` ficou só com a marca de truncamento: o GNU empurra uma coluna a linha toda
    /// pra direita nesse caso.
    pub(crate) hang: bool,
}

/// Recorta os campos de `keyword` dentro das larguras de `dims`.
///
/// `all_before` e `all_after` são o texto do contexto antes e depois do keyword. A conta dos
/// tamanhos foi medida contra o GNU: o campo `before` leva `half - gap - 2*marca`, o `after` leva
/// `half - 2*marca - keyword`, e o que sobra de espaço de um lado enche o `tail` ou o `head` do
/// outro.
pub(crate) fn compute_fields(
    dims: &Dims<'_>,
    all_before: &[u8],
    keyword: &[u8],
    all_after: &[u8],
) -> Fields {
    let before_text = Line(all_before);
    let after_text = Line(all_after);
    let trunc_len = dims.trunc.len();

    let max_before = dims.half.saturating_sub(dims.gap + 2 * trunc_len);
    let max_keyafter = dims.half.saturating_sub(2 * trunc_len);
    let max_after = max_keyafter.saturating_sub(keyword.len());

    // Os dois campos coladas ao keyword, cada um indo até onde a sua metade da linha deixa.
    let before = before_text.window_ending_at(0..before_text.len(), max_before);
    let after = after_text.window_starting_at(0..after_text.len(), max_after);
    let before_len = before.end - before.start;
    let after_len = after.end - after.start;

    // O espaço que o `before` deixou livre à esquerda é do `tail`.
    let max_tail = max_before.saturating_sub(before_len + dims.gap + 1);
    let tail_start = after_text.trim(after.end..after_text.len()).start;
    let mut tail = after_text.window_starting_at(tail_start..after_text.len(), max_tail);
    // Uma palavra de um byte no fim sobrevive ao alinhamento, porque o byte antes dela é um espaço e
    // parece uma fronteira de palavra. Cai fora, pra o `tail` não terminar no meio da frase.
    if tail.end - tail.start > 2
        && after_text.is_space_at(tail.end - 2)
        && !after_text.is_space_at(tail.end - 1)
    {
        tail = after_text.trim(tail.start..tail.end - 1);
    }

    // O espaço que o `after` deixou livre à direita é do `head`, limitado também pela maior palavra
    // do texto (é o que o GNU faz, e a conta sai do que se mede nele).
    let keyafter_len = keyword.len() + after_len;
    let room_right = max_keyafter.saturating_sub(keyafter_len + dims.gap);
    let room_word = (dims.max_word + dims.half).saturating_sub(before_len + 3);
    let max_head = cmp::min(room_right, room_word);
    let head = before_text.window_ending_at(0..before.start, max_head);

    let mut fields = Fields {
        tail: after_text.text(tail.clone()),
        before: before_text.text(before.clone()),
        keyword: keyword.to_vec(),
        after: after_text.text(after.clone()),
        head: before_text.text(head.clone()),
        hang: false,
    };

    // A marca vai no campo mais externo que perdeu texto, pra aparecer na ponta da linha e não no
    // meio. A saída em TeX não leva marcas.
    if dims.flags {
        if after.end != after_text.len() {
            if tail.is_empty() {
                fields.after.extend_from_slice(dims.trunc);
            } else if tail.end != after_text.len() {
                fields.tail.extend_from_slice(dims.trunc);
            }
        }
        if before.start != 0 {
            if head.is_empty() {
                fields.hang = !dims.trunc.is_empty() && before.is_empty();
                let mut marked = dims.trunc.to_vec();
                marked.extend_from_slice(&fields.before);
                fields.before = marked;
            } else if head.start != 0 {
                let mut marked = dims.trunc.to_vec();
                marked.extend_from_slice(&fields.head);
                fields.head = marked;
            }
        }
    }

    fields
}

/// Onde a linha de terminal assenta cada campo.
pub(crate) struct Geometry {
    /// A coluna em que o `tail` começa (a referência à esquerda e o espaço entre ela e o texto).
    pub(crate) margin: usize,
    /// A largura útil: o `head` termina em `margin + inner_width`.
    pub(crate) inner_width: usize,
    pub(crate) half: usize,
    pub(crate) gap: usize,
    /// A largura pedida com `-w`, que posiciona a referência à direita.
    pub(crate) width: usize,
}

/// Onde a referência vai na linha de terminal.
pub(crate) enum RefPlace<'a> {
    None,
    /// À esquerda; com `:` depois quando é uma referência automática.
    Left { text: &'a [u8], colon: bool },
    Right { text: &'a [u8] },
}

/// Escreve `text` na coluna `col` de `buf`, preenchendo com espaços o que faltar.
fn put(buf: &mut Vec<u8>, col: usize, text: &[u8]) {
    if text.is_empty() {
        return;
    }
    let end = col + text.len();
    if buf.len() < end {
        buf.resize(end, b' ');
    }
    buf[col..end].copy_from_slice(text);
}

/// A linha de terminal: referência, `tail` à esquerda, `before` alinhado à direita da metade
/// esquerda, keyword e `after` a partir do meio, e o `head` alinhado à direita da linha.
pub(crate) fn format_dumb(geometry: &Geometry, fields: &Fields, reference: &RefPlace<'_>) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::new();
    let edge = (geometry.margin + geometry.half).saturating_sub(geometry.gap);
    let mut keyword_col = geometry.margin + geometry.half;

    if let RefPlace::Left { text, colon } = reference {
        put(&mut buf, 0, text);
        if *colon {
            put(&mut buf, text.len(), b":");
        }
    }
    put(&mut buf, geometry.margin, &fields.tail);
    if fields.hang {
        put(&mut buf, (edge + 1).saturating_sub(fields.before.len()), &fields.before);
        keyword_col += 1;
    } else {
        put(&mut buf, edge.saturating_sub(fields.before.len()), &fields.before);
    }
    let mut keyafter = fields.keyword.clone();
    keyafter.extend_from_slice(&fields.after);
    put(&mut buf, keyword_col, &keyafter);
    if !fields.head.is_empty() {
        let head_col = (geometry.margin + geometry.inner_width).saturating_sub(fields.head.len());
        put(&mut buf, head_col, &fields.head);
    }
    if let RefPlace::Right { text } = reference {
        let target = geometry.width + geometry.gap + usize::from(fields.hang);
        if buf.len() < target {
            buf.resize(target, b' ');
        }
        buf.extend_from_slice(text);
    }
    buf
}

/// Aspas dobradas no meio do texto, pra o nroff não se confundir.
fn roff_field(text: &[u8], out: &mut Vec<u8>) {
    for &b in text {
        if b == b'"' {
            out.push(b'"');
        }
        out.push(b);
    }
}

/// `.xx "tail" "before" "keyword and after" "head" "ref"`.
pub(crate) fn format_roff(macro_name: &str, fields: &Fields, reference: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(b'.');
    out.extend_from_slice(macro_name.as_bytes());
    let mut keyafter = fields.keyword.clone();
    keyafter.extend_from_slice(&fields.after);
    for field in [&fields.tail, &fields.before, &keyafter, &fields.head] {
        out.extend_from_slice(b" \"");
        roff_field(field, &mut out);
        out.push(b'"');
    }
    if let Some(reference) = reference {
        out.extend_from_slice(b" \"");
        roff_field(reference, &mut out);
        out.push(b'"');
    }
    out
}

/// Os caracteres que o TeX trata de forma especial, protegidos como o GNU faz.
fn tex_field(text: &[u8], out: &mut Vec<u8>) {
    for &b in text {
        match b {
            b'\\' => out.extend_from_slice(b"\\backslash{}"),
            b'$' | b'%' | b'#' | b'&' | b'_' => {
                out.push(b'\\');
                out.push(b);
            }
            b'{' => out.extend_from_slice(b"$\\{$"),
            b'}' => out.extend_from_slice(b"$\\}$"),
            _ => out.push(b),
        }
    }
}

/// `\xx {tail}{before}{keyword}{after}{head}{ref}`.
pub(crate) fn format_tex(macro_name: &str, fields: &Fields, reference: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(b'\\');
    out.extend_from_slice(macro_name.as_bytes());
    out.push(b' ');
    for field in [
        &fields.tail,
        &fields.before,
        &fields.keyword,
        &fields.after,
        &fields.head,
    ] {
        out.push(b'{');
        tex_field(field, &mut out);
        out.push(b'}');
    }
    if let Some(reference) = reference {
        out.push(b'{');
        tex_field(reference, &mut out);
        out.push(b'}');
    }
    out
}
