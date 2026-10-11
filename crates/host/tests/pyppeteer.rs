//! Playwright e pyppeteer em Python por dentro do pseudo-linux: o pacote vem do espelho do PyPI da bancada
//! (`testbench/mirror/wheels`, baixado por `fetch.sh`) e a cena fala CDP com o `chromium` do próprio sandbox.
//! Plano e medidas em `wip/notes/playwright-python-plan.md`.

mod common;

use common::*;
use serde_json::json;

fn write(c: &host::client::Client, sb: &str, path: &str, text: &str) {
    let r = exec(c, sb, &format!("cat > {path} <<'PL_EOF'\n{text}PL_EOF\n"));
    assert_eq!(r["exit_code"], 0, "{r}");
}

/// O diretório de wheels do espelho, como o container de teste o enxerga (`/work` é a raiz do worktree).
fn mirror_dir() -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testbench/mirror/wheels");
    assert!(dir.is_dir(), "faltam as wheels do espelho: rode testbench/mirror/fetch.sh");
    dir.canonicalize().unwrap().display().to_string()
}

/// A cena: lançar o `chromium` do sandbox, abrir a página e ir até `goto`, `title` e `evaluate`; `SHOT` acrescenta o
/// `screenshot`.
fn scene(shot: bool) -> String {
    let tail = if shot { "    png = await page.screenshot()\n    print('png', png[:8])\n" } else { "" };
    format!(
        r#"
import asyncio
from pyppeteer import launch


async def main():
    browser = await launch(executablePath="/usr/bin/chromium", headless=True, args=["--no-sandbox", "--disable-gpu"],
                           handleSIGINT=False, handleSIGTERM=False, handleSIGHUP=False)
    print("version", (await browser.version()).split("/")[0])
    page = await browser.newPage()
    await page.goto("data:text/html,<title>t</title><h1 id=a>oi</h1>")
    print("title", await page.title())
    print("evaluate", await page.evaluate("() => document.getElementById('a').textContent"))
{tail}    await browser.close()


asyncio.run(main())
"#
    )
}

/// Instala o pyppeteer do espelho num venv e roda a cena, devolvendo o `exec` dela.
fn run_scene(shot: bool) -> serde_json::Value {
    let mirror = mirror_dir();
    let d = Daemon::kernel(&format!("pypi_mirror = \"{mirror}\"\n"));
    let t = d.user("ana", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let r = exec(&c, &sb, "python3 -m venv /tmp/pp && PIP_NO_CACHE_DIR=1 /tmp/pp/bin/pip install --progress-bar off pyppeteer 2>&1 | tail -3");
    assert_eq!(r["exit_code"], 0, "{r}");
    write(&c, &sb, "/tmp/scene.py", &scene(shot));
    c.call("exec", json!({ "sandbox_id": sb, "command": "cd /tmp && timeout 60 /tmp/pp/bin/python -u scene.py 2>&1", "timeout_ms": 180_000 })).unwrap()
}

#[test]
fn pyppeteer_goto_and_evaluate() {
    let r = run_scene(false);
    assert_eq!(r["exit_code"], 0, "{r}");
    assert_eq!(r["stdout"].as_str().unwrap(), "version Chrome\ntitle t\nevaluate oi\n", "{r}");
}

/// Lacuna declarada: `Page.captureScreenshot` não existe no ul-chromium porque depende de layout e pintura (blitz-paint
/// sobre vello_cpu), que o crate ainda não liga ao CDP. Sem fachada: o teste sai do `ignore` quando a pintura existir.
#[test]
#[ignore = "Page.captureScreenshot depende da pintura (blitz-paint sobre vello_cpu), ainda não ligada ao CDP"]
fn pyppeteer_screenshot() {
    let r = run_scene(true);
    assert_eq!(r["exit_code"], 0, "{r}");
    assert!(r["stdout"].as_str().unwrap().ends_with("png b'\\x89PNG\\r\\n\\x1a\\n'\n"), "{r}");
}
