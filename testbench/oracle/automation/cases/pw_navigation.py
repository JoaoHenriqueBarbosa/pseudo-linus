"""Playwright (async): navegação, ciclo de vida, histórico, quadros e viewport."""
import asyncio
from playwright.async_api import async_playwright
from common import BASE, EXE, serve, step


async def main():
    serve()
    async with async_playwright() as p:
        browser = await p.chromium.launch(executable_path=EXE)
        await step("version-major", asyncio.sleep(0, browser.version.split(".")[0]))
        await step("is_connected", asyncio.sleep(0, browser.is_connected()))
        page = await browser.new_page()
        response = await page.goto(BASE + "/")
        await step("response", asyncio.sleep(0, [response.status, response.url, response.ok, response.status_text,
                                                  response.headers.get("content-type")]))
        await step("title", page.title())
        await step("url", asyncio.sleep(0, page.url))
        await step("content-head", asyncio.sleep(0, (await page.content())[:40]))
        await step("inner_text", page.inner_text("#title"))
        await step("viewport_size", asyncio.sleep(0, page.viewport_size))
        await step("window-inner", page.evaluate("[innerWidth, innerHeight]"))
        await page.set_viewport_size({"width": 500, "height": 400})
        await step("viewport-after", page.evaluate("[innerWidth, innerHeight]"))
        await step("wait_for_load_state", page.wait_for_load_state("networkidle"))
        await page.click("#link")
        await step("wait_for_url", page.wait_for_url("**/second.html"))
        await step("second-title", page.title())
        await step("go_back", asyncio.sleep(0, (await page.go_back()).url))
        await step("go_forward", asyncio.sleep(0, (await page.go_forward()).url))
        await step("reload", asyncio.sleep(0, (await page.reload()).status))
        await step("goto-404", asyncio.sleep(0, (await page.goto(BASE + "/nao-existe")).status))
        await page.set_content("<p id=x>conteúdo novo</p><iframe srcdoc='<b>dentro</b>'></iframe>")
        await step("set_content", page.inner_text("#x"))
        await page.wait_for_selector("iframe")
        await step("frames", asyncio.sleep(0, [len(page.frames), page.main_frame.url, [f.parent_frame is not None for f in page.frames]]))
        await step("frame-text", page.frames[1].inner_text("b"))
        await step("goto-data", asyncio.sleep(0, (await page.goto("data:text/html,<title>d</title>x")) is None or "ok"))
        await step("data-title", page.title())
        await step("goto-bad", page.goto("http://127.0.0.1:1/"))
        await step("is_closed", asyncio.sleep(0, page.is_closed()))
        await page.close()
        await step("is_closed-after", asyncio.sleep(0, page.is_closed()))
        await browser.close()
        await step("is_connected-after", asyncio.sleep(0, browser.is_connected()))


asyncio.run(main())
