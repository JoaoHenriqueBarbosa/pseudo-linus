#!/usr/bin/env python3
"""Grava a fita de uma página: o chromium real navega com rede e cada resposta que ele recebe (redirecionamentos,
scripts, folhas de estilo, XHR e fetch) vai para a fita. Roda dentro da imagem pl-oracle-chromium, com rede.

Uso: tape-record.py <saída> <url>
Saída: <saída>/tape.json (as entradas) e <saída>/bodies/<sha256> (os corpos, sem duplicar).

Só entram cabeçalhos que mudam o comportamento da página; cookies, identificadores e tudo que viria da máquina
de quem grava ficam de fora, porque a fita é versionada num repositório público.
"""
import base64, hashlib, json, os, subprocess, sys, time, urllib.request
from urllib.parse import urlsplit
from websockets.sync.client import connect

KEEP = {"content-type", "location", "content-language", "link", "refresh", "x-content-type-options",
        "content-security-policy", "access-control-allow-origin", "access-control-allow-credentials",
        "access-control-allow-headers", "access-control-allow-methods", "access-control-expose-headers"}
QUIET_SECONDS = 4.0
MAX_SECONDS = 40.0


def scheme_only_redirect(response):
    """O upgrade http->https que o próprio chromium faz (307 interno) muda só o esquema, e a fita não distingue esquema."""
    headers = {k.lower(): v for k, v in response["headers"].items()}
    location = headers.get("location")
    if not location:
        return False
    a, b = urlsplit(response["url"]), urlsplit(location)
    return (a.netloc, a.path or "/", a.query) == (b.netloc, b.path or "/", b.query)


def main():
    out, url = sys.argv[1], sys.argv[2]
    os.makedirs(os.path.join(out, "bodies"), exist_ok=True)
    proc = subprocess.Popen(
        ["chromium", "--headless", "--no-sandbox", "--disable-gpu", "--remote-debugging-port=9222",
         "--user-data-dir=/tmp/profile", "--disable-quic", "about:blank"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        ws_url = None
        for _ in range(100):
            try:
                targets = json.load(urllib.request.urlopen("http://127.0.0.1:9222/json"))
                ws_url = next(t["webSocketDebuggerUrl"] for t in targets if t["type"] == "page")
                break
            except Exception:
                time.sleep(0.2)
        with connect(ws_url, max_size=None) as ws:
            entries = record(ws, url, out)
    finally:
        proc.terminate()
    with open(os.path.join(out, "tape.json"), "w") as f:
        json.dump({"url": url, "entries": entries}, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"{len(entries)} entradas")


def record(ws, url, out):
    counter = [0]

    def send(method, params=None):
        counter[0] += 1
        ws.send(json.dumps({"id": counter[0], "method": method, "params": params or {}}))
        return counter[0]

    def call(method, params=None):
        ident = send(method, params)
        while True:
            message = json.loads(ws.recv())
            if message.get("id") == ident:
                return message
            handle(message)

    pending = {}      # requestId -> entrada em construção
    finished = []     # requestId prontos para buscar o corpo
    entries = []
    state = {"last": time.time(), "loaded": False}

    def clean(headers):
        return [[k.lower(), v] for k, v in headers.items() if k.lower() in KEEP]

    def handle(message):
        method, params = message.get("method"), message.get("params", {})
        if method == "Network.requestWillBeSent":
            state["last"] = time.time()
            redirect = params.get("redirectResponse")
            if redirect and not scheme_only_redirect(redirect):
                entries.append({"method": "GET", "url": redirect["url"], "status": redirect["status"],
                                "headers": clean(redirect["headers"]), "body": None})
            request = params["request"]
            pending[params["requestId"]] = {"method": request["method"], "url": request["url"]}
        elif method == "Network.responseReceived":
            entry = pending.get(params["requestId"])
            if entry is not None:
                response = params["response"]
                entry["status"] = response["status"]
                entry["headers"] = clean(response["headers"])
        elif method == "Network.loadingFinished":
            state["last"] = time.time()
            finished.append(params["requestId"])
        elif method == "Network.loadingFailed":
            state["last"] = time.time()
            pending.pop(params["requestId"], None)
        elif method == "Page.loadEventFired":
            state["loaded"] = True

    call("Network.enable")
    call("Page.enable")
    call("Network.setCacheDisabled", {"cacheDisabled": True})
    call("Page.navigate", {"url": url})
    start = time.time()
    ws.recv_timeout = None
    while time.time() - start < MAX_SECONDS:
        try:
            handle(json.loads(ws.recv(timeout=0.5)))
        except TimeoutError:
            pass
        while finished:
            request_id = finished.pop()
            entry = pending.pop(request_id, None)
            if entry is None or "status" not in entry:
                continue
            try:
                body = call("Network.getResponseBody", {"requestId": request_id}).get("result", {})
                data = base64.b64decode(body["body"]) if body.get("base64Encoded") else body.get("body", "").encode()
            except Exception:
                data = b""
            digest = hashlib.sha256(data).hexdigest()
            with open(os.path.join(out, "bodies", digest), "wb") as f:
                f.write(data)
            entry["body"] = digest
            entries.append(entry)
        if state["loaded"] and not pending and time.time() - state["last"] > QUIET_SECONDS:
            break
    return entries


main()
