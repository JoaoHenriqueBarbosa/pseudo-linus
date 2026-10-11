//! Linha de comando do `chromium`: as bandeiras do modo headless e o despacho para o que cada uma pede.

use std::ffi::OsString;
use std::io::Write as _;

use sysabi::Ctx;

use crate::devtools;
use crate::disk_profile::{DiskProfile, Run};
use crate::fetch;
use crate::history::Navigation;
use crate::page::Page;
use crate::session::Profile;
use crate::session_storage::Exit;

/// O que a linha de comando pediu.
#[derive(Default, Debug, PartialEq)]
pub struct Options {
    pub headless: bool,
    pub dump_dom: bool,
    /// `--screenshot[=<arquivo>]`: o caminho do PNG (`screenshot.png` sem argumento).
    pub screenshot: Option<String>,
    /// `--window-size=<largura>,<altura>`: o viewport do screenshot.
    pub window_size: Option<(u32, u32)>,
    /// `--hide-scrollbars`: o screenshot sem a barra de rolagem.
    pub hide_scrollbars: bool,
    /// `--virtual-time-budget=<ms>`: quanto tempo virtual a página tem para ficar ociosa antes do despejo.
    pub virtual_time_budget: Option<f64>,
    /// `--remote-debugging-port`, `--remote-debugging-pipe` e `--remote-allow-origins`.
    pub devtools: devtools::server::Options,
    /// `--user-data-dir=<caminho>`: onde o navegador grava o perfil no disco.
    pub user_data_dir: Option<String>,
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
            "--hide-scrollbars" => options.hide_scrollbars = true,
            "--screenshot" => options.screenshot = Some("screenshot.png".to_owned()),
            flag if flag.starts_with("--screenshot=") => options.screenshot = Some(flag["--screenshot=".len()..].to_owned()),
            flag if flag.starts_with("--window-size=") => {
                options.window_size = flag["--window-size=".len()..].split_once(',').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
            }
            "--remote-debugging-pipe" => options.devtools.pipe = true,
            flag if flag.starts_with("--virtual-time-budget=") => options.virtual_time_budget = flag["--virtual-time-budget=".len()..].parse().ok(),
            flag if flag.starts_with("--remote-debugging-port=") => options.devtools.port = flag["--remote-debugging-port=".len()..].parse().ok(),
            flag if flag.starts_with("--remote-allow-origins=") => {
                options.devtools.allowed_origins = flag["--remote-allow-origins=".len()..].split(',').map(str::to_owned).collect();
            }
            flag if flag.starts_with("--user-data-dir=") => {
                options.devtools.user_data_dir = true;
                options.user_data_dir = Some(flag["--user-data-dir=".len()..].to_owned());
            }
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
        let disk = DiskProfile::start(options.user_data_dir.as_deref().map(str::as_bytes), Run::DevTools);
        let exit = devtools::server::run(ctx, &options.devtools, &url, Profile::from_disk(&disk));
        disk.finish(exit);
        return 0;
    }
    let Some(first) = options.urls.first() else { return 0 };
    let disk = DiskProfile::start(options.user_data_dir.as_deref().map(str::as_bytes), Run::DumpDom);
    let code = load(ctx, &options, first, &disk);
    disk.finish(Exit::DumpDom);
    code
}

/// O carregamento sem DevTools: segue as navegações e, com `--dump-dom`, escreve o DOM final.
fn load(ctx: &mut Ctx, options: &Options, first: &str, disk: &DiskProfile) -> i32 {
    let mut address = first.to_owned();
    // A aba atravessa as navegações: o storage do documento seguinte é o mesmo, no perfil em disco.
    let profile = std::sync::Arc::new(Profile::from_disk(disk));
    let tab = std::rc::Rc::new(crate::session::Tab::new(profile));
    for _ in 0..MAX_NAVIGATIONS {
        let started_us = (ul_common::time::realtime_nanos() / 1000) as i64;
        let mut page = match Page::load(&address, fetch::network()) {
            Ok(page) => page,
            Err(message) => {
                let _ = ctx.stderr().write_all(format!("[ERROR:components/headless/command_handler/headless_command_handler.cc:403] Page load failed: {message}\n").as_bytes());
                return 1;
            }
        };
        page.enter_tab(tab.clone());
        if let Some(budget) = options.virtual_time_budget {
            page.budget_ms = budget;
        }
        page.run();
        disk.visits.record_navigation(Navigation { url: page.url(), redirected_from: page.redirects(), title: page.title(), start_us: started_us });
        if let Some(captured) = page.captured() {
            disk.visits.record_resource(&page.url(), captured.clone());
        }
        if let Some(next) = page.pending_navigation() {
            address = next;
            continue;
        }
        // O navegador pede o `/favicon.ico` da página que ficou e guarda a resposta, mesmo a 404.
        if let Some(favicon) = fetch::favicon(&fetch::network(), &page.url()) {
            disk.visits.record_resource(&page.url(), favicon);
        }
        if options.dump_dom {
            let _ = ctx.stdout().write_all(page.dump_dom().as_bytes());
        }
        if let Some(path) = &options.screenshot {
            let (width, height) = options.window_size.unwrap_or(crate::document::WINDOW);
            let png = page.screenshot_png(width, height, options.hide_scrollbars);
            if sysabi::util::write_file(path.as_bytes(), &png, 0o644).is_ok() {
                let _ = ctx.stderr().write_all(format!("{} bytes written to file {path}\n", png.len()).as_bytes());
            }
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
