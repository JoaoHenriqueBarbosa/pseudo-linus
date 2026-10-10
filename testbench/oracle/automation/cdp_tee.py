#!/usr/bin/python3
"""Invólucro do executável do Chromium para gravar o CDP que o Playwright fala pelo `--remote-debugging-pipe`.

O Playwright abre o navegador com dois fds extras (3: comandos, 4: respostas e eventos; mensagens JSON separadas por
NUL). Este programa fica no meio: cria dois pipes novos para o Chromium real, repassa os bytes sem alterar nada e
grava cada mensagem em JSONL (`CDP_TEE_LOG`), com o sentido ("out": do cliente para o navegador, "in": o contrário)
e o instante relativo. Sem o `CDP_TEE_LOG`, só repassa. Uso: `executable_path` do `launch()`."""
import hashlib, json, os, subprocess, sys, threading, time

REAL = os.environ.get("CDP_TEE_REAL", "/usr/bin/chromium")
LOG = os.environ.get("CDP_TEE_LOG")
START = time.time()
lock = threading.Lock()
log_file = open(LOG, "a") if LOG else None


LONG = 2048
BLOBS = os.environ.get("CDP_TEE_BLOBS")


def shrink(value):
    """Troca string longa (o script injetado do Playwright, corpos de resposta) por `<sha256:HEX len=N>` e guarda o
    texto uma vez em `CDP_TEE_BLOBS/HEX.txt`, para as transcrições não repetirem centenas de KB a cada mensagem."""
    if isinstance(value, str) and len(value) > LONG:
        digest = hashlib.sha256(value.encode("utf-8", "surrogatepass")).hexdigest()
        if BLOBS:
            os.makedirs(BLOBS, exist_ok=True)
            path = os.path.join(BLOBS, digest + ".txt")
            if not os.path.exists(path):
                with open(path, "w", encoding="utf-8", errors="surrogatepass") as blob:
                    blob.write(value)
        return f"<sha256:{digest} len={len(value)}>"
    if isinstance(value, list):
        return [shrink(item) for item in value]
    if isinstance(value, dict):
        return {key: shrink(item) for key, item in value.items()}
    return value


def record(direction, raw):
    if log_file is None:
        return
    try:
        message = shrink(json.loads(raw))
    except ValueError:
        message = {"raw": raw.decode("utf-8", "replace")}
    with lock:
        log_file.write(json.dumps({"t": round(time.time() - START, 3), direction: message}, ensure_ascii=False) + "\n")
        log_file.flush()


def pump(source, sink, direction):
    pending = b""
    while True:
        chunk = os.read(source, 65536)
        if not chunk:
            break
        view = memoryview(chunk)
        while view:
            written = os.write(sink, view)
            view = view[written:]
        pending += chunk
        while b"\0" in pending:
            raw, pending = pending.split(b"\0", 1)
            record(direction, raw)
    os.close(sink)


commands_read, commands_write = os.pipe()
events_read, events_write = os.pipe()


def attach():
    os.dup2(commands_read, 3)
    os.dup2(events_write, 4)


child = subprocess.Popen([REAL] + sys.argv[1:], pass_fds=(3, 4), preexec_fn=attach)
os.close(commands_read)
os.close(events_write)
threads = [
    threading.Thread(target=pump, args=(3, commands_write, "out"), daemon=True),
    threading.Thread(target=pump, args=(events_read, 4, "in"), daemon=True),
]
for thread in threads:
    thread.start()
code = child.wait()
for thread in threads:
    thread.join(timeout=2)
sys.exit(code)
