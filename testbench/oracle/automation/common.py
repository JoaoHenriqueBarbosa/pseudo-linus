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


def record_pyppeteer():
    """O pyppeteer fala WebSocket, não pipe: grava no nível da conexão dele (`Connection`), com as mesmas linhas
    do `cdp_tee`. Só age com `CDP_TEE_LOG`."""
    if not os.environ.get("CDP_TEE_LOG"):
        return
    import cdp_log
    from pyppeteer import connection

    send, receive = connection.Connection._async_send, connection.Connection._on_message

    async def logged_send(self, message, callback_id):
        cdp_log.record("out", message)
        return await send(self, message, callback_id)

    async def logged_receive(self, message):
        cdp_log.record("in", message)
        return await receive(self, message)

    connection.Connection._async_send = logged_send
    connection.Connection._on_message = logged_receive


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
