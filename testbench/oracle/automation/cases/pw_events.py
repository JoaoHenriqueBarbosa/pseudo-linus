"""Playwright (async): eventos da página (console, erro de página, diálogo, requisições, ciclo de vida, popup)."""
import asyncio
from playwright.async_api import async_playwright
from common import BASE, EXE, serve, step


async def main():
    serve()
    async with async_playwright() as p:
        browser = await p.chromium.launch(executable_path=EXE)
        page = await browser.new_page()
        consoles, errors, requests, responses, finished, lifecycle = [], [], [], [], [], []
        page.on("console", lambda m: consoles.append([m.type, m.text, len(m.args), m.location.get("url", "")[-12:]]))
        page.on("pageerror", lambda e: errors.append(str(e).splitlines()[0]))
        page.on("request", lambda r: requests.append([r.method, r.url[-12:], r.resource_type, r.is_navigation_request()]))
        page.on("response", lambda r: responses.append([r.status, r.url[-12:]]))
        page.on("requestfinished", lambda r: finished.append(r.url[-12:]))
        page.on("domcontentloaded", lambda _: lifecycle.append("domcontentloaded"))
        page.on("load", lambda _: lifecycle.append("load"))
        await page.goto(BASE + "/")
        await step("lifecycle", asyncio.sleep(0, lifecycle))
        await page.fill("#name", "Eva")
        await page.click("#go")
        await page.wait_for_timeout(100)
        await step("console", asyncio.sleep(0, consoles))
        async with page.expect_console_message() as info:
            await page.evaluate("console.error('erro', [1, 2])")
        message = await info.value
        await step("expect_console", asyncio.sleep(0, [message.type, message.text]))
        await step("console-args", asyncio.gather(*[a.json_value() for a in message.args]))
        await page.evaluate("async_error()")
        await page.wait_for_timeout(100)
        await step("pageerror", asyncio.sleep(0, errors))
        async with page.expect_response("**/api.json") as response_info:
            await page.evaluate("ping()")
        response = await response_info.value
        await step("expect_response", asyncio.sleep(0, [response.status, response.url[-9:], await response.json()]))
        await step("response-text", response.text())
        await step("response-headers", asyncio.sleep(0, sorted(k for k in response.headers if k in ("content-type", "content-length"))))
        await step("request-info", asyncio.sleep(0, [response.request.method, response.request.resource_type, response.request.frame == page.main_frame]))
        await step("requests", asyncio.sleep(0, requests))
        await step("responses", asyncio.sleep(0, responses))
        await step("finished", asyncio.sleep(0, finished))
        seen = []
        page.once("dialog", lambda d: (seen.append([d.type, d.message]), asyncio.ensure_future(d.dismiss())))
        await step("dialog-result", page.evaluate("ask()"))
        await step("dialog", asyncio.sleep(0, seen))
        page.once("dialog", lambda d: asyncio.ensure_future(d.accept()))
        await step("dialog-accept", page.evaluate("ask()"))
        async with page.expect_popup() as popup_info:
            await page.evaluate("window.open('/second.html')")
        popup = await popup_info.value
        await popup.wait_for_load_state()
        await step("popup", asyncio.sleep(0, [popup.url[-12:], await popup.title(), popup.opener is not None]))
        await step("popup-pages", asyncio.sleep(0, len(page.context.pages)))
        await popup.close()
        async with page.expect_navigation():
            await page.click("#link")
        await step("navigated", asyncio.sleep(0, page.url[-12:]))
        async with page.expect_event("close"):
            await page.close()
        await browser.close()


asyncio.run(main())
