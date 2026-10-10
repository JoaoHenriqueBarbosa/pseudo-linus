//! Linha de comando do `chromium`: as bandeiras do modo headless e o despacho para o que cada uma pede.

use std::ffi::OsString;
use std::io::Write as _;

use sysabi::Ctx;

use crate::devtools;
use crate::fetch;
use crate::page::Page;

/// O que a linha de comando pediu.
#[derive(Default, Debug, PartialEq)]
pub struct Options {
    pub headless: bool,
    pub dump_dom: bool,
    /// `--virtual-time-budget=<ms>`: quanto tempo virtual a página tem para ficar ociosa antes do despejo.
    pub virtual_time_budget: Option<f64>,
    /// `--remote-debugging-port`, `--remote-debugging-pipe` e `--remote-allow-origins`.
    pub devtools: devtools::server::Options,
    pub urls: Vec<String>,
}

/// Lê `argv` (sem o nome do programa). As bandeiras de ambiente (`--no-sandbox`, `--disable-gpu` e afins) são aceitas e
/// ignoradas, como o Chromium aceita as que não se aplicam.
pub fn parse(args: &[String]) -> Options {
    let mut options = Options::default();
    for arg in args {
        match arg.as_str() {
            "--headless" | "--headless=new" | "--headless=old" => options.headless = true,
            "--dump-dom" => options.dump_dom = true,
            "--remote-debugging-pipe" => options.devtools.pipe = true,
            flag if flag.starts_with("--virtual-time-budget=") => options.virtual_time_budget = flag["--virtual-time-budget=".len()..].parse().ok(),
            flag if flag.starts_with("--remote-debugging-port=") => options.devtools.port = flag["--remote-debugging-port=".len()..].parse().ok(),
            flag if flag.starts_with("--remote-allow-origins=") => {
                options.devtools.allowed_origins = flag["--remote-allow-origins=".len()..].split(',').map(str::to_owned).collect();
            }
            flag if flag.starts_with("--user-data-dir=") => options.devtools.user_data_dir = true,
            flag if flag.starts_with("--") => {}
            url => options.urls.push(url.to_owned()),
        }
    }
    options
}

/// Quantas navegações seguidas (`location.href = ...`) uma invocação acompanha.
const MAX_NAVIGATIONS: usize = 20;

pub fn main(ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let args: Vec<String> = argv.iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let options = parse(&args);
    // Com o DevTools remoto o navegador fica de pé servindo o protocolo; sem URL, a aba abre em `about:blank`.
    if options.devtools.port.is_some() || options.devtools.pipe {
        let url = options.urls.first().cloned().unwrap_or_else(|| "about:blank".to_owned());
        return devtools::server::run(ctx, &options.devtools, &url);
    }
    let Some(first) = options.urls.first() else { return 0 };
    let mut address = first.clone();
    for _ in 0..MAX_NAVIGATIONS {
        let mut page = match Page::load(&address, fetch::network()) {
            Ok(page) => page,
            Err(message) => {
                let _ = ctx.stderr().write_all(format!("[ERROR:components/headless/command_handler/headless_command_handler.cc:403] Page load failed: {message}\n").as_bytes());
                return 1;
            }
        };
        if let Some(budget) = options.virtual_time_budget {
            page.budget_ms = budget;
        }
        page.run();
        if let Some(next) = page.pending_navigation() {
            address = next;
            continue;
        }
        if options.dump_dom {
            let _ = ctx.stdout().write_all(page.dump_dom().as_bytes());
        }
        return 0;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headless_flags() {
        let args: Vec<String> = ["--headless", "--no-sandbox", "--disable-gpu", "--virtual-time-budget=15000", "--dump-dom", "file:///a.html"].map(String::from).to_vec();
        assert_eq!(parse(&args), Options { headless: true, dump_dom: true, virtual_time_budget: Some(15000.0), urls: vec!["file:///a.html".into()], ..Options::default() });
        let args: Vec<String> = ["--headless", "--remote-debugging-port=0", "--remote-allow-origins=*"].map(String::from).to_vec();
        assert_eq!(parse(&args).devtools, devtools::server::Options { port: Some(0), pipe: false, allowed_origins: vec!["*".into()], user_data_dir: false });
    }
}
