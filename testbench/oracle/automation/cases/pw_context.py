"""Playwright (async): contexto do navegador (viewport, agente, idioma, fuso, cookies, armazenamento, scripts de
inicialização, funções expostas, geolocalização, mídia emulada, offline) e sessão CDP bruta."""
import asyncio
from playwright.async_api import async_playwright
from common import BASE, EXE, serve, step


async def main():
    serve()
    async with async_playwright() as p:
        browser = await p.chromium.launch(executable_path=EXE)
        context = await browser.new_context(
            viewport={"width": 800, "height": 600}, user_agent="PW-Teste/1.0", locale="pt-BR", timezone_id="America/Sao_Paulo",
            extra_http_headers={"x-ctx": "c"}, color_scheme="dark", device_scale_factor=2, has_touch=False,
            geolocation={"latitude": -23.5, "longitude": -46.6}, permissions=["geolocation"])
        await step("contexts", asyncio.sleep(0, len(browser.contexts)))
        page = await context.new_page()
        await context.add_init_script("window.__init = 'sim'")
        await page.goto(BASE + "/")
        await step("viewport", page.evaluate("[innerWidth, innerHeight, devicePixelRatio]"))
        await step("user-agent", page.evaluate("navigator.userAgent"))
        await step("language", page.evaluate("[navigator.language, navigator.languages]"))
        await step("timezone", page.evaluate("Intl.DateTimeFormat().resolvedOptions().timeZone"))
        await step("tz-offset", page.evaluate("new Date(0).getTimezoneOffset()"))
        await step("prefers-dark", page.evaluate("matchMedia('(prefers-color-scheme: dark)').matches"))
        await step("init-script", page.evaluate("window.__init"))
        await step("geolocation", page.evaluate("new Promise(r => navigator.geolocation.getCurrentPosition(p => r([p.coords.latitude, p.coords.longitude]), e => r(e.message)))"))
        await step("header", page.evaluate("fetch('/api.json').then(r => r.status)"))
        await context.add_cookies([{"name": "k", "value": "v", "url": BASE}])
        await step("cookies", asyncio.sleep(0, [[c["name"], c["value"], c["path"], c["httpOnly"]] for c in await context.cookies()]))
        await step("document.cookie", page.evaluate("document.cookie"))
        await page.evaluate("localStorage.setItem('a', '1'); sessionStorage.setItem('b', '2')")
        state = await context.storage_state()
        await step("storage_state", asyncio.sleep(0, [[c["name"] for c in state["cookies"]], [[o["origin"], [i["name"] for i in o["localStorage"]]] for o in state["origins"]]]))
        await context.clear_cookies()
        await step("cookies-cleared", asyncio.sleep(0, len(await context.cookies())))

        async def add(a, b):
            return a + b

        await page.expose_function("somar", add)
        await step("expose_function", page.evaluate("somar(2, 3)"))
        await page.expose_binding("quem", lambda source, x: [x, source["page"] == page])
        await step("expose_binding", page.evaluate("quem(7)"))
        await context.set_offline(True)
        await step("offline", page.evaluate("fetch('/api.json').then(() => 'ok', e => e.name)"))
        await context.set_offline(False)
        await page.emulate_media(media="print")
        await step("media-print", page.evaluate("matchMedia('print').matches"))
        await page.emulate_media(reduced_motion="reduce")
        await step("reduced-motion", page.evaluate("matchMedia('(prefers-reduced-motion: reduce)').matches"))
        session = await context.new_cdp_session(page)
        await step("cdp-session", session.send("Runtime.evaluate", {"expression": "1 + 2", "returnByValue": True}))
        await step("cdp-browser-version", asyncio.sleep(0, (await session.send("Browser.getVersion"))["product"].split("/")[0]))
        await step("cdp-error", session.send("Nao.existe"))
        await session.detach()
        second = await browser.new_context()
        other = await second.new_page()
        await other.goto(BASE + "/")
        await step("isolated-cookies", other.evaluate("document.cookie"))
        await step("isolated-storage", other.evaluate("localStorage.getItem('a')"))
        await second.close()
        await context.close()
        await step("contexts-after", asyncio.sleep(0, len(browser.contexts)))
        await browser.close()


asyncio.run(main())
