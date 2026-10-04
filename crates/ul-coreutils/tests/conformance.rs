//! Conformidade de cada utilitário contra o golden do GNU coreutils 9.7, no kernel de teste.
//!
//! `cargo test -p ul-coreutils --test conformance -- --nocapture` mostra o placar por utilitário e as
//! falhas. Os pisos (`MIN_STRICT`) só sobem: um porte que perde caso quebra o teste.

mod common;

use common::score;

/// Piso de casos estritos por utilitário (o placar medido quando o piso foi fixado).
const MIN_STRICT: &[(&str, usize)] = &[];

#[test]
fn coreutils_scoreboard() {
    let programs = ul_coreutils::programs();
    // `CONF_UTILS=cat,head` restringe o placar (variável do host, lida pelo teste).
    #[allow(clippy::disallowed_methods)]
    let only = std::env::var("CONF_UTILS").unwrap_or_default();
    let names: Vec<&str> = if only.is_empty() {
        programs.iter().map(|p| p.name).collect()
    } else {
        only.split(',').collect()
    };
    let board = score("coreutils", &names, programs.clone());
    board.print(6);
    for (util, min) in MIN_STRICT {
        let got = board.util(util).strict;
        assert!(got >= *min, "{util}: {got} casos estritos, piso {min}");
    }
}
