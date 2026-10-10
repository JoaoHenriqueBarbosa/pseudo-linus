"""Suporte comum das cenas de automação: servidor da fixture em 127.0.0.1 e `step`, que roda cada passo isolado e
imprime o resultado ou o erro (tipo e primeira linha da mensagem), para a comparação apontar passo a passo."""
import functools, http.server, json, os, threading

HERE = os.path.dirname(os.path.abspath(__file__))
PORT = 8765
BASE = f"http://127.0.0.1:{PORT}"
# Invólucro que repassa o pipe do CDP ao Chromium real e grava as mensagens (CDP_TEE_LOG).
EXE = os.path.join(HERE, "cdp_tee.py")


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


def serve():
    handler = functools.partial(Quiet, directory=os.path.join(HERE, "fixtures"))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", PORT), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def show(value):
    if isinstance(value, (bytes, bytearray)):
        return f"<{len(value)} bytes>"
    try:
        return json.dumps(value, ensure_ascii=False, sort_keys=True, default=str)
    except TypeError:
        return repr(value)


def failure(error):
    return f"!{type(error).__name__}: {str(error).splitlines()[0] if str(error) else ''}"


async def step(label, awaitable):
    try:
        print(f"{label}: {show(await awaitable)}", flush=True)
    except Exception as error:
        print(f"{label}: {failure(error)}", flush=True)


def step_sync(label, function):
    try:
        print(f"{label}: {show(function())}", flush=True)
    except Exception as error:
        print(f"{label}: {failure(error)}", flush=True)
