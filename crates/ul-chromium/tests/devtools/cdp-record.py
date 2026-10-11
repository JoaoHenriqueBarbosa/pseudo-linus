#!/usr/bin/env python3
"""Grava transcrições CDP no oráculo: cada cenário é uma lista de passos; cada passo manda uma mensagem (no
browser ou numa sessão já anexada) e espera a resposta; tudo que chega, respostas e eventos, vai para a saída na
ordem de chegada. Uso: cdp-record.py <cenários.json> <saída.json>"""
import json, socket, subprocess, sys, threading, time, urllib.request
from websockets.sync.client import connect


ROUTES = {
    "/page": b"HTTP/1.0 200 OK\r\nContent-Type: text/html\r\nContent-Length: 29\r\n\r\n<title>H</title><p>hello</p>\n",
    "/redirect": b"HTTP/1.0 302 Found\r\nLocation: /page\r\nContent-Length: 0\r\n\r\n",
    "/sub": b"HTTP/1.0 200 OK\r\nContent-Type: text/html\r\nContent-Length: 85\r\n\r\n<link rel=stylesheet href=/s.css><script src=/a.js></script><img src=/i.gif><p>x</p>\x0a",
    "/s.css": b"HTTP/1.0 200 OK\r\nContent-Type: text/css\r\nContent-Length: 13\r\n\r\np{color:red}\x0a",
    "/a.js": b"HTTP/1.0 200 OK\r\nContent-Type: application/javascript\r\nContent-Length: 12\r\n\r\nwindow.a=1;\x0a",
    "/i.gif": b"HTTP/1.0 200 OK\r\nContent-Type: image/gif\r\nContent-Length: 43\r\n\r\nGIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;",
}


def serve(port=8765):
    """O servidor local das navegações http (os mesmos bytes do `support::serve_routes` do teste, HTTP/1.0)."""
    listener = socket.socket()
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", port))
    listener.listen(16)

    def loop():
        while True:
            conn, _ = listener.accept()
            request = b""
            while b"\r\n\r\n" not in request:
                chunk = conn.recv(4096)
                if not chunk:
                    break
                request += chunk
            path = request.split(b" ")[1].decode().split("?")[0] if request.count(b" ") >= 2 else "/"
            conn.sendall(ROUTES.get(path, b"HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\n\r\n"))
            conn.close()

    threading.Thread(target=loop, daemon=True).start()
    return listener


def _pointer(value, path):
    for part in path.strip("/").split("/"):
        value = value[int(part)] if isinstance(value, list) else value[part]
    return value


def launch():
    proc = subprocess.Popen(["chromium", "--headless", "--no-sandbox", "--disable-gpu", "--remote-debugging-port=9222",
                             "--user-data-dir=/tmp/profile", "about:blank"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(100):
        try:
            version = json.load(urllib.request.urlopen("http://127.0.0.1:9222/json/version"))
            return proc, version["webSocketDebuggerUrl"]
        except Exception:
            time.sleep(0.2)
    raise SystemExit("sem DevTools")


def run(scenario, ws):
    log = []
    sessions = {}
    next_id = [1]

    def recv_until(message_id, timeout=10.0):
        end = time.time() + timeout
        while time.time() < end:
            try:
                raw = ws.recv(timeout=max(0.01, end - time.time()))
            except TimeoutError:
                break
            message = json.loads(raw)
            log.append({"in": message})
            if message.get("id") == message_id:
                return message
        return None

    def drain(seconds):
        end = time.time() + seconds
        while time.time() < end:
            try:
                raw = ws.recv(timeout=max(0.01, end - time.time()))
            except TimeoutError:
                break
            log.append({"in": json.loads(raw)})

    for step in scenario["steps"]:
        if "drain" in step:
            drain(step["drain"])
            continue
        if "raw" in step:
            text = step["raw"]
            ws.send(text)
            log.append({"out_raw": text})
            drain(0.5)
            continue
        message = {"id": next_id[0], "method": step["method"]}
        next_id[0] += 1
        if "params" in step:
            text = json.dumps(step["params"]).replace("$TARGET", sessions.get("target", ""))
            for key, value in sessions.items():
                text = text.replace('"$' + key + '"', json.dumps(value))
            message["params"] = json.loads(text)
        if "session" in step:
            message["sessionId"] = sessions.get(step["session"], step["session"])
        ws.send(json.dumps(message))
        log.append({"out": message})
        reply = recv_until(message["id"])
        if reply and "result" in reply:
            if step["method"] == "Target.getTargets" and "target" not in sessions:
                sessions["target"] = next(t["targetId"] for t in reply["result"]["targetInfos"] if t["type"] == "page")
            if "save" in step:
                field = step["save_field"]
                sessions[step["save"]] = reply["result"][field] if not field.startswith("/") else _pointer(reply["result"], field)
        if "after" in step:
            drain(step["after"])
    return log


def main():
    scenarios = json.load(open(sys.argv[1]))
    out = {}
    listener = None
    for scenario in scenarios:
        if scenario.get("server") and listener is None:
            listener = serve()
        proc, url = launch()
        try:
            with connect(url, max_size=None) as ws:
                out[scenario["name"]] = run(scenario, ws)
        finally:
            proc.terminate()
            proc.wait()
    json.dump(out, open(sys.argv[2], "w"), indent=1, ensure_ascii=False)


main()
