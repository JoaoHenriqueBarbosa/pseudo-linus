"""Playwright (async): `connect_over_cdp` num navegador que já está rodando com `--remote-debugging-port`
(o caminho de quem liga a um Chromium externo, em vez de lançá-lo)."""
import asyncio, json, subprocess, time, urllib.request
from playwright.async_api import async_playwright
from common import BASE, serve, step


def start_browser():
    process = subprocess.Popen(["/usr/bin/chromium", "--headless", "--no-sandbox", "--disable-gpu", "--remote-debugging-port=9222",
                                "--user-data-dir=/tmp/connect-profile", "about:blank"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(100):
        try:
            urllib.request.urlopen("http://127.0.0.1:9222/json/version")
            return process
        except Exception:
            time.sleep(0.2)
    raise SystemExit("sem DevTools")


async def main():
    serve()
    process = start_browser()
    try:
        async with async_playwright() as p:
            browser = await p.chromium.connect_over_cdp("http://127.0.0.1:9222")
            await step("version-major", asyncio.sleep(0, browser.version.split(".")[0]))
            await step("contexts", asyncio.sleep(0, [len(browser.contexts), [len(c.pages) for c in browser.contexts]]))
            context = browser.contexts[0]
            await step("first-page-url", asyncio.sleep(0, context.pages[0].url))
            page = await context.new_page()
            await page.goto(BASE + "/")
            await step("title", page.title())
            await step("click", page.click("#go"))
            await step("pages", asyncio.sleep(0, len(context.pages)))
            await step("targets-json", asyncio.sleep(0, sorted(t["type"] for t in json.load(urllib.request.urlopen("http://127.0.0.1:9222/json/list")))))
            new = await browser.new_context()
            fresh = await new.new_page()
            await fresh.goto(BASE + "/second.html")
            await step("new-context", fresh.title())
            await step("contexts-after", asyncio.sleep(0, len(browser.contexts)))
            await new.close()
            await browser.close()
        await step("still-alive", asyncio.sleep(0, process.poll() is None))
    finally:
        process.terminate()
        process.wait()


asyncio.run(main())
