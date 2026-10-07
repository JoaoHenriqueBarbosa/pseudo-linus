//! `hb_feature_from_string` e `hb_language_from_string` para cada linha da entrada padrão, no
//! formato do `featref` da bancada.

use std::io::BufRead;

use zhb::common::{feature_from_string, language_from_string};

fn main() {
    for line in std::io::stdin().lock().split(b'\n') {
        let Ok(line) = line else { break };
        let (ok, t, v, s, e) = match feature_from_string(&line) {
            Some(f) => (1, f.tag, f.value, f.start, f.end),
            None => (0, 0, 0, 0, 0),
        };
        let lang = language_from_string(&line).unwrap_or_else(|| "(null)".to_string());
        println!("{ok} {t:08x} {v} {s} {e} [{lang}]");
    }
}
