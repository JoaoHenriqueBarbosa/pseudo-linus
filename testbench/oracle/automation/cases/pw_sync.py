"""Playwright (API síncrona, que usa greenlet): o mesmo percurso curto de navegar, ler, preencher e clicar."""
from playwright.sync_api import sync_playwright, expect
from common import BASE, EXE, serve, step_sync


def main():
    serve()
    with sync_playwright() as p:
        browser = p.chromium.launch(executable_path=EXE)
        page = browser.new_page()
        step_sync("goto", lambda: page.goto(BASE + "/").status)
        step_sync("title", page.title)
        step_sync("evaluate", lambda: page.evaluate("[1, 'a', {x: 1}]"))
        step_sync("fill", lambda: page.fill("#name", "Gil"))
        step_sync("click", lambda: page.click("#go"))
        step_sync("text", lambda: page.text_content("#out"))
        step_sync("locator", lambda: page.get_by_role("button", name="Enviar").text_content())
        step_sync("expect", lambda: expect(page.locator("#out")).to_have_text("oi Gil"))
        step_sync("count", lambda: page.locator("li").count())
        messages = []
        page.on("console", lambda m: messages.append(m.text))
        page.evaluate("console.log('sync')")
        page.wait_for_timeout(50)
        step_sync("console", lambda: messages)
        step_sync("route", lambda: (page.route("**/api.json", lambda r: r.fulfill(body='{"s": 1}')), page.evaluate("fetch('/api.json').then(r => r.json())"))[1])
        step_sync("timeout", lambda: page.click("#nao", timeout=200))
        step_sync("screenshot", lambda: len(page.screenshot()) > 100)
        browser.close()


main()
