//! Log no stderr, com filtro em `PL_LOG` (padrão `info`) e formato em `PL_LOG_FORMAT` (`text` ou
//! `json`).

pub fn init(component: &'static str) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_env("PL_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let json = std::env::var("PL_LOG_FORMAT").is_ok_and(|v| v.eq_ignore_ascii_case("json"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).with_target(false);
    let res = if json { builder.json().try_init() } else { builder.try_init() };
    if res.is_ok() {
        tracing::debug!(component, "log iniciado");
    }
}
