//! `hb-ot-tag.cc`: tags OpenType de idioma (e o subtag privado `-hbsc`/`-hbot`) a partir de uma
//! tag BCP 47 já canonizada pelo `hb_language_from_string`.

use crate::lang_table::{COMPLEX_RULES, COMPLEX_SUBTAGS, OT_LANGUAGES2, OT_LANGUAGES3};

/// `HB_OT_MAX_TAGS_PER_SCRIPT` e `HB_OT_MAX_TAGS_PER_LANGUAGE`.
pub const MAX_TAGS: usize = 3;

const TAG_DEFAULT_SCRIPT: u32 = u32::from_be_bytes(*b"DFLT");

/// As condições do `switch (lang_str[0])` do `hb_ot_tags_from_complex_language`, sempre
/// aplicadas a partir de `&lang_str[1]`.
pub(crate) enum Cond {
    /// `0 == strcmp (&lang_str[1], s)`.
    Strcmp(&'static [u8]),
    /// `lang_matches (&lang_str[1], limit, s, len)`.
    LangMatches(&'static [u8]),
    /// `0 == strncmp (&lang_str[1], p, len) && subtag_matches (lang_str, limit, s, len)`.
    PrefixSubtag(&'static [u8], &'static [u8]),
}

/// `strstr` restrito a `hay`.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// `subtag_matches`: `lang` vai até o NUL do C; `limit` é o índice-limite.
fn subtag_matches(lang: &[u8], start: usize, limit: usize, subtag: &[u8]) -> bool {
    if limit.wrapping_sub(start) < subtag.len() {
        return false;
    }
    let mut from = start;
    loop {
        let Some(off) = find(&lang[from..], subtag) else { return false };
        let s = from + off;
        if s >= limit {
            return false;
        }
        if !lang.get(s + subtag.len()).is_some_and(u8::is_ascii_alphanumeric) {
            return true;
        }
        from = s + subtag.len();
    }
}

/// `lang_matches`.
fn lang_matches(lang: &[u8], start: usize, limit: usize, spec: &[u8]) -> bool {
    if limit.wrapping_sub(start) < spec.len() {
        return false;
    }
    lang[start..].starts_with(spec) && matches!(lang.get(start + spec.len()), None | Some(b'-'))
}

fn take(tags: &[u32], count: usize) -> Vec<u32> {
    tags.iter().copied().take(count).collect()
}

/// `hb_ot_tags_from_complex_language`.
fn complex_language(lang: &[u8], limit: usize, count: usize) -> Option<Vec<u32>> {
    if limit >= 7 {
        if let Some(p) = lang.iter().position(|&c| c == b'-') {
            if p < limit && limit - p >= 5 {
                for (sub, tags) in COMPLEX_SUBTAGS {
                    if subtag_matches(lang, p, limit, sub) {
                        return Some(take(tags, count));
                    }
                }
            }
        }
    }
    let first = *lang.first()?;
    for (c, cond, tags) in COMPLEX_RULES {
        if *c != first {
            continue;
        }
        let ok = match cond {
            Cond::Strcmp(s) => &lang[1..] == *s,
            Cond::LangMatches(s) => lang_matches(lang, 1, limit, s),
            Cond::PrefixSubtag(p, s) => lang[1..].starts_with(p) && subtag_matches(lang, 0, limit, s),
        };
        if ok {
            return Some(take(tags, count));
        }
    }
    None
}

/// `hb_tag_from_string` sobre `len` bytes, completando com espaços.
fn tag_of(s: &[u8]) -> u32 {
    let mut t = [b' '; 4];
    for (d, &b) in t.iter_mut().zip(s.iter().take(4)) {
        *d = b;
    }
    u32::from_be_bytes(t)
}

/// `hb_ot_tags_from_language`.
fn tags_from_language(lang: &[u8], limit: usize, count: usize) -> Vec<u32> {
    if let Some(t) = complex_language(lang, limit, count) {
        return t;
    }
    let s = lang.iter().position(|&c| c == b'-');
    let mut start = 0;
    if let Some(s) = s {
        if limit >= 6 {
            let ext_end = lang[s + 1..].iter().position(|&c| c == b'-');
            let ext_len = ext_end.unwrap_or(lang.len() - s - 1);
            if ext_len == 3 && lang.get(s + 1).is_some_and(u8::is_ascii_alphabetic) {
                start = s + 1;
            }
        }
    }
    let rest = &lang[start..];
    let first_len = match rest.iter().position(|&c| c == b'-') {
        Some(d) => d,
        None => limit.saturating_sub(start),
    };
    let table: &[(u32, u32)] = match first_len {
        2 => &OT_LANGUAGES2,
        3 => &OT_LANGUAGES3,
        _ => &[],
    };
    let lang_tag = tag_of(&rest[..first_len.min(rest.len())]);
    if let Ok(mut i) = table.binary_search_by(|e| e.0.cmp(&lang_tag)) {
        while i != 0 && table[i].0 == table[i - 1].0 {
            i -= 1;
        }
        return table[i..]
            .iter()
            .take_while(|e| e.1 != 0 && e.0 == lang_tag)
            .take(count)
            .map(|e| e.1)
            .collect();
    }
    // Aqui `lang_str` do C já pode ter andado até o extlang, e `s` continua no primeiro hífen.
    let s = s.unwrap_or(lang.len());
    if s.checked_sub(start) == Some(3) {
        return vec![tag_of(&lang[start..start + 3]) & !0x2020_2000];
    }
    Vec::new()
}

/// `parse_private_use_subtag`.
fn private_use_subtag(sub: Option<&[u8]>, prefix: &[u8], upper: bool) -> Option<u32> {
    let sub = sub?;
    let at = find(sub, prefix)?;
    let s = &sub[at + prefix.len()..];
    let mut tag = [0u8; 4];
    if s.first() == Some(&b'-') {
        let s = &s[1..];
        let mut i = 0;
        while i < 8 && s.get(i).is_some_and(u8::is_ascii_hexdigit) {
            let c = (s[i] as char).to_digit(16).unwrap_or(0) as u8;
            if i % 2 == 0 {
                tag[i / 2] = c << 4;
            } else {
                tag[i / 2] = tag[i / 2].wrapping_add(c);
            }
            i += 1;
        }
        if i != 8 {
            return None;
        }
    } else {
        let mut i = 0;
        while i < 4 && s.get(i).is_some_and(u8::is_ascii_alphanumeric) {
            tag[i] = if upper { s[i].to_ascii_uppercase() } else { s[i].to_ascii_lowercase() };
            i += 1;
        }
        if i == 0 {
            return None;
        }
        for t in tag.iter_mut().skip(i) {
            *t = b' ';
        }
    }
    let mut t = u32::from_be_bytes(tag);
    if t & 0xDFDF_DFDF == TAG_DEFAULT_SCRIPT {
        t ^= !0xDFDF_DFDF;
    }
    Some(t)
}

/// `hb_ot_tags_from_script_and_language` com as contagens do `hb_ot_map_builder_t`: devolve os
/// tags de escrita (`None` quando vale o `hb_ot_all_tags_from_script`) e os de idioma.
pub fn tags_from_language_string(language: Option<&str>) -> (Option<Vec<u32>>, Vec<u32>) {
    let Some(language) = language else { return (None, Vec::new()) };
    let lang = language.as_bytes();
    let mut limit = None;
    let mut private = None;
    if lang.starts_with(b"x-") {
        private = Some(0);
    } else {
        let mut s = 1;
        while s < lang.len() {
            if lang[s - 1] == b'-' && lang.get(s + 1) == Some(&b'-') {
                if lang[s] == b'x' {
                    private = Some(s);
                    limit.get_or_insert(s - 1);
                    break;
                } else if limit.is_none() {
                    limit = Some(s - 1);
                }
            }
            s += 1;
        }
        limit.get_or_insert(s.min(lang.len()));
    }
    let private = private.map(|p| &lang[p..]);
    let script = private_use_subtag(private, b"-hbsc", false).map(|t| vec![t]);
    let language_tags = match private_use_subtag(private, b"-hbot", true) {
        Some(t) => vec![t],
        None => tags_from_language(lang, limit.unwrap_or(0), MAX_TAGS),
    };
    (script, language_tags)
}
