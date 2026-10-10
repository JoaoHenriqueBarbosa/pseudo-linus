"""Playwright (async): interceptação de rede (`route`), preenchimento de resposta, aborto, repasse e cabeçalhos."""
import asyncio
from playwright.async_api import async_playwright
from common import BASE, EXE, serve, step


async def main():
    serve()
    async with async_playwright() as p:
        browser = await p.chromium.launch(executable_path=EXE)
        page = await browser.new_page()
        await page.goto(BASE + "/")

        async def fulfill(route):
            await route.fulfill(status=201, content_type="application/json", headers={"x-teste": "sim"}, body='{"falso": true}')

        await page.route("**/api.json", fulfill)
        await step("fulfilled", page.evaluate("fetch('/api.json').then(async r => [r.status, r.headers.get('x-teste'), await r.json()])"))
        await page.unroute("**/api.json")
        await step("unrouted", page.evaluate("fetch('/api.json').then(r => r.json())"))

        async def abort(route):
            await route.abort("failed")

        await page.route("**/api.json", abort)
        await step("aborted", page.evaluate("fetch('/api.json').then(() => 'ok', e => e.name + ': ' + e.message)"))
        await page.unroute("**/api.json")

        seen = []

        async def modify(route):
            request = route.request
            seen.append([request.method, request.url[-9:], request.headers.get("x-extra")])
            await route.continue_(headers={**request.headers, "x-extra": "1"})

        await page.route("**/api.json", modify)
        await step("continued", page.evaluate("fetch('/api.json').then(r => r.status)"))
        await step("seen", asyncio.sleep(0, seen))
        await page.unroute("**/api.json")

        async def fetch_and_change(route):
            response = await route.fetch()
            body = (await response.text()).replace("automação", "mudada")
            await route.fulfill(response=response, body=body)

        await page.route("**/api.json", fetch_and_change)
        await step("route.fetch", page.evaluate("fetch('/api.json').then(r => r.json())"))
        await page.unroute("**/api.json")

        await page.route("**/second.html", lambda route: route.fulfill(status=200, content_type="text/html", body="<title>interceptada</title><p>falsa"))
        await page.goto(BASE + "/second.html")
        await step("page-fulfilled", asyncio.sleep(0, [await page.title(), await page.inner_text("p")]))
        await page.unroute("**/second.html")
        await page.route(lambda url: url.endswith("/post"), lambda route: route.fulfill(status=200, body=route.request.post_data or "vazio"))
        await step("post_data", page.evaluate("fetch('/post', {method: 'POST', body: 'corpo'}).then(r => r.text())"))
        await page.set_extra_http_headers({"x-global": "g"})
        await step("extra-headers", page.evaluate("fetch('/api.json').then(r => r.status)"))
        await step("request-fails", page.goto(BASE + "/api.json"))
        context_seen = []

        async def count_and_continue(route):
            context_seen.append(1)
            await route.continue_()

        await page.context.route("**/second.html", count_and_continue)
        await page.goto(BASE + "/second.html")
        await step("context-route", asyncio.sleep(0, context_seen))
        await browser.close()


asyncio.run(main())
