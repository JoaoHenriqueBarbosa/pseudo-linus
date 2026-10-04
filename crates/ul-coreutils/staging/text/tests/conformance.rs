//! Placar do grupo contra o golden do GNU (`CONF_UTILS=a,b` restringe, `CONF_TRACE=1` mostra cada
//! caso, `CONF_KERNEL=1` com `--features kernel-tests` roda no kernel real).

#[path = "../../../tests/common/mod.rs"]
mod common;

#[test]
fn group_scoreboard() {
    let programs = staging_text::programs();
    #[allow(clippy::disallowed_methods)]
    let only = std::env::var("CONF_UTILS").unwrap_or_default();
    let names: Vec<&str> = if only.is_empty() { programs.iter().map(|p| p.name).collect() } else { only.split(',').collect() };
    if names.is_empty() {
        return;
    }
    common::score("coreutils", &names, programs.clone()).print(8);
}
