//! `hb_feature_from_string` e `hb_language_from_string` para cada linha da entrada padrão, no
//! formato do `featref` da bancada.

use std::io::BufRead;

use zhb::common::{feature_from_string, language_from_string};
use zhb::lang::tags_from_language_string;

fn main() {
    for line in std::io::stdin().lock().split(b'\n') {
        let Ok(line) = line else { break };
        let (ok, t, v, s, e) = match feature_from_string(&line) {
            Some(f) => (1, f.tag, f.value, f.start, f.end),
            None => (0, 0, 0, 0, 0),
        };
        let lang = language_from_string(&line);
        let (script, tags) = tags_from_language_string(lang.as_deref());
        let mut out = format!("{ok} {t:08x} {v} {s} {e} [{}] s", lang.as_deref().unwrap_or("(null)"));
        // Com HB_SCRIPT_INVALID o `hb_ot_all_tags_from_script` não produz tag nenhuma.
        for t in script.unwrap_or_default() {
            out += &format!(" {t:08x}");
        }
        out += " l";
        for t in tags {
            out += &format!(" {t:08x}");
        }
        println!("{out}");
    }
}
