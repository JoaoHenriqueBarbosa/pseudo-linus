"""Playwright (async): capturas (PNG, JPEG, elemento, página inteira, recorte) e PDF, medidas pelo cabeçalho e pelas
dimensões; os bytes de pixel não entram (dependem do renderizador)."""
import asyncio, struct
from playwright.async_api import async_playwright
from common import BASE, EXE, serve, step


def png_info(data):
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "não é PNG"
    width, height = struct.unpack(">II", data[16:24])
    return [width, height]


def jpeg_info(data):
    assert data[:2] == b"\xff\xd8", "não é JPEG"
    return [len(data) > 100]


async def main():
    serve()
    async with async_playwright() as p:
        browser = await p.chromium.launch(executable_path=EXE)
        page = await browser.new_page(viewport={"width": 640, "height": 480})
        await page.goto(BASE + "/")
        await step("png", asyncio.sleep(0, png_info(await page.screenshot())))
        await step("jpeg", asyncio.sleep(0, jpeg_info(await page.screenshot(type="jpeg", quality=80))))
        await step("full_page", asyncio.sleep(0, png_info(await page.screenshot(full_page=True))))
        await step("clip", asyncio.sleep(0, png_info(await page.screenshot(clip={"x": 0, "y": 0, "width": 100, "height": 50}))))
        await step("element", asyncio.sleep(0, png_info(await page.locator("#box").screenshot())))
        await step("omit_background", asyncio.sleep(0, png_info(await page.screenshot(omit_background=True))))
        await step("scale-css", asyncio.sleep(0, png_info(await page.screenshot(scale="css"))))
        await step("path", asyncio.sleep(0, png_info(await page.screenshot(path="/tmp/shot.png"))))
        await step("pdf", asyncio.sleep(0, (await page.pdf())[:5]))
        await page.set_viewport_size({"width": 320, "height": 200})
        await step("png-small", asyncio.sleep(0, png_info(await page.screenshot())))
        await page.evaluate("document.body.style.height = '2000px'")
        await step("full_page-tall", asyncio.sleep(0, png_info(await page.screenshot(full_page=True))))
        await browser.close()


asyncio.run(main())
