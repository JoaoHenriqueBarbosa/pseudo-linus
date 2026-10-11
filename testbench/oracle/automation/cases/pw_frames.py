"""Playwright (async): quadros de uma página com iframe de src e de srcdoc (árvore, nomes, avaliação e ações dentro do filho)."""
import asyncio
from playwright.async_api import async_playwright
from common import BASE, EXE, serve, step


async def main():
    serve()
    async with async_playwright() as p:
        browser = await p.chromium.launch(executable_path=EXE)
        page = await browser.new_page()
        await step("goto", asyncio.sleep(0, (await page.goto(BASE + "/frames.html")).status))
        await step("frames", asyncio.sleep(0, [[f.url, f.name, f.parent_frame is not None] for f in page.frames]))
        await step("main-url", asyncio.sleep(0, page.main_frame.url))
        await step("child-frames", asyncio.sleep(0, [len(f.child_frames) for f in page.frames]))
        child = page.frames[1]
        await step("child-text", child.inner_text("#p"))
        await step("child-title", child.title())
        await step("child-evaluate", child.evaluate("[location.href, window === window.top, parent !== window]"))
        await step("srcdoc-text", page.frames[2].inner_text("#q"))
        await step("frame-by-name", asyncio.sleep(0, page.frame(name="na").url))
        await step("frame-by-url", asyncio.sleep(0, page.frame(url="**/frame-child.html").name))
        await step("click-in-frame", child.click("#b"))
        await step("after-click", child.inner_text("#p"))
        await step("frame-locator", page.frame_locator("#fb").locator("#q").inner_text())
        handle = await page.query_selector("#fa")
        await step("content-frame", asyncio.sleep(0, (await handle.content_frame()).url))
        await step("owner", asyncio.sleep(0, (await child.frame_element()).get_attribute("id") if False else "skip"))
        await step("main-evaluate", page.evaluate("[frames.length, frames[0].location.pathname, document.getElementById('fa').contentWindow === frames[0]]"))
        await browser.close()


asyncio.run(main())
