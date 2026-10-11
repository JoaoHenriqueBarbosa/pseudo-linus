//! O que o `Preferences` do perfil guarda de cada site visitado (medido no oráculo com `measure-navigation.sh`): o
//! engajamento com o site (`site_engagement`, só do endereço digitado) e o de mídia (`media_engagement`, de cada página
//! visitada), mais o `profile.last_engagement_time`. O molde traz o arquivo da execução sem navegação; aqui entram as
//! chaves que a navegação acrescenta, no texto, com a ordem e a formatação de números do escritor JSON do Chromium.

/// A vida de uma entrada de engajamento de mídia: 90 dias, em microssegundos.
const MEDIA_LIFETIME_US: i64 = 7_776_000_000_000;
/// O que o engajamento da primeira navegação soma de pontos.
const FIRST_POINTS: f64 = 3.0;
/// Os pontos que cada execução seguinte soma ao site e à omnibox, e o teto de 15 que os dois atingem (medido no oráculo em 1 a 8 e em 10 execuções).
const NEXT_POINTS: f64 = 1.5;
const OMNIBOX_POINTS: f64 = 3.0;
const MAX_POINTS: f64 = 15.0;
/// A entrada do popup da omnibox em `site_engagement` e quanto antes da página ela é engajada (µs, medido 70 a 184 ms).
const OMNIBOX: &str = "chrome://omnibox-popup.top-chrome/,*";
const OMNIBOX_LEAD_US: i64 = 120_000;
/// Quanto depois da visita o engajamento com o site é anotado, e quanto depois do fim dela o de mídia.
const SITE_ENGAGEMENT_DELAY_US: i64 = 2_500;
const MEDIA_ENGAGEMENT_DELAY_US: i64 = 45;

/// Uma visita a um site: a origem, o instante (WebKit, µs) e quanto durou.
#[derive(Debug, Clone)]
pub struct SiteVisit {
    pub origin: String,
    pub time: i64,
    pub duration: i64,
}

/// As entradas de primeiro nível do objeto JSON que começa em `text[open]` (uma chave `{`), como pares (chave, texto).
fn entries(text: &str, open: usize) -> Option<(Vec<(String, String)>, usize)> {
    let bytes = text.as_bytes();
    let mut list = Vec::new();
    let mut at = open + 1;
    while bytes.get(at)? != &b'}' {
        if bytes[at] == b',' {
            at += 1;
        }
        let key_end = at + 1 + text[at + 1..].find('"')?;
        let key = text[at + 1..key_end].to_owned();
        let value_start = key_end + 2;
        let mut depth = 0;
        let mut end = value_start;
        for (offset, byte) in bytes[value_start..].iter().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' if depth == 0 => break,
                b'}' => depth -= 1,
                _ => {}
            }
            end = value_start + offset + 1;
            if depth == 0 && matches!(bytes.get(end), Some(b',' | b'}')) {
                break;
            }
        }
        list.push((key, text[value_start..end].to_owned()));
        at = end;
    }
    Some((list, at + 1))
}

/// Põe a chave `key` com o valor `value` no objeto de `section` (`"site_engagement"`), mantendo a ordem das chaves.
fn insert(text: &str, section: &str, key: &str, value: &str) -> Option<String> {
    let marker = format!("\"{section}\":");
    let open = text.find(&marker)? + marker.len();
    let (mut list, end) = entries(text, open)?;
    list.retain(|(existing, _)| existing != key);
    list.push((key.to_owned(), value.to_owned()));
    list.sort();
    let body: Vec<String> = list.iter().map(|(name, content)| format!("\"{name}\":{content}")).collect();
    Some(format!("{}{{{}}}{}", &text[..open], body.join(","), &text[end..]))
}

/// As origens visitadas, sem repetição e em ordem.
fn origins_of(visits: &[SiteVisit]) -> Vec<&str> {
    let mut origins: Vec<&str> = visits.iter().map(|visit| visit.origin.as_str()).collect();
    origins.sort_unstable();
    origins.dedup();
    origins
}

/// Onde começa a entrada `key` dentro da seção `section`.
fn entry_start(text: &str, section: &str, key: &str) -> Option<usize> {
    let from = text.find(&format!("\"{section}\":{{"))?;
    Some(from + text[from..].find(&format!("\"{key}\":{{"))?)
}

/// O número do campo `field` na entrada `key` (`"origem,*"`) de `text`.
fn number(text: &str, section: &str, key: &str, field: &str) -> Option<f64> {
    let entry = &text[entry_start(text, section, key)?..];
    let from = entry.find(&format!("\"{field}\":"))? + field.len() + 3;
    let length = entry[from..].find([',', '}'])?;
    entry[from..from + length].trim_matches('"').parse().ok()
}

/// Troca o valor do campo `field` da entrada `key` por `value`, mantendo o resto do texto.
fn set_value(text: &mut String, section: &str, key: &str, field: &str, value: &str) {
    let Some(entry) = entry_start(text, section, key) else { return };
    let Some(at) = text[entry..].find(&format!("\"{field}\":")) else { return };
    let from = entry + at + field.len() + 3;
    let Some(length) = text[from..].find([',', '}']) else { return };
    text.replace_range(from..from + length, value);
}

/// Um escore de engajamento como o Chromium o escreve.
fn points(value: f64) -> String {
    format!("{value:?}")
}

/// O `Preferences` com as visitas desta execução. `previous` é o arquivo que a execução anterior deixou (o engajamento
/// acumula de uma para a outra) e `floor` o instante em que o navegador subiu. `float` escreve um número como o
/// escritor JSON do Chromium.
pub fn apply(preferences: &str, previous: Option<&str>, floor: i64, visits: &[SiteVisit], float: impl Fn(f64) -> String) -> String {
    let mut text = preferences.to_owned();
    let before = |section: &str, key: &str, field: &str| previous.and_then(|old| number(old, section, key, field));
    for origin in origins_of(visits) {
        let key = format!("{origin},*");
        let own: Vec<&SiteVisit> = visits.iter().filter(|visit| visit.origin == origin).collect();
        let last = own[own.len() - 1];
        let modified = last.time + last.duration + MEDIA_ENGAGEMENT_DELAY_US;
        let seen = before("media_engagement", &key, "visits").unwrap_or(0.0) as usize + own.len();
        let value = format!(
            "{{\"expiration\":\"{}\",\"last_modified\":\"{modified}\",\"lifetime\":\"{MEDIA_LIFETIME_US}\",\"setting\":{{\"hasHighScore\":false,\"lastMediaPlaybackTime\":0.0,\"mediaPlaybacks\":0,\"visits\":{seen}}}}}",
            modified - 7 + MEDIA_LIFETIME_US
        );
        text = insert(&text, "media_engagement", &key, &value).unwrap_or(text);
    }
    let mut first_engaged = i64::MAX;
    for origin in origins_of(visits) {
        let key = format!("{origin},*");
        let first = visits.iter().find(|visit| visit.origin == origin).map_or(0, |visit| visit.time);
        let engaged = first + SITE_ENGAGEMENT_DELAY_US;
        first_engaged = first_engaged.min(engaged);
        let score = before("site_engagement", &key, "rawScore").map_or(FIRST_POINTS, |old| (old + NEXT_POINTS).min(MAX_POINTS));
        let value = format!(
            "{{\"last_modified\":\"{}\",\"setting\":{{\"lastEngagementTime\":{},\"lastShortcutLaunchTime\":0.0,\"pointsAddedToday\":{},\"rawScore\":{}}}}}",
            engaged + 8,
            float(engaged as f64),
            points(score),
            points(score)
        );
        text = insert(&text, "site_engagement", &key, &value).unwrap_or(text);
    }
    // O popup da omnibox soma pontos a cada execução depois da primeira e é engajado antes da página.
    if let Some(old) = before("site_engagement", OMNIBOX, "rawScore") {
        let score = points((old + OMNIBOX_POINTS).min(MAX_POINTS));
        set_value(&mut text, "site_engagement", OMNIBOX, "rawScore", &score);
        set_value(&mut text, "site_engagement", OMNIBOX, "pointsAddedToday", &score);
    }
    let omnibox = number(&text, "site_engagement", OMNIBOX, "lastEngagementTime").map_or(i64::MAX, |time| time as i64);
    if omnibox >= first_engaged {
        let at = (first_engaged - OMNIBOX_LEAD_US).max(floor);
        set_value(&mut text, "site_engagement", OMNIBOX, "lastEngagementTime", &float(at as f64));
        set_value(&mut text, "site_engagement", OMNIBOX, "last_modified", &format!("\"{}\"", at + 11));
    }
    let latest = visits.iter().map(|visit| visit.time).max().unwrap_or(0) + SITE_ENGAGEMENT_DELAY_US;
    if let Some(start) = text.find("\"last_engagement_time\":\"") {
        let from = start + "\"last_engagement_time\":\"".len();
        if let Some(length) = text[from..].find('"') {
            text.replace_range(from..from + length, &latest.to_string());
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn float(value: f64) -> String {
        format!("{value:e}").replace("e", "e+").replacen("e+-", "e-", 1)
    }

    const BASE: &str = r#"{"a":{"media_engagement":{},"site_engagement":{"chrome://omnibox-popup.top-chrome/,*":{"last_modified":"13436159414500000","setting":{"lastEngagementTime":1.34361594145e+16,"lastShortcutLaunchTime":0.0,"pointsAddedToday":4.5,"rawScore":4.5}}},"sound":{}},"profile":{"last_engagement_time":"5","x":1}}"#;
    const ORIGIN: &str = "http://127.0.0.1:8000";

    fn visit(time: i64) -> [SiteVisit; 1] {
        [SiteVisit { origin: ORIGIN.to_owned(), time, duration: 26_735 }]
    }

    #[test]
    fn a_first_visit_adds_site_and_media_engagement() {
        let out = apply(BASE, None, 0, &visit(13_436_159_414_423_740), float);
        assert!(out.contains(r#""media_engagement":{"http://127.0.0.1:8000,*":{"expiration":"13443935414450513","last_modified":"13436159414450520","lifetime":"7776000000000","setting":{"hasHighScore":false,"lastMediaPlaybackTime":0.0,"mediaPlaybacks":0,"visits":1}}}"#), "{out}");
        assert!(out.contains(r#""http://127.0.0.1:8000,*":{"last_modified":"13436159414426248","setting":{"lastEngagementTime":1.343615941442624e+16,"lastShortcutLaunchTime":0.0,"pointsAddedToday":3.0,"rawScore":3.0}}"#), "{out}");
        assert!(out.contains(r#""last_engagement_time":"13436159414426240","x":1"#), "{out}");
        // A omnibox estava depois da página no molde: passa a vir antes, 120 ms.
        assert!(out.contains(r#""last_modified":"13436159414306251","setting":{"lastEngagementTime":1.343615941430624e+16,"#), "{out}");
    }

    #[test]
    fn engagement_accumulates_from_the_previous_run_up_to_the_cap() {
        let mut file = apply(BASE, None, 0, &visit(13_436_159_414_423_740), float);
        let mut seen = Vec::new();
        for run in 1..=5 {
            file = apply(BASE, Some(&file), 0, &visit(13_436_159_414_423_740 + run * 1_000_000), float);
            seen.push((number(&file, "site_engagement", OMNIBOX, "rawScore"), number(&file, "site_engagement", "http://127.0.0.1:8000,*", "rawScore"), number(&file, "media_engagement", "http://127.0.0.1:8000,*", "visits")));
        }
        // Medido no oráculo: omnibox 4.5, 7.5, 10.5, 13.5, 15.0; site 3.0, 4.5, 6.0, 7.5, 9.0; visitas 1 a 5.
        assert_eq!(seen[0], (Some(7.5), Some(4.5), Some(2.0)));
        assert_eq!(seen[1], (Some(10.5), Some(6.0), Some(3.0)));
        assert_eq!(seen[3], (Some(15.0), Some(9.0), Some(5.0)));
        // Medido em 8 e 10 execuções: a omnibox em 15.0, o site em 13.5 e depois em 15.0.
        let mut file = file;
        for run in 6..=9 {
            file = apply(BASE, Some(&file), 0, &visit(13_436_159_414_423_740 + run * 1_000_000), float);
        }
        assert_eq!(number(&file, "site_engagement", "http://127.0.0.1:8000,*", "rawScore"), Some(15.0));
    }
}
