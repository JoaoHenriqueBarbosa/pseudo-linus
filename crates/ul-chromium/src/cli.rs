//! Linha de comando do `chromium`: as bandeiras do modo headless e o despacho para o que cada uma pede.

use std::ffi::OsString;
use std::io::Write as _;

use sysabi::Ctx;

use crate::page::Page;

/// O que a linha de comando pediu.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Options {
    pub headless: bool,
    pub dump_dom: bool,
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
            flag if flag.starts_with("--") => {}
            url => options.urls.push(url.to_owned()),
        }
    }
    options
}

pub fn main(ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let args: Vec<String> = argv.iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let options = parse(&args);
    let Some(url) = options.urls.first() else { return 0 };
    let page = match Page::load(url) {
        Ok(page) => page,
        Err(message) => {
            let _ = ctx.stderr().write_all(format!("{message}\n").as_bytes());
            return 1;
        }
    };
    if options.dump_dom {
        let _ = ctx.stdout().write_all(page.dump_dom().as_bytes());
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headless_flags() {
        let args: Vec<String> = ["--headless", "--no-sandbox", "--disable-gpu", "--dump-dom", "file:///a.html"].map(String::from).to_vec();
        assert_eq!(parse(&args), Options { headless: true, dump_dom: true, urls: vec!["file:///a.html".into()] });
    }
}
