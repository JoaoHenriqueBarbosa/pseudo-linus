"""Playwright (async): avaliação de JavaScript, valores serializados, handles e erros."""
import asyncio
from playwright.async_api import async_playwright
from common import BASE, EXE, serve, step


async def main():
    serve()
    async with async_playwright() as p:
        browser = await p.chromium.launch(executable_path=EXE)
        page = await browser.new_page()
        await page.goto(BASE + "/")
        for label, expression in [
            ("number", "1 + 1"), ("string", "'a' + 'b'"), ("null", "null"), ("undefined", "undefined"), ("bool", "!0"),
            ("array", "[1, 'x', null, [2]]"), ("object", "({a: 1, b: {c: [1, 2]}})"), ("nan", "NaN"), ("inf", "Infinity"),
            ("neg-zero", "-0"), ("bigint", "10n ** 20n"), ("date", "new Date(0)"), ("regexp", "/a+/gi"),
            ("map", "new Map([[1, 2]])"), ("set", "new Set([1, 2])"), ("error", "new Error('x')"),
            ("typed", "new Uint8Array([1, 2, 3])"), ("symbol", "Symbol('s')"), ("function", "() => 1"),
            ("promise", "Promise.resolve(7)"), ("later", "later()"), ("page-fn", "ping()"),
            ("throw", "fail()"), ("syntax", "1 +"), ("reference", "naoExiste"), ("type", "null.x"),
            ("ua-has-chrome", "navigator.userAgent.includes('HeadlessChrome') || navigator.userAgent.includes('Chrome')"),
            ("webdriver", "navigator.webdriver"), ("plugins", "navigator.plugins.length"),
            ("languages", "navigator.languages"), ("location", "location.href"),
        ]:
            await step("eval-" + label, page.evaluate(expression))
        await step("arg-number", page.evaluate("x => x * 2", 21))
        await step("arg-object", page.evaluate("o => o.a + o.b.length", {"a": 1, "b": [1, 2, 3]}))
        await step("arg-list", page.evaluate("([a, b]) => a + b", ["x", "y"]))
        await step("arg-none", page.evaluate("x => x === undefined", None))
        await step("arg-nan", page.evaluate("x => Number.isNaN(x)", float("nan")))
        handle = await page.evaluate_handle("document.body")
        await step("handle-tag", handle.evaluate("e => e.tagName"))
        await step("handle-props", asyncio.sleep(0, sorted((await (await page.evaluate_handle("({a: 1, b: 2})")).get_properties()).keys())))
        await step("handle-json", (await page.evaluate_handle("({a: [1, 2]})")).json_value())
        element = await page.query_selector("#title")
        await step("element-text", element.text_content())
        await step("element-bbox", asyncio.sleep(0, bool(await element.bounding_box())))
        await step("query_all", asyncio.sleep(0, len(await page.query_selector_all("li"))))
        await step("eval_on_selector", page.eval_on_selector("#title", "e => e.textContent"))
        await step("eval_on_selector_all", page.eval_on_selector_all("li", "es => es.map(e => e.textContent)"))
        await step("eval_on_selector-missing", page.eval_on_selector("#nao", "e => 1"))
        await step("wait_for_function", page.wait_for_function("document.getElementById('title').dataset.ready === '1'"))
        await step("add_script_tag", page.add_script_tag(content="window.__x = 5"))
        await step("added", page.evaluate("window.__x"))
        await step("add_style_tag", asyncio.sleep(0, bool(await page.add_style_tag(content="#title{color:red}"))))
        await step("computed-color", page.evaluate("getComputedStyle(document.getElementById('title')).color"))
        await browser.close()


asyncio.run(main())
