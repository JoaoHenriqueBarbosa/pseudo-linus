//! Tradução das mensagens de erro do jaq pras do jq 1.7.1.
//!
//! O tipo `jaq_core::Error` não expõe a estrutura (as partes são privadas), então a camada só enxerga o
//! texto renderizado. Os padrões gerados pelo jaq (`cannot index`, `cannot use .. as iterable`,
//! `cannot calculate`) são reconhecidos e os valores reparseados como JSON pra montar a mensagem do jq.
//! Mensagens específicas de cada função do jq (ex.: "explode input must be a string") não têm como
//! ser reconstruídas aqui: o jaq usa "cannot use 1 as string" pra todas.

use jaq_json::Val;

use super::json::{self, dump_trunc, type_name};

/// Erro de topo pronto pra impressão: mensagem string ou valor não string.
pub enum TopError {
    Msg(String),
    NotString(Val),
}

pub fn translate(err: jaq_core::Error<Val>) -> TopError {
    let v = err.into_val();
    let text = match &v {
        Val::TStr(b) | Val::BStr(b) => String::from_utf8_lossy(b).into_owned(),
        _ => return TopError::NotString(v),
    };
    TopError::Msg(map_message(&text).unwrap_or(text))
}

fn parse_val(s: &str) -> Option<Val> {
    json::parse_single(s.as_bytes()).ok()
}

/// Divide `s` num separador de forma que os dois lados sejam JSON válido.
fn split_vals(s: &str, sep: &str) -> Option<(Val, Val)> {
    let mut start = 0;
    while let Some(i) = s[start..].find(sep) {
        let at = start + i;
        if let (Some(l), Some(r)) = (parse_val(&s[..at]), parse_val(&s[at + sep.len()..])) {
            return Some((l, r));
        }
        start = at + 1;
    }
    None
}

fn is_zero(v: &Val) -> bool {
    matches!(v, Val::Num(n) if json::num_to_f64(n) == 0.0)
}

pub fn map_message(m: &str) -> Option<String> {
    if let Some(rest) = m.strip_prefix("cannot index ") {
        let (l, r) = split_vals(rest, " with ")?;
        return Some(match &r {
            Val::TStr(b) if b.len() < 30 => {
                format!("Cannot index {} with string \"{}\"", type_name(&l), String::from_utf8_lossy(b))
            }
            _ => format!("Cannot index {} with {}", type_name(&l), type_name(&r)),
        });
    }
    if let Some(rest) = m.strip_prefix("cannot use ")
        && let Some(val) = rest.strip_suffix(" as iterable (array or object)")
    {
        let v = parse_val(val)?;
        return Some(format!("Cannot iterate over {} ({})", type_name(&v), dump_trunc(&v)));
    }
    if let Some(rest) = m.strip_prefix("cannot calculate ") {
        for (op, verb) in [
            (" + ", "cannot be added"),
            (" - ", "cannot be subtracted"),
            (" * ", "cannot be multiplied"),
            (" / ", "cannot be divided"),
            (" % ", "cannot be divided (remainder)"),
        ] {
            if let Some((l, r)) = split_vals(rest, op) {
                let verb = if (op == " / " || op == " % ") && is_zero(&r) {
                    format!("{verb} because the divisor is zero")
                } else {
                    verb.to_string()
                };
                return Some(format!(
                    "{} ({}) and {} ({}) {verb}",
                    type_name(&l),
                    dump_trunc(&l),
                    type_name(&r),
                    dump_trunc(&r)
                ));
            }
        }
    }
    if let Some(rest) = m.strip_prefix("invalid path expression with input ") {
        let v = parse_val(rest)?;
        return Some(format!("Invalid path expression with result {}", dump_trunc(&v)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_jaq_messages() {
        assert_eq!(map_message(r#"cannot index 5 with "b""#).unwrap(), r#"Cannot index number with string "b""#);
        assert_eq!(map_message(r#"cannot index "abc" with 0"#).unwrap(), "Cannot index string with number");
        assert_eq!(map_message("cannot use null as iterable (array or object)").unwrap(), "Cannot iterate over null (null)");
        assert_eq!(
            map_message(r#"cannot calculate {"a":[1,2]} + 1"#).unwrap(),
            r#"object ({"a":[1,2]}) and number (1) cannot be added"#
        );
        assert_eq!(
            map_message("cannot calculate 1 / 0").unwrap(),
            "number (1) and number (0) cannot be divided because the divisor is zero"
        );
    }
}
